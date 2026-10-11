//! Remote resource observations and shared command recovery.

mod decoding;
mod error;
mod reports;
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

/// Remote resources retain the revision of their own confirmed observation.
pub struct RemoteApplication {
    transport: Transport,
    task_observation: Option<ResourceObservation<Vec<TaskListItem>>>,
    tracking_observation: Option<ResourceObservation<TrackingState>>,
    task_cache: HashMap<TaskId, (TaskListItem, String)>,
    last_command_receipt: Option<MutationDto>,
    report_cache: Option<ReportDto>,
    history_cache: HashMap<Option<TaskId>, WorklogPageDto>,
    worklog_cache: HashMap<WorklogId, WorklogResourceDto>,
    last_failure: Option<RemoteFailureKind>,
    last_unavailable_at: Option<Instant>,
    version_checked: bool,
    pending_create: Option<CreateTaskRequest>,
    last_write_attempted: bool,
    coherent_task_views: bool,
}

#[derive(Clone, Copy)]
enum Recovery {
    Tasks,
    Tracking,
    Worklog,
}

#[derive(Clone, Copy)]
struct MutationScope {
    recovery: Recovery,
    worklog_id: Option<WorklogId>,
    task_id: Option<TaskId>,
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

impl RemoteApplication {
    pub fn disconnected(endpoint: &str) -> Result<Self, RemoteError> {
        Ok(Self {
            transport: Transport::new(endpoint)?,
            task_observation: None,
            tracking_observation: None,
            task_cache: HashMap::new(),
            last_command_receipt: None,
            report_cache: None,
            history_cache: HashMap::new(),
            worklog_cache: HashMap::new(),
            last_failure: Some(RemoteFailureKind::Unavailable),
            last_unavailable_at: None,
            version_checked: false,
            pending_create: None,
            last_write_attempted: false,
            coherent_task_views: false,
        })
    }

    /// Keeps task commands and their preflights within a coherent task and tracking view.
    pub fn with_coherent_task_views(mut self) -> Self {
        self.coherent_task_views = true;
        self
    }

    /// Checks compatibility without loading tasks or tracking.
    pub async fn connect(endpoint: &str) -> Result<Self, RemoteError> {
        let mut client = Self::disconnected(endpoint)?;
        client.check_connection().await?;
        Ok(client)
    }

    pub async fn check_connection(&mut self) -> Result<(), RemoteError> {
        let health: HealthDto = self.read("v1/health").await?;
        if health.protocol_version != tracker_protocol::VERSION || health.status != "ok" {
            return Err(self.record_error(RemoteError::Protocol(
                "server protocol version does not match".into(),
            )));
        }
        self.version_checked = true;
        self.confirmed();
        Ok(())
    }

    async fn ensure_version(&mut self) -> Result<(), RemoteError> {
        if !self.version_checked {
            self.check_connection().await?;
        }
        Ok(())
    }

    async fn read<T: serde::de::DeserializeOwned>(&mut self, path: &str) -> Result<T, RemoteError> {
        self.transport
            .send(Method::GET, self.transport.url(path), None::<&()>)
            .await
            .map_err(|error| self.record_error(error))
    }

    fn confirmed(&mut self) {
        self.last_failure = None;
        self.last_unavailable_at = None;
    }

    fn record_error(&mut self, error: RemoteError) -> RemoteError {
        self.last_failure = Some(classify_error(&error));
        if error.is_unavailable() {
            self.last_unavailable_at = Some(Instant::now());
        }
        error
    }

    /// Adopts the task catalog and tracking from one coherent server read.
    pub async fn refresh_task_list(&mut self) -> Result<(), RemoteError> {
        self.ensure_version().await?;
        let dto: TaskListStateDto = self.read("v1/state/task-list").await?;
        let (items, state, revision) =
            decode_task_list_state(dto).map_err(|error| self.record_error(error))?;
        self.adopt_task_list(items, state, revision);
        self.confirmed();
        Ok(())
    }

    fn adopt_task_list(
        &mut self,
        items: Vec<TaskListItem>,
        state: TrackingState,
        revision: String,
    ) {
        self.task_cache.clear();
        self.task_observation = Some(ResourceObservation {
            value: items,
            revision: revision.clone(),
        });
        self.tracking_observation = Some(ResourceObservation {
            value: state,
            revision,
        });
    }

    /// Refreshes only the selected resources. Task-list observations publish together.
    pub async fn refresh_resources(
        &mut self,
        selection: ResourceSelection,
    ) -> Result<(), RemoteError> {
        match selection {
            ResourceSelection::Tasks => self.refresh_tasks().await,
            ResourceSelection::Tracking => self.refresh_tracking().await,
            ResourceSelection::TaskList => self.refresh_task_list().await,
            ResourceSelection::DailyTotals { start, end, now } => {
                self.refresh_totals_resources(start, end, now, false).await
            }
            ResourceSelection::TaskListWithTotals { start, end, now } => {
                self.refresh_totals_resources(start, end, now, true).await
            }
        }
    }

