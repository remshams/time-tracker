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
    #[error("tracking state changed in another client")]
    TrackingStateChanged,
}

/// The result of creating, renaming, or archiving a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskOutcome {
    Created(Task),
    Renamed(Task),
    Archived(Task),
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
pub trait TaskQueries {
    fn tasks(&self) -> &[Task];
    fn task(&self, id: TaskId) -> Option<&Task>;
}

/// Commands that create or change tasks.
pub trait TaskOperations {
    fn create_task(&mut self, name: TaskName) -> Result<TaskOutcome, ApplicationError>;
    fn rename_task(&mut self, id: TaskId, name: TaskName) -> Result<TaskOutcome, ApplicationError>;
    fn archive_task(&mut self, id: TaskId) -> Result<TaskOutcome, ApplicationError>;
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
/// The service owns persistence sequencing. It keeps task and current
/// tracking queries ready for a synchronous client, commits switches through
/// the repository's atomic operation, and reloads authoritative state after
/// any tracking write conflict.
pub struct TrackerApplication<R> {
    repository: R,
    tasks: Vec<Task>,
    tracker: Tracker,
}

impl<R: TrackerRepository> TrackerApplication<R> {
    /// Loads task and tracking state from the selected backend.
    pub fn load(repository: R) -> Result<Self, ApplicationError> {
        let tasks = repository.list_tasks()?;
        let tracker = Self::load_tracker(&repository)?;
        Ok(Self {
            repository,
            tasks,
            tracker,
        })
    }

    fn load_tracker(repository: &R) -> Result<Tracker, ApplicationError> {
        match repository.active_worklog()? {
            Some(worklog) => Ok(Tracker::resume(worklog)?),
            None => Ok(Tracker::idle()),
        }
    }

    fn refresh_tracking(&mut self) -> Result<(), ApplicationError> {
        self.tracker = Self::load_tracker(&self.repository)?;
        Ok(())
    }

    fn recover_after_tracking_write(&mut self, write_error: RepositoryError) -> ApplicationError {
        let loaded = (|| {
            let tasks = self.repository.list_tasks()?;
            let tracker = Self::load_tracker(&self.repository)?;
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
            | Err(ApplicationError::TrackingRecovery(error)) => {
                ApplicationError::TrackingRecovery(error)
            }
            Err(ApplicationError::TrackingStateChanged) => ApplicationError::TrackingStateChanged,
        }
    }

    fn task_for_command(&self, id: TaskId) -> Result<Task, ApplicationError> {
        self.tasks
            .iter()
            .find(|task| task.id == id)
            .cloned()
            .ok_or(RepositoryError::TaskNotFound { id }.into())
    }

    fn replace_task(&mut self, changed: Task) {
        if let Some(index) = self.tasks.iter().position(|task| task.id == changed.id) {
            self.tasks[index] = changed;
        } else {
            self.tasks.push(changed);
            self.tasks.sort_by_key(|task| task.id);
        }
    }
}

impl<R: TrackerRepository> TaskQueries for TrackerApplication<R> {
    fn tasks(&self) -> &[Task] {
        &self.tasks
    }

    fn task(&self, id: TaskId) -> Option<&Task> {
        self.tasks.iter().find(|task| task.id == id)
    }
}

impl<R: TrackerRepository> TaskOperations for TrackerApplication<R> {
    fn create_task(&mut self, name: TaskName) -> Result<TaskOutcome, ApplicationError> {
        let task = Task::new(TaskId::generate(), name);
        self.repository.create_task(task.clone())?;
        self.replace_task(task.clone());
        Ok(TaskOutcome::Created(task))
    }

    fn rename_task(&mut self, id: TaskId, name: TaskName) -> Result<TaskOutcome, ApplicationError> {
        let task = self.repository.rename_task(id, name)?;
        self.replace_task(task.clone());
        Ok(TaskOutcome::Renamed(task))
    }

    fn archive_task(&mut self, id: TaskId) -> Result<TaskOutcome, ApplicationError> {
        self.refresh_tracking()?;
        self.tracker.ensure_archivable(id)?;
        let task = self.repository.archive_task(id)?;
        self.replace_task(task.clone());
        Ok(TaskOutcome::Archived(task))
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
        self.refresh_tracking()?;
        if let Some(active) = self.tracker.active()
            && active.task_id == task_id
        {
            return Ok(SetActiveTaskOutcome::AlreadyActive {
                worklog: active.clone(),
            });
        }

        let task = self.task_for_command(task_id)?;
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
        Ok(outcome)
    }

