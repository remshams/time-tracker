use crate::TrackerBackend;
use chrono::{DateTime, Utc};
use tracker_application::{ApplicationError, ClearActiveTaskOutcome, SetActiveTaskOutcome};
use tracker_domain::{TaskId, WorklogId};

impl TrackerBackend {
    pub async fn set_active_task(
        &mut self,
        task_id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, ApplicationError> {
        match self {
            Self::Local(application) => application.set_active_task(task_id, occurred_at),
            Self::Remote(application) => application.set_active_task(task_id, occurred_at).await,
        }
    }

    pub async fn clear_active_task(
        &mut self,
        expected_active: WorklogId,
        occurred_at: DateTime<Utc>,
    ) -> Result<ClearActiveTaskOutcome, ApplicationError> {
        match self {
            Self::Local(application) => application.clear_active_task(expected_active, occurred_at),
            Self::Remote(application) => {
                application
                    .clear_active_task(expected_active, occurred_at)
                    .await
            }
        }
    }

    pub async fn set_active_task_with_expected_active(
        &mut self,
        id: TaskId,
        expected: Option<WorklogId>,
        at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, ApplicationError> {
        match self {
            Self::Local(application) => application.set_active_task(id, at),
            Self::Remote(application) => {
                application
                    .set_active_task_with_expected_active(id, expected, at)
                    .await
            }
        }
    }

    pub async fn start_tracking_if_idle(
        &mut self,
        id: TaskId,
        at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, ApplicationError> {
        match self {
            Self::Local(application) => application.start_tracking_if_idle(id, at),
            Self::Remote(application) => {
                application
                    .set_active_task_with_expected_active(id, None, at)
                    .await
            }
        }
    }
}
