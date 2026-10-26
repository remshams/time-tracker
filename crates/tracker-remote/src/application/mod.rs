//! Remote resource reads and shared command recovery.

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
use tracker_application::timestamps::canonical_timestamp as canonical;
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
    TasksDto, TrackingDto, WorklogChangeRequest, WorklogDto, WorklogPageDto, WriteGuard,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteFailureKind {
    Unavailable,
    Conflict,
    Protocol,
}

/// An inactive-task preview retained by the adapter for later confirmation.
///
/// Deserializing a saved token runs the same checks as decoding a server preview. A token
/// with an invalid period, a non-canonical cutoff, or candidates that disagree with the count
/// fails to load. Serialization writes the protocol DTO unchanged.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "InactiveTaskPreviewDto", into = "InactiveTaskPreviewDto")]
pub struct InactiveTaskPreview {
    dto: InactiveTaskPreviewDto,
    candidate_ids: Vec<TaskId>,
}

impl InactiveTaskPreview {
    fn validated(dto: InactiveTaskPreviewDto) -> Result<Self, ApplicationError> {
        let candidate_ids = validate_inactive_preview(&dto)?;
        Ok(Self { dto, candidate_ids })
    }
}

impl TryFrom<InactiveTaskPreviewDto> for InactiveTaskPreview {
    type Error = &'static str;

    fn try_from(dto: InactiveTaskPreviewDto) -> Result<Self, Self::Error> {
        Self::validated(dto).map_err(|_| "invalid inactive task preview")
    }
}

impl From<InactiveTaskPreview> for InactiveTaskPreviewDto {
    fn from(preview: InactiveTaskPreview) -> Self {
        preview.dto
    }
}

impl InactiveTaskPreview {
    /// Returns the preview cutoff retained for later confirmation.
    pub fn as_of(&self) -> DateTime<Utc> {
        self.dto.as_of
    }

    /// Returns the inactivity threshold retained by the preview.
    pub fn inactive_days(&self) -> u32 {
        self.dto.inactive_days
    }

    /// Returns the candidate count reported with the captured preview.
    pub fn count(&self) -> usize {
        self.dto.count
    }
}

pub use coordinator::RemoteApplication;

use reports::ReportsClient;
use session::RemoteSession;
use tasks::TasksClient;
use tracking::TrackingClient;
use worklogs::WorklogsClient;

#[derive(Clone, Copy)]
enum Recovery {
    Tasks,
    Tracking,
}

#[derive(Clone, Copy)]
struct MutationContext {
    worklog_id: Option<WorklogId>,
    task_id: Option<TaskId>,
}

struct HistoryRead {
    worklogs: Vec<Worklog>,
    next_cursor: Option<GlobalWorklogCursor>,
}

use decoding::{
    clear_tracking_result, decode_active, decode_history, decode_task_item, decode_task_list_state,
    decode_tasks, set_tracking_result, task_result, validate_inactive_preview, validate_report,
    worklog_result,
};
use error::{classify_error, map_application_error, protocol_failure};

fn creation_needs_recovery(error: &ApplicationError, receipt_confirmed: bool) -> bool {
    receipt_confirmed
        || matches!(
            error.failure().source(),
            ApplicationFailureSource::RemoteUnavailable | ApplicationFailureSource::RemoteProtocol
        )
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

/// Bounds repeated report attempts for five seconds after a server availability failure.
fn report_cooldown_active(last: Option<Instant>, now: Instant) -> bool {
    last.is_some_and(|at| now.saturating_duration_since(at) < Duration::from_secs(5))
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
