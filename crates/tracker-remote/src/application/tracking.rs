use super::{
    ApplicationError, ClearActiveTaskOutcome, DateTime, Method, MutationScope, Recovery,
    RemoteApplication, RemoteError, ResourceObservation, SetActiveTaskOutcome, SetTrackingRequest,
    TaskId, TrackingDto, TrackingState, Utc, Worklog, WorklogId, active_id, canonical,
    clear_tracking_result, decode_tracking, map_application_error, protocol_failure,
    reviewed_tracking_changed, set_tracking_result,
};

impl RemoteApplication {
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

    pub fn active_worklog(&self) -> Option<Worklog> {
        match self.current_tracking() {
            TrackingState::Idle => None,
            TrackingState::Running { worklog } => Some(worklog.to_worklog()),
        }
    }

    pub fn current_tracking(&self) -> &TrackingState {
        self.tracking_observation
            .as_ref()
            .map_or(&TrackingState::Idle, |observation| &observation.value)
    }

    pub(super) async fn initialize_tracking_command_view(
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

    pub(super) fn adopt_tracking_preflight(
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

    pub(super) fn reviewed_tracking_state(&self) -> Option<TrackingState> {
        if self.tracking_revision().is_empty() {
            None
        } else {
            Some(self.current_tracking().clone())
        }
    }

    pub(super) async fn tracking_guard(
        &mut self,
        target: Option<TaskId>,
    ) -> Result<String, ApplicationError> {
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

    pub(super) async fn set_active_task_guarded(
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

    pub fn tracking_observation(&self) -> Option<&ResourceObservation<TrackingState>> {
        self.tracking_observation.as_ref()
    }

    pub fn tracking_revision(&self) -> &str {
        self.tracking_observation
            .as_ref()
            .map_or("", |observation| observation.revision.as_str())
    }
}
