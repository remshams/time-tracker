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
    ArchiveInactiveTasksRequest, CreateTaskRequest, DeleteWorklogRequest, ErrorCode, ErrorDto,
    HealthDto, InactiveTaskPreviewDto, MutationDto, MutationResultDto, ReportDto,
    SetTrackingRequest, TaskChangeRequest, TaskDto, TaskResourceDto, TasksDto, TrackingDto,
    WorklogChangeRequest, WorklogDto, WorklogPageDto, WorklogResourceDto, WriteGuard,
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

    pub async fn refresh_tasks(&mut self) -> Result<(), RemoteError> {
        self.ensure_version().await?;
        let (items, revision) = self.fetch_task_catalog().await?;
        self.task_cache.clear();
        self.task_observation = Some(ResourceObservation {
            value: items,
            revision,
        });
        self.confirmed();
        Ok(())
    }

    async fn fetch_task_catalog(&mut self) -> Result<(Vec<TaskListItem>, String), RemoteError> {
        let dto: TasksDto = self.read("v1/tasks").await?;
        decode_tasks(dto).map_err(|error| self.record_error(error))
    }

    pub async fn refresh_tracking(&mut self) -> Result<(), RemoteError> {
        self.ensure_version().await?;
        let dto: TrackingDto = self.read("v1/tracking").await?;
        let (_, tracking, revision) =
            decode_tracking(dto).map_err(|error| self.record_error(error))?;
        self.tracking_observation = Some(ResourceObservation {
            value: tracking,
            revision,
        });
        self.confirmed();
        Ok(())
    }

    /// Composes a coherent task and tracking view, retrying reconciliation once.
    pub async fn refresh_task_list(&mut self) -> Result<(), RemoteError> {
        self.ensure_version().await?;
        for _ in 0..2 {
            let tasks: TasksDto = self.read("v1/tasks").await?;
            let tracking: TrackingDto = self.read("v1/tracking").await?;
            let (items, task_revision) =
                decode_tasks(tasks).map_err(|error| self.record_error(error))?;
            let (active, state, tracking_revision) =
                decode_tracking(tracking).map_err(|error| self.record_error(error))?;
            if task_revision != tracking_revision {
                continue;
            }
            validate_active_task(&items, active.as_ref())
                .map_err(|error| self.record_error(error))?;
            self.task_cache.clear();
            self.task_observation = Some(ResourceObservation {
                value: items,
                revision: task_revision,
            });
            self.tracking_observation = Some(ResourceObservation {
                value: state,
                revision: tracking_revision,
            });
            self.confirmed();
            return Ok(());
        }
        Err(self.record_error(RemoteError::Http {
            status: StatusCode::CONFLICT,
            body: vec![],
        }))
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
        for _ in 0..2 {
            let report = self.fetch_task_totals(start, end, now).await?;
            let tracking: TrackingDto = self.read("v1/tracking").await?;
            let (active, state, revision) =
                decode_tracking(tracking).map_err(|error| self.record_error(error))?;
            let tasks = if include_tasks {
                Some(self.fetch_task_catalog().await?)
            } else {
                None
            };
            if report.revision != revision
                || tasks.as_ref().is_some_and(|(_, token)| token != &revision)
            {
                continue;
            }
            if let Some((items, _)) = &tasks {
                validate_active_task(items, active.as_ref())
                    .map_err(|error| self.record_error(error))?;
            }
            if let Some((items, task_revision)) = tasks {
                self.task_cache.clear();
                self.task_observation = Some(ResourceObservation {
                    value: items,
                    revision: task_revision,
                });
            }
            self.tracking_observation = Some(ResourceObservation {
                value: state,
                revision,
            });
            self.report_cache = Some(report);
            self.confirmed();
            return Ok(());
        }
        Err(self.record_error(RemoteError::Http {
            status: StatusCode::CONFLICT,
            body: vec![],
        }))
    }

    pub fn last_write_attempted(&self) -> bool {
        self.last_write_attempted
    }

    pub fn last_failure(&self) -> Option<RemoteFailureKind> {
        self.last_failure
    }
    pub fn task_observation(&self) -> Option<&ResourceObservation<Vec<TaskListItem>>> {
        self.task_observation.as_ref()
    }
    pub fn tracking_observation(&self) -> Option<&ResourceObservation<TrackingState>> {
        self.tracking_observation.as_ref()
    }
    pub fn task_items(&self) -> &[TaskListItem] {
        self.task_observation
            .as_ref()
            .map_or(&[], |observation| observation.value.as_slice())
    }
    pub fn active_worklog(&self) -> Option<Worklog> {
        match self.current_tracking() {
            TrackingState::Idle => None,
            TrackingState::Running { worklog } => Some(worklog.to_worklog()),
        }
    }
    pub fn task_revision(&self) -> &str {
        self.task_observation
            .as_ref()
            .map_or("", |observation| observation.revision.as_str())
    }
    pub fn tracking_revision(&self) -> &str {
        self.tracking_observation
            .as_ref()
            .map_or("", |observation| observation.revision.as_str())
    }
    pub fn report_revision(&self) -> &str {
        self.report_cache
            .as_ref()
            .map_or("", |report| report.revision.as_str())
    }
    pub fn last_command_receipt(&self) -> Option<&MutationDto> {
        self.last_command_receipt.as_ref()
    }
    pub fn report_observation(&self) -> Option<&ReportDto> {
        self.report_cache.as_ref()
    }
    pub fn cached_task_totals(&self) -> Option<&ReportDto> {
        self.report_cache.as_ref()
    }
    pub fn cached_history(&self, task_id: Option<TaskId>) -> Option<&WorklogPageDto> {
        self.history_cache.get(&task_id)
    }
    pub fn cached_worklog(&self, id: WorklogId) -> Option<&WorklogResourceDto> {
        self.worklog_cache.get(&id)
    }

    pub fn tasks(&self, ordering: TaskOrdering) -> Vec<TaskListItem> {
        let mut items = self.task_items().to_vec();
        ordering.sort_items(&mut items);
        items
    }
    pub fn move_candidates(&self, source_task_id: TaskId, query: &str) -> Vec<MoveCandidate> {
        tracker_application::move_candidates_for_tasks(self.task_items(), source_task_id, query)
    }
    pub fn task_item(&self, id: TaskId) -> Option<&TaskListItem> {
        self.task_cache
            .get(&id)
            .map(|(item, _)| item)
            .or_else(|| self.task_items().iter().find(|item| item.task.id() == id))
    }
    pub fn task(&self, id: TaskId) -> Option<&Task> {
        self.task_item(id).map(|item| &item.task)
    }
    pub fn current_tracking(&self) -> &TrackingState {
        self.tracking_observation
            .as_ref()
            .map_or(&TrackingState::Idle, |observation| &observation.value)
    }

    fn guard(revision: &str) -> WriteGuard {
        WriteGuard {
            expected_revision: revision.to_owned(),
            request_id: uuid::Uuid::now_v7().to_string(),
        }
    }

    async fn fetch_task(
        &mut self,
        id: TaskId,
    ) -> Result<(TaskResourceDto, TaskListItem), RemoteError> {
        self.ensure_version().await?;
        let dto: TaskResourceDto = self.read(&format!("v1/tasks/{id}")).await?;
        if dto.task.id != id.to_string() || dto.revision.is_empty() {
            return Err(self.record_error(RemoteError::Protocol("invalid task resource".into())));
        }
        let item = decode_task_item(dto.task.clone()).map_err(|error| self.record_error(error))?;
        Ok((dto, item))
    }

    async fn read_task_item(
        &mut self,
        id: TaskId,
    ) -> Result<(TaskListItem, String), ApplicationError> {
        let (dto, item) = self
            .fetch_task(id)
            .await
            .map_err(|error| map_application_error(&error, None, Some(id)))?;
        Ok((item, dto.revision))
    }

    /// Reads one task with its own revision and preserves application error categories.
    pub async fn read_task_observation(
        &mut self,
        id: TaskId,
    ) -> Result<ResourceObservation<TaskListItem>, ApplicationError> {
        let (value, revision) = self.read_task_item(id).await?;
        self.adopt_task(value.clone(), revision.clone());
        self.confirmed();
        Ok(ResourceObservation { value, revision })
    }

    pub async fn read_task(&mut self, id: TaskId) -> Result<TaskResourceDto, RemoteError> {
        let (dto, item) = self.fetch_task(id).await?;
        self.adopt_task(item, dto.revision.clone());
        self.confirmed();
        Ok(dto)
    }

    pub async fn read_worklog(&mut self, id: WorklogId) -> Result<WorklogResourceDto, RemoteError> {
        self.ensure_version().await?;
        let dto: WorklogResourceDto = self.read(&format!("v1/worklogs/{id}")).await?;
        if dto.worklog.id != id.to_string() || dto.revision.is_empty() {
            return Err(self.record_error(RemoteError::Protocol("invalid worklog resource".into())));
        }
        decode_worklog(dto.worklog.clone()).map_err(|error| self.record_error(error))?;
        self.worklog_cache.insert(id, dto.clone());
        self.confirmed();
        Ok(dto)
    }

    pub async fn resolve_preview_tasks(
        &mut self,
        preview: &InactiveTaskPreviewDto,
    ) -> Result<Vec<TaskDto>, ApplicationError> {
        validate_inactive_preview(preview)?;
        let mut tasks = Vec::with_capacity(preview.count);
        for raw_id in &preview.candidate_task_ids {
            let id = raw_id
                .parse()
                .map_err(|_| protocol_failure("invalid candidate task id"))?;
            let dto = self
                .read_task(id)
                .await
                .map_err(|error| map_application_error(&error, None, Some(id)))?;
            if dto.revision != preview.revision || dto.task.archived {
                return Err(self.intent_changed());
            }
            tasks.push(dto.task);
        }
        Ok(tasks)
    }

    fn adopt_task(&mut self, item: TaskListItem, revision: String) {
        self.task_cache.insert(item.task.id(), (item, revision));
    }

    pub fn cached_task_revision(&self, id: TaskId) -> Option<&str> {
        self.task_cache
            .get(&id)
            .map(|(_, revision)| revision.as_str())
    }

    async fn task_guard(&mut self, id: TaskId) -> Result<String, ApplicationError> {
        let reviewed = self.task(id).cloned();
        let (item, revision) = self.read_task_item(id).await?;
        let changed = reviewed.is_some_and(|old| old != item.task);
        self.adopt_task(item, revision.clone());
        if changed {
            return Err(self.intent_changed());
        }
        Ok(revision)
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

    pub async fn create_task(
        &mut self,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        self.last_write_attempted = false;
        self.last_command_receipt = None;
        self.refresh_task_command_view()
            .await
            .map_err(|error| map_application_error(&error, None, None))?;
        if let Some(pending) = self.pending_create.clone() {
            if pending.name != name.as_str() {
                return Err(ApplicationError::semantic_failure(
                    ApplicationFailureCategory::General,
                    "Recover the pending task creation before creating another task",
                ));
            }
            let id = pending
                .task_id
                .parse()
                .map_err(|_| protocol_failure("invalid pending task id"))?;
            if let Some(task) = self.task(id).cloned() {
                self.pending_create = None;
                return Ok(task);
            }
        }
        let mut body = self
            .pending_create
            .clone()
            .unwrap_or_else(|| CreateTaskRequest {
                task_id: TaskId::generate().to_string(),
                name: name.as_str().to_owned(),
                occurred_at: canonical(occurred_at),
                guard: Self::guard(self.task_revision()),
            });
        if body.guard.expected_revision != self.task_revision() {
            body.guard = Self::guard(self.task_revision());
        }
        let id = body
            .task_id
            .parse()
            .map_err(|_| protocol_failure("invalid pending task id"))?;
        self.pending_create = Some(body.clone());
        let mut receipt_confirmed = false;
        let result = self
            .mutation(
                Method::POST,
                "v1/tasks",
                &body,
                &body.guard,
                MutationScope {
                    recovery: Recovery::Tasks,
                    worklog_id: None,
                    task_id: Some(id),
                },
                |result| {
                    let task = task_result(result, id)?;
                    if task.name().as_str() != body.name
                        || task.is_archived()
                        || task.created_at() != body.occurred_at
                        || task.updated_at() != body.occurred_at
                    {
                        return Err(protocol_failure("created task contradicts command"));
                    }
                    receipt_confirmed = true;
                    Ok(task)
                },
            )
            .await;
        if !result
            .as_ref()
            .is_err_and(|error| creation_needs_recovery(error, receipt_confirmed))
        {
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
        self.last_write_attempted = false;
        self.last_command_receipt = None;
        if let Some(expected_name) = self.task(id).map(|task| task.name().clone()) {
            return self
                .rename_task_with_expected_name(id, &expected_name, name, occurred_at)
                .await;
        }
        let revision = self.task_guard(id).await?;
        let body = TaskChangeRequest::Rename {
            name: name.as_str().to_owned(),
            occurred_at: canonical(occurred_at),
            guard: Self::guard(&revision),
        };
        self.task_mutation(id, body).await
    }
    /// Keeps the name the user reviewed even when background reads update the cache.
    pub async fn rename_task_with_expected_name(
        &mut self,
        id: TaskId,
        expected_name: &TaskName,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        self.last_write_attempted = false;
        self.last_command_receipt = None;
        let revision = self.task_guard(id).await?;
        if self
            .task(id)
            .is_none_or(|task| task.name() != expected_name)
        {
            return Err(self.intent_changed());
        }
        let body = TaskChangeRequest::Rename {
            name: name.as_str().to_owned(),
            occurred_at: canonical(occurred_at),
            guard: Self::guard(&revision),
        };
        self.task_mutation(id, body).await
    }

    pub async fn archive_task(
        &mut self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        self.last_write_attempted = false;
        self.last_command_receipt = None;
        let revision = self.task_guard(id).await?;
        self.task_mutation(
            id,
            TaskChangeRequest::Archive {
                occurred_at: canonical(occurred_at),
                guard: Self::guard(&revision),
            },
        )
        .await
    }
    pub async fn archive_task_with_expected_name(
        &mut self,
        id: TaskId,
        expected_name: &TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        self.last_write_attempted = false;
        self.last_command_receipt = None;
        let revision = self.task_guard(id).await?;
        if self
            .task(id)
            .is_none_or(|task| task.name() != expected_name)
        {
            return Err(self.intent_changed());
        }
        self.task_mutation(
            id,
            TaskChangeRequest::Archive {
                occurred_at: canonical(occurred_at),
                guard: Self::guard(&revision),
            },
        )
        .await
    }

    pub async fn unarchive_task(
        &mut self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        self.last_write_attempted = false;
        self.last_command_receipt = None;
        let revision = self.task_guard(id).await?;
        self.task_mutation(
            id,
            TaskChangeRequest::Restore {
                occurred_at: canonical(occurred_at),
                guard: Self::guard(&revision),
            },
        )
        .await
    }
    async fn task_mutation(
        &mut self,
        id: TaskId,
        body: TaskChangeRequest,
    ) -> Result<Task, ApplicationError> {
        self.mutation(
            Method::PATCH,
            &format!("v1/tasks/{id}"),
            &body,
            body.guard(),
            MutationScope {
                recovery: Recovery::Tasks,
                worklog_id: None,
                task_id: Some(id),
            },
            |result| {
                let task = task_result(result, id)?;
                let valid = match &body {
                    TaskChangeRequest::Rename { name, .. } => task.name().as_str() == name,
                    TaskChangeRequest::Archive { .. } => task.is_archived(),
                    TaskChangeRequest::Restore { .. } => !task.is_archived(),
                };
                if !valid {
                    return Err(protocol_failure("task result contradicts command"));
                }
                Ok(task)
            },
        )
        .await
    }

    pub async fn preview_inactive_tasks(
        &mut self,
        as_of: DateTime<Utc>,
    ) -> Result<InactiveTaskPreviewDto, ApplicationError> {
        self.preview_inactive_tasks_with_period(as_of, InactivityPeriod::default())
            .await
    }
    pub async fn preview_inactive_tasks_with_period(
        &mut self,
        as_of: DateTime<Utc>,
        period: InactivityPeriod,
    ) -> Result<InactiveTaskPreviewDto, ApplicationError> {
        self.ensure_version()
            .await
            .map_err(|error| map_application_error(&error, None, None))?;
        let as_of = canonical(as_of);
        let path = format!(
            "v1/tasks/inactive-preview?as_of={}&inactive_days={}",
            as_of.to_rfc3339().replace('+', "%2B"),
            period.days()
        );
        let preview: InactiveTaskPreviewDto = self
            .read(&path)
            .await
            .map_err(|error| map_application_error(&error, None, None))?;
        validate_inactive_preview(&preview)
            .inspect_err(|_| self.last_failure = Some(RemoteFailureKind::Protocol))?;
        if preview.as_of != as_of || preview.inactive_days != period.days() {
            self.last_failure = Some(RemoteFailureKind::Protocol);
            return Err(protocol_failure("invalid inactive task preview"));
        }
        self.confirmed();
        Ok(preview)
    }
    pub async fn archive_inactive_tasks(
        &mut self,
        preview: &InactiveTaskPreviewDto,
    ) -> Result<usize, ApplicationError> {
        self.last_write_attempted = false;
        self.last_command_receipt = None;
        validate_inactive_preview(preview)?;
        let body = ArchiveInactiveTasksRequest {
            as_of: preview.as_of,
            inactive_days: preview.inactive_days,
            guard: Self::guard(&preview.revision),
        };
        self.mutation(
            Method::POST,
            "v1/tasks/archive-inactive",
            &body,
            &body.guard,
            MutationScope {
                recovery: Recovery::Tasks,
                worklog_id: None,
                task_id: None,
            },
            |result| match result {
                MutationResultDto::ArchivedInactive { count } => Ok(count),
                _ => Err(protocol_failure("invalid inactive task archive result")),
            },
        )
        .await
    }
    pub async fn archive_inactive_tasks_with_period(
        &mut self,
        preview: &InactiveTaskPreviewDto,
    ) -> Result<usize, ApplicationError> {
        self.last_write_attempted = false;
        self.last_command_receipt = None;
        self.archive_inactive_tasks(preview).await
    }

    async fn initialize_tracking_command_view(
        &mut self,
        target: Option<TaskId>,
    ) -> Result<(), ApplicationError> {
        if self.coherent_task_views
            && (self.task_observation.is_none() || self.tracking_observation.is_none())
        {
            self.refresh_task_list()
                .await
                .map_err(|error| map_application_error(&error, None, target))?;
        }
        Ok(())
    }

    fn adopt_tracking_preflight(
        &mut self,
        _active: Option<Worklog>,
        state: TrackingState,
        revision: &str,
    ) {
        if !self.coherent_task_views {
            self.tracking_observation = Some(ResourceObservation {
                value: state,
                revision: revision.to_owned(),
            });
        }
    }

    fn reviewed_tracking_state(&self) -> Option<TrackingState> {
        if self.tracking_revision().is_empty() {
            None
        } else {
            Some(self.current_tracking().clone())
        }
    }

    async fn tracking_guard(&mut self, target: Option<TaskId>) -> Result<String, ApplicationError> {
        let reviewed = self.reviewed_tracking_state();
        let reviewed_task = target.and_then(|id| self.task(id).cloned());
        self.initialize_tracking_command_view(target).await?;
        let reviewed = reviewed.or_else(|| self.reviewed_tracking_state());
        let reviewed_task = reviewed_task.or_else(|| target.and_then(|id| self.task(id).cloned()));
        for _ in 0..2 {
            self.ensure_version()
                .await
                .map_err(|error| map_application_error(&error, None, target))?;
            let dto: TrackingDto = self
                .read("v1/tracking")
                .await
                .map_err(|error| map_application_error(&error, None, target))?;
            let (active, state, revision) = decode_tracking(dto)
                .map_err(|error| map_application_error(&error, None, target))?;
            let task = if let Some(id) = target {
                Some(self.read_task_item(id).await?)
            } else {
                None
            };
            if task.as_ref().is_some_and(|(_, r)| r != &revision) {
                continue;
            }
            let changed = reviewed_tracking_changed(
                reviewed.as_ref(),
                &state,
                reviewed_task.as_ref(),
                task.as_ref().map(|(item, _)| item),
            );
            self.adopt_tracking_preflight(active, state, &revision);
            if let Some((item, task_revision)) = task {
                self.adopt_task(item, task_revision);
            }
            if changed {
                return Err(self.intent_changed());
            }
            return Ok(revision);
        }
        Err(self.intent_changed())
    }
    pub async fn set_active_task(
        &mut self,
        task_id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, ApplicationError> {
        self.last_write_attempted = false;
        self.last_command_receipt = None;
        if !self.tracking_revision().is_empty() {
            return self
                .set_active_task_with_expected_active(
                    task_id,
                    active_id(self.current_tracking()),
                    occurred_at,
                )
                .await;
        }
        let revision = self.tracking_guard(Some(task_id)).await?;
        self.set_active_task_guarded(task_id, occurred_at, revision)
            .await
    }

    pub async fn set_active_task_with_expected_active(
        &mut self,
        task_id: TaskId,
        expected_active: Option<WorklogId>,
        occurred_at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, ApplicationError> {
        self.last_write_attempted = false;
        self.last_command_receipt = None;
        let revision = self.tracking_guard(Some(task_id)).await?;
        if active_id(self.current_tracking()) != expected_active {
            return Err(self.intent_changed());
        }
        self.set_active_task_guarded(task_id, occurred_at, revision)
            .await
    }

    async fn set_active_task_guarded(
        &mut self,
        task_id: TaskId,
        occurred_at: DateTime<Utc>,
        revision: String,
    ) -> Result<SetActiveTaskOutcome, ApplicationError> {
        let original_active = self.active_worklog();
        let old_active = active_id(self.current_tracking());
        let new_id = WorklogId::generate();
        let body = SetTrackingRequest {
            task_id: Some(task_id.to_string()),
            worklog_id: Some(new_id.to_string()),
            expected_active: old_active.map(|id| id.to_string()),
            occurred_at: canonical(occurred_at),
            guard: Self::guard(&revision),
        };
        self.mutation(
            Method::PUT,
            "v1/tracking",
            &body,
            &body.guard,
            MutationScope {
                recovery: Recovery::Tracking,
                worklog_id: None,
                task_id: Some(task_id),
            },
            |result| {
                let outcome = set_tracking_result(result, task_id, new_id, old_active)?;
                let valid = match &outcome {
                    SetActiveTaskOutcome::Started { worklog } => {
                        worklog.start() == body.occurred_at
                    }
                    SetActiveTaskOutcome::Switched { stopped, started } => {
                        stopped.end() == Some(body.occurred_at)
                            && started.start() == body.occurred_at
                            && original_active.as_ref().is_some_and(|original| {
                                stopped.task_id() == original.task_id()
                                    && stopped.start() == original.start()
                            })
                    }
                    SetActiveTaskOutcome::AlreadyActive { worklog } => {
                        old_active == Some(worklog.id())
                            && original_active.as_ref() == Some(&worklog.to_worklog())
                    }
                };
                if !valid {
                    return Err(protocol_failure("tracking result contradicts command"));
                }
                Ok(outcome)
            },
        )
        .await
    }
    pub async fn clear_active_task(
        &mut self,
        expected_active: WorklogId,
        occurred_at: DateTime<Utc>,
    ) -> Result<ClearActiveTaskOutcome, ApplicationError> {
        self.last_write_attempted = false;
        self.last_command_receipt = None;
        let revision = self.tracking_guard(None).await?;
        if active_id(self.current_tracking()).is_some_and(|id| id != expected_active) {
            return Err(self.intent_changed());
        }
        let original_active = self.active_worklog();
        let body = SetTrackingRequest {
            task_id: None,
            worklog_id: None,
            expected_active: active_id(self.current_tracking())
                .map(|_| expected_active.to_string()),
            occurred_at: canonical(occurred_at),
            guard: Self::guard(&revision),
        };
        self.mutation(
            Method::PUT,
            "v1/tracking",
            &body,
            &body.guard,
            MutationScope {
                recovery: Recovery::Tracking,
                worklog_id: Some(expected_active),
                task_id: None,
            },
            |result| {
                let outcome = clear_tracking_result(result, expected_active)?;
                let valid = match &outcome {
                    ClearActiveTaskOutcome::Stopped { worklog } => {
                        worklog.end() == Some(body.occurred_at)
                            && original_active.as_ref().is_some_and(|original| {
                                worklog.task_id() == original.task_id()
                                    && worklog.start() == original.start()
                            })
                    }
                    ClearActiveTaskOutcome::AlreadyIdle => original_active.is_none(),
                };
                if !valid {
                    return Err(protocol_failure("stopped worklog contradicts command"));
                }
                Ok(outcome)
            },
        )
        .await
    }

    async fn history(
        &mut self,
        task_id: Option<TaskId>,
        after: Option<(DateTime<Utc>, WorklogId, i64)>,
    ) -> Result<HistoryRead, ApplicationError> {
        self.ensure_version()
            .await
            .map_err(|error| map_application_error(&error, None, task_id))?;
        let mut url = self.transport.url("v1/worklogs");
        if let Some(id) = task_id {
            url.query_pairs_mut()
                .append_pair("task_id", &id.to_string());
        }
        if let Some((start, id, revision)) = after {
            url.query_pairs_mut()
                .append_pair("after_start", &start.to_rfc3339())
                .append_pair("after_id", &id.to_string())
                .append_pair("after_revision", &revision.to_string());
            if let Some(task_id) = task_id {
                url.query_pairs_mut()
                    .append_pair("after_task_id", &task_id.to_string());
            }
        }
        let dto: WorklogPageDto = self
            .transport
            .send(Method::GET, url, None::<&()>)
            .await
            .map_err(|error| {
                self.record_error(error.clone());
                map_application_error(&error, None, task_id)
            })?;
        let decoded = decode_history(&dto, task_id).map_err(|error| {
            self.record_error(error.clone());
            map_application_error(&error, None, task_id)
        })?;
        self.history_cache.insert(task_id, dto);
        self.confirmed();
        Ok(decoded)
    }
    pub async fn worklogs_for_task(
        &mut self,
        task_id: TaskId,
        after: Option<&WorklogCursor>,
    ) -> Result<WorklogPage, ApplicationError> {
        if after.is_some_and(|cursor| cursor.task_id != task_id) {
            return Err(ApplicationError::worklog_history_changed(task_id));
        }
        let dto = self
            .history(
                Some(task_id),
                after.map(|cursor| (cursor.start, cursor.id, cursor.revision)),
            )
            .await?;
        Ok(WorklogPage {
            worklogs: dto.worklogs,
            next_cursor: dto.next_cursor.map(|cursor| WorklogCursor {
                task_id,
                start: cursor.start,
                id: cursor.id,
                revision: cursor.revision,
            }),
            snapshot: None,
        })
    }
    pub async fn all_worklogs(
        &mut self,
        after: Option<&GlobalWorklogCursor>,
    ) -> Result<GlobalWorklogPage, ApplicationError> {
        let dto = self
            .history(
                None,
                after.map(|cursor| (cursor.start, cursor.id, cursor.revision)),
            )
            .await?;
        Ok(GlobalWorklogPage {
            next_cursor: dto.next_cursor,
            task_items: vec![],
            tracking: None,
            worklogs: dto.worklogs,
        })
    }

    async fn worklog_guard(
        &mut self,
        id: WorklogId,
        expected_task: Option<TaskId>,
        expected: WorklogTimes,
        destination: Option<TaskId>,
    ) -> Result<String, ApplicationError> {
        self.ensure_version()
            .await
            .map_err(|error| map_application_error(&error, Some(id), None))?;
        let reviewed_destination = destination.and_then(|id| self.task(id).cloned());
        for _ in 0..2 {
            let dto = self
                .read_worklog(id)
                .await
                .map_err(|error| map_application_error(&error, Some(id), None))?;
            let worklog = app_decode_worklog(dto.worklog)?;
            if !original_worklog_matches(&worklog, expected_task, expected) {
                self.last_failure = Some(RemoteFailureKind::Conflict);
                let primary = ApplicationError::worklog_changed(id);
                if let Err(recovery) = self.recover(Recovery::Worklog).await {
                    return Err(
                        primary.with_recovery_failure(map_application_error(&recovery, None, None))
                    );
                }
                return Err(primary);
            }
            if let Some(destination) = destination {
                let (item, revision) = self.read_task_item(destination).await?;
                if dto.revision != revision {
                    continue;
                }
                let changed = reviewed_destination
                    .as_ref()
                    .is_some_and(|old| old != &item.task);
                self.adopt_task(item, revision);
                if changed {
                    return Err(self.intent_changed());
                }
            }
            return Ok(dto.revision);
        }
        Err(self.intent_changed())
    }
    pub async fn move_worklog(
        &mut self,
        id: WorklogId,
        expected_source_task_id: TaskId,
        expected: WorklogTimes,
        destination_task_id: TaskId,
    ) -> Result<Worklog, ApplicationError> {
        self.last_write_attempted = false;
        self.last_command_receipt = None;
        let revision = self
            .worklog_guard(
                id,
                Some(expected_source_task_id),
                expected,
                Some(destination_task_id),
            )
            .await?;
        let body = WorklogChangeRequest::Move {
            expected_task_id: expected_source_task_id.to_string(),
            expected_start: canonical(expected.start()),
            expected_end: expected.end().map(canonical),
            destination_task_id: destination_task_id.to_string(),
            guard: Self::guard(&revision),
        };
        self.mutation(
            Method::PATCH,
            &format!("v1/worklogs/{id}"),
            &body,
            body.guard(),
            MutationScope {
                recovery: Recovery::Worklog,
                worklog_id: Some(id),
                task_id: None,
            },
            |result| {
                let worklog = worklog_result(result, id)?;
                if worklog.task_id() != destination_task_id
                    || worklog.start() != canonical(expected.start())
                    || worklog.end() != expected.end().map(canonical)
                {
                    return Err(protocol_failure("moved worklog contradicts command"));
                }
                Ok(worklog)
            },
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
        self.last_write_attempted = false;
        self.last_command_receipt = None;
        let revision = self.worklog_guard(id, None, expected, None).await?;
        let expected_task_id = self
            .worklog_cache
            .get(&id)
            .expect("successful preflight caches the worklog")
            .worklog
            .task_id
            .clone();
        let body = WorklogChangeRequest::Correct {
            expected_start: canonical(expected.start()),
            expected_end: expected.end().map(canonical),
            replacement_start: canonical(replacement.start()),
            replacement_end: replacement.end().map(canonical),
            occurred_at: canonical(occurred_at),
            guard: Self::guard(&revision),
        };
        self.mutation(
            Method::PATCH,
            &format!("v1/worklogs/{id}"),
            &body,
            body.guard(),
            MutationScope {
                recovery: Recovery::Worklog,
                worklog_id: Some(id),
                task_id: None,
            },
            |result| {
                let worklog = worklog_result(result, id)?;
                if worklog.task_id().to_string() != expected_task_id
                    || worklog.start() != canonical(replacement.start())
                    || worklog.end() != replacement.end().map(canonical)
                {
                    return Err(protocol_failure("corrected worklog contradicts command"));
                }
                Ok(worklog)
            },
        )
        .await
    }
    pub async fn delete_completed_worklog(
        &mut self,
        id: WorklogId,
        expected_task_id: TaskId,
        expected: WorklogTimes,
    ) -> Result<Worklog, ApplicationError> {
        self.last_write_attempted = false;
        self.last_command_receipt = None;
        let end = expected
            .end()
            .ok_or_else(|| ApplicationError::active_worklog(id))?;
        let revision = self
            .worklog_guard(id, Some(expected_task_id), expected, None)
            .await?;
        let body = DeleteWorklogRequest {
            expected_task_id: expected_task_id.to_string(),
            expected_start: canonical(expected.start()),
            expected_end: canonical(end),
            guard: Self::guard(&revision),
        };
        self.mutation(
            Method::DELETE,
            &format!("v1/worklogs/{id}"),
            &body,
            &body.guard,
            MutationScope {
                recovery: Recovery::Worklog,
                worklog_id: Some(id),
                task_id: None,
            },
            |result| {
                let worklog = worklog_result(result, id)?;
                if worklog.task_id() != expected_task_id
                    || worklog.start() != canonical(expected.start())
                    || worklog.end() != expected.end().map(canonical)
                {
                    return Err(protocol_failure("deleted worklog contradicts command"));
                }
                Ok(worklog)
            },
        )
        .await
    }

    async fn fetch_task_totals(
        &mut self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<ReportDto, RemoteError> {
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
        let mut url = self.transport.url("v1/reports/task-totals");
        url.query_pairs_mut()
            .append_pair("start", &start.to_rfc3339())
            .append_pair("end", &end.to_rfc3339())
            .append_pair("now", &now.to_rfc3339());
        let dto: ReportDto = self
            .transport
            .send(Method::GET, url, None::<&()>)
            .await
            .map_err(|error| self.record_error(error))?;
        validate_report(&dto)
            .map_err(|error| self.record_error(RemoteError::Protocol(error.to_string())))?;
        if dto.start != canonical(start) || dto.end != canonical(end) || dto.now != canonical(now) {
            return Err(self.record_error(RemoteError::Protocol(
                "report context does not match request".into(),
            )));
        }
        Ok(dto)
    }
    pub async fn task_totals(
        &mut self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<ReportDto, ApplicationError> {
        if end <= start {
            return Err(ApplicationError::InvalidReportRange);
        }
        let dto = self
            .fetch_task_totals(start, end, now)
            .await
            .map_err(|error| map_application_error(&error, None, None))?;
        self.report_cache = Some(dto.clone());
        self.confirmed();
        Ok(dto)
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

fn validate_inactive_preview(preview: &InactiveTaskPreviewDto) -> Result<(), ApplicationError> {
    if preview.revision.is_empty()
        || InactivityPeriod::new(preview.inactive_days).is_err()
        || preview.as_of != canonical(preview.as_of)
        || preview.count != preview.candidate_task_ids.len()
    {
        return Err(protocol_failure("invalid inactive task preview"));
    }
    let mut ids = HashSet::new();
    for raw_id in &preview.candidate_task_ids {
        let id = raw_id
            .parse::<TaskId>()
            .map_err(|_| protocol_failure("invalid inactive task preview"))?;
        if !ids.insert(id) {
            return Err(protocol_failure("invalid inactive task preview"));
        }
    }
    Ok(())
}
fn validate_report(dto: &ReportDto) -> Result<(), ApplicationError> {
    let mut ids = HashSet::new();
    let mut sum = 0_i64;
    if dto.revision.is_empty() || dto.end <= dto.start {
        return Err(protocol_failure("invalid report context"));
    }
    for row in &dto.rows {
        let id = row
            .task_id
            .parse::<TaskId>()
            .map_err(|_| protocol_failure("invalid report row"))?;
        if row.duration_us <= 0 || !ids.insert(id) {
            return Err(protocol_failure("invalid report row"));
        }
        sum = sum
            .checked_add(row.duration_us)
            .ok_or(ApplicationError::ReportDurationOverflow)?;
    }
    if sum != dto.total_us {
        return Err(protocol_failure("report total does not match rows"));
    }
    Ok(())
}
fn validate_active_task(
    items: &[TaskListItem],
    active: Option<&Worklog>,
) -> Result<(), RemoteError> {
    if let Some(active) = active
        && !items
            .iter()
            .any(|item| item.task.id() == active.task_id() && !item.task.is_archived())
    {
        return Err(RemoteError::Protocol(
            "active worklog has no active task".into(),
        ));
    }
    Ok(())
}
fn decode_tasks(dto: TasksDto) -> Result<(Vec<TaskListItem>, String), RemoteError> {
    if dto.revision.is_empty() {
        return Err(RemoteError::Protocol("empty task revision".into()));
    }
    let mut ids = HashSet::new();
    let items = dto
        .tasks
        .into_iter()
        .map(|dto| {
            let item = decode_task_item(dto)?;
            if !ids.insert(item.task.id()) {
                return Err(RemoteError::Protocol("duplicate task".into()));
            }
            Ok(item)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((items, dto.revision))
}
fn decode_task_item(dto: TaskDto) -> Result<TaskListItem, RemoteError> {
    let latest_work_start = dto.latest_work_start;
    Ok(TaskListItem {
        task: decode_task(dto)?,
        latest_work_start,
    })
}
fn decode_tracking(
    dto: TrackingDto,
) -> Result<(Option<Worklog>, TrackingState, String), RemoteError> {
    if dto.revision.is_empty() {
        return Err(RemoteError::Protocol("empty tracking revision".into()));
    }
    let (active, tracking) = decode_active(dto.active_worklog)?;
    Ok((active, tracking, dto.revision))
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
fn decode_active(dto: Option<WorklogDto>) -> Result<(Option<Worklog>, TrackingState), RemoteError> {
    let worklog = dto.map(decode_worklog).transpose()?;
    let tracking = match worklog.clone() {
        Some(worklog) => Tracker::resume(worklog)
            .map_err(|error| RemoteError::Protocol(error.to_string()))?
            .state()
            .clone(),
        None => TrackingState::Idle,
    };
    Ok((worklog, tracking))
}
fn decode_history(
    dto: &WorklogPageDto,
    task_id: Option<TaskId>,
) -> Result<HistoryRead, RemoteError> {
    if dto.revision.is_empty()
        || dto
            .next_cursor
            .as_ref()
            .is_some_and(|cursor| cursor.task_id != task_id.map(|id| id.to_string()))
    {
        return Err(RemoteError::Protocol("invalid history scope".into()));
    }
    let worklogs = dto
        .worklogs
        .iter()
        .cloned()
        .map(decode_worklog)
        .collect::<Result<Vec<_>, _>>()?;
    if task_id.is_some_and(|id| worklogs.iter().any(|worklog| worklog.task_id() != id)) {
        return Err(RemoteError::Protocol(
            "history contains another task".into(),
        ));
    }
    let next_cursor = dto
        .next_cursor
        .as_ref()
        .map(|cursor| {
            Ok::<_, RemoteError>(GlobalWorklogCursor {
                start: cursor.start,
                id: cursor
                    .id
                    .parse()
                    .map_err(|_| RemoteError::Protocol("invalid cursor worklog id".into()))?,
                revision: cursor.revision,
            })
        })
        .transpose()?;
    Ok(HistoryRead {
        worklogs,
        next_cursor,
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
    use super::*;
    use chrono::Duration;
    use std::time::Duration as StdDuration;
    fn at() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap()
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
}

#[cfg(test)]
mod resource_tests {
    use super::*;
    fn at() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap()
    }
    fn task(id: TaskId) -> TaskDto {
        TaskDto {
            id: id.to_string(),
            name: "Project work".into(),
            archived: false,
            created_at: at(),
            updated_at: at(),
            latest_work_start: None,
        }
    }
    #[test]
    fn task_catalog_rejects_duplicates_invalid_tasks_and_empty_revisions() {
        let item = task(TaskId::generate());
        for dto in [
            TasksDto {
                tasks: vec![item.clone(), item.clone()],
                revision: "r1".into(),
            },
            TasksDto {
                tasks: vec![],
                revision: String::new(),
            },
            TasksDto {
                tasks: vec![TaskDto {
                    id: "invalid".into(),
                    ..item.clone()
                }],
                revision: "r1".into(),
            },
            TasksDto {
                tasks: vec![TaskDto {
                    name: "Bad\u{1b}name".into(),
                    ..item
                }],
                revision: "r1".into(),
            },
        ] {
            assert!(decode_tasks(dto).is_err());
        }
    }
    #[test]
    fn tracking_accepts_a_task_absent_from_the_catalog_and_rejects_completed_entries() {
        let active = WorklogDto {
            id: WorklogId::generate().to_string(),
            task_id: TaskId::generate().to_string(),
            start: at(),
            end: None,
        };
        assert!(
            decode_tracking(TrackingDto {
                active_worklog: Some(active.clone()),
                revision: "r1".into()
            })
            .is_ok()
        );
        assert!(
            decode_tracking(TrackingDto {
                active_worklog: Some(WorklogDto {
                    end: Some(at()),
                    ..active
                }),
                revision: "r1".into()
            })
            .is_err()
        );
        assert!(
            decode_tracking(TrackingDto {
                active_worklog: None,
                revision: String::new()
            })
            .is_err()
        );
    }
    #[test]
    fn reports_validate_ids_durations_totals_and_revision_without_task_cache_membership() {
        let row = tracker_protocol::ReportRowDto {
            task_id: TaskId::generate().to_string(),
            duration_us: 2,
        };
        let valid = ReportDto {
            start: at(),
            end: at() + TimeDelta::seconds(10),
            now: at(),
            rows: vec![row.clone()],
            total_us: 2,
            revision: "r1".into(),
        };
        assert!(validate_report(&valid).is_ok());
        for invalid in [
            ReportDto {
                rows: vec![row.clone(), row],
                total_us: 4,
                ..valid.clone()
            },
            ReportDto {
                total_us: 3,
                ..valid.clone()
            },
            ReportDto {
                revision: String::new(),
                ..valid.clone()
            },
            ReportDto {
                rows: vec![tracker_protocol::ReportRowDto {
                    task_id: "invalid".into(),
                    duration_us: 2,
                }],
                ..valid.clone()
            },
            ReportDto {
                rows: vec![tracker_protocol::ReportRowDto {
                    task_id: TaskId::generate().to_string(),
                    duration_us: 0,
                }],
                total_us: 0,
                ..valid.clone()
            },
            ReportDto {
                rows: vec![
                    tracker_protocol::ReportRowDto {
                        task_id: TaskId::generate().to_string(),
                        duration_us: i64::MAX,
                    },
                    tracker_protocol::ReportRowDto {
                        task_id: TaskId::generate().to_string(),
                        duration_us: 1,
                    },
                ],
                ..valid.clone()
            },
        ] {
            assert!(validate_report(&invalid).is_err());
        }
    }
    #[test]
    fn previews_require_complete_unique_ids_a_period_and_a_revision() {
        let valid = InactiveTaskPreviewDto {
            as_of: at(),
            inactive_days: 7,
            count: 1,
            candidate_task_ids: vec![TaskId::generate().to_string()],
            revision: "r1".into(),
        };
        assert!(validate_inactive_preview(&valid).is_ok());
        for invalid in [
            InactiveTaskPreviewDto {
                count: 2,
                ..valid.clone()
            },
            InactiveTaskPreviewDto {
                count: 2,
                candidate_task_ids: vec![valid.candidate_task_ids[0].clone(); 2],
                ..valid.clone()
            },
            InactiveTaskPreviewDto {
                candidate_task_ids: vec!["invalid".into()],
                ..valid.clone()
            },
            InactiveTaskPreviewDto {
                inactive_days: 0,
                ..valid.clone()
            },
            InactiveTaskPreviewDto {
                revision: String::new(),
                ..valid.clone()
            },
            InactiveTaskPreviewDto {
                as_of: at() + TimeDelta::nanoseconds(1),
                ..valid.clone()
            },
        ] {
            assert!(validate_inactive_preview(&invalid).is_err());
        }
    }
    #[test]
    fn move_candidates_use_only_the_confirmed_task_cache() {
        let source = TaskId::generate();
        let destination = TaskId::generate();
        let archived = TaskId::generate();
        let mut client = RemoteApplication::disconnected("http://127.0.0.1:1").unwrap();
        client.task_observation = Some(ResourceObservation {
            revision: "confirmed".into(),
            value: vec![
                task(source),
                task(destination),
                TaskDto {
                    archived: true,
                    ..task(archived)
                },
            ]
            .into_iter()
            .map(|dto| decode_task_item(dto).unwrap())
            .collect(),
        });
        let candidates = client.move_candidates(source, "pW");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].id, destination);
        assert!(client.move_candidates(source, "zz").is_empty());
    }
}

#[cfg(test)]
mod id_validation_tests {
    use super::*;
    #[test]
    fn report_and_preview_ids_are_unique_after_uuid_parsing() {
        let id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
        let at = DateTime::UNIX_EPOCH;
        let report = ReportDto {
            start: at,
            end: at + TimeDelta::seconds(10),
            now: at,
            rows: vec![
                tracker_protocol::ReportRowDto {
                    task_id: id.to_owned(),
                    duration_us: 1,
                },
                tracker_protocol::ReportRowDto {
                    task_id: id.to_ascii_uppercase(),
                    duration_us: 1,
                },
            ],
            total_us: 2,
            revision: "r1".into(),
        };
        assert!(validate_report(&report).is_err());
        let preview = InactiveTaskPreviewDto {
            as_of: at,
            inactive_days: 14,
            count: 2,
            candidate_task_ids: vec![id.to_owned(), id.to_ascii_uppercase()],
            revision: "r1".into(),
        };
        assert!(validate_inactive_preview(&preview).is_err());
    }
}
