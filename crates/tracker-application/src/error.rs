//! Application failures and presentation-safe classification.

use tracker_domain::{TaskId, TrackingError, WorklogCorrectionError, WorklogId, WorklogMoveError};

use crate::RepositoryError;

/// A semantic failure category exposed to application clients.
///
/// Clients can branch on the few categories that change their recovery flow.
/// All other failures remain [`ApplicationFailureCategory::General`] and use
/// the sanitized message supplied by [`ApplicationFailure`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplicationFailureCategory {
    General,
    TaskNotFound,
    WorklogNotFound,
    WorklogChanged,
    ActiveWorklog,
    WorklogHistoryChanged,
    WorklogOverlap,
    ActiveTask,
    InactiveTaskCandidatesChanged,
}

/// Where an operation or its state recovery failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplicationFailureSource {
    Operation,
    Storage,
    RemoteUnavailable,
    RemoteProtocol,
}

/// A presentation-safe view of an [`ApplicationError`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationFailure {
    category: ApplicationFailureCategory,
    message: String,
    source: ApplicationFailureSource,
    recovery: Option<(ApplicationFailureCategory, String)>,
}

impl ApplicationFailure {
    /// Returns the failure source, including a failed state recovery.
    pub fn source(&self) -> ApplicationFailureSource {
        self.source
    }

    /// Returns the primary semantic failure category.
    pub fn category(&self) -> ApplicationFailureCategory {
        self.category
    }

    /// Returns a message that does not expose backend details.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Whether authoritative state recovery also failed.
    pub fn recovery_failed(&self) -> bool {
        self.recovery.is_some()
    }

    /// Returns the recovery failure category when state recovery also failed.
    pub fn recovery_category(&self) -> Option<ApplicationFailureCategory> {
        self.recovery.as_ref().map(|(category, _)| *category)
    }

    /// Returns the sanitized recovery failure message when recovery also failed.
    pub fn recovery_message(&self) -> Option<&str> {
        self.recovery.as_ref().map(|(_, message)| message.as_str())
    }
}

