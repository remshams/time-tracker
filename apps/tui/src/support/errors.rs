use tracker_application::{ApplicationError, ApplicationFailureCategory};

pub(crate) const ACTIVE_WORKLOG_DELETE_MESSAGE: &str = "Running worklogs cannot be deleted";

/// Adds TUI recovery instructions to semantic history errors without exposing backend details.
pub(crate) fn application_error_text(error: &ApplicationError) -> String {
    let failure = error.failure();
    if failure.category() == ApplicationFailureCategory::WorklogHistoryChanged {
        match failure.recovery_message() {
            None => "Worklog history changed. Press r to refresh".to_owned(),
            Some(recovery) => format!(
                "Worklog history changed. Press r to refresh. State recovery failed: {recovery}."
            ),
        }
    } else {
        failure.message().to_owned()
    }
}

pub(crate) fn correction_error_text(error: &ApplicationError) -> String {
    let failure = error.failure();
    match (failure.category(), failure.recovery_message()) {
        (
            ApplicationFailureCategory::WorklogChanged
            | ApplicationFailureCategory::WorklogNotFound,
            None,
        ) => "Worklog changed. Cancel and press r to refresh.".to_owned(),
        (ApplicationFailureCategory::WorklogOverlap, None) => {
            "The corrected time overlaps another worklog".to_owned()
        }
        (
            ApplicationFailureCategory::WorklogChanged
            | ApplicationFailureCategory::WorklogNotFound,
            Some(recovery),
        ) => format!(
            "Worklog changed. State recovery also failed: {recovery}. Cancel and press r to refresh."
        ),
        (ApplicationFailureCategory::WorklogOverlap, Some(recovery)) => format!(
            "The corrected time overlaps another worklog. State recovery also failed: {recovery}."
        ),
        _ => failure.message().to_owned(),
    }
}

pub(crate) fn deletion_conflict(error: &ApplicationError) -> bool {
    let failure = error.failure();
    !failure.recovery_failed()
        && matches!(
            failure.category(),
            ApplicationFailureCategory::WorklogChanged
                | ApplicationFailureCategory::WorklogNotFound
                | ApplicationFailureCategory::ActiveWorklog
        )
}

pub(crate) fn matches_worklog_active(error: &ApplicationError) -> bool {
    error.failure().category() == ApplicationFailureCategory::ActiveWorklog
}

pub(crate) fn matches_worklog_not_found(error: &ApplicationError) -> bool {
    error.failure().category() == ApplicationFailureCategory::WorklogNotFound
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracker_application::RepositoryError;
    use tracker_domain::TaskId;

    /// Checks that task-scoped and global history conflicts share the same keyboard guidance.
    #[test]
    fn history_errors_add_tui_refresh_instructions_for_both_history_views() {
        for error in [
            RepositoryError::WorklogHistoryChanged {
                task_id: TaskId::generate(),
            },
            RepositoryError::GlobalWorklogHistoryChanged,
        ] {
            assert_eq!(
                application_error_text(&error.into()),
                "Worklog history changed. Press r to refresh"
            );
        }
    }

    /// Checks that keyboard guidance retains the sanitized failure of an authoritative refresh.
    #[test]
    fn history_refresh_instructions_preserve_recovery_failure_details() {
        let error = ApplicationError::Repository(RepositoryError::GlobalWorklogHistoryChanged)
            .with_recovery_failure(ApplicationError::storage_failure("private diagnostic"));
        assert_eq!(
            application_error_text(&error),
            "Worklog history changed. Press r to refresh. State recovery failed: Storage error."
        );
    }
}
