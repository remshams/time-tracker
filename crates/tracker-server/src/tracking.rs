//! Tracking resource reads and guarded commands.

use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use tracker_application::{ClearActiveTaskOutcome, SetActiveTaskOutcome};
use tracker_domain::TrackingState;
use tracker_protocol::{
    MutationDto, MutationResultDto, SetTrackingRequest, TrackingDto, WorklogDto,
};

use crate::core::{Shared, lock};
use crate::error::{ApiError, ApiResult, fingerprint, payload, task_id, worklog_id};

pub(super) async fn tracking(State(shared): State<Shared>) -> ApiResult<TrackingDto> {
    let mut core = lock(&shared)?;
    core.app.refresh_tracking_resource()?;
    Ok(Json(TrackingDto {
        active_worklog: core.active_worklog(),
        revision: core.revision(),
    }))
}
pub(super) async fn set_tracking(
    State(shared): State<Shared>,
    input: Result<Json<SetTrackingRequest>, JsonRejection>,
) -> ApiResult<MutationDto> {
    let request = payload(input)?;
    let desired = request.task_id.as_deref().map(task_id).transpose()?;
    let new_id = request.worklog_id.as_deref().map(worklog_id).transpose()?;
    let expected = request
        .expected_active
        .as_deref()
        .map(worklog_id)
        .transpose()?;
    if desired.is_some() != new_id.is_some() {
        return Err(ApiError::invalid(
            "A worklog ID is required when setting an active task",
        ));
    }
    let key = fingerprint("PUT /v1/tracking", &request)?;
    let mut core = lock(&shared)?;
    let response = core.execute(&request.guard, key, |app| {
        let current = match app.current_tracking() {
            TrackingState::Idle => None,
            TrackingState::Running { worklog } => Some(worklog.id()),
        };
        if current != expected {
            return Err(ApiError::conflict(
                "Active worklog changed. Refresh and retry.",
            ));
        }
        match (desired, new_id, current) {
            (Some(task_id), Some(new_id), _) => {
                let outcome = match current {
                    None => {
                        app.start_tracking_if_idle_with_id(task_id, new_id, request.occurred_at)
                    }
                    Some(_) => app.set_active_task_with_id(task_id, new_id, request.occurred_at),
                }?;
                match outcome {
                    SetActiveTaskOutcome::Started { worklog } => {
                        Ok(MutationResultDto::Worklog(WorklogDto::from(&worklog)))
                    }
                    SetActiveTaskOutcome::Switched { stopped, started } => {
                        Ok(MutationResultDto::TrackingSwitched {
                            stopped: WorklogDto::from(&stopped),
                            started: WorklogDto::from(&started),
                        })
                    }
                    SetActiveTaskOutcome::AlreadyActive { worklog } => {
                        Ok(MutationResultDto::TrackingAlreadyActive(WorklogDto::from(
                            &worklog.to_worklog(),
                        )))
                    }
                }
            }
            (None, None, Some(active)) => match app
                .clear_active_task(active, request.occurred_at)?
            {
                ClearActiveTaskOutcome::Stopped { worklog } => {
                    Ok(MutationResultDto::Worklog(WorklogDto::from(&worklog)))
                }
                ClearActiveTaskOutcome::AlreadyIdle => Ok(MutationResultDto::TrackingAlreadyIdle),
            },
            (None, None, None) => Ok(MutationResultDto::TrackingAlreadyIdle),
            _ => Err(ApiError::invalid("Invalid tracking request")),
        }
    })?;
    Ok(Json(response))
}

#[cfg(test)]
mod tracking_tests {
    use super::*;
    use crate::bootstrap::lock_database;
    use crate::core::Core;
    use axum::http::StatusCode;
    use chrono::{DateTime, Utc};
    use std::collections::{HashMap, VecDeque};
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use tracker_application::{TaskOrdering, TrackerApplication};
    use tracker_domain::{TaskId, TaskName, WorklogId};
    use tracker_protocol::WriteGuard;
    use tracker_storage::SqliteRepository;
    use uuid::Uuid;

