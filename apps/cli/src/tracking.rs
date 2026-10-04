use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use tracker_application::{ClearActiveTaskOutcome, SetActiveTaskOutcome};
use tracker_domain::TrackingState;
use tracker_protocol::WorklogDto;

use crate::CliError;
use crate::args::Tracking;
use crate::backend::Backend;

pub(crate) async fn execute(
    backend: &mut Backend,
    command: Tracking,
    now: DateTime<Utc>,
) -> Result<Value, CliError> {
    match command {
        Tracking::Status => Ok(status(backend, now)),
        Tracking::Start { task_id } => Ok(match backend.set_active_task(task_id, now).await? {
            SetActiveTaskOutcome::Started { worklog } => {
                json!({"outcome": "started", "worklog": WorklogDto::from(&worklog)})
            }
            SetActiveTaskOutcome::Switched { stopped, started } => {
                json!({"outcome": "switched", "stopped": WorklogDto::from(&stopped), "started": WorklogDto::from(&started)})
            }
            SetActiveTaskOutcome::AlreadyActive { worklog } => {
                json!({"outcome": "already_active", "worklog": WorklogDto::from(&worklog.to_worklog())})
            }
        }),
        Tracking::Stop { expected_active } => Ok(
            match backend.clear_active_task(expected_active, now).await? {
                ClearActiveTaskOutcome::Stopped { worklog } => {
                    json!({"outcome": "stopped", "worklog": WorklogDto::from(&worklog)})
                }
                ClearActiveTaskOutcome::AlreadyIdle => json!({"outcome": "already_idle"}),
            },
        ),
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
