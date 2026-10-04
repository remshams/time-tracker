use tracker_application::{ApplicationError, ApplicationFailureCategory, RepositoryError};
use tracker_domain::{TaskId, WorklogId};
use tracker_remote::{RemoteError, RemoteFailureKind};

use crate::CliError;

#[test]
fn error_classes_keep_input_semantic_storage_and_remote_failures_distinct() {
    let input = CliError::input("invalid data");
    assert_eq!(input.exit_code, 2);
    assert_eq!(input.code, "invalid_input");
    assert_eq!(input.to_string(), "invalid data");
    let storage = CliError::storage("storage failed");
    assert_eq!(storage.exit_code, 4);
    assert_eq!(storage.code, "storage");
    assert_eq!(storage.message, "storage failed");
    let worklog = WorklogId::generate();
    let task = TaskId::generate();
    for (error, code) in [
        (
            ApplicationError::worklog_not_found(worklog),
            "worklog_not_found",
        ),
        (
            ApplicationError::worklog_changed(worklog),
            "worklog_changed",
        ),
        (ApplicationError::active_worklog(worklog), "active_worklog"),
        (
            ApplicationError::worklog_history_changed(task),
            "worklog_history_changed",
        ),
        (
            ApplicationError::worklog_overlap(worklog),
            "worklog_overlap",
        ),
        (
            RepositoryError::TaskIsActive { id: task }.into(),
            "active_task",
        ),
        (
            RepositoryError::InactiveTaskCandidatesChanged.into(),
            "inactive_candidates_changed",
        ),
        (ApplicationError::TrackingStateChanged, "operation_failed"),
    ] {
        let cli = CliError::application(error, None);
        assert_eq!(cli.exit_code, 3);
        assert_eq!(cli.code, code);
        assert!(!cli.recovery_failed);
    }
    let semantic = CliError::application(
        ApplicationError::semantic_failure(ApplicationFailureCategory::WorklogChanged, "stale"),
        Some(RemoteFailureKind::Conflict),
    );
    assert_eq!(semantic.code, "worklog_changed");
    assert_eq!(semantic.exit_code, 3);
    assert_eq!(semantic.message, "stale");
    for remote in [RemoteFailureKind::Unavailable, RemoteFailureKind::Protocol] {
        let cli = CliError::application(
            ApplicationError::storage_failure("secret SQL"),
            Some(remote),
        );
        assert_eq!(cli.code, "remote");
        assert_eq!(cli.exit_code, 5);
        assert!(!cli.message.contains("secret SQL"));
    }
}

#[test]
fn wrapped_storage_failures_and_recovery_failures_preserve_the_failure_flag() {
    let backend = || RepositoryError::Backend {
        message: "private database details".into(),
    };
    for error in [
        ApplicationError::Repository(backend()),
        ApplicationError::TrackingWrite(backend()),
        ApplicationError::TrackingRecovery(backend()),
        ApplicationError::TaskRecovery(backend()),
        ApplicationError::WorklogCorrectionWrite { write: backend() },
        ApplicationError::WorklogMoveWrite { write: backend() },
        ApplicationError::WorklogDeletionWrite { write: backend() },
        ApplicationError::Repository(RepositoryError::CorruptData { field: "timestamp" }),
    ] {
        let cli = CliError::application(error, None);
        assert_eq!(cli.exit_code, 4);
        assert_eq!(cli.code, "storage");
        assert!(!cli.message.contains("private database details"));
    }
    for error in [
        ApplicationError::WorklogCorrectionRecovery {
            write: backend(),
            recovery: backend(),
        },
        ApplicationError::WorklogMoveRecovery {
            write: backend(),
            recovery: backend(),
        },
        ApplicationError::WorklogDeletionRecovery {
            write: backend(),
            recovery: backend(),
        },
    ] {
        let cli = CliError::application(error, None);
        assert!(cli.recovery_failed);
        assert_eq!(cli.exit_code, 4);
    }
    let id = WorklogId::generate();
    let cli = CliError::application(
        ApplicationError::deletion_changed_with_recovery_failure(id, "recovery failed"),
        None,
    );
    assert_eq!(cli.exit_code, 4);
    assert!(cli.recovery_failed);
    assert_eq!(cli.code, "worklog_changed");
    for error in [
        ApplicationError::TrackingRecovery(backend()),
        ApplicationError::TaskRecovery(backend()),
    ] {
        assert!(CliError::application(error, None).recovery_failed);
    }
    let remote = CliError::remote_application(
        ApplicationError::storage_failure("invalid protocol data"),
        None,
    );
    assert_eq!(remote.exit_code, 5);
    assert_eq!(remote.code, "remote");
    let remote_semantic = CliError::remote_application(ApplicationError::worklog_changed(id), None);
    assert_eq!(remote_semantic.exit_code, 3);
    assert_eq!(remote_semantic.code, "worklog_changed");
}

#[test]
fn remote_startup_messages_never_echo_url_credentials_or_response_bodies() {
    for (error, message) in [
        (
            RemoteError::InvalidEndpoint("invalid endpoint"),
            "invalid endpoint",
        ),
        (
            RemoteError::Unavailable("http://secret.internal".into()),
            "Tracker server is unavailable",
        ),
        (
            RemoteError::Protocol("private response body".into()),
            "Tracker server returned invalid data",
        ),
        (
            RemoteError::Http {
                status: reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                body: b"private response body".to_vec(),
            },
            "Tracker server returned HTTP 500 Internal Server Error",
        ),
    ] {
        let cli = CliError::remote(error);
        assert_eq!(cli.exit_code, 5);
        assert_eq!(cli.code, "remote");
        assert_eq!(cli.message, message);
        assert!(!cli.recovery_failed);
    }
}
