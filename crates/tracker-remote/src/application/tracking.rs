use super::{
    ApplicationError, ClearActiveTaskOutcome, Method, MutationContext, RemoteError, RemoteSession,
    ResourceObservation, SetActiveTaskOutcome, SetTrackingRequest, TaskId, TrackingDto,
    TrackingState, Worklog, WorklogId, clear_tracking_result, decode_tracking, protocol_failure,
    set_tracking_result,
};

/// Tracking protocol operations and the confirmed active worklog.
#[derive(Default)]
pub(super) struct TrackingClient {
    tracking_observation: Option<ResourceObservation<TrackingState>>,
}

impl TrackingClient {
    pub(super) async fn read_tracking(
        &self,
        session: &mut RemoteSession,
    ) -> Result<(Option<Worklog>, TrackingState, String), RemoteError> {
        let dto: TrackingDto = session.read("v1/tracking").await?;
        decode_tracking(dto)
    }

    pub(super) async fn refresh_tracking(
        &mut self,
        session: &mut RemoteSession,
    ) -> Result<(), RemoteError> {
        session.ensure_version().await?;
        let (_, tracking, revision) = self
            .read_tracking(session)
            .await
            .map_err(|error| session.record_error(error))?;
        self.adopt(tracking, revision);
        session.confirmed();
        Ok(())
    }

    pub(super) fn active_worklog(&self) -> Option<Worklog> {
        match self.current_tracking() {
            TrackingState::Idle => None,
            TrackingState::Running { worklog } => Some(worklog.to_worklog()),
        }
    }

    pub(super) fn current_tracking(&self) -> &TrackingState {
        self.tracking_observation
            .as_ref()
            .map_or(&TrackingState::Idle, |observation| &observation.value)
    }

    pub(super) fn tracking_observation(&self) -> Option<&ResourceObservation<TrackingState>> {
        self.tracking_observation.as_ref()
    }

    pub(super) fn tracking_revision(&self) -> &str {
        self.tracking_observation
            .as_ref()
            .map_or("", |observation| observation.revision.as_str())
    }

    pub(super) fn adopt(&mut self, state: TrackingState, revision: String) {
        self.tracking_observation = Some(ResourceObservation {
            value: state,
            revision,
        });
    }

    pub(super) async fn set_active_task(
        &mut self,
        session: &mut RemoteSession,
        task_id: TaskId,
        new_id: WorklogId,
        expected_active: Option<WorklogId>,
        body: &SetTrackingRequest,
    ) -> Result<SetActiveTaskOutcome, ApplicationError> {
        session
            .mutation(
                Method::PUT,
                "v1/tracking",
                body,
                &body.guard,
                MutationContext {
                    worklog_id: None,
                    task_id: Some(task_id),
                },
                |result| {
                    let outcome = set_tracking_result(result, task_id, new_id, expected_active)?;
                    let valid = match &outcome {
                        SetActiveTaskOutcome::Started { worklog } => {
                            worklog.start() == body.occurred_at
                        }
                        SetActiveTaskOutcome::Switched { stopped, started } => {
                            stopped.end() == Some(body.occurred_at)
                                && started.start() == body.occurred_at
                        }
                        SetActiveTaskOutcome::AlreadyActive { worklog } => {
                            expected_active == Some(worklog.id())
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

    pub(super) async fn clear_active_task(
        &mut self,
        session: &mut RemoteSession,
        expected_active: WorklogId,
        body: &SetTrackingRequest,
    ) -> Result<ClearActiveTaskOutcome, ApplicationError> {
        session
            .mutation(
                Method::PUT,
                "v1/tracking",
                body,
                &body.guard,
                MutationContext {
                    worklog_id: Some(expected_active),
                    task_id: None,
                },
                |result| {
                    let outcome = clear_tracking_result(result, expected_active)?;
                    let valid = match &outcome {
                        ClearActiveTaskOutcome::Stopped { worklog } => {
                            worklog.end() == Some(body.occurred_at)
                        }
                        // Another client may have stopped the timer first.
                        ClearActiveTaskOutcome::AlreadyIdle => true,
                    };
                    if !valid {
                        return Err(protocol_failure("stopped worklog contradicts command"));
                    }
                    Ok(outcome)
                },
            )
            .await
    }
}
