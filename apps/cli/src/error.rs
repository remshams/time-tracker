use serde::Serialize;
use tracker_application::{ApplicationError, ApplicationFailureCategory, RepositoryError};
use tracker_remote::{RemoteError, RemoteFailureKind};

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

    pub fn application(error: ApplicationError, remote: Option<RemoteFailureKind>) -> Self {
        Self::classify(error, remote, false)
    }

    pub fn remote_application(error: ApplicationError, failure: Option<RemoteFailureKind>) -> Self {
        Self::classify(error, failure, true)
    }

    fn classify(
        error: ApplicationError,
        remote: Option<RemoteFailureKind>,
        remote_backend: bool,
    ) -> Self {
        let failure = error.failure();
        let code = category_code(failure.category());
        let storage = is_storage_failure(&error);
        let remote_failure = matches!(
            remote,
            Some(RemoteFailureKind::Unavailable | RemoteFailureKind::Protocol)
        ) || (remote_backend && storage);
        let recovery_failed = failure.recovery_failed()
            || matches!(
                error,
                ApplicationError::TrackingRecovery(_) | ApplicationError::TaskRecovery(_)
            );
        Self {
            code: if code != "operation_failed" {
                code
            } else if remote_failure {
                "remote"
            } else if storage {
                "storage"
            } else {
                code
            },
            message: failure.message().to_owned(),
            recovery_failed,
            exit_code: if remote_failure {
                5
            } else if storage {
                4
            } else {
                3
            },
        }
    }
}

fn category_code(category: ApplicationFailureCategory) -> &'static str {
    match category {
        ApplicationFailureCategory::General => "operation_failed",
        ApplicationFailureCategory::WorklogNotFound => "worklog_not_found",
        ApplicationFailureCategory::WorklogChanged => "worklog_changed",
        ApplicationFailureCategory::ActiveWorklog => "active_worklog",
        ApplicationFailureCategory::WorklogHistoryChanged => "worklog_history_changed",
        ApplicationFailureCategory::WorklogOverlap => "worklog_overlap",
        ApplicationFailureCategory::ActiveTask => "active_task",
        ApplicationFailureCategory::InactiveTaskCandidatesChanged => "inactive_candidates_changed",
    }
}

fn repository_is_storage(error: &RepositoryError) -> bool {
    matches!(
        error,
        RepositoryError::Backend { .. } | RepositoryError::CorruptData { .. }
    )
}

fn is_storage_failure(error: &ApplicationError) -> bool {
    match error {
        ApplicationError::Repository(error)
        | ApplicationError::TrackingWrite(error)
        | ApplicationError::TrackingRecovery(error)
        | ApplicationError::TaskRecovery(error) => repository_is_storage(error),
        ApplicationError::WorklogCorrectionWrite { write }
        | ApplicationError::WorklogMoveWrite { write }
        | ApplicationError::WorklogDeletionWrite { write } => repository_is_storage(write),
        ApplicationError::WorklogCorrectionRecovery { write, recovery }
        | ApplicationError::WorklogMoveRecovery { write, recovery }
        | ApplicationError::WorklogDeletionRecovery { write, recovery } => {
            repository_is_storage(write) || repository_is_storage(recovery)
        }
        _ => false,
    }
}
