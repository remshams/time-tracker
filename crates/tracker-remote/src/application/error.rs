use super::{
    ApplicationError, ApplicationFailureCategory, ErrorCode, ErrorDto, RemoteError,
    RemoteFailureKind, StatusCode, TaskId, WorklogId,
};

pub(super) fn protocol_failure(message: &str) -> ApplicationError {
    ApplicationError::RemoteProtocol(format!("invalid server response: {message}"))
}

pub(super) fn classify_error(error: &RemoteError) -> RemoteFailureKind {
    if error.is_unavailable() {
        return RemoteFailureKind::Unavailable;
    }
    match error {
        RemoteError::Http { status, .. } if *status == StatusCode::CONFLICT => {
            RemoteFailureKind::Conflict
        }
        _ => RemoteFailureKind::Protocol,
    }
}

pub(super) fn map_application_error(
    error: &RemoteError,
    worklog_id: Option<WorklogId>,
    _task_id: Option<TaskId>,
) -> ApplicationError {
    if error.is_unavailable() {
        return ApplicationError::RemoteUnavailable(error.to_string());
    }
    if let RemoteError::Http { status, body } = error {
        if let Ok(dto) = serde_json::from_slice::<ErrorDto>(body) {
            return map_server_error(dto, worklog_id);
        }
        if *status == StatusCode::CONFLICT {
            return ApplicationError::semantic_failure(
                ApplicationFailureCategory::General,
                "Tracker state changed. Refresh and retry.",
            );
        }
    }
    ApplicationError::RemoteProtocol(error.to_string())
}

pub(super) fn map_server_error(dto: ErrorDto, worklog_id: Option<WorklogId>) -> ApplicationError {
    if dto.code == ErrorCode::Internal {
        return ApplicationError::RemoteUnavailable("remote server error".into());
    }
    ApplicationError::semantic_failure(
        error_category(dto.code, worklog_id),
        safe_message(&dto.message),
    )
}

pub(super) fn error_category(
    code: ErrorCode,
    worklog_id: Option<WorklogId>,
) -> ApplicationFailureCategory {
    match code {
        ErrorCode::InvalidRequest | ErrorCode::Conflict => ApplicationFailureCategory::General,
        ErrorCode::NotFound if worklog_id.is_some() => ApplicationFailureCategory::WorklogNotFound,
        ErrorCode::NotFound => ApplicationFailureCategory::TaskNotFound,
        ErrorCode::WorklogChanged => ApplicationFailureCategory::WorklogChanged,
        ErrorCode::WorklogHistoryChanged => ApplicationFailureCategory::WorklogHistoryChanged,
        ErrorCode::WorklogOverlap => ApplicationFailureCategory::WorklogOverlap,
        ErrorCode::ActiveWorklog => ApplicationFailureCategory::ActiveWorklog,
        ErrorCode::ActiveTask => ApplicationFailureCategory::ActiveTask,
        ErrorCode::InactiveTaskCandidatesChanged => {
            ApplicationFailureCategory::InactiveTaskCandidatesChanged
        }
        ErrorCode::Internal => ApplicationFailureCategory::General,
    }
}

pub(super) fn safe_message(message: &str) -> String {
    let cleaned: String = message
        .chars()
        .filter(|character| !character.is_control())
        .take(512)
        .collect();
    if cleaned.is_empty() {
        "Server rejected the request".to_owned()
    } else {
        cleaned
    }
}
