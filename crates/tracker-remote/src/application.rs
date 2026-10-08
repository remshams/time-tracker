use std::time::{Duration, Instant};

use chrono::{DateTime, TimeDelta, Utc};
use reqwest::{Method, StatusCode};
use tracker_application::{
    ApplicationError, ApplicationFailureCategory, ApplicationFailureSource, ClearActiveTaskOutcome,
    GlobalWorklogCursor, GlobalWorklogPage, MoveCandidate, ReportRow, ReportTotals,
    SetActiveTaskOutcome, TaskListItem, TaskOrdering, TrackerSnapshot, WorklogCursor, WorklogPage,
    WorklogPageSnapshot,
};
use tracker_domain::{
    ActiveWorklog, InactivityPeriod, Task, TaskId, TaskName, Tracker, TrackingState, Worklog,
    WorklogId, WorklogTimes,
};
use tracker_protocol::{
    ArchiveInactiveCandidatesRequest, ArchiveInactiveTasksRequest, CreateTaskRequest,
    DeleteWorklogRequest, ErrorCode, ErrorDto, GlobalWorklogCursorDto, GlobalWorklogPageDto,
    HealthDto, InactiveTaskCandidatesDto, InactiveTaskPreviewDto, MutationDto, MutationResultDto,
    ReportDto, SetTrackingRequest, SnapshotDto, TaskChangeRequest, TaskDto, WorklogChangeRequest,
    WorklogCursorDto, WorklogDto, WorklogPageDto, WriteGuard,
};

use crate::{RemoteError, transport::Transport};

/// The last failed operation's effect on remote availability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteFailureKind {
    Unavailable,
    Conflict,
    Protocol,
}

/// A cached application service backed only by the configured server.
///
/// Reads of tasks and current tracking use the last confirmed snapshot.
/// Explicit refreshes and successful server responses replace that snapshot.
pub struct RemoteApplication {
    transport: Transport,
    snapshot: TrackerSnapshot,
    tracking: TrackingState,
    revision: String,
    last_failure: Option<RemoteFailureKind>,
    last_unavailable_at: Option<Instant>,
    version_checked: bool,
    pending_create: Option<CreateTaskRequest>,
}

impl RemoteApplication {
    /// Creates an empty client that can reconnect through `refresh`.
    pub fn disconnected(endpoint: &str) -> Result<Self, RemoteError> {
        Ok(Self {
            transport: Transport::new(endpoint)?,
            snapshot: TrackerSnapshot {
                task_items: Vec::new(),
                active_worklog: None,
            },
            tracking: TrackingState::Idle,
            revision: String::new(),
            last_failure: Some(RemoteFailureKind::Unavailable),
            last_unavailable_at: None,
            version_checked: false,
            pending_create: None,
        })
    }

    pub async fn connect(endpoint: &str) -> Result<Self, RemoteError> {
        let mut client = Self::disconnected(endpoint)?;
        client.refresh().await?;
        Ok(client)
    }

    /// Reloads task aggregates and active tracking in one server read.
    pub async fn refresh(&mut self) -> Result<(), RemoteError> {
        let result = async {
            if !self.version_checked {
                let health: HealthDto = self
                    .transport
                    .send(Method::GET, self.transport.url("v1/health"), None::<&()>)
                    .await?;
                if health.protocol_version != tracker_protocol::VERSION || health.status != "ok" {
                    return Err(RemoteError::Protocol(
                        "server protocol version does not match".into(),
                    ));
                }
            }
            let dto: SnapshotDto = self
                .transport
                .send(Method::GET, self.transport.url("v1/snapshot"), None::<&()>)
                .await?;
            decode_snapshot(dto)
        }
        .await;
        match result {
            Ok((snapshot, tracking, revision)) => {
                self.snapshot = snapshot;
                self.tracking = tracking;
                self.revision = revision;
                self.last_failure = None;
                self.last_unavailable_at = None;
                self.version_checked = true;
                Ok(())
            }
            Err(error) => {
                self.last_failure = Some(classify_error(&error));
                if error.is_unavailable() {
                    self.last_unavailable_at = Some(Instant::now());
                }
                Err(error)
            }
        }
    }

    pub fn last_failure(&self) -> Option<RemoteFailureKind> {
        self.last_failure
    }

    pub fn snapshot(&self) -> &TrackerSnapshot {
        &self.snapshot
    }

    fn guard(&self) -> WriteGuard {
        WriteGuard {
            expected_revision: self.revision.clone(),
            request_id: uuid::Uuid::now_v7().to_string(),
        }
    }

    async fn mutation<B: serde::Serialize, T>(
        &mut self,
        method: Method,
        path: &str,
        body: &B,
        worklog_id: Option<WorklogId>,
        task_id: Option<TaskId>,
        decode_result: impl FnOnce(MutationResultDto) -> Result<T, ApplicationError>,
    ) -> Result<T, ApplicationError> {
        let response: Result<MutationDto, RemoteError> = self
            .transport
            .send(method, self.transport.url(path), Some(body))
            .await;
        let dto = match response {
            Ok(dto) => dto,
            Err(error) => return Err(self.operation_error(error, worklog_id, task_id).await),
        };
        let result = decode_result(dto.result).inspect_err(|_| {
            self.last_failure = Some(RemoteFailureKind::Protocol);
        })?;
        let (snapshot, tracking, revision) = match decode_snapshot(dto.snapshot) {
            Ok(snapshot) => snapshot,
            Err(error) => return Err(self.operation_error(error, worklog_id, task_id).await),
        };
        self.snapshot = snapshot;
        self.tracking = tracking;
        self.revision = revision;
        self.last_failure = None;
        self.last_unavailable_at = None;
        Ok(result)
    }

    async fn operation_error(
        &mut self,
        error: RemoteError,
        worklog_id: Option<WorklogId>,
        task_id: Option<TaskId>,
    ) -> ApplicationError {
        self.last_failure = Some(classify_error(&error));
        if error.is_unavailable() {
            self.last_unavailable_at = Some(Instant::now());
        }
        let mapped = map_application_error(&error, worklog_id, task_id);
        if matches!(error, RemoteError::Http { .. })
            && let Err(recovery) = self.refresh().await
        {
            return mapped.with_recovery_failure(map_application_error(&recovery, None, None));
        }
        mapped
    }
}

impl RemoteApplication {
    pub fn tasks(&self, ordering: TaskOrdering) -> Vec<TaskListItem> {
        let mut items = self.snapshot.task_items.clone();
        ordering.sort_items(&mut items);
        items
    }

    /// Selects destinations from the last confirmed snapshot without network IO.
    pub fn move_candidates(&self, source_task_id: TaskId, query: &str) -> Vec<MoveCandidate> {
        tracker_application::move_candidates_for_tasks(
            &self.snapshot.task_items,
            source_task_id,
            query,
        )
    }

    pub fn task(&self, id: TaskId) -> Option<&Task> {
        self.snapshot
            .task_items
            .iter()
            .find(|item| item.task.id() == id)
            .map(|item| &item.task)
    }
}

impl RemoteApplication {
    pub async fn create_task(
        &mut self,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        if let Some(mut pending) = self.pending_create.clone() {
            if pending.name != name.as_str() {
                return Err(ApplicationError::semantic_failure(
                    ApplicationFailureCategory::General,
                    "Recover the pending task creation before creating another task",
                ));
            }
            // Pending intent survives polling, which can clear last_failure.
            self.refresh()
                .await
                .map_err(|error| map_application_error(&error, None, None))?;
            let pending_id: TaskId = pending
                .task_id
                .parse()
                .map_err(|_| protocol_failure("invalid pending task id"))?;
            if let Some(task) = self.task(pending_id).cloned() {
                self.pending_create = None;
                return Ok(task);
            } else {
                // The server has no record of the uncertain create. Retain
                // its task ID but use the freshly loaded server revision.
                pending.guard = self.guard();
                self.pending_create = Some(pending);
            }
        }
        let body = self.pending_create.clone().unwrap_or_else(|| {
            let id = TaskId::generate();
            CreateTaskRequest {
                task_id: id.to_string(),
                name: name.as_str().to_owned(),
                occurred_at: canonical(occurred_at),
                guard: self.guard(),
            }
        });
        let id: TaskId = body
            .task_id
            .parse()
            .map_err(|_| protocol_failure("invalid pending task id"))?;
        self.pending_create = Some(body.clone());
        let result = self
            .mutation(Method::POST, "v1/tasks", &body, None, Some(id), |result| {
                task_result(result, id)
            })
            .await
            .and_then(|task| {
                if self.task(task.id()).is_none() {
                    self.last_failure = Some(RemoteFailureKind::Protocol);
                    Err(protocol_failure(
                        "created task is missing from the returned snapshot",
                    ))
                } else {
                    Ok(task)
                }
            });
        let uncertain = result.as_ref().is_err_and(|error| {
            matches!(
                error.failure().source(),
                ApplicationFailureSource::RemoteUnavailable
                    | ApplicationFailureSource::RemoteProtocol
            )
        });
        if !uncertain {
            self.pending_create = None;
        }
        result
    }

    pub async fn rename_task(
        &mut self,
        id: TaskId,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        let body = TaskChangeRequest::Rename {
            name: name.as_str().to_owned(),
            occurred_at: canonical(occurred_at),
            guard: self.guard(),
        };
        self.mutation(
            Method::PATCH,
            &format!("v1/tasks/{id}"),
            &body,
            None,
            Some(id),
            |result| task_result(result, id),
        )
        .await
    }

    pub async fn archive_task(
        &mut self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        let body = TaskChangeRequest::Archive {
            occurred_at: canonical(occurred_at),
            guard: self.guard(),
        };
        self.mutation(
            Method::PATCH,
            &format!("v1/tasks/{id}"),
            &body,
            None,
            Some(id),
            |result| task_result(result, id),
        )
        .await
    }

    pub async fn unarchive_task(
        &mut self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        let body = TaskChangeRequest::Restore {
            occurred_at: canonical(occurred_at),
            guard: self.guard(),
        };
        self.mutation(
            Method::PATCH,
            &format!("v1/tasks/{id}"),
            &body,
            None,
            Some(id),
            |result| task_result(result, id),
        )
        .await
    }

