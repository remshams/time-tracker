//! Backend-neutral persistence ports.
//!
//! Implementations order tasks by identifier and a task's worklogs by start
//! time, then identifier. They enforce one active worklog, reject an end
//! before its start, reject worklogs on archived tasks, and reject archiving
//! the active task. Unarchiving a task is non-destructive and must not
//! discard its worklogs. A switch must stop the old worklog and start the
//! new one atomically.

use chrono::{DateTime, Utc};
use tracker_domain::{Task, TaskId, TaskName, Worklog, WorklogId};

/// A persistence failure that application callers can handle without knowing
/// which backend produced it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RepositoryError {
    #[error("task {id} not found")]
    TaskNotFound { id: TaskId },
    #[error("worklog {id} not found")]
    WorklogNotFound { id: WorklogId },
    #[error("worklog {id} is already stopped")]
    WorklogAlreadyStopped { id: WorklogId },
    #[error("worklog {id} already exists")]
    WorklogAlreadyExists { id: WorklogId },
    #[error("task {id} already exists")]
    TaskAlreadyExists { id: TaskId },
    #[error("another worklog is already active")]
    ActiveWorklogExists,
    #[error("task {id} is archived and cannot receive worklogs")]
    TaskArchived { id: TaskId },
    #[error("task {id} has an active worklog and cannot be archived")]
    TaskIsActive { id: TaskId },
    #[error("{message}")]
    Constraint { message: String },
    #[error("stored data is invalid: {field}")]
    CorruptData { field: &'static str },
    #[error("{message}")]
    Backend { message: String },
}

/// Persistence needed by task use cases.
pub trait TaskRepository {
    fn create_task(&self, task: Task) -> Result<(), RepositoryError>;
    fn find_task(&self, id: TaskId) -> Result<Option<Task>, RepositoryError>;
    fn list_tasks(&self) -> Result<Vec<Task>, RepositoryError>;
    fn rename_task(&self, id: TaskId, name: TaskName) -> Result<Task, RepositoryError>;
    fn archive_task(&self, id: TaskId) -> Result<Task, RepositoryError>;

    /// Restores an archived task. Must keep the task's worklogs intact and
    /// succeed when the task is already unarchived.
    fn unarchive_task(&self, id: TaskId) -> Result<Task, RepositoryError>;
}

/// Persistence needed to read worklog history.
pub trait WorklogRepository {
    fn list_worklogs(&self, task_id: TaskId) -> Result<Vec<Worklog>, RepositoryError>;
}

/// Persistence needed by tracking commands.
pub trait TrackingRepository {
    fn insert_worklog(&self, worklog: &Worklog) -> Result<(), RepositoryError>;
    fn stop_worklog(&self, id: WorklogId, end: DateTime<Utc>) -> Result<Worklog, RepositoryError>;
    fn active_worklog(&self) -> Result<Option<Worklog>, RepositoryError>;

    /// Stops one active worklog and inserts its replacement atomically.
    /// A failure must leave both worklogs unchanged.
    fn switch_worklog(
        &self,
        id: WorklogId,
        stop_at: DateTime<Utc>,
        next: &Worklog,
    ) -> Result<(), RepositoryError>;
}

/// A backend that supports every current use case.
pub trait TrackerRepository: TaskRepository + WorklogRepository + TrackingRepository {}

impl<T> TrackerRepository for T where T: TaskRepository + WorklogRepository + TrackingRepository {}
