use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use tracker_application::{ClearActiveTaskOutcome, SetActiveTaskOutcome};
use tracker_domain::TrackingState;
use tracker_protocol::WorklogDto;

use crate::CliError;
use crate::args::Tracking;
use crate::backend::{Backend, BackendKind};

pub(crate) async fn execute(
    backend: &mut Backend,
    command: Tracking,
    now: DateTime<Utc>,
) -> Result<Value, CliError> {
    match command {
        Tracking::Status => {
            if let BackendKind::Remote(app) = &mut backend.kind {
                app.refresh_tracking().await.map_err(CliError::remote)?;
            }
            Ok(status(backend, now))
        }
        Tracking::Start { task_id } => {
            Ok(start_result(backend.set_active_task(task_id, now).await?))
        }
        Tracking::Stop { expected_active } => Ok(stop_result(
            backend.clear_active_task(expected_active, now).await?,
        )),
    }
}

fn start_result(outcome: SetActiveTaskOutcome) -> Value {
    match outcome {
        SetActiveTaskOutcome::Started { worklog } => {
            json!({"outcome": "started", "worklog": WorklogDto::from(&worklog)})
        }
        SetActiveTaskOutcome::Switched { stopped, started } => {
            json!({"outcome": "switched", "stopped": WorklogDto::from(&stopped), "started": WorklogDto::from(&started)})
        }
        SetActiveTaskOutcome::AlreadyActive { worklog } => {
            json!({"outcome": "already_active", "worklog": WorklogDto::from(&worklog.to_worklog())})
        }
    }
}

fn stop_result(outcome: ClearActiveTaskOutcome) -> Value {
    match outcome {
        ClearActiveTaskOutcome::Stopped { worklog } => {
            json!({"outcome": "stopped", "worklog": WorklogDto::from(&worklog)})
        }
        ClearActiveTaskOutcome::AlreadyIdle => json!({"outcome": "already_idle"}),
    }
}

fn status(backend: &Backend, now: DateTime<Utc>) -> Value {
    match backend.current_tracking() {
        TrackingState::Idle => {
            json!({"state": "idle", "active_worklog": null, "as_of": now, "elapsed_seconds": 0})
        }
        TrackingState::Running { worklog } => {
            json!({"state": "running", "active_worklog": WorklogDto::from(&worklog.to_worklog()), "as_of": now, "elapsed_seconds": (now - worklog.start()).num_seconds().max(0)})
        }
    }
}