    pub async fn preview_inactive_tasks(
        &mut self,
        as_of: DateTime<Utc>,
    ) -> Result<InactiveTaskPreviewDto, ApplicationError> {
        let as_of = canonical(as_of);
        let mut url = self.transport.url("v1/tasks/inactive-preview");
        url.query_pairs_mut()
            .append_pair("as_of", &as_of.to_rfc3339());
        let response: Result<InactiveTaskPreviewDto, RemoteError> =
            self.transport.send(Method::GET, url, None::<&()>).await;
        let preview = match response {
            Ok(preview) => preview,
            Err(error) => return Err(self.operation_error(error, None, None).await),
        };
        if preview.as_of != as_of
            || preview.sample_names.len() > 5
            || preview.sample_names.len() > preview.count
            || preview
                .sample_names
                .iter()
                .any(|name| TaskName::new(name).is_err())
            || preview.revision.is_empty()
        {
            self.last_failure = Some(RemoteFailureKind::Protocol);
            return Err(protocol_failure("invalid inactive task preview"));
        }
        self.last_failure = None;
        self.last_unavailable_at = None;
        Ok(preview)
    }

    pub async fn archive_inactive_tasks(
        &mut self,
        preview: &InactiveTaskPreviewDto,
    ) -> Result<usize, ApplicationError> {
        if preview.revision.is_empty() {
            return Err(protocol_failure("invalid inactive task preview"));
        }
        let body = ArchiveInactiveTasksRequest {
            as_of: preview.as_of,
            guard: WriteGuard {
                expected_revision: preview.revision.clone(),
                request_id: uuid::Uuid::now_v7().to_string(),
            },
        };
        self.mutation(
            Method::POST,
            "v1/tasks/archive-inactive",
            &body,
            None,
            None,
            |result| match result {
                MutationResultDto::ArchivedInactive { count } => Ok(count),
                _ => Err(protocol_failure("invalid inactive task archive result")),
            },
        )
        .await
    }

    /// Loads all inactive candidates for the chosen period from the server.
    pub async fn preview_inactive_tasks_with_period(
        &mut self,
        as_of: DateTime<Utc>,
        period: InactivityPeriod,
    ) -> Result<InactiveTaskCandidatesDto, ApplicationError> {
        let as_of = canonical(as_of);
        let mut url = self.transport.url("v1/tasks/inactive-candidates");
        url.query_pairs_mut()
            .append_pair("as_of", &as_of.to_rfc3339())
            .append_pair("inactive_days", &period.days().to_string());
        let response: Result<InactiveTaskCandidatesDto, RemoteError> =
            self.transport.send(Method::GET, url, None::<&()>).await;
        let preview = match response {
            Ok(preview) => preview,
            Err(error) => return Err(self.operation_error(error, None, None).await),
        };
        if preview.as_of != as_of
            || preview.inactive_days != period.days()
            || validate_inactive_candidates(&preview).is_err()
        {
            self.last_failure = Some(RemoteFailureKind::Protocol);
            return Err(protocol_failure("invalid inactive task candidates"));
        }
        self.last_failure = None;
        self.last_unavailable_at = None;
        Ok(preview)
    }

    /// Archives the server's current candidates under the preview's revision.
    pub async fn archive_inactive_tasks_with_period(
        &mut self,
        preview: &InactiveTaskCandidatesDto,
    ) -> Result<usize, ApplicationError> {
        validate_inactive_candidates(preview)?;
        let body = ArchiveInactiveCandidatesRequest {
            as_of: preview.as_of,
            inactive_days: preview.inactive_days,
            guard: WriteGuard {
                expected_revision: preview.revision.clone(),
                request_id: uuid::Uuid::now_v7().to_string(),
            },
        };
        self.mutation(
            Method::POST,
            "v1/tasks/archive-inactive-candidates",
            &body,
            None,
            None,
            |result| match result {
                MutationResultDto::ArchivedInactive { count } => Ok(count),
                _ => Err(protocol_failure("invalid inactive task archive result")),
            },
        )
        .await
    }
}

fn validate_inactive_candidates(
    preview: &InactiveTaskCandidatesDto,
) -> Result<(), ApplicationError> {
    if preview.revision.is_empty()
        || InactivityPeriod::new(preview.inactive_days).is_err()
        || preview.as_of != canonical(preview.as_of)
    {
        return Err(protocol_failure("invalid inactive task candidates"));
    }
    let mut ids = std::collections::HashSet::new();
    for dto in &preview.tasks {
        let task = decode_task(dto.clone())
            .map_err(|_| protocol_failure("invalid inactive task candidates"))?;
        if task.is_archived()
            || task.id().to_string() != dto.id
            || task.name().as_str() != dto.name
            || !ids.insert(task.id())
        {
            return Err(protocol_failure("invalid inactive task candidates"));
        }
    }
    Ok(())
}

impl RemoteApplication {
    pub fn current_tracking(&self) -> &TrackingState {
        &self.tracking
    }

    pub async fn set_active_task(
        &mut self,
        task_id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, ApplicationError> {
        let old_active = active_id(&self.tracking);
        let new_id = WorklogId::generate();
        let body = SetTrackingRequest {
            task_id: Some(task_id.to_string()),
            worklog_id: Some(new_id.to_string()),
            expected_active: old_active.map(|id| id.to_string()),
            occurred_at: canonical(occurred_at),
            guard: self.guard(),
        };
        self.mutation(
            Method::PUT,
            "v1/tracking",
            &body,
            None,
            Some(task_id),
            |result| set_tracking_result(result, task_id, new_id, old_active),
        )
        .await
    }

    pub async fn clear_active_task(
        &mut self,
        expected_active: WorklogId,
        occurred_at: DateTime<Utc>,
    ) -> Result<ClearActiveTaskOutcome, ApplicationError> {
        let body = SetTrackingRequest {
            task_id: None,
            worklog_id: None,
            expected_active: active_id(&self.tracking).map(|_| expected_active.to_string()),
            occurred_at: canonical(occurred_at),
            guard: self.guard(),
        };
        self.mutation(
            Method::PUT,
            "v1/tracking",
            &body,
            Some(expected_active),
            None,
            |result| clear_tracking_result(result, expected_active),
        )
        .await
    }
}

impl RemoteApplication {
    pub async fn worklogs_for_task(
        &mut self,
        task_id: TaskId,
        after: Option<&WorklogCursor>,
    ) -> Result<WorklogPage, ApplicationError> {
        let mut url = self.transport.url(&format!("v1/tasks/{task_id}/worklogs"));
        if let Some(cursor) = after {
            if cursor.task_id != task_id {
                return Err(ApplicationError::worklog_history_changed(task_id));
            }
            url.query_pairs_mut()
                .append_pair("after_start", &cursor.start.to_rfc3339())
                .append_pair("after_id", &cursor.id.to_string())
                .append_pair("after_revision", &cursor.revision.to_string());
        }
        let dto: WorklogPageDto = match self.transport.send(Method::GET, url, None::<&()>).await {
            Ok(dto) => dto,
            Err(error) => return Err(self.operation_error(error, None, Some(task_id)).await),
        };
        let worklogs = dto
            .worklogs
            .into_iter()
            .map(app_decode_worklog)
            .collect::<Result<Vec<_>, _>>()?;
        let (active, tracking) = match decode_active(dto.active_worklog) {
            Ok(decoded) => decoded,
            Err(error) => return Err(self.operation_error(error, None, Some(task_id)).await),
        };
        let next_cursor = match dto.next_cursor.map(decode_cursor).transpose() {
            Ok(cursor) => cursor,
            Err(error) => return Err(self.operation_error(error, None, Some(task_id)).await),
        };
        if next_cursor
            .as_ref()
            .is_some_and(|cursor| cursor.task_id != task_id)
        {
            return Err(protocol_failure("cursor belongs to a different task"));
        }
        self.update_worklog_page_tracking(
            task_id,
            tracking,
            &active,
            dto.requested_task_latest_work_start,
            dto.active_task_latest_work_start,
        );
        // This page contains only part of the task catalog. Keep the last
        // full-snapshot revision so a later write cannot overwrite task
        // changes made by another client without first refreshing.
        self.last_failure = None;
        Ok(WorklogPage {
            worklogs,
            snapshot: WorklogPageSnapshot {
                requested_task_latest_work_start: dto.requested_task_latest_work_start,
                active_worklog: active,
                active_task_latest_work_start: dto.active_task_latest_work_start,
            },
            next_cursor,
        })
    }

    fn update_worklog_page_tracking(
        &mut self,
        task_id: TaskId,
        tracking: TrackingState,
        active: &Option<Worklog>,
        requested_task_latest_work_start: Option<DateTime<Utc>>,
        active_task_latest_work_start: Option<DateTime<Utc>>,
    ) {
        self.tracking = tracking;
        self.snapshot.active_worklog = active.clone();
        if let Some(item) = self
            .snapshot
            .task_items
            .iter_mut()
            .find(|item| item.task.id() == task_id)
        {
            item.latest_work_start = requested_task_latest_work_start;
        }
        if let Some(active) = active
            && let Some(item) = self
                .snapshot
                .task_items
                .iter_mut()
                .find(|item| item.task.id() == active.task_id())
        {
            item.latest_work_start = active_task_latest_work_start;
        }
    }