    fn clear_active_task(
        &mut self,
        expected_active: WorklogId,
        occurred_at: DateTime<Utc>,
    ) -> Result<ClearActiveTaskOutcome, ApplicationError> {
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

    use tracker_domain::{TaskName, WorklogId};

    use super::*;

    #[derive(Default)]
    struct Data {
        tasks: Vec<Task>,
        worklogs: Vec<Worklog>,
        fail_next_write: Option<RepositoryError>,
        fail_reads: bool,
        replacement_on_failure: Option<Worklog>,
        fail_reads_after_write: bool,
    }

    #[derive(Clone, Default)]
    struct MemoryRepository(Rc<RefCell<Data>>);

    impl MemoryRepository {
        fn with_tasks(mut tasks: Vec<Task>) -> Self {
            tasks.sort_by_key(|task| task.id);
            Self(Rc::new(RefCell::new(Data {
                tasks,
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
            if data.tasks.iter().any(|stored| stored.id == task.id) {
                return Err(RepositoryError::TaskAlreadyExists { id: task.id });
            }
            data.tasks.push(task);
            data.tasks.sort_by_key(|task| task.id);
            Ok(())
        }

        fn find_task(&self, id: TaskId) -> Result<Option<Task>, RepositoryError> {
            self.read_guard()?;
            Ok(self
                .0
                .borrow()
                .tasks
                .iter()
                .find(|task| task.id == id)
                .cloned())
        }

        fn list_tasks(&self) -> Result<Vec<Task>, RepositoryError> {
            self.read_guard()?;
            Ok(self.0.borrow().tasks.clone())
        }

        fn rename_task(&self, id: TaskId, name: TaskName) -> Result<Task, RepositoryError> {
            if let Some(error) = self.take_write_failure() {
                return Err(error);
            }
            let mut data = self.0.borrow_mut();
            let task = data
                .tasks
                .iter_mut()
                .find(|task| task.id == id)
                .ok_or(RepositoryError::TaskNotFound { id })?;
            task.name = name;
            Ok(task.clone())
        }

        fn archive_task(&self, id: TaskId) -> Result<Task, RepositoryError> {
            if let Some(error) = self.take_write_failure() {
                return Err(error);
            }
            let mut data = self.0.borrow_mut();
            if data
                .worklogs
                .iter()
                .any(|worklog| worklog.task_id == id && worklog.end.is_none())
            {
                return Err(RepositoryError::TaskIsActive { id });
            }
            let task = data
                .tasks
                .iter_mut()
                .find(|task| task.id == id)
                .ok_or(RepositoryError::TaskNotFound { id })?;
            task.archived = true;
            Ok(task.clone())
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
                .find(|task| task.id == worklog.task_id)
                .ok_or(RepositoryError::TaskNotFound {
                    id: worklog.task_id,
                })?;
            if task.archived {
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
                .find(|task| task.id == next.task_id)
                .ok_or(RepositoryError::TaskNotFound { id: next.task_id })?;
            if task.archived {
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

    fn task(tag: u128, name: &str) -> Task {
        Task::new(
            TaskId::from_uuid(uuid::Uuid::from_u128(tag)),
            TaskName::new(name).unwrap(),
        )
    }

    fn worklog(tag: u128, task_id: TaskId, start: i64) -> Worklog {
        Worklog::begin(
            WorklogId::from_uuid(uuid::Uuid::from_u128(tag)),
            task_id,
            at(start),
        )
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
        assert_eq!(application.tasks(), std::slice::from_ref(&alpha));
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
    fn task_operations_return_stored_outcomes_and_update_queries() {
        let repository = MemoryRepository::default();
        let mut application = TrackerApplication::load(repository).unwrap();
        let created = match application
            .create_task(TaskName::new("alpha").unwrap())
            .unwrap()
        {
            TaskOutcome::Created(task) => task,
            other => panic!("expected create, got {other:?}"),
        };
        assert_eq!(application.tasks(), std::slice::from_ref(&created));
        let renamed = application
            .rename_task(created.id, TaskName::new("beta").unwrap())
            .unwrap();
        assert!(matches!(renamed, TaskOutcome::Renamed(task) if task.name.as_str() == "beta"));
        let archived = application.archive_task(created.id).unwrap();
        assert!(matches!(archived, TaskOutcome::Archived(task) if task.archived));
        assert!(application.tasks()[0].archived);
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
        assert!(matches!(
            repository.active_worklog(),
            Ok(Some(worklog)) if worklog.task_id == beta.id
        ));
    }

    #[test]
    fn tracking_write_errors_keep_the_repository_error_as_their_source() {
        use std::error::Error as _;

        let error = ApplicationError::TrackingWrite(RepositoryError::Backend {
            message: "write failed".to_owned(),
        });
        assert!(error.source().is_some());
        let error = ApplicationError::TrackingRecovery(RepositoryError::Backend {
            message: "reload failed".to_owned(),
        });
        assert!(error.source().is_some());
    }

    #[test]
    fn memory_repository_honors_worklog_ordering_and_write_contracts() {
        let alpha = task(1, "alpha");
        let archived = Task {
            archived: true,
            ..task(2, "archived")
        };
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
            application.archive_task(alpha.id),
            Err(ApplicationError::Domain(TrackingError::TaskIsActive {
                id: alpha.id
            }))
        );
        assert!(!application.tasks()[0].archived);
    }
}
