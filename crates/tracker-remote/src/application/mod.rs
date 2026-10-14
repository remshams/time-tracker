//! Remote resource observations and shared command recovery.

mod coordinator;
mod decoding;
mod error;
mod reports;
mod session;
mod tasks;
mod tracking;
mod worklogs;

use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use crate::{RemoteError, transport::Transport};
#[cfg(test)]
use chrono::TimeDelta;
use chrono::{DateTime, Utc};
use reqwest::{Method, StatusCode};
use tracker_application::{
    ApplicationError, ApplicationFailureCategory, ApplicationFailureSource, ClearActiveTaskOutcome,
    GlobalWorklogCursor, GlobalWorklogPage, MoveCandidate, SetActiveTaskOutcome, TaskListItem,
    TaskOrdering, WorklogCursor, WorklogPage,
};
use tracker_domain::{
    ActiveWorklog, InactivityPeriod, Task, TaskId, TaskName, Tracker, TrackingState, Worklog,
    WorklogId, WorklogTimes,
};
use tracker_protocol::{
    ArchiveInactiveTasksRequest, CreateTaskRequest, DailyOverviewDto, DeleteWorklogRequest,
    ErrorCode, ErrorDto, HealthDto, InactiveTaskPreviewDto, MutationDto, MutationResultDto,
    ReportDto, SetTrackingRequest, TaskChangeRequest, TaskDto, TaskListStateDto, TaskResourceDto,
    TasksDto, TrackingDto, WorklogChangeRequest, WorklogDto, WorklogPageDto, WorklogResourceDto,
    WriteGuard,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteFailureKind {
    Unavailable,
    Conflict,
    Protocol,
}

/// A confirmed resource value and the opaque revision observed with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceObservation<T> {
    pub value: T,
    pub revision: String,
}

/// Resources requested by an explicit client workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceSelection {
    Tasks,
    Tracking,
    TaskList,
    DailyTotals {
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    },
    TaskListWithTotals {
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    },
}

/// The remote tracker coordinator, retained under the existing client API name.
pub use coordinator::RemoteTrackerCoordinator as RemoteApplication;

use reports::ReportsClient;
use session::RemoteSession;
use tasks::TasksClient;
use tracking::TrackingClient;
use worklogs::WorklogsClient;

#[derive(Clone, Copy)]
enum Recovery {
    Tasks,
    Tracking,
    Worklog,
}

#[derive(Clone, Copy)]
struct MutationContext {
    worklog_id: Option<WorklogId>,
    task_id: Option<TaskId>,
}

#[derive(Clone, Copy)]
struct MutationScope {
    recovery: Recovery,
    worklog_id: Option<WorklogId>,
}

struct HistoryRead {
    worklogs: Vec<Worklog>,
    next_cursor: Option<GlobalWorklogCursor>,
}

use decoding::{
    app_decode_worklog, clear_tracking_result, decode_history, decode_task_item,
    decode_task_list_state, decode_tasks, decode_tracking, decode_worklog, set_tracking_result,
    task_result, validate_inactive_preview, validate_report, worklog_result,
};
use error::{classify_error, map_application_error, protocol_failure};

fn creation_needs_recovery(error: &ApplicationError, receipt_confirmed: bool) -> bool {
    receipt_confirmed
        || matches!(
            error.failure().source(),
            ApplicationFailureSource::RemoteUnavailable | ApplicationFailureSource::RemoteProtocol
        )
}

fn reviewed_tracking_changed(
    reviewed: Option<&TrackingState>,
    current: &TrackingState,
    reviewed_task: Option<&Task>,
    current_task: Option<&TaskListItem>,
) -> bool {
    reviewed.is_some_and(|old| old != current)
        || current_task.is_some_and(|item| reviewed_task.is_some_and(|old| old != &item.task))
}

fn original_worklog_matches(
    worklog: &Worklog,
    expected_task: Option<TaskId>,
    expected: WorklogTimes,
) -> bool {
    expected_task.is_none_or(|id| id == worklog.task_id())
        && worklog.start() == canonical(expected.start())
        && worklog.end() == expected.end().map(canonical)
}

fn report_cooldown_active(last: Option<Instant>, now: Instant) -> bool {
    last.is_some_and(|at| now.saturating_duration_since(at) < Duration::from_secs(5))
}

fn canonical(at: DateTime<Utc>) -> DateTime<Utc> {
    DateTime::from_timestamp_micros(at.timestamp_micros()).expect("UTC timestamp fits microseconds")
}

fn active_id(state: &TrackingState) -> Option<WorklogId> {
    match state {
        TrackingState::Idle => None,
        TrackingState::Running { worklog } => Some(worklog.id()),
    }
}

#[cfg(test)]
mod id_validation_tests;
#[cfg(test)]
mod resource_tests;
#[cfg(test)]
mod tests;
