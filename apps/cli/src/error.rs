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
            code: if category_code == "operation_failed" {
                source_code
            } else {
                category_code
            },
            message: failure.message().to_owned(),
            recovery_failed: failure.recovery_failed(),
            exit_code,
        }
    }
}

fn category_code(category: ApplicationFailureCategory) -> &'static str {
    match category {
        ApplicationFailureCategory::General | ApplicationFailureCategory::TaskNotFound => {
            "operation_failed"
        }
        ApplicationFailureCategory::WorklogNotFound => "worklog_not_found",
        ApplicationFailureCategory::WorklogChanged => "worklog_changed",
        ApplicationFailureCategory::ActiveWorklog => "active_worklog",
        ApplicationFailureCategory::WorklogHistoryChanged => "worklog_history_changed",
        ApplicationFailureCategory::WorklogOverlap => "worklog_overlap",
        ApplicationFailureCategory::ActiveTask => "active_task",
        ApplicationFailureCategory::InactiveTaskCandidatesChanged => "inactive_candidates_changed",
    }
}