    async fn refresh_totals_resources(
        &mut self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
        include_tasks: bool,
    ) -> Result<(), RemoteError> {
        if end <= start {
            return Err(self.record_error(RemoteError::Protocol("invalid report range".into())));
        }
        if report_cooldown_active(self.last_unavailable_at, Instant::now()) {
            return Err(RemoteError::Unavailable(
                "tracker server is unavailable".into(),
            ));
        }
        self.ensure_version().await?;
        let (start, end, now) = (canonical(start), canonical(end), canonical(now));
        let mut url = self.transport.url("v1/state/daily-overview");
        url.query_pairs_mut()
            .append_pair("start", &start.to_rfc3339())
            .append_pair("end", &end.to_rfc3339())
            .append_pair("now", &now.to_rfc3339());
        let dto: DailyOverviewDto = self
            .transport
            .send(Method::GET, url, None::<&()>)
            .await
            .map_err(|error| self.record_error(error))?;
        validate_report(&dto.totals)
            .map_err(|error| self.record_error(RemoteError::Protocol(error.to_string())))?;
        if dto.totals.start != start || dto.totals.end != end || dto.totals.now != now {
            return Err(self.record_error(RemoteError::Protocol(
                "report context does not match request".into(),
            )));
        }
        let (items, state, revision) = decode_task_list_state(TaskListStateDto {
            tasks: dto.tasks,
            active_worklog: dto.active_worklog,
            revision: dto.totals.revision.clone(),
        })
        .map_err(|error| self.record_error(error))?;
        if include_tasks {
            self.adopt_task_list(items, state, revision);
        } else {
            self.tracking_observation = Some(ResourceObservation {
                value: state,
                revision,
            });
        }
        self.report_cache = Some(dto.totals);
        self.confirmed();
        Ok(())
    }

    pub fn last_write_attempted(&self) -> bool {
        self.last_write_attempted
    }

    pub fn last_failure(&self) -> Option<RemoteFailureKind> {
        self.last_failure
    }

    pub fn last_command_receipt(&self) -> Option<&MutationDto> {
        self.last_command_receipt.as_ref()
    }

    fn guard(revision: &str) -> WriteGuard {
        WriteGuard {
            expected_revision: revision.to_owned(),
            request_id: uuid::Uuid::now_v7().to_string(),
        }
    }

    fn intent_changed(&mut self) -> ApplicationError {
        self.last_failure = Some(RemoteFailureKind::Conflict);
        ApplicationError::semantic_failure(
            ApplicationFailureCategory::General,
            "Tracker state changed. Refresh and retry.",
        )
    }

    async fn refresh_task_command_view(&mut self) -> Result<(), RemoteError> {
        if self.coherent_task_views {
            self.refresh_task_list().await
        } else {
            self.refresh_tasks().await
        }
    }

    async fn recover(&mut self, recovery: Recovery) -> Result<(), RemoteError> {
        match recovery {
            Recovery::Tasks => self.refresh_task_command_view().await,
            Recovery::Tracking | Recovery::Worklog => self.refresh_task_list().await,
        }
    }

    async fn mutation<B: serde::Serialize, T>(
        &mut self,
        method: Method,
        path: &str,
        body: &B,
        guard: &WriteGuard,
        scope: MutationScope,
        decode: impl FnOnce(MutationResultDto) -> Result<T, ApplicationError>,
    ) -> Result<T, ApplicationError> {
        self.ensure_version()
            .await
            .map_err(|error| map_application_error(&error, scope.worklog_id, scope.task_id))?;
        self.last_write_attempted = true;
        let response = self
            .transport
            .send(method, self.transport.url(path), Some(body))
            .await;
        let result = self.decode_receipt(response, guard, scope, decode);
        let recovered = self.recover_mutation(scope).await;
        match (result, recovered) {
            (Ok(value), Ok(())) => {
                self.confirmed();
                Ok(value)
            }
            // A validated receipt confirms the write even if resource recovery fails.
            (Ok(value), Err(_)) => Ok(value),
            (Err(error), Ok(())) => Err(error),
            (Err(error), Err(recovery_error)) => {
                Err(error.with_recovery_failure(map_application_error(&recovery_error, None, None)))
            }
        }
    }

    fn decode_receipt<T>(
        &mut self,
        response: Result<MutationDto, RemoteError>,
        guard: &WriteGuard,
        scope: MutationScope,
        decode: impl FnOnce(MutationResultDto) -> Result<T, ApplicationError>,
    ) -> Result<T, ApplicationError> {
        match response {
            Ok(dto) if dto.request_id == guard.request_id && !dto.applied_revision.is_empty() => {
                let result = decode(dto.result.clone())?;
                self.last_command_receipt = Some(dto);
                Ok(result)
            }
            Ok(_) => Err(protocol_failure("invalid command receipt")),
            Err(error) => {
                self.record_error(error.clone());
                Err(map_application_error(
                    &error,
                    scope.worklog_id,
                    scope.task_id,
                ))
            }
        }
    }

    async fn recover_mutation(&mut self, scope: MutationScope) -> Result<(), RemoteError> {
        // Receipts describe a past command. Explicit reads own current caches.
        if matches!(scope.recovery, Recovery::Worklog)
            && let Some(id) = scope.worklog_id
        {
            match self.read_worklog(id).await {
                Ok(_) => {}
                Err(RemoteError::Http {
                    status: StatusCode::NOT_FOUND,
                    ..
                }) => {
                    self.worklog_cache.remove(&id);
                }
                Err(error) => return Err(error),
            }
        }
        self.recover(scope.recovery).await
    }
}

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
