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
use tracker_domain::{InactivityPeriod, Task, TaskId, TaskName, Worklog, WorklogId, WorklogTimes};

use crate::{
    GlobalWorklogCursor, GlobalWorklogPage, ReportRow, TaskListItem, WorklogCursor, WorklogPage,
};

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
    #[error("global worklog history changed since this page was read")]
    GlobalWorklogHistoryChanged,
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
    #[error("inactive task candidates changed since preview")]
    InactiveTaskCandidatesChanged,
    #[error("{message}")]
    Constraint { message: String },
    #[error("stored data is invalid: {field}")]
    CorruptData { field: &'static str },
    #[error("{message}")]
    Backend { message: String },
    #[error("report duration exceeds the supported range")]
    ReportDurationOverflow,
}

/// Active tracking and its task metadata from the same backend read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveTrackingRead {
    pub active_worklog: Option<Worklog>,
    pub active_task_item: Option<TaskListItem>,
}

/// Preview candidates and active tracking from one backend read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InactiveTaskPreviewRead {
    pub tasks: Vec<Task>,
    pub tracking: ActiveTrackingRead,
}

/// Bulk archive rows and active tracking read before its transaction commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InactiveTaskArchive {
    pub tasks: Vec<Task>,
    pub tracking: ActiveTrackingRead,
}

/// Report totals and active tracking from one backend read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportRead {
    pub rows: Vec<ReportRow>,
    pub tracking: ActiveTrackingRead,
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
    /// Reads one task and its latest-work aggregate without loading the catalog.
    fn load_task_item(&self, id: TaskId) -> Result<Option<TaskListItem>, RepositoryError>;

    /// Reads only task metadata and per-task latest-work aggregates.
    fn load_task_catalog(&self) -> Result<Vec<TaskListItem>, RepositoryError>;

    fn create_task(&self, task: Task) -> Result<(), RepositoryError>;

    /// Reads task-list aggregates and global active tracking in one coherent
    /// backend snapshot.
    fn load_task_tracking_resources(
        &self,
    ) -> Result<(Vec<TaskListItem>, Option<Worklog>), RepositoryError>;

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

    /// Reads eligible tasks and active tracking from one backend transaction.
    fn preview_inactive_tasks(
        &self,
        as_of: DateTime<Utc>,
    ) -> Result<InactiveTaskPreviewRead, RepositoryError>;

    /// Rechecks the exact eligible identifier set and archives it atomically.
    fn archive_inactive_tasks(
        &self,
        expected_ids: &[TaskId],
        as_of: DateTime<Utc>,
    ) -> Result<InactiveTaskArchive, RepositoryError>;
}

/// Persistence for selecting and archiving tasks with a chosen inactivity period.
pub trait InactiveTaskRepository {
    fn preview_inactive_tasks_with_period(
        &self,
        as_of: DateTime<Utc>,
        period: InactivityPeriod,
    ) -> Result<InactiveTaskPreviewRead, RepositoryError>;

    /// Rechecks the exact eligible identifier set under the same write lock.
    fn archive_inactive_tasks_with_period(
        &self,
        expected_ids: &[TaskId],
        as_of: DateTime<Utc>,
        period: InactivityPeriod,
    ) -> Result<InactiveTaskArchive, RepositoryError>;
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

    /// Returns one global page and tracker state from one backend snapshot.
    /// A cursor becomes stale after any committed worklog write.
    fn global_worklog_page(
        &self,
        after: Option<&GlobalWorklogCursor>,
    ) -> Result<GlobalWorklogPage, RepositoryError>;
}

/// Persistence needed by tracking commands.
pub trait TrackingRepository {
    /// Reads only the global active worklog.
    fn active_worklog(&self) -> Result<Option<Worklog>, RepositoryError>;

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

/// A current read of time spent by task in one UTC interval.
pub trait ReportRepository {
    /// Reads the explicitly requested task list, totals, and active tracking
    /// under one backend transaction for the task-list totals workflow.
    fn task_list_report_read(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<(Vec<TaskListItem>, ReportRead), RepositoryError>;

    /// Returns positive task totals and tracker state from one read snapshot.
    /// Open worklogs end at `now` for the report calculation.
    fn report_read(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<ReportRead, RepositoryError>;
}

/// A backend that supports every current use case.
pub trait TrackerRepository:
    TaskRepository + WorklogRepository + TrackingRepository + ReportRepository
{
}

impl<T> TrackerRepository for T where
    T: TaskRepository + WorklogRepository + TrackingRepository + ReportRepository
{
}
