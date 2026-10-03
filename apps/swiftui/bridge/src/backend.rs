use chrono::{DateTime, Utc};
use tokio::runtime::{Builder, Runtime};
use tracker_application::{
    ApplicationError, ApplicationFailureCategory, ClearActiveTaskOutcome, RepositoryError,
    SetActiveTaskOutcome, TaskListItem, TaskOrdering, TaskQueries, TrackerApplication,
    TrackingOperations, WorklogCursor, WorklogPage, WorklogQueries,
};
use tracker_domain::{TaskId, TrackingState, WorklogId};
use tracker_remote::{RemoteApplication, RemoteError, RemoteFailureKind};
use tracker_storage::SqliteRepository;

pub(crate) enum Backend {
    Local(TrackerApplication<SqliteRepository>),
    Remote(Box<RemoteBackend>),
}

pub(crate) struct RemoteBackend {
    application: RemoteApplication,
    runtime: Runtime,
    requires_refresh: bool,
}

pub(crate) struct BridgeError {
    pub message: String,
    pub kind: &'static str,
    pub uncertain: bool,
    pub requires_refresh: bool,
}

impl From<String> for BridgeError {
    fn from(message: String) -> Self {
        Self {
            message,
            kind: "general",
            uncertain: false,
            requires_refresh: false,
        }
    }
}

