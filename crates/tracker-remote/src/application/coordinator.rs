use super::{
    ApplicationError, ApplicationFailureCategory, ArchiveInactiveTasksRequest,
    ClearActiveTaskOutcome, CreateTaskRequest, DateTime, DeleteWorklogRequest, GlobalWorklogCursor,
    GlobalWorklogPage, InactiveTaskPreview, InactivityPeriod, MoveCandidate, MutationDto,
    MutationScope, Recovery, RemoteError, RemoteFailureKind, RemoteSession, ReportDto,
    ReportsClient, ResourceObservation, ResourceSelection, SetActiveTaskOutcome,
    SetTrackingRequest, StatusCode, Task, TaskChangeRequest, TaskId, TaskListItem,
    TaskListStateDto, TaskName, TaskOrdering, TaskResourceDto, TasksClient, TrackingClient,
    TrackingState, Utc, Worklog, WorklogChangeRequest, WorklogCursor, WorklogId, WorklogPage,
    WorklogPageDto, WorklogResourceDto, WorklogTimes, WorklogsClient, WriteGuard, active_id,
    app_decode_worklog, canonical, creation_needs_recovery, decode_task_list_state,
    map_application_error, original_worklog_matches, protocol_failure, reviewed_tracking_changed,
};

/// Coordinates remote tracker workflows across resource clients.
pub struct RemoteTrackerCoordinator {
    session: RemoteSession,
    tasks: TasksClient,
    tracking: TrackingClient,
    worklogs: WorklogsClient,
    reports: ReportsClient,
    pending_create: Option<CreateTaskRequest>,
    coherent_task_views: bool,
}

impl RemoteTrackerCoordinator {
    pub fn disconnected(endpoint: &str) -> Result<Self, RemoteError> {
        Ok(Self {
            session: RemoteSession::new(endpoint)?,
            tasks: TasksClient::default(),
            tracking: TrackingClient::default(),
            worklogs: WorklogsClient::default(),
            reports: ReportsClient::default(),
            pending_create: None,
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
        self.session.check_connection().await
    }

    /// Adopts the task catalog and tracking from one coherent server read.
    pub async fn refresh_task_list(&mut self) -> Result<(), RemoteError> {
        self.session.ensure_version().await?;
        let dto: TaskListStateDto = self.session.read("v1/state/task-list").await?;
        let (items, state, revision) =
            decode_task_list_state(dto).map_err(|error| self.session.record_error(error))?;
        self.adopt_task_list(items, state, revision);
        self.session.confirmed();
        Ok(())
    }

    fn adopt_task_list(
        &mut self,
        items: Vec<TaskListItem>,
        state: TrackingState,
        revision: String,
    ) {
        self.tasks.adopt_catalog(items, revision.clone());
        self.tracking.adopt(state, revision);
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

    /// Validates a daily overview before publishing its selected resources together.
    async fn refresh_totals_resources(
        &mut self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
        include_tasks: bool,
    ) -> Result<(), RemoteError> {
        let dto = self
            .reports
            .fetch_daily_overview(&mut self.session, start, end, now)
            .await?;
        let (items, state, revision) = decode_task_list_state(TaskListStateDto {
            tasks: dto.tasks,
            active_worklog: dto.active_worklog,
            revision: dto.totals.revision.clone(),
        })
        .map_err(|error| self.session.record_error(error))?;
        if include_tasks {
            self.adopt_task_list(items, state, revision);
        } else {
            self.tracking.adopt(state, revision);
        }
        self.reports.adopt(dto.totals);
        self.session.confirmed();
        Ok(())
    }

    pub fn last_write_attempted(&self) -> bool {
        self.session.last_write_attempted()
    }

    pub fn last_failure(&self) -> Option<RemoteFailureKind> {
        self.session.last_failure()
    }

    pub fn last_command_receipt(&self) -> Option<&MutationDto> {
        self.session.last_command_receipt()
    }

    fn guard(revision: &str) -> WriteGuard {
        WriteGuard {
            expected_revision: revision.to_owned(),
            request_id: uuid::Uuid::now_v7().to_string(),
        }
    }

    fn intent_changed(&mut self) -> ApplicationError {
        self.session.intent_changed()
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
                    self.worklogs.remove(id);
                }
                Err(error) => return Err(error),
            }
        }
        self.recover(scope.recovery).await
    }

