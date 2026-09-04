//! Backend-neutral application use cases for tasks and time tracking.

mod repository;

use chrono::{DateTime, Utc};
use tracker_domain::{
    ActiveWorklog, SwitchedWorklogs, Task, TaskId, TaskName, Tracker, TrackingError, TrackingState,
    Worklog, WorklogId,
};

pub use repository::{
    RepositoryError, TaskRepository, TrackerRepository, TrackingRepository, WorklogRepository,
};

/// Canonicalizes a client timestamp to the microsecond precision shared by
/// every current persistence adapter.
fn canonical_timestamp(timestamp: DateTime<Utc>) -> DateTime<Utc> {
    DateTime::from_timestamp_micros(timestamp.timestamp_micros())
        .expect("every UTC timestamp fits the canonical microsecond range")
}

/// How the task list is ordered.
///
/// The ordering rules live here once, backend-neutral, per ADR 0002:
/// recently worked puts the latest worklog start first and tasks without
/// worklogs last; the alternatives are newest-first by `updated_at` and by
/// `created_at`. Every ordering ends in a deterministic `TaskId` ascending
/// tie-break.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskOrdering {
    /// Latest worklog start descending, tasks without worklogs last, then
    /// `created_at` descending, then `TaskId` ascending. The default.
    #[default]
    RecentlyWorked,
    /// `updated_at` descending, then `created_at` descending, then `TaskId`
    /// ascending.
    RecentlyUpdated,
    /// `created_at` descending, then `TaskId` ascending.
    RecentlyCreated,
}

impl TaskOrdering {
    /// Sorts task-list items in place by this ordering, exactly per ADR 0002.
    pub fn sort_items(self, items: &mut [TaskListItem]) {
        match self {
            TaskOrdering::RecentlyWorked => items.sort_by(|a, b| {
                b.latest_work_start
                    .cmp(&a.latest_work_start)
                    .then_with(|| b.task.created_at().cmp(&a.task.created_at()))
                    .then_with(|| a.task.id().cmp(&b.task.id()))
            }),
            TaskOrdering::RecentlyUpdated => items.sort_by(|a, b| {
                b.task
                    .updated_at()
                    .cmp(&a.task.updated_at())
                    .then_with(|| b.task.created_at().cmp(&a.task.created_at()))
                    .then_with(|| a.task.id().cmp(&b.task.id()))
            }),
            TaskOrdering::RecentlyCreated => items.sort_by(|a, b| {
                b.task
                    .created_at()
                    .cmp(&a.task.created_at())
                    .then_with(|| a.task.id().cmp(&b.task.id()))
            }),
        };
    }
}

/// One row of the task-list read model: the task plus its latest worklog
/// start, or `None` when the task has no worklogs.
///
/// The latest work start is derived, never stored on the task, so it stays
/// correct no matter which client wrote the worklog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskListItem {
    pub task: Task,
    pub latest_work_start: Option<DateTime<Utc>>,
}

/// Why an application operation failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApplicationError {
    #[error(transparent)]
    Domain(#[from] TrackingError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
    #[error("tracking write failed: {0}")]
    TrackingWrite(#[source] RepositoryError),
    #[error("tracking state could not be recovered: {0}")]
    TrackingRecovery(#[source] RepositoryError),
    #[error("task state could not be recovered: {0}")]
    TaskRecovery(#[source] RepositoryError),
    #[error("tracking state changed in another client")]
    TrackingStateChanged,
}

/// The result of creating, renaming, archiving, or unarchiving a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskOutcome {
    Created(Task),
    Renamed(Task),
    Archived(Task),
    Unarchived(Task),
}

/// The result of making one task active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetActiveTaskOutcome {
    Started { worklog: Worklog },
    Switched { stopped: Worklog, started: Worklog },
    AlreadyActive { worklog: ActiveWorklog },
}

/// The result of making the tracker idle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClearActiveTaskOutcome {
    Stopped { worklog: Worklog },
    AlreadyIdle,
}

/// Task queries available to presentation and transport layers.
///
/// The service keeps its task snapshot current on every committed write, so
/// querying never reads the backend again and cannot fail.
pub trait TaskQueries {
    /// The tasks of the selected backend, ordered by the given ordering.
    fn tasks(&self, ordering: TaskOrdering) -> Vec<TaskListItem>;
    fn task(&self, id: TaskId) -> Option<&Task>;
}

