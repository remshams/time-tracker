//! HTTP errors and shared request validation.

use axum::Json;
use axum::extract::Query;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::http::StatusCode;
use serde::Serialize;
use tracker_application::{ApplicationError, ApplicationFailureCategory};
use tracker_domain::{TaskId, TaskName, WorklogId};
use tracker_protocol::{ErrorCode, ErrorDto, parse_task_id, parse_worklog_id};

pub(super) type ApiResult<T> = Result<Json<T>, ApiError>;

#[derive(Debug)]
pub(super) struct ApiError {
    pub(super) status: StatusCode,
    pub(super) code: ErrorCode,
    pub(super) message: String,
}

impl ApiError {
    pub(super) fn invalid(message: &str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: ErrorCode::InvalidRequest,
            message: message.to_owned(),
        }
    }

    pub(super) fn conflict(message: &str) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: ErrorCode::Conflict,
            message: message.to_owned(),
        }
    }

    pub(super) fn internal() -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: ErrorCode::Internal,
            message: "Server could not complete the request".to_owned(),
        }
    }
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (
            self.status,
            Json(ErrorDto {
                code: self.code,
                message: self.message,
            }),
        )
            .into_response()
    }
}

impl From<ApplicationError> for ApiError {
    fn from(error: ApplicationError) -> Self {
        let failure = error.failure();
        if matches!(error, ApplicationError::TrackingStateChanged) {
            return Self::conflict(failure.message());
        }
        if failure.recovery_failed()
            || failure.source() != tracker_application::ApplicationFailureSource::Operation
        {
            return Self::internal();
        }
        let (status, code) = failure_status(failure.category());
        Self {
            status,
            code,
            message: failure.message().to_owned(),
        }
    }
}

/// Preserves public HTTP status and protocol codes for each semantic failure category.
fn failure_status(category: ApplicationFailureCategory) -> (StatusCode, ErrorCode) {
    match category {
        ApplicationFailureCategory::WorklogNotFound | ApplicationFailureCategory::TaskNotFound => {
            (StatusCode::NOT_FOUND, ErrorCode::NotFound)
        }
        ApplicationFailureCategory::WorklogChanged => {
            (StatusCode::CONFLICT, ErrorCode::WorklogChanged)
        }
        ApplicationFailureCategory::TaskChanged => (StatusCode::CONFLICT, ErrorCode::Conflict),
        ApplicationFailureCategory::WorklogHistoryChanged => {
            (StatusCode::CONFLICT, ErrorCode::WorklogHistoryChanged)
        }
        ApplicationFailureCategory::WorklogOverlap => {
            (StatusCode::CONFLICT, ErrorCode::WorklogOverlap)
        }
        ApplicationFailureCategory::ActiveWorklog => {
            (StatusCode::CONFLICT, ErrorCode::ActiveWorklog)
        }
        ApplicationFailureCategory::ActiveTask => (StatusCode::CONFLICT, ErrorCode::ActiveTask),
        ApplicationFailureCategory::InactiveTaskCandidatesChanged => (
            StatusCode::CONFLICT,
            ErrorCode::InactiveTaskCandidatesChanged,
        ),
        ApplicationFailureCategory::General => (StatusCode::BAD_REQUEST, ErrorCode::InvalidRequest),
    }
}
pub(super) fn task_id(raw: &str) -> Result<TaskId, ApiError> {
    parse_task_id(raw).map_err(ApiError::invalid)
}

pub(super) fn worklog_id(raw: &str) -> Result<WorklogId, ApiError> {
    parse_worklog_id(raw).map_err(ApiError::invalid)
}

pub(super) fn task_name(raw: &str) -> Result<TaskName, ApiError> {
    TaskName::new(raw).map_err(|error| ApiError::invalid(&error.to_string()))
}

pub(super) fn fingerprint(route: &str, request: &impl Serialize) -> Result<String, ApiError> {
    serde_json::to_string(request)
        .map(|payload| format!("{route}:{payload}"))
        .map_err(|_| ApiError::internal())
}
pub(super) fn query_payload<T>(value: Result<Query<T>, QueryRejection>) -> Result<T, ApiError> {
    value
        .map(|Query(value)| value)
        .map_err(|_| ApiError::invalid("Invalid query parameters"))
}