/// Why an application operation failed.
///
/// The original repository errors remain attached for diagnostics. Clients
/// should use [`ApplicationError::failure`] rather than inspect repository
/// variants.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApplicationError {
    #[error("tracker server is unavailable: {0}")]
    RemoteUnavailable(String),
    #[error("tracker server returned invalid data: {0}")]
    RemoteProtocol(String),
    #[error("{primary}; state recovery failed: {recovery}")]
    Recovery {
        #[source]
        primary: Box<ApplicationError>,
        recovery: Box<ApplicationError>,
    },
    #[error("{message}")]
    Semantic {
        category: ApplicationFailureCategory,
        message: String,
    },
    #[error(transparent)]
    Domain(#[from] TrackingError),
    #[error(transparent)]
    InvalidWorklogCorrection(#[from] WorklogCorrectionError),
    #[error(transparent)]
    InvalidWorklogMove(#[from] WorklogMoveError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
    #[error("tracking write failed: {0}")]
    TrackingWrite(#[source] RepositoryError),
    #[error("tracking state could not be recovered: {0}")]
    TrackingRecovery(#[source] RepositoryError),
    #[error("task state could not be recovered: {0}")]
    TaskRecovery(#[source] RepositoryError),
    #[error("worklog correction write failed: {write}")]
    WorklogCorrectionWrite {
        #[source]
        write: RepositoryError,
    },
    #[error("worklog correction write failed: {write}; state recovery failed: {recovery}")]
    WorklogCorrectionRecovery {
        #[source]
        write: RepositoryError,
        recovery: RepositoryError,
    },
    #[error("worklog move write failed: {write}")]
    WorklogMoveWrite {
        #[source]
        write: RepositoryError,
    },
    #[error("worklog move write failed: {write}; state recovery failed: {recovery}")]
    WorklogMoveRecovery {
        #[source]
        write: RepositoryError,
        recovery: RepositoryError,
    },
    #[error("worklog deletion write failed: {write}")]
    WorklogDeletionWrite {
        #[source]
        write: RepositoryError,
    },
    #[error("worklog deletion write failed: {write}; state recovery failed: {recovery}")]
    WorklogDeletionRecovery {
        #[source]
        write: RepositoryError,
        recovery: RepositoryError,
    },
    #[error("tracking state changed in another client")]
    TrackingStateChanged,
    #[error("report end must be later than report start")]
    InvalidReportRange,
    #[error("report duration exceeds the supported range")]
    ReportDurationOverflow,
}

impl ApplicationError {
    /// Attaches a failed authoritative refresh without losing the primary category.
    pub fn with_recovery_failure(self, recovery: Self) -> Self {
        Self::Recovery {
            primary: Box::new(self),
            recovery: Box::new(recovery),
        }
    }

    /// Rebuilds a presentation-safe semantic failure received from another
    /// application service. Backend diagnostics must never be passed here.
    pub fn semantic_failure(
        category: ApplicationFailureCategory,
        message: impl Into<String>,
    ) -> Self {
        Self::Semantic {
            category,
            message: message.into(),
        }
    }

    /// Builds a backend failure while keeping its diagnostic text out of the
    /// presentation-safe classification.
    pub fn storage_failure(message: impl Into<String>) -> Self {
        RepositoryError::Backend {
            message: message.into(),
        }
        .into()
    }

    /// Builds a semantic missing-worklog failure.
    pub fn worklog_not_found(id: WorklogId) -> Self {
        RepositoryError::WorklogNotFound { id }.into()
    }

    /// Builds a semantic stale-worklog failure.
    pub fn worklog_changed(id: WorklogId) -> Self {
        RepositoryError::WorklogChanged { id }.into()
    }

    /// Builds a failure for a worklog that must remain active.
    pub fn active_worklog(id: WorklogId) -> Self {
        RepositoryError::WorklogIsActive { id }.into()
    }

    /// Builds a failure for an invalidated history cursor.
    pub fn worklog_history_changed(task_id: TaskId) -> Self {
        RepositoryError::WorklogHistoryChanged { task_id }.into()
    }

    /// Builds a same-task overlap failure.
    pub fn worklog_overlap(id: WorklogId) -> Self {
        RepositoryError::SameTaskWorklogOverlap { id }.into()
    }

    /// Builds a stale correction failure whose state recovery also failed.
    pub fn correction_changed_with_recovery_failure(
        id: WorklogId,
        recovery_message: impl Into<String>,
    ) -> Self {
        Self::WorklogCorrectionRecovery {
            write: RepositoryError::WorklogChanged { id },
            recovery: RepositoryError::Backend {
                message: recovery_message.into(),
            },
        }
    }

    /// Builds an overlap correction failure whose state recovery also failed.
    pub fn correction_overlap_with_recovery_failure(
        id: WorklogId,
        recovery_message: impl Into<String>,
    ) -> Self {
        Self::WorklogCorrectionRecovery {
            write: RepositoryError::SameTaskWorklogOverlap { id },
            recovery: RepositoryError::Backend {
                message: recovery_message.into(),
            },
        }
    }

    /// Builds a stale deletion failure whose state recovery also failed.
    pub fn deletion_changed_with_recovery_failure(
        id: WorklogId,
        recovery_message: impl Into<String>,
    ) -> Self {
        Self::WorklogDeletionRecovery {
            write: RepositoryError::WorklogChanged { id },
            recovery: RepositoryError::Backend {
                message: recovery_message.into(),
            },
        }
    }

    /// Classifies this error without exposing repository-specific failures or
    /// unsanitized backend messages.
    pub fn failure(&self) -> ApplicationFailure {
        match self {
            Self::RemoteUnavailable(_) => ApplicationFailure {
                category: ApplicationFailureCategory::General,
                message: "Tracker server is unavailable".to_owned(),
                source: ApplicationFailureSource::RemoteUnavailable,
                recovery: None,
            },
            Self::RemoteProtocol(_) => ApplicationFailure {
                category: ApplicationFailureCategory::General,
                message: "Tracker server returned invalid data".to_owned(),
                source: ApplicationFailureSource::RemoteProtocol,
                recovery: None,
            },
            Self::Recovery { primary, recovery } => {
                let mut failure = primary.failure();
                let recovery = recovery.failure();
                failure.message = format!(
                    "{}. State recovery failed: {}.",
                    failure.message, recovery.message
                );
                attach_recovery(failure, recovery)
            }
            Self::Semantic { category, message } => ApplicationFailure {
                category: *category,
                message: message.clone(),
                source: ApplicationFailureSource::Operation,
                recovery: None,
            },
            Self::Domain(error) => ApplicationFailure {
                category: match error {
                    TrackingError::TaskIsActive { .. } => ApplicationFailureCategory::ActiveTask,
                    _ => ApplicationFailureCategory::General,
                },
                message: error.to_string(),
                source: ApplicationFailureSource::Operation,
                recovery: None,
            },
            Self::InvalidWorklogCorrection(error) => ApplicationFailure {
                category: ApplicationFailureCategory::General,
                message: error.to_string(),
                source: ApplicationFailureSource::Operation,
                recovery: None,
            },
            Self::InvalidWorklogMove(error) => ApplicationFailure {
                category: ApplicationFailureCategory::General,
                message: error.to_string(),
                source: ApplicationFailureSource::Operation,
                recovery: None,
            },
            Self::Repository(error) | Self::TrackingWrite(error) => repository_failure(error),
            Self::TrackingRecovery(error) | Self::TaskRecovery(error) => {
                let mut failure = repository_failure(error);
                failure.recovery = Some((failure.category, failure.message.clone()));
                failure
            }
            Self::WorklogCorrectionWrite { write }
            | Self::WorklogMoveWrite { write }
            | Self::WorklogDeletionWrite { write } => repository_failure(write),
            Self::WorklogCorrectionRecovery { write, recovery } => {
                failure_with_recovery("Correction", write, recovery)
            }
            Self::WorklogMoveRecovery { write, recovery } => {
                failure_with_recovery("Move", write, recovery)
            }
            Self::WorklogDeletionRecovery { write, recovery } => {
                failure_with_recovery("Deletion", write, recovery)
            }
            Self::TrackingStateChanged => ApplicationFailure {
                category: ApplicationFailureCategory::General,
                message: "Tracking state changed in another client. Refreshed state.".to_owned(),
                source: ApplicationFailureSource::Operation,
                recovery: None,
            },
            Self::InvalidReportRange => ApplicationFailure {
                category: ApplicationFailureCategory::General,
                message: "Report end must be later than start".to_owned(),
                source: ApplicationFailureSource::Operation,
                recovery: None,
            },
            Self::ReportDurationOverflow => ApplicationFailure {
                category: ApplicationFailureCategory::General,
                message: "Report duration is too large".to_owned(),
                source: ApplicationFailureSource::Operation,
                recovery: None,
            },
        }
    }
}

fn failure_with_recovery(
    operation: &str,
    primary: &RepositoryError,
    recovery: &RepositoryError,
) -> ApplicationFailure {
    let mut failure = repository_failure(primary);
    let recovery_failure = repository_failure(recovery);
    let recovery_message = recovery_failure.message();
    failure.message = format!(
        "{operation} failed: {}. State recovery failed: {recovery_message}.",
        failure.message
    );
    attach_recovery(failure, recovery_failure)
}

fn attach_recovery(
    mut failure: ApplicationFailure,
    recovery: ApplicationFailure,
) -> ApplicationFailure {
    if recovery.source != ApplicationFailureSource::Operation {
        failure.source = recovery.source;
    }
    failure.recovery = Some((recovery.category, recovery.message));
    failure
}

fn repository_failure(error: &RepositoryError) -> ApplicationFailure {
    ApplicationFailure {
        category: repository_error_category(error),
        message: repository_error_message(error).to_owned(),
        source: if matches!(
            error,
            RepositoryError::Backend { .. } | RepositoryError::CorruptData { .. }
        ) {
            ApplicationFailureSource::Storage
        } else {
            ApplicationFailureSource::Operation
        },
        recovery: None,
    }
}

fn repository_error_category(error: &RepositoryError) -> ApplicationFailureCategory {
    match error {
        RepositoryError::TaskNotFound { .. } => ApplicationFailureCategory::TaskNotFound,
        RepositoryError::WorklogNotFound { .. } => ApplicationFailureCategory::WorklogNotFound,
        RepositoryError::WorklogChanged { .. } => ApplicationFailureCategory::WorklogChanged,
        RepositoryError::WorklogIsActive { .. } => ApplicationFailureCategory::ActiveWorklog,
        RepositoryError::WorklogHistoryChanged { .. }
        | RepositoryError::GlobalWorklogHistoryChanged => {
            ApplicationFailureCategory::WorklogHistoryChanged
        }
        RepositoryError::SameTaskWorklogOverlap { .. } => {
            ApplicationFailureCategory::WorklogOverlap
        }
        RepositoryError::TaskIsActive { .. } => ApplicationFailureCategory::ActiveTask,
        RepositoryError::InactiveTaskCandidatesChanged => {
            ApplicationFailureCategory::InactiveTaskCandidatesChanged
        }
        _ => ApplicationFailureCategory::General,
    }
}

fn repository_error_message(error: &RepositoryError) -> &'static str {
    match error {
        RepositoryError::TaskNotFound { .. } => "Task not found",
        RepositoryError::WorklogNotFound { .. } => "Worklog not found",
        RepositoryError::WorklogAlreadyStopped { .. } => "Worklog is already stopped",
        RepositoryError::WorklogChanged { .. } => "Worklog changed in another client",
        RepositoryError::WorklogIsActive { .. } => "Running worklogs cannot be deleted",
        RepositoryError::WorklogHistoryChanged { .. }
        | RepositoryError::GlobalWorklogHistoryChanged => {
            "Worklog history changed. Press r to refresh"
        }
        RepositoryError::SameTaskWorklogOverlap { .. } => "The worklog overlaps another worklog",
        RepositoryError::WorklogAlreadyExists { .. } => "Worklog already exists",
        RepositoryError::TaskAlreadyExists { .. } => "Task already exists",
        RepositoryError::ActiveWorklogExists => "Another worklog is active",
        RepositoryError::TaskArchived { .. } => "Task is archived",
        RepositoryError::TaskIsActive { .. } => "Task has active work",
        RepositoryError::InactiveTaskCandidatesChanged => {
            "Inactive task list changed. Preview again"
        }
        RepositoryError::Constraint { .. } => "Storage rejected the change",
        RepositoryError::CorruptData { .. } => "Stored data is invalid",
        RepositoryError::ReportDurationOverflow => "Report duration is too large",
        RepositoryError::Backend { .. } => "Storage error",
    }
}