    pub async fn all_worklogs(
        &mut self,
        after: Option<&GlobalWorklogCursor>,
    ) -> Result<GlobalWorklogPage, ApplicationError> {
        let mut url = self.transport.url("v1/worklogs");
        if let Some(cursor) = after {
            url.query_pairs_mut()
                .append_pair("after_start", &cursor.start.to_rfc3339())
                .append_pair("after_id", &cursor.id.to_string())
                .append_pair("after_revision", &cursor.revision.to_string());
        }
        let dto: GlobalWorklogPageDto =
            match self.transport.send(Method::GET, url, None::<&()>).await {
                Ok(dto) => dto,
                Err(error) => return Err(self.operation_error(error, None, None).await),
            };
        let worklogs = dto
            .worklogs
            .into_iter()
            .map(app_decode_worklog)
            .collect::<Result<Vec<_>, _>>()?;
        let (snapshot, tracking, revision) = match decode_snapshot(dto.snapshot) {
            Ok(snapshot) => snapshot,
            Err(error) => return Err(self.operation_error(error, None, None).await),
        };
        let next_cursor = match dto.next_cursor.map(decode_global_cursor).transpose() {
            Ok(cursor) => cursor,
            Err(error) => return Err(self.operation_error(error, None, None).await),
        };
        self.snapshot = snapshot.clone();
        self.tracking = tracking;
        self.revision = revision;
        self.last_failure = None;
        Ok(GlobalWorklogPage {
            worklogs,
            snapshot,
            next_cursor,
        })
    }
}

impl RemoteApplication {
    pub async fn move_worklog(
        &mut self,
        id: WorklogId,
        expected_source_task_id: TaskId,
        expected: WorklogTimes,
        destination_task_id: TaskId,
    ) -> Result<Worklog, ApplicationError> {
        let body = WorklogChangeRequest::Move {
            expected_task_id: expected_source_task_id.to_string(),
            expected_start: canonical(expected.start()),
            expected_end: expected.end().map(canonical),
            destination_task_id: destination_task_id.to_string(),
            guard: self.guard(),
        };
        self.mutation(
            Method::PATCH,
            &format!("v1/worklogs/{id}"),
            &body,
            Some(id),
            None,
            |result| worklog_result(result, id),
        )
        .await
    }

    pub async fn correct_worklog(
        &mut self,
        id: WorklogId,
        expected: WorklogTimes,
        replacement: WorklogTimes,
        occurred_at: DateTime<Utc>,
    ) -> Result<Worklog, ApplicationError> {
        let body = WorklogChangeRequest::Correct {
            expected_start: canonical(expected.start()),
            expected_end: expected.end().map(canonical),
            replacement_start: canonical(replacement.start()),
            replacement_end: replacement.end().map(canonical),
            occurred_at: canonical(occurred_at),
            guard: self.guard(),
        };
        self.mutation(
            Method::PATCH,
            &format!("v1/worklogs/{id}"),
            &body,
            Some(id),
            None,
            |result| worklog_result(result, id),
        )
        .await
    }

