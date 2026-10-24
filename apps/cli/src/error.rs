use serde::Serialize;
use tracker_application::{ApplicationError, ApplicationFailureCategory, ApplicationFailureSource};
use tracker_remote::RemoteError;

#[derive(Debug, thiserror::Error, Serialize)]
#[error("{message}")]
pub struct CliError {
    pub code: &'static str,
    pub message: String,
    pub recovery_failed: bool,
    #[serde(skip)]
    pub exit_code: u8,
}

impl CliError {
    pub fn input(message: impl std::fmt::Display) -> Self {
        Self {
            code: "invalid_input",
            message: message.to_string(),
            recovery_failed: false,
            exit_code: 2,
        }
    }

    pub fn storage(error: impl std::fmt::Display) -> Self {
        Self {
            code: "storage",
            message: error.to_string(),
            recovery_failed: false,
            exit_code: 4,
        }
    }

    pub fn remote(error: RemoteError) -> Self {
        let message = match error {
            RemoteError::InvalidEndpoint(message) => message.to_owned(),
            RemoteError::Unavailable(_) => "Tracker server is unavailable".to_owned(),
            RemoteError::Protocol(_) => "Tracker server returned invalid data".to_owned(),
            RemoteError::Http { status, .. } => format!("Tracker server returned HTTP {status}"),
        };
        Self {
            code: "remote",
            message,
            recovery_failed: false,
            exit_code: 5,
        }
    }

    pub fn application(error: ApplicationError) -> Self {
        let failure = error.failure();
        let category_code = category_code(failure.category());
        let (source_code, exit_code) = match failure.source() {
            ApplicationFailureSource::Operation => ("operation_failed", 3),
            ApplicationFailureSource::Storage => ("storage", 4),
            ApplicationFailureSource::RemoteUnavailable
            | ApplicationFailureSource::RemoteProtocol => ("remote", 5),
        };
        Self {
            code: category_code.unwrap_or(source_code),
            message: failure.message().to_owned(),
            recovery_failed: failure.recovery_failed(),
            exit_code,
        }
    }
}

/// Keeps conflict-specific JSON codes independent of local or remote failure sources.
/// Categories without their own code return `None` and use the source code instead.
fn category_code(category: ApplicationFailureCategory) -> Option<&'static str> {
    match category {
        ApplicationFailureCategory::General | ApplicationFailureCategory::TaskNotFound => None,
        ApplicationFailureCategory::WorklogNotFound => Some("worklog_not_found"),
        ApplicationFailureCategory::WorklogChanged => Some("worklog_changed"),
        ApplicationFailureCategory::TaskChanged => Some("task_changed"),
        ApplicationFailureCategory::ActiveWorklog => Some("active_worklog"),
        ApplicationFailureCategory::WorklogHistoryChanged => Some("worklog_history_changed"),
        ApplicationFailureCategory::WorklogOverlap => Some("worklog_overlap"),
        ApplicationFailureCategory::ActiveTask => Some("active_task"),
        ApplicationFailureCategory::InactiveTaskCandidatesChanged => {
            Some("inactive_candidates_changed")
        }
    }
}

impl From<ApplicationError> for CliError {
    fn from(error: ApplicationError) -> Self {
        Self::application(error)
    }
}
