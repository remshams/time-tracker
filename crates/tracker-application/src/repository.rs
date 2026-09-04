//! Backend-neutral persistence ports.
//!
//! Implementations return task rows in identifier order and a task's
//! worklog history as bounded pages ordered by start descending, then
//! identifier. They enforce one active worklog, reject an end before its
//! start, reject worklogs on archived tasks, and reject archiving the
//! active task. Unarchiving a task is non-destructive and must not discard
//! its worklogs. A switch must stop the old worklog and start the new one
//! atomically. The task-list read model carries each task's latest worklog
//! start as a per-task `MAX(start)` aggregate, without loading full
//! worklogs.

use chrono::{DateTime, Utc};
use tracker_domain::{Task, TaskId, TaskName, Worklog, WorklogId};

use crate::{TaskListItem, WorklogCursor, WorklogPage};

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
///
/// Each metadata operation is atomic and preserves unrelated concurrent
/// metadata changes. `updated_at` advances only when the requested value
/// changes, and to the later of the stored value and `occurred_at`, so it
/// never moves backward. Every method returns the authoritative stored task
/// after the operation.
pub trait TaskRepository {
    fn create_task(&self, task: Task) -> Result<(), RepositoryError>;
    fn find_task(&self, id: TaskId) -> Result<Option<Task>, RepositoryError>;

    /// Lists every task with its latest worklog start, or `None` for tasks
    /// without worklogs. The result does not include worklog histories.
    fn list_task_items(&self) -> Result<Vec<TaskListItem>, RepositoryError>;

    /// Renames the task. Renaming to the stored name changes nothing.
    /// Rejects a missing task.
    fn rename_task(
        &self,
        id: TaskId,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, RepositoryError>;

    /// Archives the task. Archiving an archived task changes nothing.
    /// Rejects a missing task and refuses a task with an active worklog.
    fn archive_task(&self, id: TaskId, occurred_at: DateTime<Utc>)
    -> Result<Task, RepositoryError>;

    /// Restores an archived task. Restoring an active task changes nothing,
    /// and the task's worklogs must survive. Rejects a missing task.
    fn unarchive_task(
        &self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, RepositoryError>;
}

/// Persistence needed to read worklog history.
pub trait WorklogRepository {
    /// One bounded page of the task's worklog history, continuing strictly
    /// after the optional cursor in history order: start descending, then
    /// `WorklogId` ascending. A page carries
    /// [`WORKLOG_PAGE_SIZE`](crate::WORKLOG_PAGE_SIZE) worklogs unless the
    /// history ends inside it, and includes active worklogs. A boundary
    /// between worklogs that share a start time must neither duplicate nor
    /// skip any of them.
    fn worklog_page(
        &self,
        task_id: TaskId,
        after: Option<&WorklogCursor>,
    ) -> Result<WorklogPage, RepositoryError>;
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