impl Backend {
    pub fn remote(endpoint: &str) -> Result<Self, String> {
        let application =
            RemoteApplication::disconnected(endpoint).map_err(|error| error.to_string())?;
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self::Remote(Box::new(RemoteBackend {
            application,
            runtime,
            requires_refresh: true,
        })))
    }

    pub fn tasks(&self, ordering: TaskOrdering) -> Vec<TaskListItem> {
        match self {
            Self::Local(application) => application.tasks(ordering),
            Self::Remote(remote) => remote.application.tasks(ordering),
        }
    }

    pub fn current_tracking(&self) -> &TrackingState {
        match self {
            Self::Local(application) => application.current_tracking(),
            Self::Remote(remote) => remote.application.current_tracking(),
        }
    }

    pub fn refresh(&mut self) -> Result<(), BridgeError> {
        match self {
            Self::Local(application) => application
                .refresh_authoritative_state()
                .map_err(local_error),
            Self::Remote(remote) => match remote.runtime.block_on(remote.application.refresh()) {
                Ok(()) => {
                    remote.requires_refresh = false;
                    Ok(())
                }
                Err(error) => {
                    remote.requires_refresh = true;
                    Err(BridgeError {
                        message: error.to_string(),
                        kind: remote_error_kind(&error),
                        uncertain: false,
                        requires_refresh: true,
                    })
                }
            },
        }
    }

    pub fn set_active_task(
        &mut self,
        task_id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, BridgeError> {
        match self {
            Self::Local(application) => application
                .set_active_task(task_id, occurred_at)
                .map_err(local_error),
            Self::Remote(remote) => {
                remote.check_write()?;
                let result = remote
                    .runtime
                    .block_on(remote.application.set_active_task(task_id, occurred_at));
                result.map_err(|error| remote.operation_error(error, true))
            }
        }
    }

    pub fn clear_active_task(
        &mut self,
        worklog_id: WorklogId,
        occurred_at: DateTime<Utc>,
    ) -> Result<ClearActiveTaskOutcome, BridgeError> {
        match self {
            Self::Local(application) => application
                .clear_active_task(worklog_id, occurred_at)
                .map_err(local_error),
            Self::Remote(remote) => {
                remote.check_write()?;
                let result = remote.runtime.block_on(
                    remote
                        .application
                        .clear_active_task(worklog_id, occurred_at),
                );
                result.map_err(|error| remote.operation_error(error, true))
            }
        }
    }

    pub fn set_active_task_if_active(
        &mut self,
        task_id: TaskId,
        expected_active: Option<WorklogId>,
        occurred_at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, BridgeError> {
        let current_active = match self.current_tracking() {
            TrackingState::Idle => None,
            TrackingState::Running { worklog } => Some(worklog.id()),
        };
        if current_active != expected_active {
            if let Self::Remote(remote) = self {
                remote.requires_refresh = true;
            }
            return Err(BridgeError {
                message: "Tracking state changed. Refresh and retry.".into(),
                kind: "conflict",
                uncertain: false,
                requires_refresh: true,
            });
        }
        self.set_active_task(task_id, occurred_at)
    }

    pub fn worklogs_for_task(
        &mut self,
        task_id: TaskId,
        cursor: Option<&WorklogCursor>,
    ) -> Result<WorklogPage, ApplicationError> {
        match self {
            Self::Local(application) => application.worklogs_for_task(task_id, cursor),
            Self::Remote(remote) => remote
                .runtime
                .block_on(remote.application.worklogs_for_task(task_id, cursor)),
        }
    }

    pub fn history_error(&mut self, error: ApplicationError) -> BridgeError {
        match self {
            Self::Local(_) => local_error(error),
            Self::Remote(remote) => remote.operation_error(error, false),
        }
    }

    #[cfg(test)]
    pub fn archive_task(
        &mut self,
        task_id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<(), ApplicationError> {
        use tracker_application::TaskOperations;
        match self {
            Self::Local(application) => application.archive_task(task_id, occurred_at).map(|_| ()),
            Self::Remote(_) => panic!("test helper expects local backend"),
        }
    }
}

impl RemoteBackend {
    fn check_write(&self) -> Result<(), BridgeError> {
        if self.requires_refresh {
            return Err(BridgeError {
                message: "Refresh server state before tracking again".into(),
                kind: "unavailable",
                uncertain: false,
                requires_refresh: true,
            });
        }
        Ok(())
    }

    fn operation_error(&mut self, error: ApplicationError, write: bool) -> BridgeError {
        let failure = error.failure();
        let kind = operation_error_kind(self.application.last_failure(), &error);
        let uncertain = write && matches!(kind, "unavailable" | "protocol");
        self.requires_refresh |= write || matches!(kind, "unavailable" | "protocol");
        BridgeError {
            message: failure.message().to_owned(),
            kind,
            uncertain,
            requires_refresh: self.requires_refresh,
        }
    }
}

fn operation_error_kind(
    last_failure: Option<RemoteFailureKind>,
    error: &ApplicationError,
) -> &'static str {
    let failure = error.failure();
    match last_failure {
        Some(RemoteFailureKind::Unavailable) => "unavailable",
        Some(RemoteFailureKind::Protocol) => "protocol",
        Some(RemoteFailureKind::Conflict) => "conflict",
        None if matches!(
            error,
            ApplicationError::Repository(RepositoryError::Backend { .. })
        ) =>
        {
            "protocol"
        }
        None if failure.category() != ApplicationFailureCategory::General
            || failure.message().contains("changed") =>
        {
            "conflict"
        }
        None => "general",
    }
}

fn local_error(error: ApplicationError) -> BridgeError {
    error.failure().message().to_owned().into()
}

fn remote_error_kind(error: &RemoteError) -> &'static str {
    if error.is_unavailable() {
        "unavailable"
    } else {
        "protocol"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovered_application_errors_keep_general_and_conflict_distinct() {
        for (category, message, expected) in [
            (
                ApplicationFailureCategory::General,
                "Task not found",
                "general",
            ),
            (
                ApplicationFailureCategory::General,
                "Tracker state changed. Refresh and retry.",
                "conflict",
            ),
            (
                ApplicationFailureCategory::WorklogHistoryChanged,
                "Reload worklog history",
                "conflict",
            ),
        ] {
            let error = ApplicationError::semantic_failure(category, message);
            assert_eq!(operation_error_kind(None, &error), expected);
        }
    }

    #[test]
    fn recovered_backend_failures_still_require_an_uncertain_write_result() {
        let error = ApplicationError::storage_failure("remote server error");
        assert_eq!(operation_error_kind(None, &error), "protocol");
    }
}
