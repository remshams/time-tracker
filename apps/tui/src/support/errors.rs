use tracker_application::{ApplicationError, ApplicationFailureCategory};

pub(crate) const ACTIVE_WORKLOG_DELETE_MESSAGE: &str = "Running worklogs cannot be deleted";

pub(crate) fn application_error_text(error: &ApplicationError) -> String {
    error.failure().message().to_owned()
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
