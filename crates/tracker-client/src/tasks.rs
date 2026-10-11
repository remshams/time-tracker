use crate::{InactivePreview, TrackerBackend};
use chrono::{DateTime, Utc};
use tracker_application::{ApplicationError, ApplicationFailureCategory};
use tracker_domain::{InactivityPeriod, Task, TaskId, TaskName};

impl TrackerBackend {
    pub async fn preview_inactive_tasks(
        &mut self,
        as_of: DateTime<Utc>,
        period: InactivityPeriod,
    ) -> Result<InactivePreview, ApplicationError> {
        match self {
            Self::Local(application) => application
                .preview_inactive_tasks_with_period(as_of, period)
                .map(|tasks| InactivePreview::Local {
                    as_of,
                    period,
                    tasks,
                }),
            Self::Remote(application) => {
                let preview = application
                    .preview_inactive_tasks_with_period(as_of, period)
                    .await?;
                let tasks = application.resolve_preview_tasks(&preview).await?;
                Ok(InactivePreview::Remote { preview, tasks })
            }
        }
    }

    pub async fn archive_inactive_tasks(
        &mut self,
        preview: InactivePreview,
    ) -> Result<usize, ApplicationError> {
        match (self, preview) {
            (
                Self::Local(application),
                InactivePreview::Local {
                    as_of,
                    period,
                    tasks,
                },
            ) => {
                let ids = tasks.iter().map(Task::id).collect::<Vec<_>>();
                application
                    .archive_inactive_tasks_with_period(&ids, as_of, period)
                    .map(|tasks| tasks.len())
            }
            (Self::Remote(application), InactivePreview::Remote { preview, .. }) => {
                application
                    .archive_inactive_tasks_with_period(&preview)
                    .await
            }
            _ => Err(ApplicationError::semantic_failure(
                ApplicationFailureCategory::General,
                "Archive preview does not match the current data source",
            )),
        }
    }

    pub async fn create_task(
        &mut self,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        match self {
            Self::Local(application) => application.create_task(name, occurred_at),
            Self::Remote(application) => application.create_task(name, occurred_at).await,
        }
    }

    pub async fn rename_task(
        &mut self,
        id: TaskId,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        match self {
            Self::Local(application) => application.rename_task(id, name, occurred_at),
            Self::Remote(application) => application.rename_task(id, name, occurred_at).await,
        }
    }

    pub async fn archive_task(
        &mut self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        match self {
            Self::Local(application) => application.archive_task(id, occurred_at),
            Self::Remote(application) => application.archive_task(id, occurred_at).await,
        }
    }

    pub async fn unarchive_task(
        &mut self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        match self {
            Self::Local(application) => application.unarchive_task(id, occurred_at),
            Self::Remote(application) => application.unarchive_task(id, occurred_at).await,
        }
    }

    pub async fn rename_task_with_expected_name(
        &mut self,
        id: TaskId,
        expected: &TaskName,
        name: TaskName,
        at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        match self {
            Self::Local(application) => application.rename_task(id, name, at),
            Self::Remote(application) => {
                application
                    .rename_task_with_expected_name(id, expected, name, at)
                    .await
            }
        }
    }

    pub async fn archive_task_with_expected_name(
        &mut self,
        id: TaskId,
        expected: &TaskName,
        at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        match self {
            Self::Local(application) => application.archive_task(id, at),
            Self::Remote(application) => {
                application
                    .archive_task_with_expected_name(id, expected, at)
                    .await
            }
        }
    }
}