/// Commands that create or change tasks.
///
/// Every operation takes the client-created instant it occurred at; the
/// domain decides how that instant moves `updated_at`.
pub trait TaskOperations {
    fn create_task(
        &mut self,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<TaskOutcome, ApplicationError>;
    fn rename_task(
        &mut self,
        id: TaskId,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<TaskOutcome, ApplicationError>;
    fn archive_task(
        &mut self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<TaskOutcome, ApplicationError>;
    fn unarchive_task(
        &mut self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<TaskOutcome, ApplicationError>;
}

/// Current tracking state and desired-state tracking commands.
pub trait TrackingOperations {
    fn current_tracking(&self) -> &TrackingState;
    fn set_active_task(
        &mut self,
        task_id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, ApplicationError>;
    fn clear_active_task(
        &mut self,
        expected_active: WorklogId,
        occurred_at: DateTime<Utc>,
    ) -> Result<ClearActiveTaskOutcome, ApplicationError>;
}

/// Queries for completed and active worklogs. No presentation snapshot is
/// built; each call reads the selected backend.
pub trait WorklogQueries {
    fn worklogs_for_task(&self, task_id: TaskId) -> Result<Vec<Worklog>, ApplicationError>;
}

/// The application service required by presentation and transport clients.
///
/// This keeps repository ports behind the application boundary.
pub trait TrackerApplicationService:
    TaskQueries + TaskOperations + TrackingOperations + WorklogQueries
{
}

impl<T> TrackerApplicationService for T where
    T: TaskQueries + TaskOperations + TrackingOperations + WorklogQueries
{
}

/// Stateful application service backed by one repository.
///
/// The service owns persistence sequencing. It keeps the task snapshot and
/// current tracking queries ready for a synchronous client, updates the
/// snapshot itself on every committed write instead of reading the backend
/// again behind an ambiguous result, commits switches through the
/// repository's atomic operation, and reloads authoritative state after any
/// tracking write conflict.
pub struct TrackerApplication<R> {
    repository: R,
    /// The snapshot in canonical `TaskId` order; queries sort it per the
    /// requested ordering.
    tasks: Vec<TaskListItem>,
    tracker: Tracker,
}

impl<R: TrackerRepository> TrackerApplication<R> {
    /// Loads task and tracking state from the selected backend.
    pub fn load(repository: R) -> Result<Self, ApplicationError> {
        let mut tasks = Self::load_task_items(&repository)?;
        let tracker = Self::load_tracker(&repository)?;
        Self::align_active_work(&mut tasks, &tracker);
        Ok(Self {
            repository,
            tasks,
            tracker,
        })
    }

    fn load_task_items(repository: &R) -> Result<Vec<TaskListItem>, ApplicationError> {
        let mut items = repository.list_task_items()?;
        items.sort_by_key(|a| a.task.id());
        Ok(items)
    }

    fn load_tracker(repository: &R) -> Result<Tracker, ApplicationError> {
        match repository.active_worklog()? {
            Some(worklog) => Ok(Tracker::resume(worklog)?),
            None => Ok(Tracker::idle()),
        }
    }

    fn align_active_work(items: &mut [TaskListItem], tracker: &Tracker) {
        if let Some(active) = tracker.active()
            && let Some(item) = items
                .iter_mut()
                .find(|item| item.task.id() == active.task_id)
        {
            item.latest_work_start = Some(
                item.latest_work_start
                    .map_or(active.start, |old| old.max(active.start)),
            );
        }
    }

    fn refresh_tracking(&mut self) -> Result<(), ApplicationError> {
        let tracker = Self::load_tracker(&self.repository)?;
        // Another client may have started or switched this worklog after our
        // task snapshot was loaded. Aligning the active start keeps the two
        // snapshots consistent at the point of the authoritative active read.
        Self::align_active_work(&mut self.tasks, &tracker);
        self.tracker = tracker;
        Ok(())
    }

    fn recover_after_tracking_write(&mut self, write_error: RepositoryError) -> ApplicationError {
        let loaded = (|| {
            let mut tasks = self.repository.list_task_items()?;
            tasks.sort_by_key(|a| a.task.id());
            let tracker = Self::load_tracker(&self.repository)?;
            Self::align_active_work(&mut tasks, &tracker);
            Ok::<_, ApplicationError>((tasks, tracker))
        })();
        match loaded {
            Ok((tasks, tracker)) => {
                self.tasks = tasks;
                self.tracker = tracker;
                ApplicationError::TrackingWrite(write_error)
            }
            Err(ApplicationError::Repository(error)) => ApplicationError::TrackingRecovery(error),
            Err(ApplicationError::Domain(error)) => ApplicationError::Domain(error),
            Err(ApplicationError::TrackingWrite(error))
            | Err(ApplicationError::TrackingRecovery(error))
            | Err(ApplicationError::TaskRecovery(error)) => {
                ApplicationError::TrackingRecovery(error)
            }
            Err(ApplicationError::TrackingStateChanged) => ApplicationError::TrackingStateChanged,
        }
    }

    fn refresh_after_task_operation(&mut self) -> Result<(), ApplicationError> {
        match self.refresh_tracking() {
            Err(ApplicationError::Repository(error)) => Err(ApplicationError::TaskRecovery(error)),
            result => result,
        }
    }

    fn replace_task(&mut self, changed: Task) {
        if let Some(item) = self
            .tasks
            .iter_mut()
            .find(|item| item.task.id() == changed.id())
        {
            item.task = changed;
        } else {
            self.tasks.push(TaskListItem {
                task: changed,
                latest_work_start: None,
            });
            self.tasks.sort_by_key(|a| a.task.id());
        }
    }

    /// Records that a worklog started on a task at `at`, keeping the
    /// snapshot's latest-work value equal to the stored `MAX(start)`
    /// without reading the backend again.
    fn note_work_start(&mut self, task_id: TaskId, at: DateTime<Utc>) {
        if let Some(item) = self.tasks.iter_mut().find(|item| item.task.id() == task_id) {
            item.latest_work_start = Some(item.latest_work_start.map_or(at, |old| old.max(at)));
        }
    }
}

impl<R: TrackerRepository> TaskQueries for TrackerApplication<R> {
    fn tasks(&self, ordering: TaskOrdering) -> Vec<TaskListItem> {
        let mut items = self.tasks.clone();
        ordering.sort_items(&mut items);
        items
    }

    fn task(&self, id: TaskId) -> Option<&Task> {
        self.tasks
            .iter()
            .find(|item| item.task.id() == id)
            .map(|TaskListItem { task, .. }| task)
    }
}

impl<R: TrackerRepository> TaskOperations for TrackerApplication<R> {
    fn create_task(
        &mut self,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<TaskOutcome, ApplicationError> {
        let occurred_at = canonical_timestamp(occurred_at);
        let task = Task::create(TaskId::generate(), name, occurred_at);
        self.repository.create_task(task.clone())?;
        self.replace_task(task.clone());
        Ok(TaskOutcome::Created(task))
    }

    fn rename_task(
        &mut self,
        id: TaskId,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<TaskOutcome, ApplicationError> {
        let occurred_at = canonical_timestamp(occurred_at);
        // The port changes only the name and its metadata timestamp, so a
        // concurrent archive state survives the write.
        let task = self.repository.rename_task(id, name, occurred_at)?;
        self.replace_task(task.clone());
        Ok(TaskOutcome::Renamed(task))
    }

    fn archive_task(
        &mut self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<TaskOutcome, ApplicationError> {
        let occurred_at = canonical_timestamp(occurred_at);
        self.refresh_tracking()?;
        self.tracker.ensure_archivable(id)?;
        // The operation preserves unrelated metadata, so a concurrent rename
        // survives the archive.
        match self.repository.archive_task(id, occurred_at) {
            Ok(task) => {
                self.replace_task(task.clone());
                self.refresh_after_task_operation()?;
                Ok(TaskOutcome::Archived(task))
            }
            Err(error) => {
                self.refresh_after_task_operation()?;
                match error {
                    RepositoryError::TaskIsActive { id } => {
                        Err(TrackingError::TaskIsActive { id }.into())
                    }
                    error => Err(error.into()),
                }
            }
        }
    }

    fn unarchive_task(
        &mut self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<TaskOutcome, ApplicationError> {
        let occurred_at = canonical_timestamp(occurred_at);
        // Another client may have restored this task and started tracking it
        // since our last load. Refresh before the write so a read failure
        // prevents the unarchive and Idle is never reported over an active
        // worklog that survived the call.
        self.refresh_tracking()?;
        match self.repository.unarchive_task(id, occurred_at) {
            Ok(task) => {
                self.replace_task(task.clone());
                self.refresh_after_task_operation()?;
                Ok(TaskOutcome::Unarchived(task))
            }
            Err(error) => {
                self.refresh_after_task_operation()?;
                Err(error.into())
            }
        }
    }
}

impl<R: TrackerRepository> TrackingOperations for TrackerApplication<R> {
    fn current_tracking(&self) -> &TrackingState {
        self.tracker.state()
    }

    fn set_active_task(
        &mut self,
        task_id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, ApplicationError> {
        let occurred_at = canonical_timestamp(occurred_at);
        self.refresh_tracking()?;
        if let Some(active) = self.tracker.active().cloned()
            && active.task_id == task_id
        {
            // Another client may have started this worklog after our task
            // snapshot was loaded. Keep the derived latest-work value current
            // even though the desired tracking state already exists.
            self.note_work_start(task_id, active.start);
            return Ok(SetActiveTaskOutcome::AlreadyActive { worklog: active });
        }

        let task = self
            .tasks
            .iter()
            .find(|item| item.task.id() == task_id)
            .map(|TaskListItem { task, .. }| task.clone())
            .ok_or_else(|| ApplicationError::from(RepositoryError::TaskNotFound { id: task_id }))?;
        let mut candidate = self.tracker.clone();
        let outcome = match candidate.active() {
            None => {
                let worklog = candidate.start(&task, occurred_at)?;
                match self.repository.insert_worklog(&worklog) {
                    Ok(()) => SetActiveTaskOutcome::Started { worklog },
                    Err(error) => return Err(self.recover_after_tracking_write(error)),
                }
            }
            Some(_) => {
                let SwitchedWorklogs { stopped, started } =
                    candidate.switch(&task, occurred_at, occurred_at)?;
                match self
                    .repository
                    .switch_worklog(stopped.id, occurred_at, &started)
                {
                    Ok(()) => SetActiveTaskOutcome::Switched { stopped, started },
                    Err(error) => return Err(self.recover_after_tracking_write(error)),
                }
            }
        };
        self.tracker = candidate;
        // Starting or switching work makes this task the latest work on the
        // snapshot; stopping never touches it.
        self.note_work_start(task_id, occurred_at);
        Ok(outcome)
    }

    fn clear_active_task(
        &mut self,
        expected_active: WorklogId,
        occurred_at: DateTime<Utc>,
    ) -> Result<ClearActiveTaskOutcome, ApplicationError> {
        let occurred_at = canonical_timestamp(occurred_at);
        self.refresh_tracking()?;
        let Some(active) = self.tracker.active() else {
            return Ok(ClearActiveTaskOutcome::AlreadyIdle);
        };
        if active.id != expected_active {
            return Err(ApplicationError::TrackingStateChanged);
        }

        let mut candidate = self.tracker.clone();
        let stopped = candidate.stop(occurred_at)?;
        match self.repository.stop_worklog(stopped.id, occurred_at) {
            Ok(_) => {
                self.tracker = candidate;
                Ok(ClearActiveTaskOutcome::Stopped { worklog: stopped })
            }
            Err(error) => Err(self.recover_after_tracking_write(error)),
        }
    }
}

impl<R: TrackerRepository> WorklogQueries for TrackerApplication<R> {
    fn worklogs_for_task(&self, task_id: TaskId) -> Result<Vec<Worklog>, ApplicationError> {
        Ok(self.repository.list_worklogs(task_id)?)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use tracker_domain::WorklogId;

    use super::*;

    #[derive(Default)]
    struct Data {
        tasks: Vec<TaskListItem>,
        worklogs: Vec<Worklog>,
        fail_next_write: Option<RepositoryError>,
        fail_reads: bool,
        replacement_on_failure: Option<Worklog>,
        fail_reads_after_write: bool,
        list_reads: usize,
    }

    #[derive(Clone, Default)]
    struct MemoryRepository(Rc<RefCell<Data>>);

    impl MemoryRepository {
        fn with_tasks(tasks: Vec<Task>) -> Self {
            let items = tasks
                .into_iter()
                .map(|task| TaskListItem {
                    task,
                    latest_work_start: None,
                })
                .collect::<Vec<_>>();
            Self(Rc::new(RefCell::new(Data {
                tasks: items,
                ..Data::default()
            })))
        }

        fn fail_next_write(&self, error: RepositoryError, replacement: Option<Worklog>) {
            let mut data = self.0.borrow_mut();
            data.fail_next_write = Some(error);
            data.replacement_on_failure = replacement;
        }

        fn fail_recovery_after_next_write(&self) {
            self.0.borrow_mut().fail_reads_after_write = true;
        }

        fn read_guard(&self) -> Result<(), RepositoryError> {
            if self.0.borrow().fail_reads {
                Err(RepositoryError::Backend {
                    message: "read failed".to_owned(),
                })
            } else {
                Ok(())
            }
        }

        fn take_write_failure(&self) -> Option<RepositoryError> {
            let mut data = self.0.borrow_mut();
            let error = data.fail_next_write.take()?;
            if let Some(replacement) = data.replacement_on_failure.take() {
                data.worklogs.retain(|worklog| worklog.end.is_some());
                data.worklogs.push(replacement);
            }
            if data.fail_reads_after_write {
                data.fail_reads = true;
                data.fail_reads_after_write = false;
            }
            Some(error)
        }
    }

    impl TaskRepository for MemoryRepository {
        fn create_task(&self, task: Task) -> Result<(), RepositoryError> {
            if let Some(error) = self.take_write_failure() {
                return Err(error);
            }
            let mut data = self.0.borrow_mut();
            if data.tasks.iter().any(|item| item.task.id() == task.id()) {
                return Err(RepositoryError::TaskAlreadyExists { id: task.id() });
            }
            data.tasks.push(TaskListItem {
                task,
                latest_work_start: None,
            });
            data.tasks.sort_by_key(|a| a.task.id());
            Ok(())
        }

        fn find_task(&self, id: TaskId) -> Result<Option<Task>, RepositoryError> {
            self.read_guard()?;
            Ok(self
                .0
                .borrow()
                .tasks
                .iter()
                .find(|item| item.task.id() == id)
                .map(|item| item.task.clone()))
        }

        fn list_task_items(&self) -> Result<Vec<TaskListItem>, RepositoryError> {
            self.read_guard()?;
            let mut data = self.0.borrow_mut();
            data.list_reads += 1;
            let mut items = data.tasks.clone();
            items.sort_by_key(|a| a.task.id());
            Ok(items)
        }

        fn rename_task(
            &self,
            id: TaskId,
            name: TaskName,
            occurred_at: DateTime<Utc>,
        ) -> Result<Task, RepositoryError> {
            if let Some(error) = self.take_write_failure() {
                return Err(error);
            }
            let mut data = self.0.borrow_mut();
            let index = data
                .tasks
                .iter()
                .position(|item| item.task.id() == id)
                .ok_or(RepositoryError::TaskNotFound { id })?;
            // The domain method carries the semantics the storage statement
            // implements: an equal name changes nothing, and a real rename
            // never moves `updated_at` backward.
            data.tasks[index].task.rename(name, occurred_at);
            Ok(data.tasks[index].task.clone())
        }

        fn archive_task(
            &self,
            id: TaskId,
            occurred_at: DateTime<Utc>,
        ) -> Result<Task, RepositoryError> {
            if let Some(error) = self.take_write_failure() {
                return Err(error);
            }
            let mut data = self.0.borrow_mut();
            let index = data
                .tasks
                .iter()
                .position(|item| item.task.id() == id)
                .ok_or(RepositoryError::TaskNotFound { id })?;
            // The archive trigger's rule: a running task cannot archive. An
            // already archived task cannot run, so the idempotent case
            // passes.
            if !data.tasks[index].task.is_archived()
                && data
                    .worklogs
                    .iter()
                    .any(|worklog| worklog.task_id == id && worklog.end.is_none())
            {
                return Err(RepositoryError::TaskIsActive { id });
            }
            data.tasks[index].task.archive(occurred_at);
            Ok(data.tasks[index].task.clone())
        }

        fn unarchive_task(
            &self,
            id: TaskId,
            occurred_at: DateTime<Utc>,
        ) -> Result<Task, RepositoryError> {
            if let Some(error) = self.take_write_failure() {
                return Err(error);
            }
            let mut data = self.0.borrow_mut();
            let index = data
                .tasks
                .iter()
                .position(|item| item.task.id() == id)
                .ok_or(RepositoryError::TaskNotFound { id })?;
            data.tasks[index].task.restore(occurred_at);
            Ok(data.tasks[index].task.clone())
        }
    }

    impl WorklogRepository for MemoryRepository {
        fn list_worklogs(&self, task_id: TaskId) -> Result<Vec<Worklog>, RepositoryError> {
            self.read_guard()?;
            let mut worklogs = self
                .0
                .borrow()
                .worklogs
                .iter()
                .filter(|worklog| worklog.task_id == task_id)
                .cloned()
                .collect::<Vec<_>>();
            worklogs.sort_by_key(|worklog| (worklog.start, worklog.id));
            Ok(worklogs)
        }
    }

    impl TrackingRepository for MemoryRepository {
        fn insert_worklog(&self, worklog: &Worklog) -> Result<(), RepositoryError> {
            if let Some(error) = self.take_write_failure() {
                return Err(error);
            }
            let mut data = self.0.borrow_mut();
            let task = data
                .tasks
                .iter()
                .find(|item| item.task.id() == worklog.task_id)
                .ok_or(RepositoryError::TaskNotFound {
                    id: worklog.task_id,
                })?;
            if task.task.is_archived() {
                return Err(RepositoryError::TaskArchived {
                    id: worklog.task_id,
                });
            }
            if data.worklogs.iter().any(|stored| stored.id == worklog.id) {
                return Err(RepositoryError::WorklogAlreadyExists { id: worklog.id });
            }
            if worklog.end.is_none() && data.worklogs.iter().any(|stored| stored.end.is_none()) {
                return Err(RepositoryError::ActiveWorklogExists);
            }
            data.worklogs.push(worklog.clone());
            Ok(())
        }

        fn stop_worklog(
            &self,
            id: WorklogId,
            end: DateTime<Utc>,
        ) -> Result<Worklog, RepositoryError> {
            if let Some(error) = self.take_write_failure() {
                return Err(error);
            }
            let mut data = self.0.borrow_mut();
            let worklog = data
                .worklogs
                .iter_mut()
                .find(|worklog| worklog.id == id)
                .ok_or(RepositoryError::WorklogNotFound { id })?;
            if worklog.end.is_some() {
                return Err(RepositoryError::WorklogAlreadyStopped { id });
            }
            if end < worklog.start {
                return Err(RepositoryError::Constraint {
                    message: "end precedes start".to_owned(),
                });
            }
            worklog.end = Some(end);
            Ok(worklog.clone())
        }

        fn active_worklog(&self) -> Result<Option<Worklog>, RepositoryError> {
            self.read_guard()?;
            Ok(self
                .0
                .borrow()
                .worklogs
                .iter()
                .find(|worklog| worklog.end.is_none())
                .cloned())
        }

        fn switch_worklog(
            &self,
            id: WorklogId,
            stop_at: DateTime<Utc>,
            next: &Worklog,
        ) -> Result<(), RepositoryError> {
            if let Some(error) = self.take_write_failure() {
                return Err(error);
            }
            let mut data = self.0.borrow_mut();
            let Some(index) = data.worklogs.iter().position(|worklog| worklog.id == id) else {
                return Err(RepositoryError::WorklogNotFound { id });
            };
            if data.worklogs[index].end.is_some() {
                return Err(RepositoryError::WorklogAlreadyStopped { id });
            };
            let active = &data.worklogs[index];
            if stop_at < active.start {
                return Err(RepositoryError::Constraint {
                    message: "end precedes start".to_owned(),
                });
            }
            let task = data
                .tasks
                .iter()
                .find(|item| item.task.id() == next.task_id)
                .ok_or(RepositoryError::TaskNotFound { id: next.task_id })?;
            if task.task.is_archived() {
                return Err(RepositoryError::TaskArchived { id: next.task_id });
            }
            if data.worklogs.iter().any(|worklog| worklog.id == next.id) {
                return Err(RepositoryError::WorklogAlreadyExists { id: next.id });
            }
            data.worklogs[index].end = Some(stop_at);
            data.worklogs.push(next.clone());
            Ok(())
        }
    }

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).unwrap()
    }

    fn at_nanos(seconds: i64, nanos: u32) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, nanos).unwrap()
    }

    fn task(tag: u128, name: &str) -> Task {
        Task::create(
            TaskId::from_uuid(uuid::Uuid::from_u128(tag)),
            TaskName::new(name).unwrap(),
            at(100),
        )
    }

    fn stamped_task(tag: u128, name: &str, created: i64, updated: i64) -> Task {
        Task::rehydrate(
            TaskId::from_uuid(uuid::Uuid::from_u128(tag)),
            TaskName::new(name).unwrap(),
            false,
            at(created),
            at(updated),
        )
        .unwrap()
    }

    fn worklog(tag: u128, task_id: TaskId, start: i64) -> Worklog {
        Worklog::begin(
            WorklogId::from_uuid(uuid::Uuid::from_u128(tag)),
            task_id,
            at(start),
        )
    }

    fn ordered_names(
        application: &TrackerApplication<MemoryRepository>,
        ordering: TaskOrdering,
    ) -> Vec<String> {
        application
            .tasks(ordering)
            .into_iter()
            .map(|item| item.task.name().to_string())
            .collect()
    }

    #[test]
    fn the_default_ordering_is_recently_worked() {
        assert_eq!(TaskOrdering::default(), TaskOrdering::RecentlyWorked);
    }

    #[test]
    fn recently_worked_orders_by_latest_work_then_creation_then_id() {
        let one = stamped_task(1, "one", 100, 100);
        let two = stamped_task(2, "two", 300, 300);
        let three = stamped_task(3, "three", 200, 200);
        let repository =
            MemoryRepository::with_tasks(vec![one.clone(), two.clone(), three.clone()]);
        // one worked most recently, then three; two never worked.
        repository.0.borrow_mut().tasks[0].latest_work_start = Some(at(900));
        repository.0.borrow_mut().tasks[2].latest_work_start = Some(at(800));
        let application = TrackerApplication::load(repository).unwrap();

        assert_eq!(
            ordered_names(&application, TaskOrdering::RecentlyWorked),
            ["one".to_owned(), "three".to_owned(), "two".to_owned()]
        );
    }

    #[test]
    fn recently_worked_puts_tasks_without_worklogs_last() {
        let never = stamped_task(1, "never", 900, 900);
        let worked = stamped_task(2, "worked", 100, 100);
        let repository = MemoryRepository::with_tasks(vec![never, worked.clone()]);
        repository.0.borrow_mut().tasks[1].latest_work_start = Some(at(50));
        let application = TrackerApplication::load(repository).unwrap();

        // Even a task created much later stays behind any worked task.
        assert_eq!(
            ordered_names(&application, TaskOrdering::RecentlyWorked),
            ["worked".to_owned(), "never".to_owned()]
        );
    }

    #[test]
    fn ties_break_by_created_descending_then_id_ascending() {
        let early = stamped_task(3, "early", 100, 500);
        let late = stamped_task(1, "late", 400, 400);
        let same_created = stamped_task(2, "same created", 400, 600);
        let repository = MemoryRepository::with_tasks(vec![early, late, same_created]);
        // Every task shares the same latest work start, so the tie rules
        // decide the whole list.
        for item in &mut repository.0.borrow_mut().tasks {
            item.latest_work_start = Some(at(700));
        }
        let application = TrackerApplication::load(repository).unwrap();

        assert_eq!(
            ordered_names(&application, TaskOrdering::RecentlyWorked),
            [
                "late".to_owned(),
                "same created".to_owned(),
                "early".to_owned()
            ]
        );
    }

    #[test]
    fn recently_updated_orders_by_updated_then_created_then_id() {
        let one = stamped_task(1, "one", 100, 500);
        let two = stamped_task(2, "two", 500, 900);
        let three = stamped_task(3, "three", 600, 800);
        let four = stamped_task(4, "four", 600, 800);
        let five = stamped_task(5, "five", 400, 800);
        let repository =
            MemoryRepository::with_tasks(vec![one, two.clone(), three.clone(), four.clone(), five]);
        let application = TrackerApplication::load(repository).unwrap();

        assert_eq!(
            ordered_names(&application, TaskOrdering::RecentlyUpdated),
            [
                "two".to_owned(),
                "three".to_owned(),
                "four".to_owned(),
                "five".to_owned(),
                "one".to_owned()
            ]
        );
    }

    #[test]
    fn recently_created_orders_by_created_then_id() {
        let one = stamped_task(1, "one", 300, 900);
        let two = stamped_task(2, "two", 700, 700);
        let three = stamped_task(3, "three", 700, 800);
        let repository = MemoryRepository::with_tasks(vec![one, two, three]);
        let application = TrackerApplication::load(repository).unwrap();

        assert_eq!(
            ordered_names(&application, TaskOrdering::RecentlyCreated),
            ["two".to_owned(), "three".to_owned(), "one".to_owned()]
        );
    }

    #[test]
    fn load_aligns_an_active_worklog_missing_from_the_task_aggregate_read() {
        let active_task = stamped_task(1, "active", 100, 100);
        let newer_unworked = stamped_task(2, "newer unworked", 500, 500);
        let repository = MemoryRepository::with_tasks(vec![active_task.clone(), newer_unworked]);
        repository
            .0
            .borrow_mut()
            .worklogs
            .push(worklog(10, active_task.id, 900));

        let application = TrackerApplication::load(repository).unwrap();

        assert_eq!(
            ordered_names(&application, TaskOrdering::RecentlyWorked),
            ["active".to_owned(), "newer unworked".to_owned()]
        );
    }

    #[test]
    fn load_exposes_tasks_and_recovered_tracking() {
        let alpha = task(1, "alpha");
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        repository
            .0
            .borrow_mut()
            .worklogs
            .push(worklog(10, alpha.id, 100));
        let application = TrackerApplication::load(repository).unwrap();
        assert_eq!(
            application
                .tasks(TaskOrdering::RecentlyWorked)
                .into_iter()
                .map(|item| item.task)
                .collect::<Vec<_>>(),
            std::slice::from_ref(&alpha)
        );
        assert_eq!(application.task(alpha.id), Some(&alpha));
        assert_eq!(
            application.task(TaskId::from_uuid(uuid::Uuid::from_u128(99))),
            None
        );
        assert_eq!(
            application.current_tracking(),
            &TrackingState::Running {
                worklog: ActiveWorklog::begin(
                    WorklogId::from_uuid(uuid::Uuid::from_u128(10)),
                    alpha.id,
                    at(100)
                )
            }
        );
    }

    #[test]
    fn create_task_stamps_the_client_timestamp_on_both_values() {
        let repository = MemoryRepository::default();
        let mut application = TrackerApplication::load(repository.clone()).unwrap();
        let created = match application
            .create_task(TaskName::new("alpha").unwrap(), at(500))
            .unwrap()
        {
            TaskOutcome::Created(task) => task,
            other => panic!("expected create, got {other:?}"),
        };
        assert_eq!(created.created_at(), at(500));
        assert_eq!(created.updated_at(), at(500));
        assert_eq!(application.task(created.id), Some(&created));
        assert_eq!(
            application
                .tasks(TaskOrdering::RecentlyCreated)
                .into_iter()
                .map(|item| item.task)
                .collect::<Vec<_>>(),
            std::slice::from_ref(&created)
        );
    }

    #[test]
    fn client_timestamps_are_canonicalized_to_microseconds() {
        let repository = MemoryRepository::default();
        let mut application = TrackerApplication::load(repository).unwrap();
        let created = match application
            .create_task(
                TaskName::new("precise").unwrap(),
                at_nanos(100, 123_456_789),
            )
            .unwrap()
        {
            TaskOutcome::Created(task) => task,
            other => panic!("expected create, got {other:?}"),
        };
        assert_eq!(created.created_at(), at_nanos(100, 123_456_000));

        let renamed = application
            .rename_task(
                created.id,
                TaskName::new("canonical").unwrap(),
                at_nanos(100, 123_456_100),
            )
            .unwrap();
        assert!(
            matches!(renamed, TaskOutcome::Renamed(task) if task.updated_at() == at_nanos(100, 123_456_000))
        );

        let started = application
            .set_active_task(created.id, at_nanos(200, 987_654_321))
            .unwrap();
        assert!(
            matches!(started, SetActiveTaskOutcome::Started { worklog } if worklog.start == at_nanos(200, 987_654_000))
        );
    }

    #[test]
    fn task_operations_return_stored_outcomes_and_update_queries() {
        let repository = MemoryRepository::default();
        let mut application = TrackerApplication::load(repository).unwrap();
        let created = match application
            .create_task(TaskName::new("alpha").unwrap(), at(100))
            .unwrap()
        {
            TaskOutcome::Created(task) => task,
            other => panic!("expected create, got {other:?}"),
        };
        assert_eq!(application.task(created.id), Some(&created));
        let renamed = application
            .rename_task(created.id, TaskName::new("beta").unwrap(), at(200))
            .unwrap();
        assert!(
            matches!(&renamed, TaskOutcome::Renamed(task) if task.name().as_str() == "beta" && task.updated_at() == at(200))
        );
        let archived = application.archive_task(created.id, at(300)).unwrap();
        assert!(
            matches!(&archived, TaskOutcome::Archived(task) if task.is_archived() && task.updated_at() == at(300))
        );
        assert!(application.task(created.id).unwrap().is_archived());
    }

    #[test]
    fn renaming_to_the_stored_name_keeps_updated_at() {
        let alpha = stamped_task(1, "alpha", 100, 200);
        let repository = MemoryRepository::with_tasks(vec![alpha]);
        let mut application = TrackerApplication::load(repository).unwrap();

        let renamed = application
            .rename_task(
                TaskId::from_uuid(uuid::Uuid::from_u128(1)),
                TaskName::new("alpha").unwrap(),
                at(900),
            )
            .unwrap();
        match renamed {
            TaskOutcome::Renamed(task) => {
                assert_eq!(task.name().as_str(), "alpha");
                assert_eq!(
                    task.updated_at(),
                    at(200),
                    "a no-op rename does not advance"
                );
            }
            other => panic!("expected rename, got {other:?}"),
        }
        assert_eq!(
            application
                .task(TaskId::from_uuid(uuid::Uuid::from_u128(1)))
                .unwrap()
                .updated_at(),
            at(200)
        );
    }

    #[test]
    fn metadata_operations_never_move_updated_at_backward() {
        let alpha = stamped_task(1, "alpha", 100, 500);
        let repository = MemoryRepository::with_tasks(vec![alpha]);
        let mut application = TrackerApplication::load(repository).unwrap();
        let id = TaskId::from_uuid(uuid::Uuid::from_u128(1));

        let renamed = application
            .rename_task(id, TaskName::new("beta").unwrap(), at(200))
            .unwrap();
        assert!(
            matches!(&renamed, TaskOutcome::Renamed(task) if task.updated_at() == at(500)),
            "a late client clock cannot rewind updated_at"
        );
    }

    #[test]
    fn archiving_an_archived_task_and_restoring_an_active_task_are_noops() {
        let mut archived = stamped_task(1, "archived", 100, 400);
        assert!(archived.archive(at(400)));
        let active = stamped_task(2, "active", 100, 300);
        let repository = MemoryRepository::with_tasks(vec![archived, active]);
        let mut application = TrackerApplication::load(repository).unwrap();

        let again = application
            .archive_task(TaskId::from_uuid(uuid::Uuid::from_u128(1)), at(900))
            .unwrap();
        assert!(matches!(&again, TaskOutcome::Archived(task) if task.updated_at() == at(400)));
        let restore = application
            .unarchive_task(TaskId::from_uuid(uuid::Uuid::from_u128(2)), at(900))
            .unwrap();
        assert!(
            matches!(&restore, TaskOutcome::Unarchived(task) if !task.is_archived() && task.updated_at() == at(300))
        );
        assert_eq!(
            application
                .task(TaskId::from_uuid(uuid::Uuid::from_u128(1)))
                .unwrap()
                .updated_at(),
            at(400)
        );
        assert_eq!(
            application
                .task(TaskId::from_uuid(uuid::Uuid::from_u128(2)))
                .unwrap()
                .updated_at(),
            at(300)
        );
    }

    #[test]
    fn unarchive_task_restores_the_task_and_keeps_its_worklogs() {
        let alpha = task(1, "alpha");
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        repository.0.borrow_mut().worklogs.push(
            Worklog::new(
                worklog(10, alpha.id, 100).id,
                alpha.id,
                at(100),
                Some(at(150)),
            )
            .unwrap(),
        );
        let mut application = TrackerApplication::load(repository.clone()).unwrap();
        application.archive_task(alpha.id, at(200)).unwrap();

        let unarchived = application.unarchive_task(alpha.id, at(300)).unwrap();
        assert!(
            matches!(&unarchived, TaskOutcome::Unarchived(task) if !task.is_archived() && task.updated_at() == at(300))
        );
        assert!(!application.task(alpha.id).unwrap().is_archived());
        assert_eq!(application.worklogs_for_task(alpha.id).unwrap().len(), 1);
        assert!(!repository.0.borrow().tasks[0].task.is_archived());
    }

    #[test]
    fn unarchiving_a_missing_task_reports_task_not_found() {
        let repository = MemoryRepository::default();
        let mut application = TrackerApplication::load(repository).unwrap();
        let missing = TaskId::from_uuid(uuid::Uuid::from_u128(99));
        assert_eq!(
            application.unarchive_task(missing, at(100)),
            Err(ApplicationError::Repository(
                RepositoryError::TaskNotFound { id: missing }
            ))
        );
    }

    #[test]
    fn unarchive_loads_a_worklog_that_another_client_started_first() {
        let alpha = task(1, "alpha");
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        let mut application = TrackerApplication::load(repository.clone()).unwrap();
        application.archive_task(alpha.id, at(150)).unwrap();

        // A second client restores the archived task and starts tracking it
        // before the stale client issues its unarchive.
        repository
            .0
            .borrow_mut()
            .tasks
            .iter_mut()
            .find(|item| item.task.id() == alpha.id)
            .unwrap()
            .task
            .restore(at(160));
        repository
            .insert_worklog(&worklog(10, alpha.id, 150))
            .unwrap();

        let unarchived = application.unarchive_task(alpha.id, at(200)).unwrap();
        assert!(matches!(&unarchived, TaskOutcome::Unarchived(task) if !task.is_archived()));
        assert_eq!(
            application.current_tracking(),
            &TrackingState::Running {
                worklog: ActiveWorklog::begin(
                    WorklogId::from_uuid(uuid::Uuid::from_u128(10)),
                    alpha.id,
                    at(150)
                )
            }
        );
        assert!(matches!(
            repository.active_worklog(),
            Ok(Some(worklog)) if worklog.task_id == alpha.id
        ));
    }

    #[test]
    fn a_failing_tracking_refresh_prevents_the_unarchive_write() {
        let alpha = task(1, "alpha");
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        let mut application = TrackerApplication::load(repository.clone()).unwrap();
        application.archive_task(alpha.id, at(100)).unwrap();
        repository.0.borrow_mut().fail_reads = true;

        assert_eq!(
            application.unarchive_task(alpha.id, at(150)),
            Err(ApplicationError::Repository(RepositoryError::Backend {
                message: "read failed".to_owned()
            }))
        );
        assert!(repository.0.borrow().tasks[0].task.is_archived());
        assert_eq!(application.current_tracking(), &TrackingState::Idle);
    }

    #[test]
    fn unarchiving_an_unarchived_task_is_a_harmless_noop() {
        let alpha = task(1, "alpha");
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        let mut application = TrackerApplication::load(repository.clone()).unwrap();
        let first = application.unarchive_task(alpha.id, at(100)).unwrap();
        assert!(matches!(&first, TaskOutcome::Unarchived(task) if !task.is_archived()));
        assert!(!application.task(alpha.id).unwrap().is_archived());
        let second = application.unarchive_task(alpha.id, at(200)).unwrap();
        assert!(matches!(&second, TaskOutcome::Unarchived(task) if !task.is_archived()));
        assert_eq!(repository.0.borrow().tasks.len(), 1);
    }

    #[test]
    fn set_active_task_uses_the_explicit_client_timestamp() {
        let alpha = task(1, "alpha");
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        let mut application = TrackerApplication::load(repository.clone()).unwrap();
        let outcome = application.set_active_task(alpha.id, at(100)).unwrap();
        assert!(matches!(
            outcome,
            SetActiveTaskOutcome::Started { worklog } if worklog.start == at(100)
        ));
        assert_eq!(repository.active_worklog().unwrap().unwrap().start, at(100));
    }

    #[test]
    fn setting_the_active_task_again_is_a_harmless_noop() {
        let alpha = task(1, "alpha");
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        let mut application = TrackerApplication::load(repository.clone()).unwrap();
        application.set_active_task(alpha.id, at(100)).unwrap();
        let first = repository.active_worklog().unwrap().unwrap();
        let outcome = application.set_active_task(alpha.id, at(999)).unwrap();
        assert!(matches!(
            outcome,
            SetActiveTaskOutcome::AlreadyActive { worklog } if worklog.id == first.id
        ));
        assert_eq!(repository.0.borrow().worklogs.len(), 1);
    }

    #[test]
    fn adopting_another_clients_active_work_updates_recent_work_ordering() {
        let alpha = task(1, "alpha");
        let beta = task(2, "beta");
        let repository = MemoryRepository::with_tasks(vec![alpha, beta.clone()]);
        let mut application = TrackerApplication::load(repository.clone()).unwrap();
        repository
            .insert_worklog(&worklog(10, beta.id, 900))
            .unwrap();

        let outcome = application.set_active_task(beta.id, at(999)).unwrap();
        assert!(matches!(
            outcome,
            SetActiveTaskOutcome::AlreadyActive { worklog } if worklog.start == at(900)
        ));
        assert_eq!(
            ordered_names(&application, TaskOrdering::RecentlyWorked),
            ["beta".to_owned(), "alpha".to_owned()]
        );
    }

    #[test]
    fn setting_another_task_switches_at_one_atomic_boundary() {
        let alpha = task(1, "alpha");
        let beta = task(2, "beta");
        let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone()]);
        let mut application = TrackerApplication::load(repository.clone()).unwrap();
        application.set_active_task(alpha.id, at(100)).unwrap();
        let outcome = application.set_active_task(beta.id, at(150)).unwrap();
        assert!(matches!(
            outcome,
            SetActiveTaskOutcome::Switched { stopped, started }
                if stopped.end == Some(at(150)) && started.start == at(150)
        ));
        let data = repository.0.borrow();
        assert_eq!(data.worklogs.len(), 2);
        assert_eq!(
            data.worklogs
                .iter()
                .filter(|item| item.end.is_none())
                .count(),
            1
        );
        assert_eq!(
            data.worklogs
                .iter()
                .find(|item| item.task_id == alpha.id)
                .unwrap()
                .end,
            Some(at(150))
        );
    }

    #[test]
    fn clear_active_task_stops_once_and_is_then_a_harmless_noop() {
        let alpha = task(1, "alpha");
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        let mut application = TrackerApplication::load(repository.clone()).unwrap();
        application.set_active_task(alpha.id, at(100)).unwrap();
        let TrackingState::Running { worklog: active } = application.current_tracking() else {
            panic!("the worklog must be active");
        };
        let active = active.id;
        let outcome = application.clear_active_task(active, at(150)).unwrap();
        assert!(matches!(
            outcome,
            ClearActiveTaskOutcome::Stopped { worklog } if worklog.end == Some(at(150))
        ));
        assert_eq!(
            application.clear_active_task(active, at(200)).unwrap(),
            ClearActiveTaskOutcome::AlreadyIdle
        );
        assert_eq!(repository.0.borrow().worklogs.len(), 1);
    }

    #[test]
    fn stale_clear_refreshes_state_without_stopping_another_clients_worklog() {
        let alpha = task(1, "alpha");
        let beta = task(2, "beta");
        let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone()]);
        let mut first = TrackerApplication::load(repository.clone()).unwrap();
        first.set_active_task(alpha.id, at(100)).unwrap();
        let expected = match first.current_tracking() {
            TrackingState::Running { worklog } => worklog.id,
            TrackingState::Idle => panic!("alpha must be active"),
        };
        let mut second = TrackerApplication::load(repository.clone()).unwrap();
        second.set_active_task(beta.id, at(150)).unwrap();

        assert_eq!(
            first.clear_active_task(expected, at(200)),
            Err(ApplicationError::TrackingStateChanged)
        );
        assert!(matches!(
            first.current_tracking(),
            TrackingState::Running { worklog } if worklog.task_id == beta.id
        ));
        assert_eq!(
            ordered_names(&first, TaskOrdering::RecentlyWorked),
            ["beta".to_owned(), "alpha".to_owned()],
            "the adopted active worklog supplies authoritative recent activity"
        );
        assert!(matches!(
            repository.active_worklog(),
            Ok(Some(worklog)) if worklog.task_id == beta.id
        ));
    }

    #[test]
    fn tracking_never_changes_a_tasks_updated_at() {
        let alpha = stamped_task(1, "alpha", 100, 200);
        let beta = stamped_task(2, "beta", 100, 200);
        let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone()]);
        let mut application = TrackerApplication::load(repository).unwrap();

        application.set_active_task(alpha.id, at(500)).unwrap();
        application.set_active_task(beta.id, at(600)).unwrap();
        application
            .clear_active_task(
                match application.current_tracking() {
                    TrackingState::Running { worklog } => worklog.id,
                    TrackingState::Idle => panic!("beta must be active"),
                },
                at(700),
            )
            .unwrap();

        assert_eq!(application.task(alpha.id).unwrap().updated_at(), at(200));
        assert_eq!(application.task(beta.id).unwrap().updated_at(), at(200));
    }

    #[test]
    fn start_and_switch_reorder_recently_worked_but_stop_does_not() {
        let alpha = stamped_task(1, "alpha", 100, 100);
        let beta = stamped_task(2, "beta", 200, 200);
        let gamma = stamped_task(3, "gamma", 300, 300);
        let repository =
            MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone(), gamma.clone()]);
        let mut application = TrackerApplication::load(repository.clone()).unwrap();
        // No work has happened yet, so created_at descending decides:
        // gamma first, alpha last.
        assert_eq!(
            ordered_names(&application, TaskOrdering::RecentlyWorked),
            ["gamma".to_owned(), "beta".to_owned(), "alpha".to_owned()]
        );