pub(super) fn payload<T>(value: Result<Json<T>, JsonRejection>) -> Result<T, ApiError> {
    value.map(|Json(value)| value).map_err(|rejection| {
        let mut error = ApiError::invalid("Invalid JSON request");
        if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
            error.status = StatusCode::PAYLOAD_TOO_LARGE;
        }
        error
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracker_application::RepositoryError;

    #[test]
    fn operation_errors_preserve_the_http_status_and_protocol_code() {
        use tracker_application::{ApplicationError, ApplicationFailureCategory as Category};
        use tracker_protocol::ErrorCode;

        let cases = [
            (
                Category::General,
                StatusCode::BAD_REQUEST,
                ErrorCode::InvalidRequest,
            ),
            (
                Category::TaskNotFound,
                StatusCode::NOT_FOUND,
                ErrorCode::NotFound,
            ),
            (
                Category::WorklogNotFound,
                StatusCode::NOT_FOUND,
                ErrorCode::NotFound,
            ),
            (
                Category::WorklogChanged,
                StatusCode::CONFLICT,
                ErrorCode::WorklogChanged,
            ),
            (
                Category::WorklogHistoryChanged,
                StatusCode::CONFLICT,
                ErrorCode::WorklogHistoryChanged,
            ),
            (
                Category::WorklogOverlap,
                StatusCode::CONFLICT,
                ErrorCode::WorklogOverlap,
            ),
            (
                Category::ActiveWorklog,
                StatusCode::CONFLICT,
                ErrorCode::ActiveWorklog,
            ),
            (
                Category::ActiveTask,
                StatusCode::CONFLICT,
                ErrorCode::ActiveTask,
            ),
            (
                Category::InactiveTaskCandidatesChanged,
                StatusCode::CONFLICT,
                ErrorCode::InactiveTaskCandidatesChanged,
            ),
        ];
        for (category, status, code) in cases {
            let error = ApplicationError::semantic_failure(category, "Operation rejected");
            let response = super::ApiError::from(error);
            assert_eq!(response.status, status, "{category:?}");
            assert_eq!(response.code, code, "{category:?}");
            assert_eq!(response.message, "Operation rejected");
        }
    }

    #[test]
    fn backend_and_corrupt_data_errors_map_to_sanitized_internal_errors() {
        let task_id = tracker_domain::TaskId::generate();
        let cases = [
            (
                tracker_application::ApplicationError::Repository(RepositoryError::CorruptData {
                    field: "task name",
                }),
                StatusCode::INTERNAL_SERVER_ERROR,
                tracker_protocol::ErrorCode::Internal,
            ),
            (
                tracker_application::ApplicationError::Repository(RepositoryError::Backend {
                    message: "database password leaked".to_owned(),
                }),
                StatusCode::INTERNAL_SERVER_ERROR,
                tracker_protocol::ErrorCode::Internal,
            ),
            (
                tracker_application::ApplicationError::TrackingWrite(RepositoryError::Backend {
                    message: "tracking storage secret".to_owned(),
                }),
                StatusCode::INTERNAL_SERVER_ERROR,
                tracker_protocol::ErrorCode::Internal,
            ),
            (
                tracker_application::ApplicationError::WorklogCorrectionWrite {
                    write: RepositoryError::Backend {
                        message: "correction secret".into(),
                    },
                },
                StatusCode::INTERNAL_SERVER_ERROR,
                tracker_protocol::ErrorCode::Internal,
            ),
            (
                tracker_application::ApplicationError::WorklogMoveWrite {
                    write: RepositoryError::CorruptData { field: "password" },
                },
                StatusCode::INTERNAL_SERVER_ERROR,
                tracker_protocol::ErrorCode::Internal,
            ),
            (
                tracker_application::ApplicationError::WorklogDeletionWrite {
                    write: RepositoryError::Backend {
                        message: "deletion secret".into(),
                    },
                },
                StatusCode::INTERNAL_SERVER_ERROR,
                tracker_protocol::ErrorCode::Internal,
            ),
            (
                tracker_application::ApplicationError::WorklogDeletionRecovery {
                    write: RepositoryError::WorklogChanged {
                        id: tracker_domain::WorklogId::generate(),
                    },
                    recovery: RepositoryError::Backend {
                        message: "recovery secret".into(),
                    },
                },
                StatusCode::INTERNAL_SERVER_ERROR,
                tracker_protocol::ErrorCode::Internal,
            ),
            (
                tracker_application::ApplicationError::TaskRecovery(
                    RepositoryError::TaskNotFound { id: task_id },
                ),
                StatusCode::INTERNAL_SERVER_ERROR,
                tracker_protocol::ErrorCode::Internal,
            ),
            (
                tracker_application::ApplicationError::Repository(RepositoryError::TaskNotFound {
                    id: task_id,
                }),
                StatusCode::NOT_FOUND,
                tracker_protocol::ErrorCode::NotFound,
            ),
            (
                tracker_application::ApplicationError::semantic_failure(
                    tracker_application::ApplicationFailureCategory::TaskNotFound,
                    "Task not found",
                ),
                StatusCode::NOT_FOUND,
                tracker_protocol::ErrorCode::NotFound,
            ),
            (
                tracker_application::ApplicationError::WorklogMoveWrite {
                    write: RepositoryError::TaskNotFound { id: task_id },
                },
                StatusCode::NOT_FOUND,
                tracker_protocol::ErrorCode::NotFound,
            ),
        ];
        for (error, expected_status, expected_code) in cases {
            let mapped = super::ApiError::from(error);
            assert_eq!(mapped.status, expected_status);
            assert_eq!(mapped.code, expected_code);
            if expected_status.is_server_error() {
                assert_eq!(mapped.message, "Server could not complete the request");
            }
            assert!(!mapped.message.contains("password"));
            assert!(!mapped.message.contains("secret"));
        }
    }
}
