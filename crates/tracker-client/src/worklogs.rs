use crate::TrackerBackend;
use chrono::{DateTime, Utc};
use tracker_application::{
    ApplicationError, GlobalWorklogCursor, GlobalWorklogPage, WorklogCursor, WorklogPage,
};
use tracker_domain::{TaskId, Worklog, WorklogId, WorklogTimes};

impl TrackerBackend {
    pub async fn worklogs_for_task(
        &mut self,
        task_id: TaskId,
        after: Option<&WorklogCursor>,
    ) -> Result<WorklogPage, ApplicationError> {
        match self {
            Self::Local(application) => application.worklogs_for_task(task_id, after),
            Self::Remote(application) => application.worklogs_for_task(task_id, after).await,
        }
    }

    pub async fn all_worklogs(
        &mut self,
        after: Option<&GlobalWorklogCursor>,
    ) -> Result<GlobalWorklogPage, ApplicationError> {
        match self {
            Self::Local(application) => application.all_worklogs(after),
            Self::Remote(application) => application.all_worklogs(after).await,
        }
    }

    pub async fn move_worklog(
        &mut self,
        id: WorklogId,
        expected_source: TaskId,
        expected: WorklogTimes,
        destination: TaskId,
    ) -> Result<Worklog, ApplicationError> {
        match self {
            Self::Local(application) => {
                application.move_worklog(id, expected_source, expected, destination)
            }
            Self::Remote(application) => {
                application
                    .move_worklog(id, expected_source, expected, destination)
                    .await
            }
        }
    }

    pub async fn correct_worklog(
        &mut self,
        id: WorklogId,
        expected: WorklogTimes,
        replacement: WorklogTimes,
        occurred_at: DateTime<Utc>,
    ) -> Result<Worklog, ApplicationError> {
        match self {
            Self::Local(application) => {
                application.correct_worklog(id, expected, replacement, occurred_at)
            }
            Self::Remote(application) => {
                application
                    .correct_worklog(id, expected, replacement, occurred_at)
                    .await
            }
        }
    }

    pub async fn delete_completed_worklog(
        &mut self,
        id: WorklogId,
        expected_task: TaskId,
        expected: WorklogTimes,
    ) -> Result<Worklog, ApplicationError> {
        match self {
            Self::Local(application) => {
                application.delete_completed_worklog(id, expected_task, expected)
            }
            Self::Remote(application) => {
                application
                    .delete_completed_worklog(id, expected_task, expected)
                    .await
            }
        }
    }
}