    fn core(path: &Path) -> Shared {
        let database_lock = lock_database(path).unwrap();
        let repository = SqliteRepository::open(path).unwrap();
        let mut app = TrackerApplication::load(repository).unwrap();
        app.create_task(TaskName::new("Write release notes").unwrap(), at(1))
            .unwrap();
        app.create_task(TaskName::new("Review changes").unwrap(), at(2))
            .unwrap();
        Arc::new(Mutex::new(Core {
            _database_lock: database_lock,
            app,
            needs_refresh: false,
            epoch: "tracking-test".to_owned(),
            sequence: 0,
            completed: HashMap::new(),
            completion_order: VecDeque::new(),
        }))
    }

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).unwrap()
    }

    fn request(
        shared: &Shared,
        task_id: TaskId,
        worklog_id: WorklogId,
        expected: Option<WorklogId>,
        seconds: i64,
    ) -> SetTrackingRequest {
        SetTrackingRequest {
            task_id: Some(task_id.to_string()),
            worklog_id: Some(worklog_id.to_string()),
            expected_active: expected.map(|id| id.to_string()),
            occurred_at: at(seconds),
            guard: WriteGuard {
                expected_revision: shared.lock().unwrap().revision(),
                request_id: Uuid::now_v7().to_string(),
            },
        }
    }

    #[tokio::test]
    async fn idle_request_preserves_a_timer_started_directly_in_the_database() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tracker.db");
        let shared = core(&path);
        let tasks = shared.lock().unwrap().app.tasks(TaskOrdering::default());
        let mut other = TrackerApplication::load(SqliteRepository::open(&path).unwrap()).unwrap();
        other.set_active_task(tasks[1].task.id(), at(100)).unwrap();
        let foreign = other.current_tracking().clone();
        let new_id = WorklogId::generate();
        let request = request(&shared, tasks[0].task.id(), new_id, None, 200);
        let rejected = set_tracking(State(shared.clone()), Ok(Json(request)))
            .await
            .unwrap_err();
        assert_eq!(rejected.status, StatusCode::CONFLICT);
        assert_eq!(shared.lock().unwrap().app.current_tracking(), &foreign);
        let repository = SqliteRepository::open(&path).unwrap();
        assert!(repository.find_worklog(new_id).unwrap().is_none());
        assert_eq!(
            repository.active_worklog().unwrap().unwrap().task_id(),
            tasks[1].task.id()
        );
    }

    #[tokio::test]
    async fn conditional_idle_start_keeps_the_id_and_explicit_switch_remains_supported() {
        let directory = tempfile::tempdir().unwrap();
        let shared = core(&directory.path().join("tracker.db"));
        let tasks = shared.lock().unwrap().app.tasks(TaskOrdering::default());
        let first_id = WorklogId::generate();
        let first_request = request(&shared, tasks[0].task.id(), first_id, None, 100);
        let first = set_tracking(State(shared.clone()), Ok(Json(first_request.clone())))
            .await
            .unwrap()
            .0;
        assert_eq!(
            shared.lock().unwrap().active_worklog().unwrap().id,
            first_id.to_string()
        );
        assert_eq!(
            shared.lock().unwrap().active_worklog().unwrap().start,
            at(100)
        );
        let repeated = set_tracking(State(shared.clone()), Ok(Json(first_request)))
            .await
            .unwrap()
            .0;
        assert_eq!(repeated.result, first.result);
        let next_id = WorklogId::generate();
        let next_request = request(&shared, tasks[1].task.id(), next_id, Some(first_id), 200);
        let switched = set_tracking(State(shared.clone()), Ok(Json(next_request)))
            .await
            .unwrap()
            .0;
        assert_eq!(
            shared.lock().unwrap().active_worklog().unwrap().id,
            next_id.to_string()
        );
        assert!(
            matches!(switched.result, MutationResultDto::TrackingSwitched { stopped, started }
            if stopped.id == first_id.to_string() && stopped.end == Some(at(200))
                && started.start == at(200))
        );
    }
}