        // Starting alpha works it most recently: alpha rises to the top.
        application.set_active_task(alpha.id, at(500)).unwrap();
        assert_eq!(
            ordered_names(&application, TaskOrdering::RecentlyWorked),
            ["alpha".to_owned(), "gamma".to_owned(), "beta".to_owned()]
        );

        // Switching to beta makes beta the latest work: beta rises above
        // alpha without a backend read.
        let list_reads_before = repository.0.borrow().list_reads;
        application.set_active_task(beta.id, at(600)).unwrap();
        assert_eq!(
            ordered_names(&application, TaskOrdering::RecentlyWorked),
            ["beta".to_owned(), "alpha".to_owned(), "gamma".to_owned()]
        );
        assert_eq!(
            repository.0.borrow().list_reads,
            list_reads_before,
            "the snapshot reordered without rereading the backend"
        );

        // Stopping changes nothing about the ordering.
        application
            .clear_active_task(
                match application.current_tracking() {
                    TrackingState::Running { worklog } => worklog.id,
                    TrackingState::Idle => panic!("beta must be active"),
                },
                at(700),
            )
            .unwrap();
        assert_eq!(
            ordered_names(&application, TaskOrdering::RecentlyWorked),
            ["beta".to_owned(), "alpha".to_owned(), "gamma".to_owned()]
        );
    }

    #[test]
    fn a_committed_write_never_triggers_a_fallible_backend_read() {
        let alpha = stamped_task(1, "alpha", 100, 100);
        let repository = MemoryRepository::with_tasks(vec![alpha]);
        let mut application = TrackerApplication::load(repository.clone()).unwrap();
        let list_reads_after_load = repository.0.borrow().list_reads;

        application
            .create_task(TaskName::new("beta").unwrap(), at(200))
            .unwrap();
        application
            .rename_task(
                TaskId::from_uuid(uuid::Uuid::from_u128(1)),
                TaskName::new("renamed").unwrap(),
                at(300),
            )
            .unwrap();
        application
            .set_active_task(TaskId::from_uuid(uuid::Uuid::from_u128(1)), at(400))
            .unwrap();
        assert_eq!(
            repository.0.borrow().list_reads,
            list_reads_after_load,
            "successful writes update the snapshot without another list read"
        );
        // The snapshot still reflects every write.
        let names: Vec<String> = application
            .tasks(TaskOrdering::RecentlyWorked)
            .into_iter()
            .map(|item| item.task.name().to_string())
            .collect();
        assert_eq!(names, ["renamed".to_owned(), "beta".to_owned()]);
    }

    #[test]
    fn recovery_errors_keep_the_repository_error_as_their_source() {
        use std::error::Error as _;

        let error = ApplicationError::TrackingWrite(RepositoryError::Backend {
            message: "write failed".to_owned(),
        });
        assert!(error.source().is_some());
        let error = ApplicationError::TrackingRecovery(RepositoryError::Backend {
            message: "reload failed".to_owned(),
        });
        assert!(error.source().is_some());
        let error = ApplicationError::TaskRecovery(RepositoryError::Backend {
            message: "task reload failed".to_owned(),
        });
        assert!(error.source().is_some());
    }

    #[test]
    fn memory_repository_honors_worklog_ordering_and_write_contracts() {
        let alpha = task(1, "alpha");
        let mut archived = task(2, "archived");
        assert!(archived.archive(at(100)));
        let repository = MemoryRepository::with_tasks(vec![archived.clone(), alpha.clone()]);
        let later =
            Worklog::new(worklog(2, alpha.id, 20).id, alpha.id, at(20), Some(at(30))).unwrap();
        let earlier =
            Worklog::new(worklog(1, alpha.id, 10).id, alpha.id, at(10), Some(at(15))).unwrap();
        repository.insert_worklog(&later).unwrap();
        repository.insert_worklog(&earlier).unwrap();
        assert_eq!(
            repository
                .list_worklogs(alpha.id)
                .unwrap()
                .into_iter()
                .map(|worklog| worklog.id)
                .collect::<Vec<_>>(),
            vec![earlier.id, later.id]
        );
        assert!(matches!(
            repository.insert_worklog(&worklog(3, archived.id, 40)),
            Err(RepositoryError::TaskArchived { id }) if id == archived.id
        ));
        let active = worklog(4, alpha.id, 50);
        repository.insert_worklog(&active).unwrap();
        assert!(matches!(
            repository.stop_worklog(active.id, at(49)),
            Err(RepositoryError::Constraint { .. })
        ));
        let next = worklog(5, archived.id, 60);
        assert!(matches!(
            repository.switch_worklog(active.id, at(55), &next),
            Err(RepositoryError::TaskArchived { id }) if id == archived.id
        ));
        assert!(matches!(
            repository.active_worklog(),
            Ok(Some(worklog)) if worklog.id == active.id
        ));
    }

    #[test]
    fn worklog_queries_read_the_backend_without_building_a_snapshot() {
        let alpha = task(1, "alpha");
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        let application = TrackerApplication::load(repository.clone()).unwrap();
        repository
            .0
            .borrow_mut()
            .worklogs
            .push(worklog(10, alpha.id, 100));
        assert_eq!(application.worklogs_for_task(alpha.id).unwrap().len(), 1);
        repository.0.borrow_mut().worklogs.clear();
        assert!(application.worklogs_for_task(alpha.id).unwrap().is_empty());
    }

    #[test]
    fn tracking_write_conflicts_reload_authoritative_state() {
        let alpha = task(1, "alpha");
        let beta = task(2, "beta");
        let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone()]);
        let mut application = TrackerApplication::load(repository.clone()).unwrap();
        let foreign = worklog(99, beta.id, 90);
        repository.fail_next_write(RepositoryError::ActiveWorklogExists, Some(foreign.clone()));

        let error = application
            .set_active_task(alpha.id, at(100))
            .expect_err("the simulated conflict must fail");
        assert_eq!(
            error,
            ApplicationError::TrackingWrite(RepositoryError::ActiveWorklogExists)
        );
        assert_eq!(
            application.current_tracking(),
            &TrackingState::Running {
                worklog: ActiveWorklog::begin(foreign.id, foreign.task_id, foreign.start)
            }
        );
        // The authoritative active start is aligned with the aggregate read,
        // even when the simulated list read did not include the new worklog.
        assert_eq!(
            ordered_names(&application, TaskOrdering::RecentlyWorked),
            ["beta".to_owned(), "alpha".to_owned()]
        );
    }

    #[test]
    fn a_failed_authoritative_reload_is_reported_separately() {
        let alpha = task(1, "alpha");
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        let mut application = TrackerApplication::load(repository.clone()).unwrap();
        repository.fail_next_write(RepositoryError::ActiveWorklogExists, None);
        repository.fail_recovery_after_next_write();
        let error = application
            .set_active_task(alpha.id, at(100))
            .expect_err("write and recovery must fail");
        assert_eq!(
            error,
            ApplicationError::TrackingRecovery(RepositoryError::Backend {
                message: "read failed".to_owned()
            })
        );
        assert_eq!(application.current_tracking(), &TrackingState::Idle);
    }

    #[test]
    fn archiving_the_active_task_is_rejected_before_persistence() {
        let alpha = task(1, "alpha");
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        let mut application = TrackerApplication::load(repository).unwrap();
        application.set_active_task(alpha.id, at(100)).unwrap();
        assert_eq!(
            application.archive_task(alpha.id, at(150)),
            Err(ApplicationError::Domain(TrackingError::TaskIsActive {
                id: alpha.id
            }))
        );
        assert!(!application.task(alpha.id).unwrap().is_archived());
    }

    #[test]
    fn rename_task_never_moves_stored_updated_at_backward() {
        let alpha = stamped_task(1, "alpha", 100, 500);
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        let saved = repository
            .rename_task(alpha.id, TaskName::new("older clock").unwrap(), at(200))
            .unwrap();
        assert_eq!(
            saved.updated_at(),
            at(500),
            "the backend keeps the newer value"
        );
        assert_eq!(saved.name().as_str(), "older clock");
        assert_eq!(saved.created_at(), at(100), "creation is never rewritten");
    }

    #[test]
    fn renaming_a_missing_task_reports_task_not_found() {
        let repository = MemoryRepository::default();
        let missing = TaskId::from_uuid(uuid::Uuid::from_u128(99));
        assert_eq!(
            repository.rename_task(missing, TaskName::new("ghost").unwrap(), at(100)),
            Err(RepositoryError::TaskNotFound { id: missing })
        );
    }

    #[test]
    fn renaming_after_a_concurrent_archive_keeps_the_archived_state() {
        let alpha = stamped_task(1, "alpha", 100, 100);
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        // Another client archives the task after this client last looked.
        repository.archive_task(alpha.id, at(150)).unwrap();

        // The stale client renames from its unarchived view: the rename
        // lands and the archive state survives it.
        let renamed = repository
            .rename_task(alpha.id, TaskName::new("beta").unwrap(), at(200))
            .unwrap();
        assert_eq!(renamed.name().as_str(), "beta");
        assert!(
            renamed.is_archived(),
            "the rename did not resurrect the task"
        );
    }

    #[test]
    fn archiving_after_a_concurrent_rename_keeps_the_name() {
        let alpha = stamped_task(1, "alpha", 100, 100);
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        repository
            .rename_task(alpha.id, TaskName::new("beta").unwrap(), at(150))
            .unwrap();

        let archived = repository.archive_task(alpha.id, at(200)).unwrap();
        assert!(archived.is_archived());
        assert_eq!(
            archived.name().as_str(),
            "beta",
            "the archive kept the rename"
        );
        assert_eq!(archived.updated_at(), at(200));
    }
}