    pub async fn delete_completed_worklog(
        &mut self,
        id: WorklogId,
        expected_task_id: TaskId,
        expected: WorklogTimes,
    ) -> Result<Worklog, ApplicationError> {
        let end = expected
            .end()
            .ok_or_else(|| ApplicationError::active_worklog(id))?;
        let body = DeleteWorklogRequest {
            expected_task_id: expected_task_id.to_string(),
            expected_start: canonical(expected.start()),
            expected_end: canonical(end),
            guard: self.guard(),
        };
        self.mutation(
            Method::DELETE,
            &format!("v1/worklogs/{id}"),
            &body,
            Some(id),
            None,
            |result| worklog_result(result, id),
        )
        .await
    }
}

impl RemoteApplication {
    pub async fn report_totals(
        &mut self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<ReportTotals, ApplicationError> {
        if end <= start {
            return Err(ApplicationError::InvalidReportRange);
        }
        if report_cooldown_active(self.last_unavailable_at, Instant::now()) {
            return Err(ApplicationError::RemoteUnavailable(
                "tracker server is unavailable".into(),
            ));
        }
        let mut url = self.transport.url("v1/reports");
        url.query_pairs_mut()
            .append_pair("start", &start.to_rfc3339())
            .append_pair("end", &end.to_rfc3339())
            .append_pair("now", &now.to_rfc3339());
        let dto: ReportDto = match self.transport.send(Method::GET, url, None::<&()>).await {
            Ok(dto) => dto,
            Err(error) => return Err(self.operation_error(error, None, None).await),
        };
        let totals = decode_report_rows(dto.rows, dto.total_us)?;
        let (snapshot, tracking, revision) = match decode_snapshot(dto.snapshot) {
            Ok(snapshot) => snapshot,
            Err(error) => return Err(self.operation_error(error, None, None).await),
        };
        self.snapshot = snapshot;
        self.tracking = tracking;
        self.revision = revision;
        self.last_failure = None;
        Ok(totals)
    }
}

fn decode_report_rows(
    row_dtos: Vec<tracker_protocol::ReportRowDto>,
    total_us: i64,
) -> Result<ReportTotals, ApplicationError> {
    let mut rows = Vec::with_capacity(row_dtos.len());
    let mut sum = 0_i64;
    for row in row_dtos {
        if row.duration_us <= 0 {
            return Err(protocol_failure("report row has invalid duration"));
        }
        sum = sum
            .checked_add(row.duration_us)
            .ok_or(ApplicationError::ReportDurationOverflow)?;
        rows.push(ReportRow {
            task: app_decode_task(row.task)?,
            duration: TimeDelta::microseconds(row.duration_us),
        });
    }
    if sum != total_us {
        return Err(protocol_failure("report total does not match rows"));
    }
    Ok(ReportTotals {
        rows,
        total: TimeDelta::microseconds(sum),
    })
}

fn report_cooldown_active(last_unavailable_at: Option<Instant>, now: Instant) -> bool {
    last_unavailable_at.is_some_and(|at| now.saturating_duration_since(at) < Duration::from_secs(5))
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

fn decode_task(dto: TaskDto) -> Result<Task, RemoteError> {
    let id = dto
        .id
        .parse()
        .map_err(|_| RemoteError::Protocol("invalid task id".into()))?;
    let name =
        TaskName::new(&dto.name).map_err(|_| RemoteError::Protocol("invalid task name".into()))?;
    Task::rehydrate(id, name, dto.archived, dto.created_at, dto.updated_at)
        .map_err(|error| RemoteError::Protocol(error.to_string()))
}

fn decode_worklog(dto: WorklogDto) -> Result<Worklog, RemoteError> {
    let id = dto
        .id
        .parse()
        .map_err(|_| RemoteError::Protocol("invalid worklog id".into()))?;
    let task_id = dto
        .task_id
        .parse()
        .map_err(|_| RemoteError::Protocol("invalid worklog task id".into()))?;
    Worklog::new(id, task_id, dto.start, dto.end)
        .map_err(|error| RemoteError::Protocol(error.to_string()))
}

fn decode_active(
    active: Option<WorklogDto>,
) -> Result<(Option<Worklog>, TrackingState), RemoteError> {
    let worklog = active.map(decode_worklog).transpose()?;
    let tracking = match worklog.clone() {
        Some(worklog) => Tracker::resume(worklog)
            .map_err(|error| RemoteError::Protocol(error.to_string()))?
            .state()
            .clone(),
        None => TrackingState::Idle,
    };
    Ok((worklog, tracking))
}

fn decode_snapshot(
    dto: SnapshotDto,
) -> Result<(TrackerSnapshot, TrackingState, String), RemoteError> {
    let mut task_items = dto
        .task_items
        .into_iter()
        .map(|item| {
            Ok(TaskListItem {
                task: decode_task(item.task)?,
                latest_work_start: item.latest_work_start,
            })
        })
        .collect::<Result<Vec<_>, RemoteError>>()?;
    task_items.sort_by_key(|item| item.task.id());
    if task_items
        .windows(2)
        .any(|pair| pair[0].task.id() == pair[1].task.id())
    {
        return Err(RemoteError::Protocol("duplicate task in snapshot".into()));
    }
    let active_worklog = dto.active_worklog.map(decode_worklog).transpose()?;
    let tracking = match active_worklog.clone() {
        Some(worklog) => {
            if !task_items
                .iter()
                .any(|item| item.task.id() == worklog.task_id() && !item.task.is_archived())
            {
                return Err(RemoteError::Protocol(
                    "active worklog has no active task".into(),
                ));
            }
            Tracker::resume(worklog)
                .map_err(|error| RemoteError::Protocol(error.to_string()))?
                .state()
                .clone()
        }
        None => TrackingState::Idle,
    };
    if let Some(active) = &active_worklog
        && let Some(item) = task_items
            .iter_mut()
            .find(|item| item.task.id() == active.task_id())
    {
        item.latest_work_start = Some(
            item.latest_work_start
                .map_or(active.start(), |old| old.max(active.start())),
        );
    }
    Ok((
        TrackerSnapshot {
            task_items,
            active_worklog,
        },
        tracking,
        dto.revision,
    ))
}

fn decode_cursor(dto: WorklogCursorDto) -> Result<WorklogCursor, RemoteError> {
    Ok(WorklogCursor {
        task_id: dto
            .task_id
            .parse()
            .map_err(|_| RemoteError::Protocol("invalid cursor task id".into()))?,
        start: dto.start,
        id: dto
            .id
            .parse()
            .map_err(|_| RemoteError::Protocol("invalid cursor worklog id".into()))?,
        revision: dto.revision,
    })
}

fn decode_global_cursor(dto: GlobalWorklogCursorDto) -> Result<GlobalWorklogCursor, RemoteError> {
    Ok(GlobalWorklogCursor {
        start: dto.start,
        id: dto
            .id
            .parse()
            .map_err(|_| RemoteError::Protocol("invalid cursor worklog id".into()))?,
        revision: dto.revision,
    })
}

fn app_decode_task(dto: TaskDto) -> Result<Task, ApplicationError> {
    decode_task(dto).map_err(|error| protocol_failure(&error.to_string()))
}

fn app_decode_worklog(dto: WorklogDto) -> Result<Worklog, ApplicationError> {
    decode_worklog(dto).map_err(|error| protocol_failure(&error.to_string()))
}

fn task_result(result: MutationResultDto, expected_id: TaskId) -> Result<Task, ApplicationError> {
    match result {
        MutationResultDto::Task(task) => {
            let task = app_decode_task(task)?;
            if task.id() != expected_id {
                return Err(protocol_failure("task result has a different id"));
            }
            Ok(task)
        }
        _ => Err(protocol_failure("wrong task result")),
    }
}

fn worklog_result(
    result: MutationResultDto,
    expected_id: WorklogId,
) -> Result<Worklog, ApplicationError> {
    match result {
        MutationResultDto::Worklog(worklog) => {
            let worklog = app_decode_worklog(worklog)?;
            if worklog.id() != expected_id {
                return Err(protocol_failure("worklog result has a different id"));
            }
            Ok(worklog)
        }
        _ => Err(protocol_failure("wrong worklog result")),
    }
}

fn set_tracking_result(
    result: MutationResultDto,
    task_id: TaskId,
    new_id: WorklogId,
    old_active: Option<WorklogId>,
) -> Result<SetActiveTaskOutcome, ApplicationError> {
    match result {
        MutationResultDto::Worklog(dto) => {
            let worklog = app_decode_worklog(dto)?;
            started_tracking_result(worklog, task_id, new_id, old_active)
        }
        MutationResultDto::TrackingSwitched { stopped, started } => {
            let stopped = app_decode_worklog(stopped)?;
            let started = app_decode_worklog(started)?;
            switched_tracking_result(stopped, started, task_id, new_id, old_active)
        }
        MutationResultDto::TrackingAlreadyActive(dto) => {
            let worklog = app_decode_worklog(dto)?;
            if !worklog.is_active() || worklog.task_id() != task_id {
                return Err(protocol_failure("invalid already-active worklog"));
            }
            Ok(SetActiveTaskOutcome::AlreadyActive {
                worklog: ActiveWorklog::begin(worklog.id(), worklog.task_id(), worklog.start()),
            })
        }
        _ => Err(protocol_failure("wrong tracking result")),
    }
}

fn started_tracking_result(
    worklog: Worklog,
    task_id: TaskId,
    new_id: WorklogId,
    old_active: Option<WorklogId>,
) -> Result<SetActiveTaskOutcome, ApplicationError> {
    if old_active.is_some()
        || worklog.id() != new_id
        || worklog.task_id() != task_id
        || !worklog.is_active()
    {
        return Err(protocol_failure("invalid started worklog"));
    }
    Ok(SetActiveTaskOutcome::Started { worklog })
}

fn switched_tracking_result(
    stopped: Worklog,
    started: Worklog,
    task_id: TaskId,
    new_id: WorklogId,
    old_active: Option<WorklogId>,
) -> Result<SetActiveTaskOutcome, ApplicationError> {
    if old_active != Some(stopped.id())
        || stopped.is_active()
        || started.id() != new_id
        || started.task_id() != task_id
        || !started.is_active()
    {
        return Err(protocol_failure("invalid switched worklogs"));
    }
    Ok(SetActiveTaskOutcome::Switched { stopped, started })
}

fn clear_tracking_result(
    result: MutationResultDto,
    expected_active: WorklogId,
) -> Result<ClearActiveTaskOutcome, ApplicationError> {
    match result {
        MutationResultDto::Worklog(dto) => {
            let worklog = app_decode_worklog(dto)?;
            if worklog.id() != expected_active || worklog.is_active() {
                return Err(protocol_failure("invalid stopped worklog"));
            }
            Ok(ClearActiveTaskOutcome::Stopped { worklog })
        }
        MutationResultDto::TrackingAlreadyIdle => Ok(ClearActiveTaskOutcome::AlreadyIdle),
        _ => Err(protocol_failure("wrong tracking result")),
    }
}

fn protocol_failure(message: &str) -> ApplicationError {
    ApplicationError::RemoteProtocol(format!("invalid server response: {message}"))
}

fn classify_error(error: &RemoteError) -> RemoteFailureKind {
    match error {
        RemoteError::Unavailable(_) => RemoteFailureKind::Unavailable,
        RemoteError::Http { status, .. }
            if status.is_server_error() || *status == StatusCode::REQUEST_TIMEOUT =>
        {
            RemoteFailureKind::Unavailable
        }
        RemoteError::Http { status, .. } if *status == StatusCode::CONFLICT => {
            RemoteFailureKind::Conflict
        }
        _ => RemoteFailureKind::Protocol,
    }
}

fn map_application_error(
    error: &RemoteError,
    worklog_id: Option<WorklogId>,
    _task_id: Option<TaskId>,
) -> ApplicationError {
    if error.is_unavailable() {
        return ApplicationError::RemoteUnavailable(error.to_string());
    }
    if let RemoteError::Http { status, body } = error {
        if let Ok(dto) = serde_json::from_slice::<ErrorDto>(body) {
            return map_server_error(dto, worklog_id);
        }
        if *status == StatusCode::CONFLICT {
            return ApplicationError::semantic_failure(
                ApplicationFailureCategory::General,
                "Tracker state changed. Refresh and retry.",
            );
        }
    }
    ApplicationError::RemoteProtocol(error.to_string())
}

fn map_server_error(dto: ErrorDto, worklog_id: Option<WorklogId>) -> ApplicationError {
    if dto.code == ErrorCode::Internal {
        return ApplicationError::RemoteUnavailable("remote server error".into());
    }
    ApplicationError::semantic_failure(
        error_category(dto.code, worklog_id),
        safe_message(&dto.message),
    )
}

fn error_category(code: ErrorCode, worklog_id: Option<WorklogId>) -> ApplicationFailureCategory {
    match code {
        ErrorCode::InvalidRequest | ErrorCode::Conflict | ErrorCode::StaleRevision => {
            ApplicationFailureCategory::General
        }
        ErrorCode::NotFound if worklog_id.is_some() => ApplicationFailureCategory::WorklogNotFound,
        ErrorCode::NotFound => ApplicationFailureCategory::TaskNotFound,
        ErrorCode::WorklogChanged => ApplicationFailureCategory::WorklogChanged,
        ErrorCode::WorklogHistoryChanged => ApplicationFailureCategory::WorklogHistoryChanged,
        ErrorCode::WorklogOverlap => ApplicationFailureCategory::WorklogOverlap,
        ErrorCode::ActiveWorklog => ApplicationFailureCategory::ActiveWorklog,
        ErrorCode::ActiveTask => ApplicationFailureCategory::ActiveTask,
        ErrorCode::InactiveTaskCandidatesChanged => {
            ApplicationFailureCategory::InactiveTaskCandidatesChanged
        }
        ErrorCode::Internal => ApplicationFailureCategory::General,
    }
}

#[cfg(test)]
mod mutation_tests {
    use super::{InactiveTaskCandidatesDto, InactivityPeriod, TaskOrdering, protocol_failure};
    use std::{
        io::{Read, Write},
        net::{SocketAddr, TcpListener, TcpStream},
        sync::{Arc, Mutex, mpsc},
        thread,
        time::{Duration as StdDuration, Instant},
    };

    use axum::{
        Json, Router,
        routing::{get, post},
    };
    use chrono::{DateTime, Duration, Utc};
    use reqwest::StatusCode;
    use tokio::sync::oneshot;
    use tracker_application::{
        ApplicationError, ApplicationFailureCategory, ClearActiveTaskOutcome, SetActiveTaskOutcome,
    };
    use tracker_domain::{TaskId, WorklogId};
    use tracker_protocol::{
        CreateTaskRequest, ErrorCode, ErrorDto, HealthDto, InactiveTaskPreviewDto, MutationDto,
        MutationResultDto, SnapshotDto, TaskDto, TaskItemDto, WorklogDto, WriteGuard,
    };

    use super::{
        RemoteApplication, RemoteError, RemoteFailureKind, classify_error, clear_tracking_result,
        decode_snapshot, map_application_error, report_cooldown_active, set_tracking_result,
    };
    use tracker_domain::TaskName;

    fn at() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap()
    }

    fn task(id: TaskId, archived: bool) -> TaskDto {
        TaskDto {
            id: id.to_string(),
            name: "Project".into(),
            archived,
            created_at: at(),
            updated_at: at(),
        }
    }

    fn valid_inactive_preview() -> InactiveTaskPreviewDto {
        InactiveTaskPreviewDto {
            as_of: at(),
            count: 1,
            sample_names: vec!["Project".into()],
            revision: "before-archive".into(),
        }
    }

    fn valid_inactive_candidates() -> InactiveTaskCandidatesDto {
        InactiveTaskCandidatesDto {
            as_of: at(),
            inactive_days: 7,
            tasks: vec![task(TaskId::generate(), false)],
            revision: "preview-revision".into(),
        }
    }

    #[tokio::test]
    async fn configurable_preview_rejects_invalid_context_and_candidate_metadata() {
        let valid = valid_inactive_candidates();
        let mut cases = Vec::new();
        for index in 0..11 {
            let mut preview = valid.clone();
            match index {
                0 => preview.as_of += Duration::seconds(1),
                1 => preview.inactive_days = 14,
                2 => preview.inactive_days = 0,
                3 => preview.revision.clear(),
                4 => preview.tasks[0].id = "invalid".into(),
                5 => preview.tasks[0].id = preview.tasks[0].id.to_uppercase(),
                6 => preview.tasks[0].name = "Bad\u{1b}name".into(),
                7 => preview.tasks[0].name = " Project ".into(),
                8 => preview.tasks[0].archived = true,
                9 => preview.tasks[0].updated_at -= Duration::seconds(1),
                10 => preview.tasks.push(preview.tasks[0].clone()),
                _ => unreachable!(),
            }
            cases.push(preview);
        }
        for preview in cases {
            let (endpoint, server) = serve_json_once(serde_json::to_vec(&preview).unwrap());
            let mut client = RemoteApplication::disconnected(&endpoint).unwrap();
            assert_eq!(
                client
                    .preview_inactive_tasks_with_period(at(), InactivityPeriod::new(7).unwrap())
                    .await,
                Err(protocol_failure("invalid inactive task candidates")),
            );
            assert_eq!(client.last_failure(), Some(RemoteFailureKind::Protocol));
            server.join().unwrap();
        }
    }

    #[tokio::test]
    async fn configurable_preview_keeps_all_candidates_and_captured_revision_without_replacing_snapshot()
     {
        let mut preview = valid_inactive_candidates();
        preview.tasks = (0..8).map(|_| task(TaskId::generate(), false)).collect();
        for empty in [false, true] {
            if empty {
                preview.tasks.clear();
            }
            let (endpoint, server) = serve_json_once(serde_json::to_vec(&preview).unwrap());
            let mut client = RemoteApplication::disconnected(&endpoint).unwrap();
            client.revision = "snapshot-revision".into();
            client.last_unavailable_at = Some(Instant::now());
            let actual = client
                .preview_inactive_tasks_with_period(
                    at() + Duration::nanoseconds(123),
                    InactivityPeriod::new(7).unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(actual, preview);
            assert_eq!(client.revision, "snapshot-revision");
            assert!(client.tasks(TaskOrdering::default()).is_empty());
            assert_eq!(client.last_failure(), None);
            assert_eq!(client.last_unavailable_at, None);
            server.join().unwrap();
        }
    }

    #[tokio::test]
    async fn configurable_archive_validates_preview_before_sending() {
        for index in 0..4 {
            let mut preview = valid_inactive_candidates();
            match index {
                0 => preview.revision.clear(),
                1 => preview.inactive_days = 0,
                2 => preview.tasks[0].archived = true,
                3 => preview.as_of += Duration::nanoseconds(1),
                _ => unreachable!(),
            }
            let mut client = RemoteApplication::disconnected("http://127.0.0.1:1/").unwrap();
            assert_eq!(
                client.archive_inactive_tasks_with_period(&preview).await,
                Err(protocol_failure("invalid inactive task candidates"))
            );
        }
    }

    #[tokio::test]
    async fn configurable_archive_accepts_actual_count_and_replay_with_restored_tasks() {
        let preview = valid_inactive_candidates();
        let id = preview.tasks[0].id.parse().unwrap();
        for count in [0, 1, 2] {
            let response = MutationDto {
                result: MutationResultDto::ArchivedInactive { count },
                snapshot: SnapshotDto {
                    task_items: vec![TaskItemDto {
                        task: task(id, false),
                        latest_work_start: None,
                    }],
                    active_worklog: None,
                    revision: "current-revision".into(),
                },
            };
            let (endpoint, server) = serve_json_once(serde_json::to_vec(&response).unwrap());
            let mut client = RemoteApplication::disconnected(&endpoint).unwrap();
            assert_eq!(
                client
                    .archive_inactive_tasks_with_period(&preview)
                    .await
                    .unwrap(),
                count
            );
            assert!(!client.task(id).unwrap().is_archived());
            assert_eq!(client.revision, "current-revision");
            server.join().unwrap();
        }
    }

    #[tokio::test]
    async fn configurable_archive_rejects_wrong_results_and_invalid_snapshots() {
        let preview = valid_inactive_candidates();
        let id = preview.tasks[0].id.parse().unwrap();
        for invalid_result in [false, true] {
            let item = TaskItemDto {
                task: task(id, true),
                latest_work_start: None,
            };
            let response = MutationDto {
                result: if invalid_result {
                    MutationResultDto::TrackingAlreadyIdle
                } else {
                    MutationResultDto::ArchivedInactive { count: 1 }
                },
                snapshot: SnapshotDto {
                    task_items: if invalid_result {
                        vec![item]
                    } else {
                        vec![item.clone(), item]
                    },
                    active_worklog: None,
                    revision: "current-revision".into(),
                },
            };
            let (endpoint, server) = serve_json_once(serde_json::to_vec(&response).unwrap());
            let mut client = RemoteApplication::disconnected(&endpoint).unwrap();
            assert!(matches!(
                client.archive_inactive_tasks_with_period(&preview).await,
                Err(ApplicationError::RemoteProtocol(_))
            ));
            assert_eq!(client.last_failure(), Some(RemoteFailureKind::Protocol));
            server.join().unwrap();
        }
    }

    fn read_request(stream: &mut TcpStream) -> (String, Vec<u8>) {
        stream
            .set_read_timeout(Some(StdDuration::from_secs(2)))
            .unwrap();
        let mut bytes = Vec::new();
        let header_end = loop {
            let mut chunk = [0_u8; 1024];
            let count = stream.read(&mut chunk).unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&chunk[..count]);
            if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
        let length: usize = headers
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length: ")
                    .and_then(|value| value.parse().ok())
            })
            .unwrap();
        while bytes.len() < header_end + length {
            let mut chunk = [0_u8; 1024];
            let count = stream.read(&mut chunk).unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&chunk[..count]);
        }
        (headers, bytes[header_end..header_end + length].to_vec())
    }

