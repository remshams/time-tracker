//! Backend-neutral application use cases for tasks and time tracking.

mod repository;

use chrono::{DateTime, Utc};
use tracker_domain::{
    ActiveWorklog, SwitchedWorklogs, Task, TaskId, TaskName, Tracker, TrackingError, TrackingState,
    Worklog, WorklogCorrectionError, WorklogId, WorklogTimes,
};

pub use repository::{
    RepositoryError, TaskRepository, TrackerRepository, TrackerSnapshot, TrackingRepository,
    WorklogCorrection, WorklogRepository,
};

/// Canonicalizes a client timestamp to the microsecond precision shared by
/// every current persistence adapter.
fn canonical_timestamp(timestamp: DateTime<Utc>) -> DateTime<Utc> {
    DateTime::from_timestamp_micros(timestamp.timestamp_micros())
        .expect("every UTC timestamp fits the canonical microsecond range")
}

fn canonical_worklog_times(times: WorklogTimes) -> WorklogTimes {
    WorklogTimes::new(
        canonical_timestamp(times.start()),
        times.end().map(canonical_timestamp),
    )
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

/// How many worklogs one history page carries.
///
/// The size is an application constant, not a caller argument, so no caller
/// can request an unbounded page.
pub const WORKLOG_PAGE_SIZE: usize = 50;

/// The position of one worklog in the history order, used to fetch the next
/// page after it.
///
/// History is ordered by start descending, then `WorklogId` ascending as the
/// deterministic tie-break, so a cursor carries both values. Rows that share
/// a start are ordered and bounded by identifier, which is what keeps page
/// boundaries from repeating or dropping them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorklogCursor {
    /// The task whose history produced this cursor.
    pub task_id: TaskId,
    /// The start time of the page's last worklog, in UTC.
    pub start: DateTime<Utc>,
    /// The identifier of the page's last worklog.
    pub id: WorklogId,
    /// The task history ordering revision that produced this cursor.
    pub revision: i64,
}

/// The bounded state read with one worklog-history page.
///
/// This contains only the aggregates that page adoption can change: the
/// requested task and, when tracking is active, the active task. It avoids
/// materializing every task and worklog aggregate for each history page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorklogPageSnapshot {
    /// The requested task's authoritative latest worklog start.
    pub requested_task_latest_work_start: Option<DateTime<Utc>>,
    /// The global active worklog, if one exists.
    pub active_worklog: Option<Worklog>,
    /// The active task's authoritative latest worklog start. It is present
    /// when `active_worklog` is present, including when it is the requested
    /// task.
    pub active_task_latest_work_start: Option<DateTime<Utc>>,
}

/// One bounded page of a task's worklog history.
///
/// Worklogs appear in history order: start descending, then `WorklogId`
/// ascending. Active worklogs are part of the history. `snapshot` comes from
/// the same backend read as the page. A `next_cursor` of `None` means the page
/// reached the end of the history; it does not mean the page was full.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorklogPage {
    /// The worklogs of this page, in history order.
    pub worklogs: Vec<Worklog>,
    /// Bounded aggregate and tracking state read with this page.
    pub snapshot: WorklogPageSnapshot,
    /// The cursor to pass for the next page, or `None` at the end of the
    /// history.
    pub next_cursor: Option<WorklogCursor>,
}

