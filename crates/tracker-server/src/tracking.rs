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
    }))
}
/// Guards authoritative tracking state and preserves request worklog identity and retry receipts.
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
        match (desired, new_id, expected) {
            (Some(task_id), Some(new_id), _) => {
                let outcome = app.set_active_task_with_id_and_expected_active(
                    task_id,
                    new_id,
                    expected,
                    request.occurred_at,
                )?;
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
            (None, None, None) => {
                app.refresh_tracking_resource()?;
                if app.current_tracking() != &TrackingState::Idle {
                    return Err(ApiError::conflict(
                        "Active worklog changed. Refresh and retry.",
                    ));
                }
                Ok(MutationResultDto::TrackingAlreadyIdle)
            }
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
        Arc::new(Mutex::new(Core::new(database_lock, app)))
    }

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).unwrap()
    }

    fn request(
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
        let request = request(tasks[0].task.id(), new_id, None, 200);
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
        let first_request = request(tasks[0].task.id(), first_id, None, 100);
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
        let next_request = request(tasks[1].task.id(), next_id, Some(first_id), 200);
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
    /// A cached timer cannot authorize a switch after another writer replaces that timer.
    #[tokio::test]
    async fn switch_preserves_a_timer_replaced_directly_in_the_database() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tracker.db");
        let shared = core(&path);
        let tasks = shared.lock().unwrap().app.tasks(TaskOrdering::default());
        let original_id = WorklogId::generate();
        let initial = request(tasks[0].task.id(), original_id, None, 100);
        let initial = set_tracking(State(shared.clone()), Ok(Json(initial)))
            .await
            .unwrap()
            .0;
        assert!(
            matches!(initial.result, MutationResultDto::Worklog(worklog) if worklog.id == original_id.to_string())
        );

        let mut other = TrackerApplication::load(SqliteRepository::open(&path).unwrap()).unwrap();
        other.clear_active_task(original_id, at(150)).unwrap();
        other.set_active_task(tasks[0].task.id(), at(160)).unwrap();
        let replacement = other.current_tracking().clone();
        let new_id = WorklogId::generate();
        let stale = request(tasks[1].task.id(), new_id, Some(original_id), 200);

        let rejected = set_tracking(State(shared.clone()), Ok(Json(stale)))
            .await
            .unwrap_err();

        assert_eq!(rejected.status, StatusCode::CONFLICT);
        assert_eq!(shared.lock().unwrap().app.current_tracking(), &replacement);
        let repository = SqliteRepository::open(&path).unwrap();
        assert!(repository.find_worklog(new_id).unwrap().is_none());
        let TrackingState::Running { worklog } = replacement else {
            panic!("the replacement timer must remain active");
        };
        assert_eq!(
            repository.active_worklog().unwrap().unwrap().id(),
            worklog.id()
        );
        assert_eq!(
            repository.find_worklog(original_id).unwrap().unwrap().end(),
            Some(at(150))
        );
    }

    /// Cached idle state cannot confirm idle after another writer starts tracking.
    #[tokio::test]
    async fn idle_confirmation_rechecks_a_timer_started_directly_in_the_database() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tracker.db");
        let shared = core(&path);
        let tasks = shared.lock().unwrap().app.tasks(TaskOrdering::default());
        let mut other = TrackerApplication::load(SqliteRepository::open(&path).unwrap()).unwrap();
        other.set_active_task(tasks[0].task.id(), at(100)).unwrap();
        let active = other.current_tracking().clone();
        let request = SetTrackingRequest {
            task_id: None,
            worklog_id: None,
            expected_active: None,
            occurred_at: at(200),
            guard: WriteGuard {
                request_id: Uuid::now_v7().to_string(),
            },
        };

        let rejected = set_tracking(State(shared.clone()), Ok(Json(request)))
            .await
            .unwrap_err();

        assert_eq!(rejected.status, StatusCode::CONFLICT);
        assert_eq!(shared.lock().unwrap().app.current_tracking(), &active);
        let TrackingState::Running { worklog } = active else {
            panic!("the competing timer must remain active");
        };
        assert_eq!(
            SqliteRepository::open(path)
                .unwrap()
                .active_worklog()
                .unwrap()
                .unwrap()
                .id(),
            worklog.id()
        );
    }
}