    #[tokio::test]
    async fn configurable_archive_retries_identical_payload_with_captured_context() {
        let preview = valid_inactive_candidates();
        let expected = preview.clone();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let reply = serde_json::to_vec(&MutationDto {
            result: MutationResultDto::ArchivedInactive { count: 2 },
            snapshot: SnapshotDto {
                task_items: vec![],
                active_worklog: None,
                revision: "current-revision".into(),
            },
        })
        .unwrap();
        let server = thread::spawn(move || {
            let mut bodies = Vec::new();
            for attempt in 0..2 {
                let mut stream = accept_with_deadline(&listener).expect("expected archive request");
                let (headers, body) = read_request(&mut stream);
                assert!(headers.starts_with("POST /v1/tasks/archive-inactive-candidates HTTP/1.1"));
                let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(payload.as_object().unwrap().len(), 4);
                assert_eq!(
                    payload["as_of"],
                    serde_json::to_value(expected.as_of).unwrap()
                );
                assert_eq!(payload["inactive_days"], expected.inactive_days);
                assert_eq!(payload["expected_revision"], expected.revision);
                assert!(uuid::Uuid::parse_str(payload["request_id"].as_str().unwrap()).is_ok());
                bodies.push(body);
                let headers = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    reply.len()
                );
                stream.write_all(headers.as_bytes()).unwrap();
                stream
                    .write_all(if attempt == 0 {
                        &reply[..reply.len() / 2]
                    } else {
                        &reply
                    })
                    .unwrap();
            }
            assert_eq!(bodies[0], bodies[1]);
        });
        let mut client = RemoteApplication::disconnected(&endpoint).unwrap();
        client.revision = "later-snapshot-revision".into();
        assert_eq!(
            client
                .archive_inactive_tasks_with_period(&preview)
                .await
                .unwrap(),
            2
        );
        assert_eq!(client.revision, "current-revision");
        server.join().unwrap();
    }

    fn accept_with_deadline(listener: &TcpListener) -> Option<TcpStream> {
        listener.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + StdDuration::from_secs(2);
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    return Some(stream);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return None;
                    }
                    thread::sleep(StdDuration::from_millis(10));
                }
                Err(error) => panic!("listener failed: {error}"),
            }
        }
    }

    fn serve_json_once(body: Vec<u8>) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let mut stream = accept_with_deadline(&listener).expect("expected one request");
            stream
                .set_read_timeout(Some(StdDuration::from_secs(2)))
                .unwrap();
            let mut request = [0_u8; 2048];
            assert!(stream.read(&mut request).unwrap() > 0);
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(header.as_bytes()).unwrap();
            stream.write_all(&body).unwrap();
        });
        (endpoint, server)
    }

    #[tokio::test]
    async fn inactive_preview_rejects_each_invalid_field_and_accepts_five_names() {
        let mut valid = valid_inactive_preview();
        valid.count = 5;
        valid.sample_names = (0..5).map(|index| format!("Task {index}")).collect();
        let mut cases = Vec::new();

        let mut wrong_time = valid.clone();
        wrong_time.as_of += Duration::seconds(1);
        cases.push(wrong_time);

        let mut too_many_names = valid.clone();
        too_many_names.count = 6;
        too_many_names.sample_names.push("Task 5".into());
        cases.push(too_many_names);

        let mut more_names_than_count = valid.clone();
        more_names_than_count.count = 4;
        cases.push(more_names_than_count);

        let mut invalid_name = valid.clone();
        invalid_name.sample_names[2] = "Bad\u{1b}name".into();
        cases.push(invalid_name);

        let mut empty_revision = valid.clone();
        empty_revision.revision.clear();
        cases.push(empty_revision);

        for preview in cases {
            let (endpoint, server) = serve_json_once(serde_json::to_vec(&preview).unwrap());
            let mut client = RemoteApplication::disconnected(&endpoint).unwrap();
            assert_eq!(
                client.preview_inactive_tasks(at()).await.unwrap_err(),
                ApplicationError::RemoteProtocol(
                    "invalid server response: invalid inactive task preview".into()
                )
            );
            assert_eq!(client.last_failure(), Some(RemoteFailureKind::Protocol));
            server.join().unwrap();
        }

        let (endpoint, server) = serve_json_once(serde_json::to_vec(&valid).unwrap());
        let mut client = RemoteApplication::disconnected(&endpoint).unwrap();
        assert_eq!(client.preview_inactive_tasks(at()).await.unwrap(), valid);
        assert_eq!(client.last_failure(), None);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn inactive_archive_rejects_empty_preview_revision_before_sending() {
        let endpoint = "http://127.0.0.1:1/";
        let mut empty_revision = valid_inactive_preview();
        empty_revision.revision.clear();
        let mut client = RemoteApplication::disconnected(endpoint).unwrap();
        assert_eq!(
            client
                .archive_inactive_tasks(&empty_revision)
                .await
                .unwrap_err(),
            ApplicationError::RemoteProtocol(
                "invalid server response: invalid inactive task preview".into()
            )
        );
    }

    #[tokio::test]
    async fn inactive_archive_accepts_actual_counts_below_and_above_preview() {
        let preview = valid_inactive_preview();
        let task_id = TaskId::generate();
        let extra_task_id = TaskId::generate();
        for archived_count in [0, 2] {
            let mut task_items = vec![TaskItemDto {
                task: task(task_id, archived_count != 0),
                latest_work_start: None,
            }];
            if archived_count == 2 {
                task_items.push(TaskItemDto {
                    task: task(extra_task_id, true),
                    latest_work_start: None,
                });
            }
            let response = MutationDto {
                result: MutationResultDto::ArchivedInactive {
                    count: archived_count,
                },
                snapshot: SnapshotDto {
                    task_items,
                    active_worklog: None,
                    revision: "after-archive".into(),
                },
            };
            let (endpoint, server) = serve_json_once(serde_json::to_vec(&response).unwrap());
            let mut client = RemoteApplication::disconnected(&endpoint).unwrap();
            assert_eq!(
                client.archive_inactive_tasks(&preview).await.unwrap(),
                archived_count
            );
            assert_eq!(
                client.task(task_id).unwrap().is_archived(),
                archived_count != 0
            );
            if archived_count == 2 {
                assert!(client.task(extra_task_id).unwrap().is_archived());
            } else {
                assert!(client.task(extra_task_id).is_none());
            }
            assert_eq!(client.revision, "after-archive");
            assert_eq!(client.last_failure(), None);
            server.join().unwrap();
        }
    }

    #[tokio::test]
    async fn inactive_archive_accepts_replayed_count_after_tasks_are_restored() {
        let task_id = TaskId::generate();
        let response = MutationDto {
            result: MutationResultDto::ArchivedInactive { count: 1 },
            snapshot: SnapshotDto {
                task_items: vec![TaskItemDto {
                    task: task(task_id, false),
                    latest_work_start: None,
                }],
                active_worklog: None,
                revision: "after-restore".into(),
            },
        };
        let (endpoint, server) = serve_json_once(serde_json::to_vec(&response).unwrap());
        let mut client = RemoteApplication::disconnected(&endpoint).unwrap();

        assert_eq!(
            client
                .archive_inactive_tasks(&valid_inactive_preview())
                .await
                .unwrap(),
            1
        );
        assert!(!client.task(task_id).unwrap().is_archived());
        assert_eq!(client.revision, "after-restore");
        assert_eq!(client.last_failure(), None);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn inactive_archive_rejects_wrong_result_variants_and_invalid_snapshots() {
        let task_id = TaskId::generate();
        let task_item = TaskItemDto {
            task: task(task_id, true),
            latest_work_start: None,
        };
        let valid_snapshot = SnapshotDto {
            task_items: vec![task_item.clone()],
            active_worklog: None,
            revision: "after-archive".into(),
        };
        let wrong_result = MutationDto {
            result: MutationResultDto::Task(task(task_id, true)),
            snapshot: valid_snapshot.clone(),
        };
        let mut invalid_snapshot = valid_snapshot;
        invalid_snapshot.task_items.push(task_item);
        let wrong_snapshot = MutationDto {
            result: MutationResultDto::ArchivedInactive { count: 1 },
            snapshot: invalid_snapshot,
        };

        for response in [wrong_result, wrong_snapshot] {
            let (endpoint, server) = serve_json_once(serde_json::to_vec(&response).unwrap());
            let mut client = RemoteApplication::disconnected(&endpoint).unwrap();
            assert!(matches!(
                client
                    .archive_inactive_tasks(&valid_inactive_preview())
                    .await,
                Err(ApplicationError::RemoteProtocol(_))
            ));
            assert!(client.task(task_id).is_none());
            assert_eq!(client.last_failure(), Some(RemoteFailureKind::Protocol));
            server.join().unwrap();
        }
    }

    #[tokio::test]
    async fn bulk_archive_recovers_after_a_committed_write_loses_its_response_body() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let task_id = TaskId::generate();
        let response = serde_json::to_vec(&MutationDto {
            result: MutationResultDto::ArchivedInactive { count: 1 },
            snapshot: SnapshotDto {
                task_items: vec![TaskItemDto {
                    task: task(task_id, true),
                    latest_work_start: None,
                }],
                active_worklog: None,
                revision: "after-archive".into(),
            },
        })
        .unwrap();
        let server = thread::spawn(move || {
            let mut bodies = Vec::new();
            let mut committed = 0;
            let mut completed_request_id = None;
            for attempt in 0..2 {
                let Some(mut stream) = accept_with_deadline(&listener) else {
                    break;
                };
                stream
                    .set_read_timeout(Some(StdDuration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let header_end = loop {
                    let mut chunk = [0_u8; 1024];
                    let count = stream.read(&mut chunk).unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&chunk[..count]);
                    if let Some(index) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                        break index + 4;
                    }
                };
                let headers = String::from_utf8_lossy(&request[..header_end]);
                assert!(headers.starts_with("POST /v1/tasks/archive-inactive HTTP/1.1"));
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|value| value.parse().ok())
                    })
                    .unwrap();
                while request.len() < header_end + length {
                    let mut chunk = [0_u8; 1024];
                    let count = stream.read(&mut chunk).unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&chunk[..count]);
                }
                let body = request[header_end..header_end + length].to_vec();
                let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
                assert!(payload.get("candidate_fingerprint").is_none());
                assert_eq!(payload["expected_revision"], "before-archive");
                assert_eq!(
                    payload["as_of"],
                    at().to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
                );
                let request_id = payload["request_id"].as_str().unwrap();
                assert!(uuid::Uuid::parse_str(request_id).is_ok());
                if completed_request_id.as_deref() != Some(request_id) {
                    committed += 1;
                    completed_request_id = Some(request_id.to_owned());
                }
                bodies.push(body);
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    response.len()
                );
                stream.write_all(header.as_bytes()).unwrap();
                if attempt == 0 {
                    stream.write_all(&response[..response.len() / 2]).unwrap();
                } else {
                    stream.write_all(&response).unwrap();
                }
            }
            (bodies, committed)
        });

        let mut client = RemoteApplication::disconnected(&endpoint).unwrap();
        let preview = InactiveTaskPreviewDto {
            as_of: at(),
            count: 1,
            sample_names: vec!["Project".into()],
            revision: "before-archive".into(),
        };
        assert_eq!(client.archive_inactive_tasks(&preview).await.unwrap(), 1);
        assert!(client.task(task_id).unwrap().is_archived());
        let (bodies, committed) = server.join().unwrap();
        assert_eq!(bodies.len(), 2);
        assert_eq!(bodies[0], bodies[1]);
        assert_eq!(committed, 1);
    }

    #[test]
    fn report_cooldown_ends_at_five_seconds() {
        let now = Instant::now();
        assert!(!report_cooldown_active(None, now));
        assert!(report_cooldown_active(
            Some(now - StdDuration::from_millis(4_999)),
            now
        ));
        assert!(!report_cooldown_active(
            Some(now - StdDuration::from_secs(5)),
            now
        ));
        assert!(!report_cooldown_active(
            Some(now - StdDuration::from_millis(5_001)),
            now
        ));
    }

    fn worklog(id: WorklogId, task_id: TaskId, end: Option<DateTime<Utc>>) -> WorklogDto {
        WorklogDto {
            id: id.to_string(),
            task_id: task_id.to_string(),
            start: at() + Duration::seconds(10),
            end,
        }
    }

    #[test]
    fn snapshot_requires_an_active_task_for_the_running_worklog() {
        let task_id = TaskId::generate();
        let running = worklog(WorklogId::generate(), task_id, None);
        let snapshot = |archived| SnapshotDto {
            task_items: vec![TaskItemDto {
                task: task(task_id, archived),
                latest_work_start: Some(at()),
            }],
            active_worklog: Some(running.clone()),
            revision: "revision".into(),
        };

        assert!(decode_snapshot(snapshot(true)).is_err());
        let (decoded, tracking, revision) = decode_snapshot(snapshot(false)).unwrap();
        assert_eq!(revision, "revision");
        assert!(matches!(
            tracking,
            tracker_domain::TrackingState::Running { .. }
        ));
        assert_eq!(decoded.active_worklog.unwrap().id().to_string(), running.id);
        assert_eq!(decoded.task_items[0].latest_work_start, Some(running.start));
    }

    #[test]
    fn snapshot_rejects_a_running_worklog_for_a_missing_task() {
        let snapshot = SnapshotDto {
            task_items: vec![TaskItemDto {
                task: task(TaskId::generate(), false),
                latest_work_start: None,
            }],
            active_worklog: Some(worklog(WorklogId::generate(), TaskId::generate(), None)),
            revision: "revision".into(),
        };
        assert!(decode_snapshot(snapshot).is_err());
    }

    #[test]
    fn tracking_result_checks_every_started_and_stopped_identifier_and_state() {
        let task_id = TaskId::generate();
        let other_task = TaskId::generate();
        let started_id = WorklogId::generate();
        let old_id = WorklogId::generate();
        let start = worklog(started_id, task_id, None);
        let stop = worklog(old_id, task_id, Some(at() + Duration::seconds(20)));

        assert!(matches!(
            set_tracking_result(
                MutationResultDto::Worklog(start.clone()),
                task_id,
                started_id,
                None
            )
            .unwrap(),
            SetActiveTaskOutcome::Started { .. }
        ));
        for (dto, expected_task, expected_id, previous) in [
            (start.clone(), task_id, started_id, Some(old_id)),
            (start.clone(), task_id, old_id, None),
            (start.clone(), other_task, started_id, None),
            (stop.clone(), task_id, old_id, None),
        ] {
            assert!(
                set_tracking_result(
                    MutationResultDto::Worklog(dto),
                    expected_task,
                    expected_id,
                    previous
                )
                .is_err()
            );
        }

        let switched = MutationResultDto::TrackingSwitched {
            stopped: stop.clone(),
            started: start.clone(),
        };
        assert!(matches!(
            set_tracking_result(switched, task_id, started_id, Some(old_id)).unwrap(),
            SetActiveTaskOutcome::Switched { .. }
        ));
        for (stopped, started, expected_task, expected_id, previous) in [
            (stop.clone(), start.clone(), task_id, started_id, None),
            (
                start.clone(),
                start.clone(),
                task_id,
                started_id,
                Some(started_id),
            ),
            (stop.clone(), start.clone(), task_id, old_id, Some(old_id)),
            (
                stop.clone(),
                start.clone(),
                other_task,
                started_id,
                Some(old_id),
            ),
            (stop.clone(), stop.clone(), task_id, old_id, Some(old_id)),
        ] {
            assert!(
                set_tracking_result(
                    MutationResultDto::TrackingSwitched { stopped, started },
                    expected_task,
                    expected_id,
                    previous
                )
                .is_err()
            );
        }
        assert!(matches!(
            set_tracking_result(
                MutationResultDto::TrackingAlreadyActive(start.clone()),
                task_id,
                started_id,
                Some(old_id)
            )
            .unwrap(),
            SetActiveTaskOutcome::AlreadyActive { .. }
        ));
        assert!(
            set_tracking_result(
                MutationResultDto::TrackingAlreadyActive(stop),
                task_id,
                started_id,
                Some(old_id)
            )
            .is_err()
        );
        assert!(
            set_tracking_result(
                MutationResultDto::TrackingAlreadyActive(start),
                other_task,
                started_id,
                Some(old_id)
            )
            .is_err()
        );
    }

    #[test]
    fn idle_tracking_result_is_a_successful_no_op() {
        assert_eq!(
            clear_tracking_result(
                MutationResultDto::TrackingAlreadyIdle,
                WorklogId::generate()
            )
            .unwrap(),
            ClearActiveTaskOutcome::AlreadyIdle
        );
    }

    #[test]
    fn stopping_result_rejects_the_wrong_worklog_or_an_unfinished_worklog() {
        let expected_id = WorklogId::generate();
        let task_id = TaskId::generate();
        let stopped = worklog(expected_id, task_id, Some(at() + Duration::seconds(20)));
        assert!(matches!(
            clear_tracking_result(MutationResultDto::Worklog(stopped.clone()), expected_id)
                .unwrap(),
            ClearActiveTaskOutcome::Stopped { .. }
        ));
        assert!(
            clear_tracking_result(MutationResultDto::Worklog(stopped), WorklogId::generate())
                .is_err()
        );
        assert!(
            clear_tracking_result(
                MutationResultDto::Worklog(worklog(expected_id, task_id, None)),
                expected_id
            )
            .is_err()
        );
    }

    #[test]
    fn http_failures_keep_unavailable_conflict_and_protocol_distinct() {
        let error = |status| RemoteError::Http {
            status,
            body: Vec::new(),
        };
        assert_eq!(
            classify_error(&error(StatusCode::INTERNAL_SERVER_ERROR)),
            RemoteFailureKind::Unavailable
        );
        assert_eq!(
            classify_error(&error(StatusCode::REQUEST_TIMEOUT)),
            RemoteFailureKind::Unavailable
        );
        assert_eq!(
            classify_error(&error(StatusCode::CONFLICT)),
            RemoteFailureKind::Conflict
        );
        assert_eq!(
            classify_error(&error(StatusCode::BAD_REQUEST)),
            RemoteFailureKind::Protocol
        );
        assert_eq!(
            classify_error(&RemoteError::Unavailable("offline".into())),
            RemoteFailureKind::Unavailable
        );
    }

    #[test]
    fn not_found_category_depends_on_whether_a_worklog_was_requested() {
        let error = RemoteError::Http {
            status: StatusCode::NOT_FOUND,
            body: serde_json::to_vec(&ErrorDto {
                code: ErrorCode::NotFound,
                message: "Missing entry".into(),
            })
            .unwrap(),
        };
        assert_eq!(
            map_application_error(&error, Some(WorklogId::generate()), None)
                .failure()
                .category(),
            ApplicationFailureCategory::WorklogNotFound
        );
        assert_eq!(
            map_application_error(&error, None, Some(TaskId::generate()))
                .failure()
                .category(),
            ApplicationFailureCategory::TaskNotFound
        );
        let invalid_body = RemoteError::Http {
            status: StatusCode::CONFLICT,
            body: Vec::new(),
        };
        assert_eq!(
            map_application_error(&invalid_body, None, None)
                .failure()
                .message(),
            "Tracker state changed. Refresh and retry."
        );
    }

    #[test]
    fn remote_conflicts_keep_editor_and_archive_reasons_with_safe_messages() {
        let cases = [
            (
                ErrorCode::WorklogHistoryChanged,
                ApplicationFailureCategory::WorklogHistoryChanged,
                "History changed\nRefresh the page".to_owned(),
                "History changedRefresh the page".to_owned(),
            ),
            (
                ErrorCode::WorklogOverlap,
                ApplicationFailureCategory::WorklogOverlap,
                "Worklog overlaps another entry".to_owned(),
                "Worklog overlaps another entry".to_owned(),
            ),
            (
                ErrorCode::ActiveWorklog,
                ApplicationFailureCategory::ActiveWorklog,
                "\tStop the active worklog\r".to_owned(),
                "Stop the active worklog".to_owned(),
            ),
            (
                ErrorCode::ActiveTask,
                ApplicationFailureCategory::ActiveTask,
                "\n\t\r".to_owned(),
                "Server rejected the request".to_owned(),
            ),
            (
                ErrorCode::InactiveTaskCandidatesChanged,
                ApplicationFailureCategory::InactiveTaskCandidatesChanged,
                "x".repeat(520),
                "x".repeat(512),
            ),
        ];
        for (code, category, message, expected_message) in cases {
            let error = RemoteError::Http {
                status: StatusCode::CONFLICT,
                body: serde_json::to_vec(&ErrorDto { code, message }).unwrap(),
            };
            let failure =
                map_application_error(&error, Some(WorklogId::generate()), None).failure();
            assert_eq!(failure.category(), category);
            assert_eq!(failure.message(), expected_message);
            assert_eq!(
                failure.source(),
                tracker_application::ApplicationFailureSource::Operation
            );
            assert!(!failure.recovery_failed());
        }
    }

    #[test]
    fn application_failures_preserve_transport_origin_and_redact_internal_responses() {
        use tracker_application::ApplicationFailureSource;

        let cases = [
            (
                RemoteError::Unavailable("private URL".into()),
                ApplicationFailureSource::RemoteUnavailable,
            ),
            (
                RemoteError::Protocol("private response body".into()),
                ApplicationFailureSource::RemoteProtocol,
            ),
            (
                RemoteError::InvalidEndpoint("private endpoint"),
                ApplicationFailureSource::RemoteProtocol,
            ),
            (
                RemoteError::Http {
                    status: StatusCode::BAD_REQUEST,
                    body: b"private body".to_vec(),
                },
                ApplicationFailureSource::RemoteProtocol,
            ),
            (
                RemoteError::Http {
                    status: StatusCode::REQUEST_TIMEOUT,
                    body: Vec::new(),
                },
                ApplicationFailureSource::RemoteUnavailable,
            ),
            (
                RemoteError::Http {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    body: serde_json::to_vec(&ErrorDto {
                        code: ErrorCode::WorklogChanged,
                        message: "private database credentials".into(),
                    })
                    .unwrap(),
                },
                ApplicationFailureSource::RemoteUnavailable,
            ),
            (
                RemoteError::Http {
                    status: StatusCode::BAD_REQUEST,
                    body: serde_json::to_vec(&ErrorDto {
                        code: ErrorCode::Internal,
                        message: "private database credentials".into(),
                    })
                    .unwrap(),
                },
                ApplicationFailureSource::RemoteUnavailable,
            ),
        ];
        for (error, source) in cases {
            let failure = map_application_error(&error, None, None).failure();
            assert_eq!(failure.source(), source);
            assert_eq!(failure.category(), ApplicationFailureCategory::General);
            assert!(!failure.recovery_failed());
            assert!(!failure.message().contains("private"));
        }
        let error = RemoteError::Http {
            status: StatusCode::CONFLICT,
            body: serde_json::to_vec(&ErrorDto {
                code: ErrorCode::WorklogChanged,
                message: "Worklog changed".into(),
            })
            .unwrap(),
        };
        let failure = map_application_error(&error, None, None).failure();
        assert_eq!(
            failure.category(),
            ApplicationFailureCategory::WorklogChanged
        );
        assert_eq!(failure.source(), ApplicationFailureSource::Operation);
    }

    #[tokio::test]
    async fn successful_refresh_keeps_the_returned_error_source_after_clearing_availability() {
        use tracker_application::ApplicationFailureSource;

        let router = Router::new()
            .route(
                "/v1/health",
                get(|| async {
                    Json(HealthDto {
                        status: "ok".into(),
                        protocol_version: tracker_protocol::VERSION,
                    })
                }),
            )
            .route(
                "/v1/snapshot",
                get(|| async {
                    Json(SnapshotDto {
                        task_items: Vec::new(),
                        active_worklog: None,
                        revision: "refreshed".into(),
                    })
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let mut client = RemoteApplication::disconnected(&endpoint).unwrap();
        for (status, code, category, source) in [
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::Internal,
                ApplicationFailureCategory::General,
                ApplicationFailureSource::RemoteUnavailable,
            ),
            (
                StatusCode::CONFLICT,
                ErrorCode::WorklogChanged,
                ApplicationFailureCategory::WorklogChanged,
                ApplicationFailureSource::Operation,
            ),
        ] {
            let error = RemoteError::Http {
                status,
                body: serde_json::to_vec(&ErrorDto {
                    code,
                    message: "Worklog changed".into(),
                })
                .unwrap(),
            };
            let failure = client.operation_error(error, None, None).await.failure();
            assert_eq!(failure.category(), category);
            assert_eq!(failure.source(), source);
            assert!(!failure.recovery_failed());
            assert_eq!(client.last_failure(), None);
            assert_eq!(client.revision, "refreshed");
        }
        server.abort();
    }

    #[tokio::test]
    async fn failed_authoritative_refresh_keeps_primary_semantics_and_transport_failure() {
        use tracker_application::ApplicationFailureSource;

        let mut client = RemoteApplication::disconnected("http://127.0.0.1:1/").unwrap();
        let error = RemoteError::Http {
            status: StatusCode::CONFLICT,
            body: serde_json::to_vec(&ErrorDto {
                code: ErrorCode::WorklogChanged,
                message: "Worklog changed".into(),
            })
            .unwrap(),
        };
        let failure = client.operation_error(error, None, None).await.failure();
        assert_eq!(
            failure.category(),
            ApplicationFailureCategory::WorklogChanged
        );
        assert_eq!(
            failure.source(),
            ApplicationFailureSource::RemoteUnavailable
        );
        assert!(failure.recovery_failed());
        assert_eq!(
            failure.recovery_message(),
            Some("Tracker server is unavailable")
        );
        assert_eq!(
            failure.message(),
            "Worklog changed. State recovery failed: Tracker server is unavailable."
        );
    }

    #[tokio::test]
    async fn pending_create_recognizes_a_committed_task_or_reuses_its_id_after_reconnect() {
        for (committed, recovering) in [(false, true), (true, true), (false, false), (true, false)]
        {
            let pending_id = TaskId::generate();
            let pending_task = task(pending_id, false);
            let sent = Arc::new(Mutex::new(Vec::<CreateTaskRequest>::new()));
            let sent_for_route = Arc::clone(&sent);
            let snapshot = SnapshotDto {
                task_items: if committed {
                    vec![TaskItemDto {
                        task: pending_task.clone(),
                        latest_work_start: None,
                    }]
                } else {
                    Vec::new()
                },
                active_worklog: None,
                revision: "new-revision".into(),
            };
            let router = Router::new()
                .route(
                    "/v1/health",
                    get(|| async {
                        Json(HealthDto {
                            status: "ok".into(),
                            protocol_version: tracker_protocol::VERSION,
                        })
                    }),
                )
                .route(
                    "/v1/snapshot",
                    get(move || {
                        let snapshot = snapshot.clone();
                        async move { Json(snapshot) }
                    }),
                )
                .route(
                    "/v1/tasks",
                    post(move |Json(body): Json<CreateTaskRequest>| {
                        let sent = Arc::clone(&sent_for_route);
                        async move {
                            sent.lock().unwrap().push(body.clone());
                            let created = TaskDto {
                                id: body.task_id,
                                name: body.name,
                                archived: false,
                                created_at: body.occurred_at,
                                updated_at: body.occurred_at,
                            };
                            Json(MutationDto {
                                result: MutationResultDto::Task(created.clone()),
                                snapshot: SnapshotDto {
                                    task_items: vec![TaskItemDto {
                                        task: created,
                                        latest_work_start: None,
                                    }],
                                    active_worklog: None,
                                    revision: "next-revision".into(),
                                },
                            })
                        }
                    }),
                );
            let (address_tx, address_rx) = mpsc::channel::<SocketAddr>();
            let (shutdown_tx, shutdown_rx) = oneshot::channel();
            let server = thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                runtime.block_on(async move {
                    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                    address_tx.send(listener.local_addr().unwrap()).unwrap();
                    axum::serve(listener, router)
                        .with_graceful_shutdown(async {
                            let _ = shutdown_rx.await;
                        })
                        .await
                        .unwrap();
                });
            });

            let address = address_rx.recv().unwrap();
            let mut client =
                RemoteApplication::disconnected(&format!("http://{address}/")).unwrap();
            client.pending_create = Some(CreateTaskRequest {
                task_id: pending_id.to_string(),
                name: "Project".into(),
                occurred_at: at(),
                guard: WriteGuard {
                    expected_revision: "previous-revision".into(),
                    request_id: uuid::Uuid::now_v7().to_string(),
                },
            });
            client.last_failure = recovering.then_some(RemoteFailureKind::Unavailable);
            let different = client
                .create_task(TaskName::new("Another project").unwrap(), at())
                .await
                .unwrap_err();
            assert_eq!(
                different.failure().message(),
                "Recover the pending task creation before creating another task"
            );
            assert!(sent.lock().unwrap().is_empty());
            let result = client
                .create_task(
                    TaskName::new("Project").unwrap(),
                    at() + chrono::TimeDelta::seconds(90),
                )
                .await
                .unwrap();
            assert_eq!(result.id(), pending_id);
            assert!(client.pending_create.is_none());
            let requests = sent.lock().unwrap();
            assert_eq!(requests.len(), usize::from(!committed));
            if !committed {
                assert_eq!(requests[0].task_id, pending_id.to_string());
                assert_eq!(requests[0].guard.expected_revision, "new-revision");
                assert_eq!(requests[0].occurred_at, at());
            }
            drop(requests);
            shutdown_tx.send(()).unwrap();
            server.join().unwrap();
        }
    }

    #[tokio::test]
    async fn failed_creation_keeps_its_identity_after_a_successful_recovery_read() {
        use axum::response::IntoResponse;

        for committed in [false, true] {
            let requests = Arc::new(Mutex::new(Vec::<CreateTaskRequest>::new()));
            let saved = Arc::new(Mutex::new(None::<TaskDto>));
            let healthy = Arc::new(Mutex::new(false));
            let saved_for_get = Arc::clone(&saved);
            let saved_for_post = Arc::clone(&saved);
            let requests_for_post = Arc::clone(&requests);
            let healthy_for_post = Arc::clone(&healthy);
            let router = Router::new()
                .route(
                    "/v1/health",
                    get(|| async {
                        Json(HealthDto {
                            status: "ok".into(),
                            protocol_version: tracker_protocol::VERSION,
                        })
                    }),
                )
                .route(
                    "/v1/snapshot",
                    get(move || {
                        let saved = Arc::clone(&saved_for_get);
                        async move {
                            Json(SnapshotDto {
                                task_items: saved
                                    .lock()
                                    .unwrap()
                                    .clone()
                                    .into_iter()
                                    .map(|task| TaskItemDto {
                                        task,
                                        latest_work_start: None,
                                    })
                                    .collect(),
                                active_worklog: None,
                                revision: "recovered".into(),
                            })
                        }
                    }),
                )
                .route(
                    "/v1/tasks",
                    post(move |Json(body): Json<CreateTaskRequest>| {
                        let saved = Arc::clone(&saved_for_post);
                        let requests = Arc::clone(&requests_for_post);
                        let healthy = Arc::clone(&healthy_for_post);
                        async move {
                            requests.lock().unwrap().push(body.clone());
                            let task = TaskDto {
                                id: body.task_id,
                                name: body.name,
                                archived: false,
                                created_at: body.occurred_at,
                                updated_at: body.occurred_at,
                            };
                            if committed || *healthy.lock().unwrap() {
                                *saved.lock().unwrap() = Some(task.clone());
                            }
                            if !*healthy.lock().unwrap() {
                                return (
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    "Response lost after execution",
                                )
                                    .into_response();
                            }
                            Json(MutationDto {
                                result: MutationResultDto::Task(task.clone()),
                                snapshot: SnapshotDto {
                                    task_items: vec![TaskItemDto {
                                        task,
                                        latest_work_start: None,
                                    }],
                                    active_worklog: None,
                                    revision: "created".into(),
                                },
                            })
                            .into_response()
                        }
                    }),
                );
            let (address_tx, address_rx) = mpsc::channel::<SocketAddr>();
            let (shutdown_tx, shutdown_rx) = oneshot::channel();
            let server = thread::spawn(move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(async move {
                        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                        address_tx.send(listener.local_addr().unwrap()).unwrap();
                        axum::serve(listener, router)
                            .with_graceful_shutdown(async {
                                let _ = shutdown_rx.await;
                            })
                            .await
                            .unwrap();
                    });
            });
            let address = address_rx.recv().unwrap();
            let mut client = RemoteApplication::connect(&format!("http://{address}"))
                .await
                .unwrap();
            let failure = client
                .create_task(TaskName::new("Project").unwrap(), at())
                .await
                .unwrap_err();
            assert_eq!(
                failure.failure().source(),
                tracker_application::ApplicationFailureSource::RemoteUnavailable
            );
            assert_eq!(client.last_failure, None);
            let first = requests.lock().unwrap()[0].clone();
            assert_eq!(
                client.pending_create.as_ref().unwrap().task_id,
                first.task_id
            );
            client.refresh().await.unwrap();
            *healthy.lock().unwrap() = true;
            let created = client
                .create_task(
                    TaskName::new("Project").unwrap(),
                    at() + Duration::seconds(90),
                )
                .await
                .unwrap();
            assert_eq!(created.id().to_string(), first.task_id);
            assert_eq!(created.created_at(), at());
            assert!(client.pending_create.is_none());
            assert_eq!(
                requests.lock().unwrap().len(),
                if committed { 1 } else { 2 }
            );
            shutdown_tx.send(()).unwrap();
            server.join().unwrap();
        }
    }
}