    async fn finish_mutation<T>(
        &mut self,
        result: Result<T, ApplicationError>,
        scope: MutationScope,
    ) -> Result<T, ApplicationError> {
        if !self.session.last_write_attempted() {
            return result;
        }
        let recovered = self.recover_mutation(scope).await;
        match (result, recovered) {
            (Ok(value), Ok(())) => {
                self.session.confirmed();
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
    pub async fn refresh_tasks(&mut self) -> Result<(), RemoteError> {
        self.tasks.refresh_tasks(&mut self.session).await
    }

    pub fn tasks(&self, ordering: TaskOrdering) -> Vec<TaskListItem> {
        self.tasks.tasks(ordering)
    }

    pub fn move_candidates(&self, source_task_id: TaskId, query: &str) -> Vec<MoveCandidate> {
        self.tasks.move_candidates(source_task_id, query)
    }

    pub fn task_item(&self, id: TaskId) -> Option<&TaskListItem> {
        self.tasks.task_item(id)
    }

    pub fn task(&self, id: TaskId) -> Option<&Task> {
        self.tasks.task(id)
    }

    async fn read_task_item(
        &mut self,
        id: TaskId,
    ) -> Result<(TaskListItem, String), ApplicationError> {
        self.tasks.read_task_item(&mut self.session, id).await
    }

    /// Reads one task with its own revision and preserves application error categories.
    pub async fn read_task_observation(
        &mut self,
        id: TaskId,
    ) -> Result<ResourceObservation<TaskListItem>, ApplicationError> {
        self.tasks
            .read_task_observation(&mut self.session, id)
            .await
    }

    pub async fn read_task(&mut self, id: TaskId) -> Result<TaskResourceDto, RemoteError> {
        self.tasks.read_task(&mut self.session, id).await
    }

    /// Loads domain tasks only while every candidate matches the preview revision and remains unarchived.
    pub async fn resolve_preview_tasks(
        &mut self,
        preview: &InactiveTaskPreview,
    ) -> Result<Vec<Task>, ApplicationError> {
        self.tasks
            .resolve_preview_tasks(&mut self.session, preview)
            .await
    }

    fn adopt_task(&mut self, item: TaskListItem, revision: String) {
        self.tasks.adopt_task(item, revision)
    }

    pub fn cached_task_revision(&self, id: TaskId) -> Option<&str> {
        self.tasks.cached_task_revision(id)
    }

    /// Captures candidates in the default inactivity window with their confirmation guard.
    pub async fn preview_inactive_tasks(
        &mut self,
        as_of: DateTime<Utc>,
    ) -> Result<InactiveTaskPreview, ApplicationError> {
        self.tasks
            .preview_inactive_tasks(&mut self.session, as_of)
            .await
    }

    /// Captures candidate identities for the chosen cutoff and threshold with the server revision.
    pub async fn preview_inactive_tasks_with_period(
        &mut self,
        as_of: DateTime<Utc>,
        period: InactivityPeriod,
    ) -> Result<InactiveTaskPreview, ApplicationError> {
        self.tasks
            .preview_inactive_tasks_with_period(&mut self.session, as_of, period)
            .await
    }

    pub fn task_observation(&self) -> Option<&ResourceObservation<Vec<TaskListItem>>> {
        self.tasks.task_observation()
    }

    pub fn task_items(&self) -> &[TaskListItem] {
        self.tasks.task_items()
    }

    pub fn task_revision(&self) -> &str {
        self.tasks.task_revision()
    }

    pub async fn refresh_tracking(&mut self) -> Result<(), RemoteError> {
        self.tracking.refresh_tracking(&mut self.session).await
    }

    pub fn active_worklog(&self) -> Option<Worklog> {
        self.tracking.active_worklog()
    }

    pub fn current_tracking(&self) -> &TrackingState {
        self.tracking.current_tracking()
    }

    pub fn tracking_observation(&self) -> Option<&ResourceObservation<TrackingState>> {
        self.tracking.tracking_observation()
    }

    pub fn tracking_revision(&self) -> &str {
        self.tracking.tracking_revision()
    }

    pub async fn read_worklog(&mut self, id: WorklogId) -> Result<WorklogResourceDto, RemoteError> {
        self.worklogs.read_worklog(&mut self.session, id).await
    }

    pub async fn worklogs_for_task(
        &mut self,
        task_id: TaskId,
        after: Option<&WorklogCursor>,
    ) -> Result<WorklogPage, ApplicationError> {
        self.worklogs
            .worklogs_for_task(&mut self.session, task_id, after)
            .await
    }

    pub async fn all_worklogs(
        &mut self,
        after: Option<&GlobalWorklogCursor>,
    ) -> Result<GlobalWorklogPage, ApplicationError> {
        self.worklogs.all_worklogs(&mut self.session, after).await
    }

    pub fn cached_history(&self, task_id: Option<TaskId>) -> Option<&WorklogPageDto> {
        self.worklogs.cached_history(task_id)
    }

    pub fn cached_worklog(&self, id: WorklogId) -> Option<&WorklogResourceDto> {
        self.worklogs.cached_worklog(id)
    }

    /// Publishes validated totals only after the echoed microsecond context matches the request.
    pub async fn task_totals(
        &mut self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<ReportDto, ApplicationError> {
        self.reports
            .task_totals(&mut self.session, start, end, now)
            .await
    }

    pub fn report_revision(&self) -> &str {
        self.reports.report_revision()
    }

    pub fn report_observation(&self) -> Option<&ReportDto> {
        self.reports.report_observation()
    }

    pub fn cached_task_totals(&self) -> Option<&ReportDto> {
        self.reports.cached_task_totals()
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

    pub async fn create_task(
        &mut self,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        self.session.begin_command();
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
        let result = self.tasks.create_task(&mut self.session, id, &body).await;
        let receipt_confirmed = result.is_ok();
        let result = self
            .finish_mutation(
                result,
                MutationScope {
                    recovery: Recovery::Tasks,
                    worklog_id: None,
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
        self.session.begin_command();
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

    pub async fn archive_task(
        &mut self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        self.session.begin_command();
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

    pub async fn unarchive_task(
        &mut self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        self.session.begin_command();
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
        let result = self.tasks.task_mutation(&mut self.session, id, &body).await;
        self.finish_mutation(
            result,
            MutationScope {
                recovery: Recovery::Tasks,
                worklog_id: None,
            },
        )
        .await
    }

    /// Confirms the captured preview under its original revision guard.
    pub async fn archive_inactive_tasks(
        &mut self,
        preview: &InactiveTaskPreview,
    ) -> Result<usize, ApplicationError> {
        self.session.begin_command();
        let preview = &preview.dto;
        let body = ArchiveInactiveTasksRequest {
            as_of: preview.as_of,
            inactive_days: preview.inactive_days,
            guard: Self::guard(&preview.revision),
        };
        let result = self
            .tasks
            .archive_inactive_tasks(&mut self.session, &body)
            .await;
        self.finish_mutation(
            result,
            MutationScope {
                recovery: Recovery::Tasks,
                worklog_id: None,
            },
        )
        .await
    }

    /// Confirms the period already retained by the preview and returns the archived count.
    pub async fn archive_inactive_tasks_with_period(
        &mut self,
        preview: &InactiveTaskPreview,
    ) -> Result<usize, ApplicationError> {
        self.session.begin_command();
        self.archive_inactive_tasks(preview).await
    }

    /// Keeps the name the user reviewed even when background reads update the cache.
    pub async fn rename_task_with_expected_name(
        &mut self,
        id: TaskId,
        expected_name: &TaskName,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        self.session.begin_command();
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

    /// Keeps the name the user reviewed even when background reads update the cache.
    pub async fn archive_task_with_expected_name(
        &mut self,
        id: TaskId,
        expected_name: &TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        self.session.begin_command();
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

    async fn initialize_tracking_command_view(
        &mut self,
        target: Option<TaskId>,
    ) -> Result<(), ApplicationError> {
        if self.coherent_task_views
            && (self.tasks.task_observation().is_none()
                || self.tracking.tracking_observation().is_none())
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
            self.tracking.adopt(state, revision.to_owned());
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
            self.session
                .ensure_version()
                .await
                .map_err(|error| map_application_error(&error, None, target))?;
            let (active, state, revision) = self
                .tracking
                .read_tracking(&mut self.session)
                .await
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
        self.session.begin_command();
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
        self.session.begin_command();
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
        let result = self
            .tracking
            .set_active_task_guarded(
                &mut self.session,
                task_id,
                new_id,
                old_active,
                original_active,
                &body,
            )
            .await;
        self.finish_mutation(
            result,
            MutationScope {
                recovery: Recovery::Tracking,
                worklog_id: None,
            },
        )
        .await
    }

    pub async fn clear_active_task(
        &mut self,
        expected_active: WorklogId,
        occurred_at: DateTime<Utc>,
    ) -> Result<ClearActiveTaskOutcome, ApplicationError> {
        self.session.begin_command();
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
        let result = self
            .tracking
            .clear_active_task(&mut self.session, expected_active, original_active, &body)
            .await;
        self.finish_mutation(
            result,
            MutationScope {
                recovery: Recovery::Tracking,
                worklog_id: Some(expected_active),
            },
        )
        .await
    }

    async fn worklog_guard(
        &mut self,
        id: WorklogId,
        expected_task: Option<TaskId>,
        expected: WorklogTimes,
        destination: Option<TaskId>,
    ) -> Result<(String, TaskId), ApplicationError> {
        self.session
            .ensure_version()
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
                self.session.set_failure(RemoteFailureKind::Conflict);
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
            return Ok((dto.revision, worklog.task_id()));
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
        self.session.begin_command();
        let (revision, _) = self
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
        let result = self
            .worklogs
            .move_worklog(&mut self.session, id, expected, destination_task_id, &body)
            .await;
        self.finish_mutation(
            result,
            MutationScope {
                recovery: Recovery::Worklog,
                worklog_id: Some(id),
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
        self.session.begin_command();
        let (revision, expected_task_id) = self.worklog_guard(id, None, expected, None).await?;
        let body = WorklogChangeRequest::Correct {
            expected_start: canonical(expected.start()),
            expected_end: expected.end().map(canonical),
            replacement_start: canonical(replacement.start()),
            replacement_end: replacement.end().map(canonical),
            occurred_at: canonical(occurred_at),
            guard: Self::guard(&revision),
        };
        let result = self
            .worklogs
            .correct_worklog(&mut self.session, id, expected_task_id, replacement, &body)
            .await;
        self.finish_mutation(
            result,
            MutationScope {
                recovery: Recovery::Worklog,
                worklog_id: Some(id),
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
        self.session.begin_command();
        let end = expected
            .end()
            .ok_or_else(|| ApplicationError::active_worklog(id))?;
        let (revision, _) = self
            .worklog_guard(id, Some(expected_task_id), expected, None)
            .await?;
        let body = DeleteWorklogRequest {
            expected_task_id: expected_task_id.to_string(),
            expected_start: canonical(expected.start()),
            expected_end: canonical(end),
            guard: Self::guard(&revision),
        };
        let result = self
            .worklogs
            .delete_completed_worklog(&mut self.session, id, expected_task_id, expected, &body)
            .await;
        self.finish_mutation(
            result,
            MutationScope {
                recovery: Recovery::Worklog,
                worklog_id: Some(id),
            },
        )
        .await
    }
}
