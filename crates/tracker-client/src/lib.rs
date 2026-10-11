//! Shared operations for clients that select one local or remote backend.

use chrono::{DateTime, TimeDelta, Utc};
use tracker_application::{
    ApplicationError, MoveCandidate, TaskListItem, TaskOrdering, TrackerApplication,
};
use tracker_domain::{Task, TaskId, TrackingState};
use tracker_protocol::{InactiveTaskPreviewDto, MutationDto, TaskDto};
use tracker_remote::{RemoteApplication, RemoteError, ResourceSelection};
use tracker_storage::SqliteRepository;

/// The backend selected by a client. Remote failures never select local storage.
pub enum TrackerBackend {
    Local(TrackerApplication<SqliteRepository>),
    Remote(Box<RemoteApplication>),
}

/// A refresh failure retains its local or transport origin for client policies.
#[derive(Debug, thiserror::Error)]
pub enum RefreshError {
    #[error(transparent)]
    Local(#[from] ApplicationError),
    #[error(transparent)]
    Remote(#[from] RemoteError),
}

impl RefreshError {
    pub fn into_application_error(self) -> ApplicationError {
        match self {
            Self::Local(error) => error,
            Self::Remote(error) if error.is_unavailable() => {
                ApplicationError::RemoteUnavailable(error.to_string())
            }
            Self::Remote(error) => ApplicationError::RemoteProtocol(error.to_string()),
        }
    }
}

/// Report rows remain useful when task metadata cannot be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientReportRow {
    pub task_id: TaskId,
    pub duration: TimeDelta,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientReport {
    pub rows: Vec<ClientReportRow>,
    pub total: TimeDelta,
    pub now: DateTime<Utc>,
    pub revision: Option<String>,
}

/// The original reviewed candidates and backend-specific confirmation guard.
pub enum InactivePreview {
    Local {
        as_of: DateTime<Utc>,
        period: tracker_domain::InactivityPeriod,
        tasks: Vec<Task>,
    },
    Remote {
        preview: InactiveTaskPreviewDto,
        tasks: Vec<TaskDto>,
    },
}

impl InactivePreview {
    pub fn as_of(&self) -> DateTime<Utc> {
        match self {
            Self::Local { as_of, .. } => *as_of,
            Self::Remote { preview, .. } => preview.as_of,
        }
    }

    pub fn days(&self) -> u32 {
        match self {
            Self::Local { period, .. } => period.days(),
            Self::Remote { preview, .. } => preview.inactive_days,
        }
    }
}

mod reports;
mod tasks;
mod tracking;
mod worklogs;

impl TrackerBackend {
    pub fn remote_application(&self) -> Option<&RemoteApplication> {
        match self {
            Self::Local(_) => None,
            Self::Remote(application) => Some(application),
        }
    }

    pub fn remote(endpoint: &str) -> Result<Self, RemoteError> {
        RemoteApplication::disconnected(endpoint)
            .map(|application| Self::Remote(Box::new(application)))
    }

    pub fn is_remote(&self) -> bool {
        matches!(self, Self::Remote(_))
    }

    pub fn tasks(&self, ordering: TaskOrdering) -> Vec<TaskListItem> {
        match self {
            Self::Local(application) => application.tasks(ordering),
            Self::Remote(application) => application.tasks(ordering),
        }
    }

    pub fn task(&self, id: TaskId) -> Option<&Task> {
        match self {
            Self::Local(application) => application.task(id),
            Self::Remote(application) => application.task(id),
        }
    }

    pub fn current_tracking(&self) -> &TrackingState {
        match self {
            Self::Local(application) => application.current_tracking(),
            Self::Remote(application) => application.current_tracking(),
        }
    }

    pub fn move_candidates(&self, source: TaskId, query: &str) -> Vec<MoveCandidate> {
        match self {
            Self::Local(application) => application.move_candidates(source, query),
            Self::Remote(application) => application.move_candidates(source, query),
        }
    }

    pub fn command_receipt(&self) -> Option<&MutationDto> {
        match self {
            Self::Local(_) => None,
            Self::Remote(application) => application.last_command_receipt(),
        }
    }

    pub fn task_observation(&self) -> Option<(Vec<TaskListItem>, Option<String>)> {
        match self {
            Self::Local(application) => Some((application.tasks(TaskOrdering::default()), None)),
            Self::Remote(application) => application.task_observation().map(|observation| {
                (
                    observation.value.clone(),
                    Some(observation.revision.clone()),
                )
            }),
        }
    }

    pub fn tracking_observation(&self) -> Option<(TrackingState, Option<String>)> {
        match self {
            Self::Local(application) => Some((application.current_tracking().clone(), None)),
            Self::Remote(application) => application.tracking_observation().map(|observation| {
                (
                    observation.value.clone(),
                    Some(observation.revision.clone()),
                )
            }),
        }
    }

    pub async fn check_connection(&mut self) -> Result<(), RemoteError> {
        match self {
            Self::Local(_) => Ok(()),
            Self::Remote(application) => application.check_connection().await,
        }
    }

    pub async fn refresh_resources(
        &mut self,
        selection: ResourceSelection,
    ) -> Result<(), RefreshError> {
        match self {
            Self::Local(application) => match selection {
                ResourceSelection::Tasks => application.refresh_task_catalog(),
                ResourceSelection::Tracking => application.refresh_tracking_resource(),
                ResourceSelection::TaskList => application.refresh_task_list(),
                ResourceSelection::DailyTotals { start, end, now } => {
                    application.report_totals(start, end, now).map(|_| ())
                }
                ResourceSelection::TaskListWithTotals { start, end, now } => application
                    .task_list_report_totals(start, end, now)
                    .map(|_| ()),
            }
            .map_err(RefreshError::Local),
            Self::Remote(application) => application
                .refresh_resources(selection)
                .await
                .map_err(RefreshError::Remote),
        }
    }

    pub async fn refresh_task_list(&mut self) -> Result<(), RefreshError> {
        self.refresh_resources(ResourceSelection::TaskList).await
    }

    pub async fn read_task(&mut self, id: TaskId) -> Result<TaskListItem, ApplicationError> {
        match self {
            Self::Local(application) => application
                .read_task_item(id)?
                .ok_or_else(|| tracker_application::RepositoryError::TaskNotFound { id }.into()),
            Self::Remote(application) => application
                .read_task_observation(id)
                .await
                .map(|observation| observation.value),
        }
    }
}