fn safe_message(message: &str) -> String {
    let cleaned: String = message
        .chars()
        .filter(|character| !character.is_control())
        .take(512)
        .collect();
    if cleaned.is_empty() {
        "Server rejected the request".to_owned()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use tracker_protocol::{TaskItemDto, WorklogDto};

    use super::*;

    fn task(id: TaskId) -> TaskDto {
        let at = Utc.timestamp_opt(100, 0).unwrap();
        TaskDto {
            id: id.to_string(),
            name: "Project work".into(),
            archived: false,
            created_at: at,
            updated_at: at,
        }
    }

    #[test]
    fn move_candidates_use_the_last_confirmed_snapshot_without_network_access() {
        let source = TaskId::generate();
        let destination = TaskId::generate();
        let archived = TaskId::generate();
        let mut client = RemoteApplication::disconnected("http://127.0.0.1:1").unwrap();
        assert!(client.move_candidates(source, "").is_empty());
        let mut archived_task = task(archived);
        archived_task.archived = true;
        client.snapshot.task_items = vec![task(source), task(destination), archived_task]
            .into_iter()
            .map(|dto| TaskListItem {
                task: decode_task(dto).unwrap(),
                latest_work_start: None,
            })
            .collect();
        let expected = vec![MoveCandidate {
            id: destination,
            name: "Project work".to_owned(),
        }];
        assert_eq!(client.move_candidates(source, ""), expected);
        assert_eq!(client.move_candidates(source, "pW"), expected);
        assert!(client.move_candidates(source, "zz").is_empty());
        assert_eq!(client.last_failure, Some(RemoteFailureKind::Unavailable));
    }

    #[test]
    fn rejects_invalid_active_worklog_before_adopting_snapshot() {
        let task_id = TaskId::generate();
        let at = Utc.timestamp_opt(100, 0).unwrap();
        let dto = SnapshotDto {
            task_items: vec![TaskItemDto {
                task: task(task_id),
                latest_work_start: None,
            }],
            active_worklog: Some(WorklogDto {
                id: WorklogId::generate().to_string(),
                task_id: task_id.to_string(),
                start: at,
                end: Some(at),
            }),
            revision: "r1".into(),
        };
        assert!(matches!(
            decode_snapshot(dto),
            Err(RemoteError::Protocol(_))
        ));
    }

    #[test]
    fn rejects_duplicate_tasks_in_server_snapshot() {
        let id = TaskId::generate();
        let item = TaskItemDto {
            task: task(id),
            latest_work_start: None,
        };
        let dto = SnapshotDto {
            task_items: vec![item.clone(), item],
            active_worklog: None,
            revision: "r1".into(),
        };
        assert!(matches!(
            decode_snapshot(dto),
            Err(RemoteError::Protocol(_))
        ));
    }
}
