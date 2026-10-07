//! Versioned JSON messages shared by the server and remote client.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracker_application::{
    GlobalWorklogCursor, GlobalWorklogPage, ReportTotals, TaskListItem, TrackerSnapshot,
    WorklogCursor, WorklogPage,
};
use tracker_domain::{Task, TaskId, Worklog, WorklogId, WorklogTimes};

pub const VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HealthDto {
    pub status: String,
    pub protocol_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskDto {
    pub id: String,
    pub name: String,
    pub archived: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<&Task> for TaskDto {
    fn from(task: &Task) -> Self {
        Self {
            id: task.id().to_string(),
            name: task.name().as_str().to_owned(),
            archived: task.is_archived(),
            created_at: task.created_at(),
            updated_at: task.updated_at(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorklogDto {
    pub id: String,
    pub task_id: String,
    pub start: DateTime<Utc>,
    pub end: Option<DateTime<Utc>>,
}

impl From<&Worklog> for WorklogDto {
    fn from(worklog: &Worklog) -> Self {
        Self {
            id: worklog.id().to_string(),
            task_id: worklog.task_id().to_string(),
            start: worklog.start(),
            end: worklog.end(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskItemDto {
    pub task: TaskDto,
    pub latest_work_start: Option<DateTime<Utc>>,
}

impl From<&TaskListItem> for TaskItemDto {
    fn from(item: &TaskListItem) -> Self {
        Self {
            task: TaskDto::from(&item.task),
            latest_work_start: item.latest_work_start,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SnapshotDto {
    pub task_items: Vec<TaskItemDto>,
    pub active_worklog: Option<WorklogDto>,
    pub revision: String,
}

impl SnapshotDto {
    pub fn from_snapshot(snapshot: &TrackerSnapshot, revision: String) -> Self {
        Self {
            task_items: snapshot.task_items.iter().map(TaskItemDto::from).collect(),
            active_worklog: snapshot.active_worklog.as_ref().map(WorklogDto::from),
            revision,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WriteGuard {
    pub expected_revision: String,
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InactiveTaskPreviewDto {
    pub as_of: DateTime<Utc>,
    pub count: usize,
    pub sample_names: Vec<String>,
    pub revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArchiveInactiveTasksRequest {
    pub as_of: DateTime<Utc>,
    #[serde(flatten)]
    pub guard: WriteGuard,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InactiveTaskCandidatesDto {
    pub as_of: DateTime<Utc>,
    pub inactive_days: u32,
    pub tasks: Vec<TaskDto>,
    pub revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArchiveInactiveCandidatesRequest {
    pub as_of: DateTime<Utc>,
    pub inactive_days: u32,
    #[serde(flatten)]
    pub guard: WriteGuard,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CreateTaskRequest {
    pub task_id: String,
    pub name: String,
    pub occurred_at: DateTime<Utc>,
    #[serde(flatten)]
    pub guard: WriteGuard,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum TaskChangeRequest {
    Rename {
        name: String,
        occurred_at: DateTime<Utc>,
        #[serde(flatten)]
        guard: WriteGuard,
    },
    Archive {
        occurred_at: DateTime<Utc>,
        #[serde(flatten)]
        guard: WriteGuard,
    },
    Restore {
        occurred_at: DateTime<Utc>,
        #[serde(flatten)]
        guard: WriteGuard,
    },
}

impl TaskChangeRequest {
    pub fn guard(&self) -> &WriteGuard {
        match self {
            Self::Rename { guard, .. }
            | Self::Archive { guard, .. }
            | Self::Restore { guard, .. } => guard,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SetTrackingRequest {
    pub task_id: Option<String>,
    pub worklog_id: Option<String>,
    pub expected_active: Option<String>,
    pub occurred_at: DateTime<Utc>,
    #[serde(flatten)]
    pub guard: WriteGuard,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorklogChangeRequest {
    Move {
        expected_task_id: String,
        expected_start: DateTime<Utc>,
        expected_end: Option<DateTime<Utc>>,
        destination_task_id: String,
        #[serde(flatten)]
        guard: WriteGuard,
    },
    Correct {
        expected_start: DateTime<Utc>,
        expected_end: Option<DateTime<Utc>>,
        replacement_start: DateTime<Utc>,
        replacement_end: Option<DateTime<Utc>>,
        occurred_at: DateTime<Utc>,
        #[serde(flatten)]
        guard: WriteGuard,
    },
}

impl WorklogChangeRequest {
    pub fn guard(&self) -> &WriteGuard {
        match self {
            Self::Move { guard, .. } | Self::Correct { guard, .. } => guard,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeleteWorklogRequest {
    pub expected_task_id: String,
    pub expected_start: DateTime<Utc>,
    pub expected_end: DateTime<Utc>,
    #[serde(flatten)]
    pub guard: WriteGuard,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum MutationResultDto {
    Task(TaskDto),
    ArchivedInactive {
        count: usize,
    },
    Worklog(WorklogDto),
    TrackingAlreadyActive(WorklogDto),
    TrackingSwitched {
        stopped: WorklogDto,
        started: WorklogDto,
    },
    TrackingAlreadyIdle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MutationDto {
    pub result: MutationResultDto,
    pub snapshot: SnapshotDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorklogCursorDto {
    pub task_id: String,
    pub start: DateTime<Utc>,
    pub id: String,
    pub revision: i64,
}

impl From<WorklogCursor> for WorklogCursorDto {
    fn from(cursor: WorklogCursor) -> Self {
        Self {
            task_id: cursor.task_id.to_string(),
            start: cursor.start,
            id: cursor.id.to_string(),
            revision: cursor.revision,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GlobalWorklogCursorDto {
    pub start: DateTime<Utc>,
    pub id: String,
    pub revision: i64,
}

impl From<GlobalWorklogCursor> for GlobalWorklogCursorDto {
    fn from(cursor: GlobalWorklogCursor) -> Self {
        Self {
            start: cursor.start,
            id: cursor.id.to_string(),
            revision: cursor.revision,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorklogPageDto {
    pub worklogs: Vec<WorklogDto>,
    pub requested_task_latest_work_start: Option<DateTime<Utc>>,
    pub active_worklog: Option<WorklogDto>,
    pub active_task_latest_work_start: Option<DateTime<Utc>>,
    pub next_cursor: Option<WorklogCursorDto>,
    pub revision: String,
}

impl WorklogPageDto {
    pub fn from_page(page: &WorklogPage, revision: String) -> Self {
        Self {
            worklogs: page.worklogs.iter().map(WorklogDto::from).collect(),
            requested_task_latest_work_start: page.snapshot.requested_task_latest_work_start,
            active_worklog: page.snapshot.active_worklog.as_ref().map(WorklogDto::from),
            active_task_latest_work_start: page.snapshot.active_task_latest_work_start,
            next_cursor: page.next_cursor.map(Into::into),
            revision,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GlobalWorklogPageDto {
    pub worklogs: Vec<WorklogDto>,
    pub snapshot: SnapshotDto,
    pub next_cursor: Option<GlobalWorklogCursorDto>,
}

impl GlobalWorklogPageDto {
    pub fn from_page(page: &GlobalWorklogPage, revision: String) -> Self {
        Self {
            worklogs: page.worklogs.iter().map(WorklogDto::from).collect(),
            snapshot: SnapshotDto::from_snapshot(&page.snapshot, revision),
            next_cursor: page.next_cursor.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReportRowDto {
    pub task: TaskDto,
    pub duration_us: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReportDto {
    pub rows: Vec<ReportRowDto>,
    pub total_us: i64,
    pub snapshot: SnapshotDto,
}

impl ReportDto {
    pub fn from_totals(totals: &ReportTotals, snapshot: SnapshotDto) -> Self {
        Self {
            rows: totals
                .rows
                .iter()
                .map(|row| ReportRowDto {
                    task: TaskDto::from(&row.task),
                    duration_us: row
                        .duration
                        .num_microseconds()
                        .expect("report duration validated by application"),
                })
                .collect(),
            total_us: totals
                .total
                .num_microseconds()
                .expect("report duration validated by application"),
            snapshot,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    NotFound,
    Conflict,
    StaleRevision,
    WorklogChanged,
    WorklogHistoryChanged,
    WorklogOverlap,
    ActiveWorklog,
    ActiveTask,
    InactiveTaskCandidatesChanged,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ErrorDto {
    pub code: ErrorCode,
    pub message: String,
}

pub fn parse_task_id(value: &str) -> Result<TaskId, &'static str> {
    value.parse().map_err(|_| "invalid task id")
}

pub fn parse_worklog_id(value: &str) -> Result<WorklogId, &'static str> {
    value.parse().map_err(|_| "invalid worklog id")
}

pub fn worklog_times(start: DateTime<Utc>, end: Option<DateTime<Utc>>) -> WorklogTimes {
    WorklogTimes::new(start, end)
}
