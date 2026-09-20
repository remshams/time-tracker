//! Backend-neutral persistence ports.
//!
//! Implementations return task rows in identifier order and a task's
//! worklog history as bounded pages ordered by start descending, then
//! identifier. They enforce one active worklog, reject an end before its
//! start, reject worklogs on archived tasks, and reject archiving the
//! active task. Unarchiving a task is non-destructive and must not discard
//! its worklogs. A switch must stop the old worklog and start the new one
//! atomically. Every worklog write rejects overlaps on the same task.
//! Intervals are half-open: touching is allowed, zero-duration worklogs
//! overlap nothing, and different tasks may overlap. Worklog corrections use
//! exact timestamp compare-and-swap and preserve identity, task, and active or
//! completed state. Completed worklog deletion uses an exact match on
//! identity, task, start, and end; active worklogs cannot be deleted. A move
//! uses the same exact match and changes only the task. The task-list read
//! model carries each task's latest worklog start as a per-task `MAX(start)`
//! aggregate, without loading full worklogs.

use chrono::{DateTime, Utc};
use tracker_domain::{Task, TaskId, TaskName, Worklog, WorklogId, WorklogTimes};

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
    #[error("worklog {id} changed since it was read")]
    WorklogChanged { id: WorklogId },
    #[error("worklog {id} is active and cannot be deleted")]
    WorklogIsActive { id: WorklogId },
    #[error("worklog history for task {task_id} changed since this page was read")]
    WorklogHistoryChanged { task_id: TaskId },
    #[error("worklog {id} overlaps another worklog for the same task")]
    SameTaskWorklogOverlap { id: WorklogId },
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

/// A coherent read of task-list aggregates and global tracking state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackerSnapshot {
    pub task_items: Vec<TaskListItem>,
    pub active_worklog: Option<Worklog>,
}

/// The committed result of a worklog correction.
///
/// These values come from the correction transaction. The application must
/// adopt them instead of issuing a post-commit read that could observe a later
/// writer and misreport the committed correction as a failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorklogCorrection {
    pub worklog: Worklog,
    pub task_latest_work_start: Option<DateTime<Utc>>,
    pub active_worklog: Option<Worklog>,
    pub active_task_latest_work_start: Option<DateTime<Utc>>,
}

/// The committed result of deleting a completed worklog.
///
/// The latest-work aggregate comes from the delete transaction. Callers must
/// adopt it rather than issue a post-commit read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorklogDeletion {
    pub worklog: Worklog,
    pub task_latest_work_start: Option<DateTime<Utc>>,
}

/// The committed result of moving a worklog to another task.
///
/// The aggregate values and active row come from the move transaction. The
/// application adopts them without a post-commit read, which would otherwise
/// race another writer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorklogMove {
    pub worklog: Worklog,
    pub source_task_latest_work_start: Option<DateTime<Utc>>,
    pub destination_task_latest_work_start: Option<DateTime<Utc>>,
    pub active_worklog: Option<Worklog>,
    pub active_task_latest_work_start: Option<DateTime<Utc>>,
}

/// Persistence needed by task use cases.
pub trait TaskRepository {
    fn create_task(&self, task: Task) -> Result<(), RepositoryError>;

    /// Reads task-list aggregates and global active tracking in one coherent
    /// backend snapshot.
    fn tracker_snapshot(&self) -> Result<TrackerSnapshot, RepositoryError>;

    fn rename_task(
        &self,
        id: TaskId,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, RepositoryError>;
    fn archive_task(&self, id: TaskId, occurred_at: DateTime<Utc>)
    -> Result<Task, RepositoryError>;
    fn unarchive_task(
        &self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, RepositoryError>;
}

/// Persistence needed to read and change worklog history.
pub trait WorklogRepository {
    fn find_worklog(&self, id: WorklogId) -> Result<Option<Worklog>, RepositoryError>;

    /// Atomically moves a worklog when its task and timestamps exactly match
    /// the caller's expected values.
    fn compare_and_move_worklog(
        &self,
        id: WorklogId,
        expected_source_task_id: TaskId,
        expected: WorklogTimes,
        destination_task_id: TaskId,
    ) -> Result<WorklogMove, RepositoryError>;

    /// Atomically replaces a worklog's timestamps when its stored timestamps
    /// exactly match `expected`.
    fn compare_and_set_worklog_times(
        &self,
        id: WorklogId,
        expected: WorklogTimes,
        replacement: WorklogTimes,
    ) -> Result<WorklogCorrection, RepositoryError>;

    /// Atomically deletes a completed worklog when its task and timestamps
    /// exactly match the caller's expected values.
    fn compare_and_delete_completed_worklog(
        &self,
        id: WorklogId,
        expected_task_id: TaskId,
        expected: WorklogTimes,
    ) -> Result<WorklogDeletion, RepositoryError>;

    /// One bounded page of task history plus only the page-adoption state
    /// from the same read transaction. A continuation cursor becomes invalid
    /// when another client changes an existing row's start time.
    fn worklog_page(
        &self,
        task_id: TaskId,
        after: Option<&WorklogCursor>,
    ) -> Result<WorklogPage, RepositoryError>;
}

/// Persistence needed by tracking commands.
pub trait TrackingRepository {
    fn insert_worklog(&self, worklog: &Worklog) -> Result<(), RepositoryError>;
    /// Stops the active worklog only when its stored start still matches
    /// `expected_start`.
    fn stop_worklog(
        &self,
        id: WorklogId,
        expected_start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Worklog, RepositoryError>;
    /// Stops the active worklog only when its stored start still matches
    /// `expected_start`, then creates `next` in the same transaction.
    fn switch_worklog(
        &self,
        id: WorklogId,
        expected_start: DateTime<Utc>,
        stop_at: DateTime<Utc>,
        next: &Worklog,
    ) -> Result<(), RepositoryError>;
}

/// A backend that supports every current use case.
pub trait TrackerRepository: TaskRepository + WorklogRepository + TrackingRepository {}

impl<T> TrackerRepository for T where T: TaskRepository + WorklogRepository + TrackingRepository {}
