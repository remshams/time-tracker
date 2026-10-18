use super::{
    ApplicationError, ApplicationFailureCategory, ArchiveInactiveTasksRequest,
    ClearActiveTaskOutcome, CreateTaskRequest, DateTime, DeleteWorklogRequest, GlobalWorklogCursor,
    GlobalWorklogPage, InactiveTaskPreview, InactivityPeriod, MoveCandidate, MutationDto, Recovery,
    RemoteError, RemoteFailureKind, RemoteSession, ReportDto, ReportsClient, ResourceObservation,
    ResourceSelection, SetActiveTaskOutcome, SetTrackingRequest, Task, TaskChangeRequest, TaskId,
    TaskListItem, TaskListStateDto, TaskName, TaskOrdering, TaskResourceDto, TasksClient,
    TrackingClient, TrackingState, Utc, Worklog, WorklogChangeRequest, WorklogCursor, WorklogId,
    WorklogPage, WorklogResourceDto, WorklogTimes, WorklogsClient, WriteGuard, active_id,
    canonical, creation_needs_recovery, decode_task_list_state, map_application_error,
    protocol_failure,
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
            worklogs: WorklogsClient,
            reports: ReportsClient::default(),
            pending_create: None,
            coherent_task_views: false,
        })
    }

    /// Reads tasks and tracking together for command baselines and recovery.
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

    fn guard() -> WriteGuard {
        WriteGuard {
            request_id: uuid::Uuid::now_v7().to_string(),
        }
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
            Recovery::Tracking => self.refresh_task_list().await,
        }
    }

    async fn finish_mutation<T>(
        &mut self,
        result: Result<T, ApplicationError>,
        recovery: Recovery,
    ) -> Result<T, ApplicationError> {
        if !self.session.last_write_attempted() {
            return result;
        }
        let recovered = self.recover(recovery).await;
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

    pub fn cached_task_revision(&self, id: TaskId) -> Option<&str> {
        self.tasks.cached_task_revision(id)
    }

    /// Captures candidates in the default inactivity window with their read revision.
    pub async fn preview_inactive_tasks(
        &mut self,
        as_of: DateTime<Utc>,
    ) -> Result<InactiveTaskPreview, ApplicationError> {
        self.tasks
            .preview_inactive_tasks(&mut self.session, as_of)
            .await
    }

    /// Captures candidate identities for the chosen cutoff and threshold.
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

    /// Returns the cached task name, or reads the task once when it is not cached.
    async fn current_task_name(&mut self, id: TaskId) -> Result<TaskName, ApplicationError> {
        if let Some(task) = self.task(id) {
            return Ok(task.name().clone());
        }
        let observation = self.read_task_observation(id).await?;
        Ok(observation.value.task.name().clone())
    }

    pub async fn create_task(
        &mut self,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        self.session.begin_command();
        let pending_create = self.pending_create.clone();
        if let Some(pending) = &pending_create {
            self.refresh_task_command_view()
                .await
                .map_err(|error| map_application_error(&error, None, None))?;
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
        let body = self
            .pending_create
            .clone()
            .unwrap_or_else(|| CreateTaskRequest {
                task_id: TaskId::generate().to_string(),
                name: name.as_str().to_owned(),
                occurred_at: canonical(occurred_at),
                guard: Self::guard(),
            });
        let id = body
            .task_id
            .parse()
            .map_err(|_| protocol_failure("invalid pending task id"))?;
        self.session
            .ensure_version()
            .await
            .map_err(|error| map_application_error(&error, None, Some(id)))?;
        self.pending_create = Some(body.clone());
        let result = self.tasks.create_task(&mut self.session, id, &body).await;
        let receipt_confirmed = result.is_ok();
        let result = self.finish_mutation(result, Recovery::Tasks).await;
        if !self.session.last_write_attempted() {
            self.pending_create = pending_create;
        } else if !result
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
        let expected_name = self.current_task_name(id).await?;
        self.rename_task_with_expected_name(id, &expected_name, name, occurred_at)
            .await
    }

    pub async fn archive_task(
        &mut self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        self.session.begin_command();
        let expected_name = self.current_task_name(id).await?;
        self.archive_task_with_expected_name(id, &expected_name, occurred_at)
            .await
    }

    pub async fn unarchive_task(
        &mut self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        self.session.begin_command();
        self.task_mutation(
            id,
            TaskChangeRequest::Restore {
                occurred_at: canonical(occurred_at),
                guard: Self::guard(),
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
        self.finish_mutation(result, Recovery::Tasks).await
    }

    /// Archives the tasks that match the preview's period when the command runs.
    pub async fn archive_inactive_tasks(
        &mut self,
        preview: &InactiveTaskPreview,
    ) -> Result<usize, ApplicationError> {
        self.session.begin_command();
        let preview = &preview.dto;
        let body = ArchiveInactiveTasksRequest {
            as_of: preview.as_of,
            inactive_days: preview.inactive_days,
            guard: Self::guard(),
        };
        let result = self
            .tasks
            .archive_inactive_tasks(&mut self.session, &body)
            .await;
        self.finish_mutation(result, Recovery::Tasks).await
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
        let body = TaskChangeRequest::Rename {
            expected_name: expected_name.as_str().to_owned(),
            name: name.as_str().to_owned(),
            occurred_at: canonical(occurred_at),
            guard: Self::guard(),
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
        self.task_mutation(
            id,
            TaskChangeRequest::Archive {
                expected_name: expected_name.as_str().to_owned(),
                occurred_at: canonical(occurred_at),
                guard: Self::guard(),
            },
        )
        .await
    }

    /// Reads tracking once so a command without a reviewed timer names the one it replaces.
    async fn establish_tracking_baseline(
        &mut self,
        task_id: TaskId,
    ) -> Result<(), ApplicationError> {
        let refreshed = if self.coherent_task_views {
            self.refresh_task_list().await
        } else {
            self.refresh_tracking().await
        };
        refreshed.map_err(|error| map_application_error(&error, None, Some(task_id)))
    }

    pub async fn set_active_task(
        &mut self,
        task_id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, ApplicationError> {
        self.session.begin_command();
        if self.tracking_observation().is_none() {
            self.establish_tracking_baseline(task_id).await?;
        }
        self.set_active_task_with_expected_active(
            task_id,
            active_id(self.current_tracking()),
            occurred_at,
        )
        .await
    }

    /// Sends the active worklog the caller reviewed; the server rejects the switch if it changed.
    pub async fn set_active_task_with_expected_active(
        &mut self,
        task_id: TaskId,
        expected_active: Option<WorklogId>,
        occurred_at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, ApplicationError> {
        self.session.begin_command();
        let new_id = WorklogId::generate();
        let body = SetTrackingRequest {
            task_id: Some(task_id.to_string()),
            worklog_id: Some(new_id.to_string()),
            expected_active: expected_active.map(|id| id.to_string()),
            occurred_at: canonical(occurred_at),
            guard: Self::guard(),
        };
        let result = self
            .tracking
            .set_active_task(&mut self.session, task_id, new_id, expected_active, &body)
            .await;
        self.finish_mutation(result, Recovery::Tracking).await
    }

    pub async fn clear_active_task(
        &mut self,
        expected_active: WorklogId,
        occurred_at: DateTime<Utc>,
    ) -> Result<ClearActiveTaskOutcome, ApplicationError> {
        self.session.begin_command();
        let body = SetTrackingRequest {
            task_id: None,
            worklog_id: None,
            expected_active: Some(expected_active.to_string()),
            occurred_at: canonical(occurred_at),
            guard: Self::guard(),
        };
        let result = self
            .tracking
            .clear_active_task(&mut self.session, expected_active, &body)
            .await;
        self.finish_mutation(result, Recovery::Tracking).await
    }

    pub async fn move_worklog(
        &mut self,
        id: WorklogId,
        expected_source_task_id: TaskId,
        expected: WorklogTimes,
        destination_task_id: TaskId,
    ) -> Result<Worklog, ApplicationError> {
        self.session.begin_command();
        let body = WorklogChangeRequest::Move {
            expected_task_id: expected_source_task_id.to_string(),
            expected_start: canonical(expected.start()),
            expected_end: expected.end().map(canonical),
            destination_task_id: destination_task_id.to_string(),
            guard: Self::guard(),
        };
        let result = self
            .worklogs
            .move_worklog(&mut self.session, id, expected, destination_task_id, &body)
            .await;
        self.finish_mutation(result, Recovery::Tracking).await
    }

    pub async fn correct_worklog(
        &mut self,
        id: WorklogId,
        expected: WorklogTimes,
        replacement: WorklogTimes,
        occurred_at: DateTime<Utc>,
    ) -> Result<Worklog, ApplicationError> {
        self.session.begin_command();
        let body = WorklogChangeRequest::Correct {
            expected_start: canonical(expected.start()),
            expected_end: expected.end().map(canonical),
            replacement_start: canonical(replacement.start()),
            replacement_end: replacement.end().map(canonical),
            occurred_at: canonical(occurred_at),
            guard: Self::guard(),
        };
        let result = self
            .worklogs
            .correct_worklog(&mut self.session, id, replacement, &body)
            .await;
        self.finish_mutation(result, Recovery::Tracking).await
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
        let body = DeleteWorklogRequest {
            expected_task_id: expected_task_id.to_string(),
            expected_start: canonical(expected.start()),
            expected_end: canonical(end),
            guard: Self::guard(),
        };
        let result = self
            .worklogs
            .delete_completed_worklog(&mut self.session, id, expected_task_id, expected, &body)
            .await;
        self.finish_mutation(result, Recovery::Tracking).await
    }
}