/// Why an application operation failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApplicationError {
    #[error(transparent)]
    Domain(#[from] TrackingError),
    #[error(transparent)]
    InvalidWorklogCorrection(#[from] WorklogCorrectionError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
    #[error("tracking write failed: {0}")]
    TrackingWrite(#[source] RepositoryError),
    #[error("tracking state could not be recovered: {0}")]
    TrackingRecovery(#[source] RepositoryError),
    #[error("task state could not be recovered: {0}")]
    TaskRecovery(#[source] RepositoryError),
    #[error("worklog correction write failed: {write}")]
    WorklogCorrectionWrite {
        #[source]
        write: RepositoryError,
    },
    #[error("worklog correction write failed: {write}; state recovery failed: {recovery}")]
    WorklogCorrectionRecovery {
        #[source]
        write: RepositoryError,
        recovery: RepositoryError,
    },
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

/// The result of correcting one worklog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorrectWorklogOutcome {
    Corrected { worklog: Worklog },
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

/// Queries for worklog history. Each call adopts the task and tracking
/// snapshot read with its page before returning.
pub trait WorklogQueries {
    /// One bounded page of the task's worklog history, continuing strictly
    /// after the optional cursor. The page size is the fixed
    /// [`WORKLOG_PAGE_SIZE`]; callers cannot request more.
    fn worklogs_for_task(
        &mut self,
        task_id: TaskId,
        after: Option<&WorklogCursor>,
    ) -> Result<WorklogPage, ApplicationError>;
}

/// Commands that correct existing worklogs.
pub trait WorklogOperations {
    /// Replaces a worklog's timestamps if the stored timestamps still match
    /// `expected`.
    ///
    /// The operation canonicalizes expected, replacement, and operation
    /// timestamps to UTC microseconds. It cannot change the worklog's
    /// identity, task, or active state. Stale and failed writes return an
    /// error after an authoritative state reload, so callers can keep their
    /// own replacement draft.
    fn correct_worklog(
        &mut self,
        id: WorklogId,
        expected: WorklogTimes,
        replacement: WorklogTimes,
        occurred_at: DateTime<Utc>,
    ) -> Result<CorrectWorklogOutcome, ApplicationError>;
}

/// The application service required by presentation and transport clients.
///
/// This keeps repository ports behind the application boundary.
pub trait TrackerApplicationService:
    TaskQueries + TaskOperations + TrackingOperations + WorklogQueries + WorklogOperations
{
}

impl<T> TrackerApplicationService for T where
    T: TaskQueries + TaskOperations + TrackingOperations + WorklogQueries + WorklogOperations
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
    /// Loads task and tracking state from one coherent backend snapshot.
    pub fn load(repository: R) -> Result<Self, ApplicationError> {
        let snapshot = repository.tracker_snapshot()?;
        let (tasks, tracker) = Self::state_from_snapshot(snapshot)?;
        Ok(Self {
            repository,
            tasks,
            tracker,
        })
    }

    fn state_from_snapshot(
        mut snapshot: TrackerSnapshot,
    ) -> Result<(Vec<TaskListItem>, Tracker), ApplicationError> {
        snapshot.task_items.sort_by_key(|item| item.task.id());
        let tracker = match snapshot.active_worklog {
            Some(worklog) => Tracker::resume(worklog)?,
            None => Tracker::idle(),
        };
        Self::align_active_work(&mut snapshot.task_items, &tracker);
        Ok((snapshot.task_items, tracker))
    }

    fn align_active_work(items: &mut [TaskListItem], tracker: &Tracker) {
        if let Some(active) = tracker.active()
            && let Some(item) = items
                .iter_mut()
                .find(|item| item.task.id() == active.task_id())
        {
            item.latest_work_start = Some(
                item.latest_work_start
                    .map_or(active.start(), |old| old.max(active.start())),
            );
        }
    }

    fn set_latest_work_start(&mut self, task_id: TaskId, latest_work_start: Option<DateTime<Utc>>) {
        if let Some(item) = self.tasks.iter_mut().find(|item| item.task.id() == task_id) {
            item.latest_work_start = latest_work_start;
        }
    }

    fn adopt_worklog_page_snapshot(
        &mut self,
        task_id: TaskId,
        snapshot: WorklogPageSnapshot,
    ) -> Result<(), ApplicationError> {
        self.set_latest_work_start(task_id, snapshot.requested_task_latest_work_start);
        self.tracker = match snapshot.active_worklog {
            Some(worklog) => Tracker::resume(worklog)?,
            None => Tracker::idle(),
        };
        if let Some(active) = self.tracker.active() {
            self.set_latest_work_start(active.task_id(), snapshot.active_task_latest_work_start);
        }
        Ok(())
    }

    fn adopt_snapshot(&mut self, snapshot: TrackerSnapshot) -> Result<(), ApplicationError> {
        let (tasks, tracker) = Self::state_from_snapshot(snapshot)?;
        self.tasks = tasks;
        self.tracker = tracker;
        Ok(())
    }

    fn refresh_tracking(&mut self) -> Result<(), ApplicationError> {
        self.adopt_snapshot(self.repository.tracker_snapshot()?)
    }

    fn reload_authoritative_state(&mut self) -> Result<(), RepositoryError> {
        let snapshot = self.repository.tracker_snapshot()?;
        self.adopt_snapshot(snapshot)
            .map_err(|_| RepositoryError::CorruptData {
                field: "active worklog",
            })
    }

    fn recover_after_tracking_write(&mut self, write_error: RepositoryError) -> ApplicationError {
        match self.reload_authoritative_state() {
            Ok(()) if matches!(write_error, RepositoryError::WorklogChanged { .. }) => {
                ApplicationError::TrackingStateChanged
            }
            Ok(()) => ApplicationError::TrackingWrite(write_error),
            Err(error) => ApplicationError::TrackingRecovery(error),
        }
    }

    fn recover_after_worklog_correction(&mut self, write: RepositoryError) -> ApplicationError {
        match self.reload_authoritative_state() {
            Ok(()) => ApplicationError::WorklogCorrectionWrite { write },
            Err(recovery) => ApplicationError::WorklogCorrectionRecovery { write, recovery },
        }
    }

    fn adopt_worklog_correction(
        &mut self,
        correction: WorklogCorrection,
    ) -> Result<Worklog, ApplicationError> {
        self.set_latest_work_start(
            correction.worklog.task_id(),
            correction.task_latest_work_start,
        );
        let worklog = correction.worklog;
        self.tracker = match correction.active_worklog {
            Some(worklog) => Tracker::resume(worklog)?,
            None => Tracker::idle(),
        };
        if let Some(active) = self.tracker.active() {
            self.set_latest_work_start(active.task_id(), correction.active_task_latest_work_start);
        }
        Ok(worklog)
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
        let cached_active = self.tracker.active().cloned();
        self.refresh_tracking()?;
        if cached_active.as_ref().is_some_and(|cached| {
            self.tracker.active().is_some_and(|active| {
                active.id() == cached.id() && active.start() != cached.start()
            })
        }) {
            return Err(ApplicationError::TrackingStateChanged);
        }
        if let Some(active) = self.tracker.active().cloned()
            && active.task_id() == task_id
        {
            // Another client may have started this worklog after our task
            // snapshot was loaded. Keep the derived latest-work value current
            // even though the desired tracking state already exists.
            self.note_work_start(task_id, active.start());
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
                match self.repository.switch_worklog(
                    stopped.id(),
                    stopped.start(),
                    occurred_at,
                    &started,
                ) {
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
        let cached_active = self.tracker.active().cloned();
        self.refresh_tracking()?;
        if cached_active.as_ref().is_some_and(|cached| {
            self.tracker.active().is_some_and(|active| {
                active.id() == cached.id() && active.start() != cached.start()
            })
        }) {
            return Err(ApplicationError::TrackingStateChanged);
        }
        let Some(active) = self.tracker.active() else {
            return Ok(ClearActiveTaskOutcome::AlreadyIdle);
        };
        if active.id() != expected_active {
            return Err(ApplicationError::TrackingStateChanged);
        }

        let mut candidate = self.tracker.clone();
        let stopped = candidate.stop(occurred_at)?;
        match self
            .repository
            .stop_worklog(stopped.id(), stopped.start(), occurred_at)
        {
            Ok(_) => {
                self.tracker = candidate;
                Ok(ClearActiveTaskOutcome::Stopped { worklog: stopped })
            }
            Err(error) => Err(self.recover_after_tracking_write(error)),
        }
    }
}

impl<R: TrackerRepository> WorklogQueries for TrackerApplication<R> {
    fn worklogs_for_task(
        &mut self,
        task_id: TaskId,
        after: Option<&WorklogCursor>,
    ) -> Result<WorklogPage, ApplicationError> {
        let page = self.repository.worklog_page(task_id, after)?;
        self.adopt_worklog_page_snapshot(task_id, page.snapshot.clone())?;
        Ok(page)
    }
}

impl<R: TrackerRepository> WorklogOperations for TrackerApplication<R> {
    fn correct_worklog(
        &mut self,
        id: WorklogId,
        expected: WorklogTimes,
        replacement: WorklogTimes,
        occurred_at: DateTime<Utc>,
    ) -> Result<CorrectWorklogOutcome, ApplicationError> {
        let expected = canonical_worklog_times(expected);
        let replacement = canonical_worklog_times(replacement);
        let occurred_at = canonical_timestamp(occurred_at);

        let current = match self.repository.find_worklog(id) {
            Ok(Some(worklog)) => worklog,
            Ok(None) => {
                return Err(
                    self.recover_after_worklog_correction(RepositoryError::WorklogNotFound { id })
                );
            }
            Err(error) => return Err(self.recover_after_worklog_correction(error)),
        };
        if current.times() != expected {
            return Err(
                self.recover_after_worklog_correction(RepositoryError::WorklogChanged { id })
            );
        }
        current.corrected(replacement, occurred_at)?;

        let correction =
            match self
                .repository
                .compare_and_set_worklog_times(id, expected, replacement)
            {
                Ok(correction) => correction,
                Err(error) => return Err(self.recover_after_worklog_correction(error)),
            };
        let worklog = self.adopt_worklog_correction(correction)?;
        Ok(CorrectWorklogOutcome::Corrected { worklog })
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
        fail_reads_after_task_write: bool,
        hide_next_active_read: bool,
        list_reads: usize,
        history_revisions: Vec<(TaskId, i64)>,
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

        fn fail_refresh_after_next_task_write(&self) {
            self.0.borrow_mut().fail_reads_after_task_write = true;
        }

        fn hide_next_active_read(&self) {
            self.0.borrow_mut().hide_next_active_read = true;
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

        fn snapshot(data: &Data) -> TrackerSnapshot {
            let mut task_items = data.tasks.clone();
            for item in &mut task_items {
                item.latest_work_start = data
                    .worklogs
                    .iter()
                    .filter(|worklog| worklog.task_id() == item.task.id())
                    .map(Worklog::start)
                    .max();
            }
            TrackerSnapshot {
                task_items,
                active_worklog: data
                    .worklogs
                    .iter()
                    .find(|worklog| worklog.is_active())
                    .cloned(),
            }
        }

        fn revision(data: &Data, task_id: TaskId) -> i64 {
            data.history_revisions
                .iter()
                .find(|(id, _)| *id == task_id)
                .map_or(0, |(_, revision)| *revision)
        }

        fn bump_revision(data: &mut Data, task_id: TaskId) {
            if let Some((_, revision)) = data
                .history_revisions
                .iter_mut()
                .find(|(id, _)| *id == task_id)
            {
                *revision += 1;
            } else {
                data.history_revisions.push((task_id, 1));
            }
        }

        fn take_write_failure(&self) -> Option<RepositoryError> {
            let mut data = self.0.borrow_mut();
            let error = data.fail_next_write.take()?;
            if let Some(replacement) = data.replacement_on_failure.take() {
                data.worklogs.retain(|worklog| !worklog.is_active());
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
            for item in &mut items {
                item.latest_work_start = data
                    .worklogs
                    .iter()
                    .filter(|worklog| worklog.task_id() == item.task.id())
                    .map(Worklog::start)
                    .max();
            }
            items.sort_by_key(|item| item.task.id());
            Ok(items)
        }

        fn tracker_snapshot(&self) -> Result<TrackerSnapshot, RepositoryError> {
            self.read_guard()?;
            Ok(Self::snapshot(&self.0.borrow()))
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
                    .any(|worklog| worklog.task_id() == id && worklog.is_active())
            {
                return Err(RepositoryError::TaskIsActive { id });
            }
            data.tasks[index].task.archive(occurred_at);
            let task = data.tasks[index].task.clone();
            if data.fail_reads_after_task_write {
                data.fail_reads = true;
                data.fail_reads_after_task_write = false;
            }
            Ok(task)
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
            let task = data.tasks[index].task.clone();
            if data.fail_reads_after_task_write {
                data.fail_reads = true;
                data.fail_reads_after_task_write = false;
            }
            Ok(task)
        }
    }

    fn intervals_overlap(left: &Worklog, right: &Worklog) -> bool {
        if left.end() == Some(left.start()) || right.end() == Some(right.start()) {
            return false;
        }
        let left_starts_before_right_ends = right.end().is_none_or(|end| left.start() < end);
        let right_starts_before_left_ends = left.end().is_none_or(|end| right.start() < end);
        left_starts_before_right_ends && right_starts_before_left_ends
    }

    fn has_same_task_overlap(
        worklogs: &[Worklog],
        candidate: &Worklog,
        excluded: Option<WorklogId>,
    ) -> bool {
        worklogs.iter().any(|stored| {
            Some(stored.id()) != excluded
                && stored.task_id() == candidate.task_id()
                && intervals_overlap(stored, candidate)
        })
    }

    impl WorklogRepository for MemoryRepository {
        fn find_worklog(&self, id: WorklogId) -> Result<Option<Worklog>, RepositoryError> {
            self.read_guard()?;
            Ok(self
                .0
                .borrow()
                .worklogs
                .iter()
                .find(|worklog| worklog.id() == id)
                .cloned())
        }

        fn compare_and_set_worklog_times(
            &self,
            id: WorklogId,
            expected: WorklogTimes,
            replacement: WorklogTimes,
        ) -> Result<WorklogCorrection, RepositoryError> {
            if let Some(error) = self.take_write_failure() {
                return Err(error);
            }
            let mut data = self.0.borrow_mut();
            let index = data
                .worklogs
                .iter()
                .position(|worklog| worklog.id() == id)
                .ok_or(RepositoryError::WorklogNotFound { id })?;
            let stored = &data.worklogs[index];
            let stored_start = stored.start();
            if stored.times() != expected {
                return Err(RepositoryError::WorklogChanged { id });
            }
            if stored.is_active() != replacement.is_active() {
                return Err(RepositoryError::Constraint {
                    message: "a correction must preserve active state".to_owned(),
                });
            }
            let corrected = Worklog::new(
                stored.id(),
                stored.task_id(),
                replacement.start(),
                replacement.end(),
            )
            .map_err(|error| RepositoryError::Constraint {
                message: error.to_string(),
            })?;
            if has_same_task_overlap(&data.worklogs, &corrected, Some(id)) {
                return Err(RepositoryError::SameTaskWorklogOverlap { id });
            }
            if corrected.start() != stored_start {
                Self::bump_revision(&mut data, corrected.task_id());
            }
            data.worklogs[index] = corrected.clone();
            let task_latest_work_start = data
                .worklogs
                .iter()
                .filter(|worklog| worklog.task_id() == corrected.task_id())
                .map(Worklog::start)
                .max();
            let active_worklog = data
                .worklogs
                .iter()
                .find(|worklog| worklog.is_active())
                .cloned();
            let active_task_latest_work_start = match active_worklog.as_ref() {
                Some(active) if active.task_id() == corrected.task_id() => task_latest_work_start,
                Some(active) => data
                    .worklogs
                    .iter()
                    .filter(|worklog| worklog.task_id() == active.task_id())
                    .map(Worklog::start)
                    .max(),
                None => None,
            };
            Ok(WorklogCorrection {
                worklog: corrected,
                task_latest_work_start,
                active_worklog,
                active_task_latest_work_start,
            })
        }

        fn worklog_page(
            &self,
            task_id: TaskId,
            after: Option<&WorklogCursor>,
        ) -> Result<WorklogPage, RepositoryError> {
            self.read_guard()?;
            let data = self.0.borrow();
            let revision = Self::revision(&data, task_id);
            if after.is_some_and(|cursor| cursor.task_id != task_id || cursor.revision != revision)
            {
                return Err(RepositoryError::WorklogHistoryChanged { task_id });
            }
            let active_worklog = data
                .worklogs
                .iter()
                .find(|worklog| worklog.is_active())
                .cloned();
            let requested_task_latest_work_start = data
                .worklogs
                .iter()
                .filter(|worklog| worklog.task_id() == task_id)
                .map(Worklog::start)
                .max();
            let active_task_latest_work_start = active_worklog.as_ref().and_then(|active| {
                if active.task_id() == task_id {
                    requested_task_latest_work_start
                } else {
                    data.worklogs
                        .iter()
                        .filter(|worklog| worklog.task_id() == active.task_id())
                        .map(Worklog::start)
                        .max()
                }
            });
            let mut worklogs = data
                .worklogs
                .iter()
                .filter(|worklog| worklog.task_id() == task_id)
                .cloned()
                .collect::<Vec<_>>();
            // History order: start descending, then identifier ascending.
            worklogs.sort_by_key(|worklog| (std::cmp::Reverse(worklog.start()), worklog.id()));
            let first = match after {
                None => 0,
                Some(cursor) => worklogs
                    .iter()
                    .position(|worklog| {
                        (std::cmp::Reverse(worklog.start()), worklog.id())
                            > (std::cmp::Reverse(cursor.start), cursor.id)
                    })
                    .unwrap_or(worklogs.len()),
            };
            // One worklog past the page size reveals whether a next page
            // follows; the extra worklog is dropped again before returning.
            let overshot = worklogs
                .into_iter()
                .skip(first)
                .take(WORKLOG_PAGE_SIZE + 1)
                .collect::<Vec<_>>();
            let has_next = overshot.len() > WORKLOG_PAGE_SIZE;
            let worklogs = overshot
                .into_iter()
                .take(WORKLOG_PAGE_SIZE)
                .collect::<Vec<_>>();
            let next_cursor = has_next.then(|| {
                let last = worklogs.last().expect("a full page has a last worklog");
                WorklogCursor {
                    task_id,
                    start: last.start(),
                    id: last.id(),
                    revision,
                }
            });
            Ok(WorklogPage {
                worklogs,
                snapshot: WorklogPageSnapshot {
                    requested_task_latest_work_start,
                    active_worklog,
                    active_task_latest_work_start,
                },
                next_cursor,
            })
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
                .find(|item| item.task.id() == worklog.task_id())
                .ok_or(RepositoryError::TaskNotFound {
                    id: worklog.task_id(),
                })?;
            if task.task.is_archived() {
                return Err(RepositoryError::TaskArchived {
                    id: worklog.task_id(),
                });
            }
            if data
                .worklogs
                .iter()
                .any(|stored| stored.id() == worklog.id())
            {
                return Err(RepositoryError::WorklogAlreadyExists { id: worklog.id() });
            }
            if worklog.is_active() && data.worklogs.iter().any(Worklog::is_active) {
                return Err(RepositoryError::ActiveWorklogExists);
            }
            if has_same_task_overlap(&data.worklogs, worklog, None) {
                return Err(RepositoryError::SameTaskWorklogOverlap { id: worklog.id() });
            }
            data.worklogs.push(worklog.clone());
            Ok(())
        }

        fn stop_worklog(
            &self,
            id: WorklogId,
            expected_start: DateTime<Utc>,
            end: DateTime<Utc>,
        ) -> Result<Worklog, RepositoryError> {
            if let Some(error) = self.take_write_failure() {
                return Err(error);
            }
            let mut data = self.0.borrow_mut();
            let index = data
                .worklogs
                .iter()
                .position(|worklog| worklog.id() == id)
                .ok_or(RepositoryError::WorklogNotFound { id })?;
            let stored = &data.worklogs[index];
            if !stored.is_active() {
                return Err(RepositoryError::WorklogAlreadyStopped { id });
            }
            if stored.start() != expected_start {
                return Err(RepositoryError::WorklogChanged { id });
            }
            let stopped = Worklog::new(stored.id(), stored.task_id(), stored.start(), Some(end))
                .map_err(|error| RepositoryError::Constraint {
                    message: error.to_string(),
                })?;
            if has_same_task_overlap(&data.worklogs, &stopped, Some(id)) {
                return Err(RepositoryError::SameTaskWorklogOverlap { id });
            }
            data.worklogs[index] = stopped.clone();
            Ok(stopped)
        }

        fn active_worklog(&self) -> Result<Option<Worklog>, RepositoryError> {
            self.read_guard()?;
            let mut data = self.0.borrow_mut();
            if data.hide_next_active_read {
                data.hide_next_active_read = false;
                return Ok(None);
            }
            Ok(data
                .worklogs
                .iter()
                .find(|worklog| worklog.is_active())
                .cloned())
        }

        fn switch_worklog(
            &self,
            id: WorklogId,
            expected_start: DateTime<Utc>,
            stop_at: DateTime<Utc>,
            next: &Worklog,
        ) -> Result<(), RepositoryError> {
            if let Some(error) = self.take_write_failure() {
                return Err(error);
            }
            let mut data = self.0.borrow_mut();
            let Some(index) = data.worklogs.iter().position(|worklog| worklog.id() == id) else {
                return Err(RepositoryError::WorklogNotFound { id });
            };
            if !data.worklogs[index].is_active() {
                return Err(RepositoryError::WorklogAlreadyStopped { id });
            }
            if data.worklogs[index].start() != expected_start {
                return Err(RepositoryError::WorklogChanged { id });
            }
            if !next.is_active() {
                return Err(RepositoryError::Constraint {
                    message: "a switch replacement must be active".to_owned(),
                });
            }
            let task = data
                .tasks
                .iter()
                .find(|item| item.task.id() == next.task_id())
                .ok_or(RepositoryError::TaskNotFound { id: next.task_id() })?;
            if task.task.is_archived() {
                return Err(RepositoryError::TaskArchived { id: next.task_id() });
            }
            if data
                .worklogs
                .iter()
                .any(|worklog| worklog.id() == next.id())
            {
                return Err(RepositoryError::WorklogAlreadyExists { id: next.id() });
            }

            let active = &data.worklogs[index];
            let stopped =
                Worklog::new(active.id(), active.task_id(), active.start(), Some(stop_at))
                    .map_err(|error| RepositoryError::Constraint {
                        message: error.to_string(),
                    })?;
            let mut staged = data.worklogs.clone();
            staged[index] = stopped;
            if has_same_task_overlap(&staged, next, None) {
                return Err(RepositoryError::SameTaskWorklogOverlap { id: next.id() });
            }
            staged.push(next.clone());
            data.worklogs = staged;
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

    fn completed_worklog(tag: u128, task_id: TaskId, start: i64, end: i64) -> Worklog {
        Worklog::new(
            WorklogId::from_uuid(uuid::Uuid::from_u128(tag)),
            task_id,
            at(start),
            Some(at(end)),
        )
        .unwrap()
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
        repository.0.borrow_mut().worklogs.extend([
            Worklog::new(
                worklog(10, one.id, 900).id(),
                one.id,
                at(900),
                Some(at(900)),
            )
            .unwrap(),
            Worklog::new(
                worklog(11, three.id, 800).id(),
                three.id,
                at(800),
                Some(at(800)),
            )
            .unwrap(),
        ]);
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
        repository.0.borrow_mut().worklogs.push(
            Worklog::new(
                worklog(10, worked.id, 50).id(),
                worked.id,
                at(50),
                Some(at(50)),
            )
            .unwrap(),
        );
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
        let task_ids = repository
            .0
            .borrow()
            .tasks
            .iter()
            .map(|item| item.task.id())
            .collect::<Vec<_>>();
        for (tag, task_id) in task_ids.into_iter().enumerate() {
            repository.0.borrow_mut().worklogs.push(
                Worklog::new(
                    WorklogId::from_uuid(uuid::Uuid::from_u128(tag as u128 + 10)),
                    task_id,
                    at(700),
                    Some(at(700)),
                )
                .unwrap(),
            );
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
            matches!(started, SetActiveTaskOutcome::Started { worklog } if worklog.start() == at_nanos(200, 987_654_000))
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
    fn committed_task_writes_report_a_failed_tracking_refresh() {
        let alpha = stamped_task(1, "alpha", 100, 100);
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        let mut application = TrackerApplication::load(repository.clone()).unwrap();
        repository.fail_refresh_after_next_task_write();

        assert_eq!(
            application.archive_task(alpha.id, at(200)),
            Err(ApplicationError::TaskRecovery(RepositoryError::Backend {
                message: "read failed".to_owned()
            }))
        );
        assert!(application.task(alpha.id).unwrap().is_archived());
        assert!(repository.0.borrow().tasks[0].task.is_archived());

        repository.0.borrow_mut().fail_reads = false;
        repository.fail_refresh_after_next_task_write();
        assert_eq!(
            application.unarchive_task(alpha.id, at(300)),
            Err(ApplicationError::TaskRecovery(RepositoryError::Backend {
                message: "read failed".to_owned()
            }))
        );
        assert!(!application.task(alpha.id).unwrap().is_archived());
        assert!(!repository.0.borrow().tasks[0].task.is_archived());
    }

    #[test]
    fn unarchive_task_restores_the_task_and_keeps_its_worklogs() {
        let alpha = task(1, "alpha");
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        repository.0.borrow_mut().worklogs.push(
            Worklog::new(
                worklog(10, alpha.id, 100).id(),
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
        assert_eq!(
            application
                .worklogs_for_task(alpha.id, None)
                .unwrap()
                .worklogs
                .len(),
            1
        );
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
            Ok(Some(worklog)) if worklog.task_id() == alpha.id
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
            SetActiveTaskOutcome::Started { worklog } if worklog.start() == at(100)
        ));
        assert_eq!(
            repository.active_worklog().unwrap().unwrap().start(),
            at(100)
        );
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
            SetActiveTaskOutcome::AlreadyActive { worklog } if worklog.id() == first.id()
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
            SetActiveTaskOutcome::AlreadyActive { worklog } if worklog.start() == at(900)
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
                if stopped.end() == Some(at(150)) && started.start() == at(150)
        ));
        let data = repository.0.borrow();
        assert_eq!(data.worklogs.len(), 2);
        assert_eq!(
            data.worklogs
                .iter()
                .filter(|item| item.end().is_none())
                .count(),
            1
        );
        assert_eq!(
            data.worklogs
                .iter()
                .find(|item| item.task_id() == alpha.id)
                .unwrap()
                .end(),
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
        let active = active.id();
        let outcome = application.clear_active_task(active, at(150)).unwrap();
        assert!(matches!(
            outcome,
            ClearActiveTaskOutcome::Stopped { worklog } if worklog.end() == Some(at(150))
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
            TrackingState::Running { worklog } => worklog.id(),
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
            TrackingState::Running { worklog } if worklog.task_id() == beta.id
        ));
        assert_eq!(
            ordered_names(&first, TaskOrdering::RecentlyWorked),
            ["beta".to_owned(), "alpha".to_owned()],
            "the adopted active worklog supplies authoritative recent activity"
        );
        assert!(matches!(
            repository.active_worklog(),
            Ok(Some(worklog)) if worklog.task_id() == beta.id
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
                    TrackingState::Running { worklog } => worklog.id(),
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
                    TrackingState::Running { worklog } => worklog.id(),
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
        let error = ApplicationError::WorklogCorrectionWrite {
            write: RepositoryError::Backend {
                message: "correction failed".to_owned(),
            },
        };
        assert!(error.source().is_some());
        let error = ApplicationError::WorklogCorrectionRecovery {
            write: RepositoryError::Backend {
                message: "correction failed".to_owned(),
            },
            recovery: RepositoryError::Backend {
                message: "correction reload failed".to_owned(),
            },
        };
        assert!(error.source().is_some());
    }

    #[test]
    fn memory_repository_honors_worklog_ordering_and_write_contracts() {
        let alpha = task(1, "alpha");
        let mut archived = task(2, "archived");
        assert!(archived.archive(at(100)));
        let repository = MemoryRepository::with_tasks(vec![archived.clone(), alpha.clone()]);
        let later = Worklog::new(
            worklog(2, alpha.id, 20).id(),
            alpha.id,
            at(20),
            Some(at(30)),
        )
        .unwrap();
        let earlier = Worklog::new(
            worklog(1, alpha.id, 10).id(),
            alpha.id,
            at(10),
            Some(at(15)),
        )
        .unwrap();
        repository.insert_worklog(&later).unwrap();
        repository.insert_worklog(&earlier).unwrap();
        // History order is start descending, so the later worklog leads.
        assert_eq!(WORKLOG_PAGE_SIZE, 50);
        assert_eq!(
            repository
                .worklog_page(alpha.id, None)
                .unwrap()
                .worklogs
                .into_iter()
                .map(|worklog| worklog.id())
                .collect::<Vec<_>>(),
            vec![later.id(), earlier.id()]
        );
        assert!(matches!(
            repository.insert_worklog(&worklog(3, archived.id, 40)),
            Err(RepositoryError::TaskArchived { id }) if id == archived.id
        ));
        let active = worklog(4, alpha.id, 50);
        repository.insert_worklog(&active).unwrap();
        assert!(matches!(
            repository.stop_worklog(active.id(), active.start(), at(49)),
            Err(RepositoryError::Constraint { .. })
        ));
        let next = worklog(5, archived.id, 60);
        assert!(matches!(
            repository.switch_worklog(active.id(), active.start(), at(55), &next),
            Err(RepositoryError::TaskArchived { id }) if id == archived.id
        ));
        assert!(matches!(
            repository.active_worklog(),
            Ok(Some(worklog)) if worklog.id() == active.id()
        ));
    }

    #[test]
    fn worklog_queries_adopt_the_backend_snapshot() {
        let alpha = task(1, "alpha");
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        let mut application = TrackerApplication::load(repository.clone()).unwrap();
        repository
            .0
            .borrow_mut()
            .worklogs
            .push(worklog(10, alpha.id, 100));
        assert_eq!(
            application
                .worklogs_for_task(alpha.id, None)
                .unwrap()
                .worklogs
                .len(),
            1
        );
        assert!(matches!(
            application.current_tracking(),
            TrackingState::Running { worklog: active }
                if active.id() == worklog(10, alpha.id, 100).id()
        ));
        repository.0.borrow_mut().worklogs.clear();
        assert!(
            application
                .worklogs_for_task(alpha.id, None)
                .unwrap()
                .worklogs
                .is_empty()
        );
        assert_eq!(application.current_tracking(), &TrackingState::Idle);
    }

    #[test]
    fn a_worklog_page_replaces_requested_and_active_task_aggregates_exactly() {
        let alpha = task(1, "alpha");
        let beta = task(2, "beta");
        let alpha_work = completed_worklog(10, alpha.id, 300, 310);
        let beta_work = worklog(11, beta.id, 250);
        let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone()]);
        repository.insert_worklog(&alpha_work).unwrap();
        repository.insert_worklog(&beta_work).unwrap();
        let mut application = TrackerApplication::load(repository.clone()).unwrap();
        assert_eq!(
            ordered_names(&application, TaskOrdering::RecentlyWorked),
            ["alpha".to_owned(), "beta".to_owned()]
        );

        repository
            .compare_and_set_worklog_times(
                alpha_work.id(),
                alpha_work.times(),
                WorklogTimes::new(at(200), Some(at(210))),
            )
            .unwrap();
        application.worklogs_for_task(alpha.id, None).unwrap();

        assert_eq!(
            ordered_names(&application, TaskOrdering::RecentlyWorked),
            ["beta".to_owned(), "alpha".to_owned()]
        );
    }

    #[test]
    fn a_running_history_row_wins_a_race_after_the_tracking_refresh() {
        let alpha = task(1, "alpha");
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        let mut application = TrackerApplication::load(repository.clone()).unwrap();
        let active = worklog(10, alpha.id, 100);
        repository.0.borrow_mut().worklogs.push(active.clone());
        repository.hide_next_active_read();

        let page = application.worklogs_for_task(alpha.id, None).unwrap();

        assert_eq!(page.worklogs, vec![active.clone()]);
        assert!(matches!(
            application.current_tracking(),
            TrackingState::Running { worklog } if worklog.id() == active.id()
        ));
    }

    #[test]
    fn worklog_history_pages_are_bounded_ordered_and_continue_after_the_cursor() {
        let alpha = task(1, "alpha");
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        // 55 worklogs with distinct starts; the latest one stays active,
        // and active worklogs belong to the history.
        for tag in 1..=55u128 {
            let worklog = if tag == 55 {
                worklog(tag, alpha.id, i64::try_from(tag).unwrap())
            } else {
                Worklog::new(
                    WorklogId::from_uuid(uuid::Uuid::from_u128(tag)),
                    alpha.id,
                    at(i64::try_from(tag).unwrap()),
                    Some(at(i64::try_from(tag).unwrap() + 1)),
                )
                .unwrap()
            };
            repository.insert_worklog(&worklog).unwrap();
        }
        let mut application = TrackerApplication::load(repository).unwrap();

        let first = application.worklogs_for_task(alpha.id, None).unwrap();
        assert_eq!(first.worklogs.len(), WORKLOG_PAGE_SIZE);
        let starts: Vec<i64> = first
            .worklogs
            .iter()
            .map(|w| w.start().timestamp())
            .collect();
        assert_eq!(
            starts,
            (6..=55).rev().collect::<Vec<_>>(),
            "start descending"
        );
        let cursor = first.next_cursor.expect("more history follows");
        assert_eq!(cursor.start, at(6));
        assert_eq!(cursor.id, WorklogId::from_uuid(uuid::Uuid::from_u128(6)));

        let second = application
            .worklogs_for_task(alpha.id, Some(&cursor))
            .unwrap();
        assert_eq!(second.worklogs.len(), 5);
        assert_eq!(
            second.next_cursor, None,
            "the history ended inside the page"
        );
        let rest_starts: Vec<i64> = second
            .worklogs
            .iter()
            .map(|w| w.start().timestamp())
            .collect();
        assert_eq!(rest_starts, [5, 4, 3, 2, 1]);

        // The two pages cover every worklog exactly once: none skipped,
        // none repeated.
        let mut ids: Vec<u128> = first
            .worklogs
            .iter()
            .chain(&second.worklogs)
            .map(|w| w.id().as_uuid().as_u128())
            .collect();
        ids.sort_unstable();
        assert_eq!(ids, (1..=55).collect::<Vec<_>>());
    }

    #[test]
    fn a_page_boundary_inside_equal_starts_neither_dups_nor_skips_rows() {
        let alpha = task(1, "alpha");
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        // 55 worklogs share one start, so identifier ascending decides the
        // whole order and the page boundary falls between two of them. A
        // weaker boundary rule would repeat or drop rows 51 to 55.
        for tag in 1..=55u128 {
            let worklog = if tag == 1 {
                Worklog::begin(
                    WorklogId::from_uuid(uuid::Uuid::from_u128(tag)),
                    alpha.id,
                    at(500),
                )
            } else {
                Worklog::new(
                    WorklogId::from_uuid(uuid::Uuid::from_u128(tag)),
                    alpha.id,
                    at(500),
                    Some(at(500)),
                )
                .unwrap()
            };
            repository.insert_worklog(&worklog).unwrap();
        }
        let mut application = TrackerApplication::load(repository).unwrap();

        let page = application.worklogs_for_task(alpha.id, None).unwrap();
        let ids: Vec<u128> = page
            .worklogs
            .iter()
            .map(|w| w.id().as_uuid().as_u128())
            .collect();
        assert_eq!(ids, (1..=50).collect::<Vec<_>>(), "id ascending");
        let cursor = page.next_cursor.expect("equal starts continue");
        assert_eq!(cursor.start, at(500));
        assert_eq!(cursor.id.as_uuid().as_u128(), 50);

        let rest = application
            .worklogs_for_task(alpha.id, Some(&cursor))
            .unwrap()
            .worklogs;
        let rest_ids: Vec<u128> = rest.iter().map(|w| w.id().as_uuid().as_u128()).collect();
        assert_eq!(rest_ids, (51..=55).collect::<Vec<_>>());
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
                worklog: ActiveWorklog::begin(foreign.id(), foreign.task_id(), foreign.start())
            }
        );
        // Recovery refreshes both tracking and recently worked ordering.
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

    #[test]
    fn correction_canonicalizes_expected_and_replacement_before_validation() {
        let alpha = task(1, "alpha");
        let original = Worklog::new(
            WorklogId::from_uuid(uuid::Uuid::from_u128(10)),
            alpha.id,
            at_nanos(100, 123_456_000),
            Some(at_nanos(100, 123_457_000)),
        )
        .unwrap();
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        repository.insert_worklog(&original).unwrap();
        let mut application = TrackerApplication::load(repository.clone()).unwrap();

        let outcome = application
            .correct_worklog(
                original.id(),
                WorklogTimes::new(at_nanos(100, 123_456_999), Some(at_nanos(100, 123_457_999))),
                WorklogTimes::new(at_nanos(200, 900), Some(at_nanos(200, 100))),
                at_nanos(200, 999),
            )
            .unwrap();

        let CorrectWorklogOutcome::Corrected { worklog } = outcome;
        assert_eq!(worklog.id(), original.id());
        assert_eq!(worklog.task_id(), alpha.id);
        assert_eq!(worklog.start(), at(200));
        assert_eq!(worklog.end(), Some(at(200)));
        assert_eq!(
            repository.find_worklog(original.id()).unwrap(),
            Some(worklog)
        );
    }

    #[test]
    fn correction_rejects_future_and_active_state_changes() {
        let alpha = task(1, "alpha");
        let active = worklog(10, alpha.id, 100);
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        repository.insert_worklog(&active).unwrap();
        let mut application = TrackerApplication::load(repository.clone()).unwrap();

        assert_eq!(
            application.correct_worklog(
                active.id(),
                active.times(),
                WorklogTimes::new(at(301), None),
                at(300),
            ),
            Err(ApplicationError::InvalidWorklogCorrection(
                WorklogCorrectionError::StartAfterOccurredAt
            ))
        );
        assert_eq!(
            application.correct_worklog(
                active.id(),
                active.times(),
                WorklogTimes::new(at(100), Some(at(200))),
                at(300),
            ),
            Err(ApplicationError::InvalidWorklogCorrection(
                WorklogCorrectionError::CompletionStateChanged
            ))
        );
        assert_eq!(repository.find_worklog(active.id()).unwrap(), Some(active));
    }

    #[test]
    fn active_correction_refreshes_tracking_and_latest_work() {
        let alpha = task(1, "alpha");
        let active = worklog(10, alpha.id, 100);
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        repository.insert_worklog(&active).unwrap();
        let mut application = TrackerApplication::load(repository.clone()).unwrap();
        let list_reads = repository.0.borrow().list_reads;

        let outcome = application
            .correct_worklog(
                active.id(),
                active.times(),
                WorklogTimes::new(at(150), None),
                at(200),
            )
            .unwrap();

        assert!(matches!(
            outcome,
            CorrectWorklogOutcome::Corrected { worklog }
                if worklog.id() == active.id()
                    && worklog.task_id() == alpha.id
                    && worklog.start() == at(150)
                    && worklog.is_active()
        ));
        assert!(matches!(
            application.current_tracking(),
            TrackingState::Running { worklog }
                if worklog.id() == active.id() && worklog.start() == at(150)
        ));
        assert_eq!(
            application.tasks(TaskOrdering::RecentlyWorked)[0].latest_work_start,
            Some(at(150))
        );
        assert_eq!(repository.0.borrow().list_reads, list_reads);
    }

    #[test]
    fn correcting_another_task_adopts_the_active_tasks_exact_aggregate() {
        let active_task = task(1, "active");
        let corrected_task = task(2, "corrected");
        let active = worklog(10, active_task.id, 600);
        let corrected = completed_worklog(11, corrected_task.id, 400, 410);
        let repository =
            MemoryRepository::with_tasks(vec![active_task.clone(), corrected_task.clone()]);
        repository.insert_worklog(&active).unwrap();
        repository.insert_worklog(&corrected).unwrap();
        let mut application = TrackerApplication::load(repository.clone()).unwrap();

        repository
            .compare_and_set_worklog_times(
                active.id(),
                active.times(),
                WorklogTimes::new(at(540), None),
            )
            .unwrap();
        application
            .correct_worklog(
                corrected.id(),
                corrected.times(),
                WorklogTimes::new(at(350), Some(at(360))),
                at(500),
            )
            .unwrap();

        assert!(matches!(
            application.current_tracking(),
            TrackingState::Running { worklog }
                if worklog.id() == active.id() && worklog.start() == at(540)
        ));
        let active_item = application
            .tasks(TaskOrdering::RecentlyWorked)
            .into_iter()
            .find(|item| item.task.id() == active_task.id)
            .unwrap();
        assert_eq!(active_item.latest_work_start, Some(at(540)));
    }

    #[test]
    fn completed_correction_reloads_the_true_latest_work_aggregate() {
        let alpha = task(1, "alpha");
        let beta = task(2, "beta");
        let old_alpha = completed_worklog(10, alpha.id, 100, 110);
        let latest_alpha = completed_worklog(11, alpha.id, 300, 310);
        let beta_work = completed_worklog(12, beta.id, 250, 260);
        let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone()]);
        for worklog in [&old_alpha, &latest_alpha, &beta_work] {
            repository.insert_worklog(worklog).unwrap();
        }
        let mut application = TrackerApplication::load(repository).unwrap();
        assert_eq!(
            ordered_names(&application, TaskOrdering::RecentlyWorked),
            ["alpha".to_owned(), "beta".to_owned()]
        );

        application
            .correct_worklog(
                latest_alpha.id(),
                latest_alpha.times(),
                WorklogTimes::new(at(200), Some(at(210))),
                at(400),
            )
            .unwrap();

        assert_eq!(
            ordered_names(&application, TaskOrdering::RecentlyWorked),
            ["beta".to_owned(), "alpha".to_owned()]
        );
        let alpha_item = application
            .tasks(TaskOrdering::RecentlyWorked)
            .into_iter()
            .find(|item| item.task.id() == alpha.id)
            .unwrap();
        assert_eq!(alpha_item.latest_work_start, Some(at(200)));
    }

    #[test]
    fn stale_active_correction_recovers_the_stopped_state() {
        let alpha = task(1, "alpha");
        let active = worklog(10, alpha.id, 100);
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        repository.insert_worklog(&active).unwrap();
        let mut application = TrackerApplication::load(repository.clone()).unwrap();
        repository
            .rename_task(alpha.id, TaskName::new("renamed").unwrap(), at(120))
            .unwrap();
        repository
            .stop_worklog(active.id(), active.start(), at(150))
            .unwrap();

        assert_eq!(
            application.correct_worklog(
                active.id(),
                active.times(),
                WorklogTimes::new(at(90), None),
                at(200),
            ),
            Err(ApplicationError::WorklogCorrectionWrite {
                write: RepositoryError::WorklogChanged { id: active.id() },
            })
        );
        assert_eq!(application.current_tracking(), &TrackingState::Idle);
        assert_eq!(
            application.task(alpha.id).unwrap().name().as_str(),
            "renamed"
        );
        assert_eq!(
            application.tasks(TaskOrdering::RecentlyWorked)[0].latest_work_start,
            Some(at(100))
        );
    }

    #[test]
    fn a_missing_correction_target_recovers_authoritative_state() {
        let alpha = task(1, "alpha");
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        let mut application = TrackerApplication::load(repository.clone()).unwrap();
        let foreign = worklog(10, alpha.id, 100);
        repository.insert_worklog(&foreign).unwrap();
        let missing = WorklogId::from_uuid(uuid::Uuid::from_u128(99));

        assert_eq!(
            application.correct_worklog(
                missing,
                WorklogTimes::new(at(50), Some(at(60))),
                WorklogTimes::new(at(55), Some(at(65))),
                at(200),
            ),
            Err(ApplicationError::WorklogCorrectionWrite {
                write: RepositoryError::WorklogNotFound { id: missing },
            })
        );
        assert!(matches!(
            application.current_tracking(),
            TrackingState::Running { worklog } if worklog.id() == foreign.id()
        ));
    }

    #[test]
    fn correction_write_errors_recover_state_without_returning_success() {
        let alpha = task(1, "alpha");
        let original = completed_worklog(10, alpha.id, 100, 110);
        let repository = MemoryRepository::with_tasks(vec![alpha]);
        repository.insert_worklog(&original).unwrap();
        let mut application = TrackerApplication::load(repository.clone()).unwrap();
        repository.fail_next_write(
            RepositoryError::Backend {
                message: "write failed".to_owned(),
            },
            None,
        );

        assert_eq!(
            application.correct_worklog(
                original.id(),
                original.times(),
                WorklogTimes::new(at(120), Some(at(130))),
                at(200),
            ),
            Err(ApplicationError::WorklogCorrectionWrite {
                write: RepositoryError::Backend {
                    message: "write failed".to_owned(),
                },
            })
        );
        assert_eq!(
            repository.find_worklog(original.id()).unwrap(),
            Some(original)
        );
        assert_eq!(application.current_tracking(), &TrackingState::Idle);
    }

    #[test]
    fn overlap_errors_propagate_and_leave_the_original_worklog() {
        let alpha = task(1, "alpha");
        let first = completed_worklog(10, alpha.id, 100, 150);
        let second = completed_worklog(11, alpha.id, 200, 250);
        let repository = MemoryRepository::with_tasks(vec![alpha]);
        repository.insert_worklog(&first).unwrap();
        repository.insert_worklog(&second).unwrap();
        let mut application = TrackerApplication::load(repository.clone()).unwrap();

        assert_eq!(
            application.correct_worklog(
                second.id(),
                second.times(),
                WorklogTimes::new(at(140), Some(at(220))),
                at(300),
            ),
            Err(ApplicationError::WorklogCorrectionWrite {
                write: RepositoryError::SameTaskWorklogOverlap { id: second.id() },
            })
        );
        assert_eq!(repository.find_worklog(second.id()).unwrap(), Some(second));
        assert_eq!(application.current_tracking(), &TrackingState::Idle);
    }

    #[test]
    fn compare_and_swap_distinguishes_stale_and_missing_worklogs() {
        let alpha = task(1, "alpha");
        let original = completed_worklog(10, alpha.id, 100, 150);
        let repository = MemoryRepository::with_tasks(vec![alpha]);
        repository.insert_worklog(&original).unwrap();

        assert_eq!(
            repository.compare_and_set_worklog_times(
                original.id(),
                WorklogTimes::new(at(100), Some(at(151))),
                WorklogTimes::new(at(110), Some(at(160))),
            ),
            Err(RepositoryError::WorklogChanged { id: original.id() })
        );
        let missing = WorklogId::from_uuid(uuid::Uuid::from_u128(99));
        assert_eq!(
            repository.compare_and_set_worklog_times(
                missing,
                WorklogTimes::new(at(100), Some(at(150))),
                WorklogTimes::new(at(110), Some(at(160))),
            ),
            Err(RepositoryError::WorklogNotFound { id: missing })
        );
        assert_eq!(
            repository.find_worklog(original.id()).unwrap(),
            Some(original)
        );
    }

    #[test]
    fn half_open_overlap_rules_allow_touching_zero_duration_and_other_tasks() {
        let alpha = task(1, "alpha");
        let beta = task(2, "beta");
        let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone()]);
        let first = completed_worklog(10, alpha.id, 100, 150);
        let touching = completed_worklog(11, alpha.id, 150, 200);
        let zero = completed_worklog(12, alpha.id, 125, 125);
        let other_task = completed_worklog(13, beta.id, 120, 180);
        repository.insert_worklog(&first).unwrap();
        repository.insert_worklog(&touching).unwrap();
        repository.insert_worklog(&zero).unwrap();
        repository.insert_worklog(&other_task).unwrap();

        let overlapping = completed_worklog(14, alpha.id, 149, 151);
        assert_eq!(
            repository.insert_worklog(&overlapping),
            Err(RepositoryError::SameTaskWorklogOverlap {
                id: overlapping.id()
            })
        );
        let moved = repository
            .compare_and_set_worklog_times(
                touching.id(),
                touching.times(),
                WorklogTimes::new(at(200), Some(at(210))),
            )
            .unwrap()
            .worklog;
        assert_eq!(moved.start(), at(200));
    }
    #[test]
    fn correction_preserves_write_and_recovery_errors() {
        let alpha = task(1, "alpha");
        let original = completed_worklog(10, alpha.id, 100, 110);
        let repository = MemoryRepository::with_tasks(vec![alpha]);
        repository.insert_worklog(&original).unwrap();
        let mut application = TrackerApplication::load(repository.clone()).unwrap();
        let write = RepositoryError::Backend {
            message: "write failed".to_owned(),
        };
        repository.fail_next_write(write.clone(), None);
        repository.fail_recovery_after_next_write();

        assert_eq!(
            application.correct_worklog(
                original.id(),
                original.times(),
                WorklogTimes::new(at(120), Some(at(130))),
                at(200),
            ),
            Err(ApplicationError::WorklogCorrectionRecovery {
                write,
                recovery: RepositoryError::Backend {
                    message: "read failed".to_owned(),
                },
            })
        );
    }

    #[test]
    fn active_start_changed_before_refresh_requires_a_clear_retry() {
        let alpha = task(1, "alpha");
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        let mut application = TrackerApplication::load(repository.clone()).unwrap();
        let active = match application.set_active_task(alpha.id, at(100)).unwrap() {
            SetActiveTaskOutcome::Started { worklog } => worklog,
            other => panic!("expected start, got {other:?}"),
        };
        repository.0.borrow_mut().worklogs[0] =
            worklog(active.id().as_uuid().as_u128(), alpha.id, 50);

        assert_eq!(
            application.clear_active_task(active.id(), at(150)),
            Err(ApplicationError::TrackingStateChanged)
        );
        assert!(matches!(
            application.current_tracking(),
            TrackingState::Running { worklog } if worklog.id() == active.id() && worklog.start() == at(50)
        ));
    }

    #[test]
    fn active_start_changed_between_refresh_and_write_requires_stop_and_switch_retries() {
        let alpha = task(1, "alpha");
        let beta = task(2, "beta");
        let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone()]);
        let mut application = TrackerApplication::load(repository.clone()).unwrap();
        let active = match application.set_active_task(alpha.id, at(100)).unwrap() {
            SetActiveTaskOutcome::Started { worklog } => worklog,
            other => panic!("expected start, got {other:?}"),
        };
        let corrected = worklog(active.id().as_uuid().as_u128(), alpha.id, 50);
        repository.fail_next_write(
            RepositoryError::WorklogChanged { id: active.id() },
            Some(corrected.clone()),
        );
        assert_eq!(
            application.clear_active_task(active.id(), at(150)),
            Err(ApplicationError::TrackingStateChanged)
        );
        assert!(matches!(
            application.current_tracking(),
            TrackingState::Running { worklog } if worklog.start() == corrected.start()
        ));

        repository.fail_next_write(
            RepositoryError::WorklogChanged { id: active.id() },
            Some(corrected.clone()),
        );
        assert_eq!(
            application.set_active_task(beta.id, at(160)),
            Err(ApplicationError::TrackingStateChanged)
        );
        assert!(matches!(
            application.current_tracking(),
            TrackingState::Running { worklog } if worklog.id() == active.id() && worklog.start() == corrected.start()
        ));
        assert!(
            repository
                .0
                .borrow()
                .worklogs
                .iter()
                .all(|worklog| worklog.end().is_none())
        );
    }

    #[test]
    fn snapshot_alignment_keeps_an_active_task_recently_worked() {
        let active_task = stamped_task(1, "active", 100, 100);
        let other_task = stamped_task(2, "other", 500, 500);
        let active = worklog(10, active_task.id, 200);
        let (items, _) =
            TrackerApplication::<MemoryRepository>::state_from_snapshot(TrackerSnapshot {
                task_items: vec![
                    TaskListItem {
                        task: active_task.clone(),
                        latest_work_start: None,
                    },
                    TaskListItem {
                        task: other_task.clone(),
                        latest_work_start: None,
                    },
                ],
                active_worklog: Some(active),
            })
            .unwrap();
        assert_eq!(
            items
                .into_iter()
                .find(|item| item.task.id() == active_task.id)
                .unwrap()
                .latest_work_start,
            Some(at(200))
        );
    }

    #[test]
    fn active_start_changed_before_refresh_requires_a_set_active_retry() {
        let alpha = task(1, "alpha");
        let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
        let mut application = TrackerApplication::load(repository.clone()).unwrap();
        let active = match application.set_active_task(alpha.id, at(100)).unwrap() {
            SetActiveTaskOutcome::Started { worklog } => worklog,
            other => panic!("expected start, got {other:?}"),
        };
        repository.0.borrow_mut().worklogs[0] =
            worklog(active.id().as_uuid().as_u128(), alpha.id, 50);
        assert_eq!(
            application.set_active_task(alpha.id, at(150)),
            Err(ApplicationError::TrackingStateChanged)
        );
    }
}
