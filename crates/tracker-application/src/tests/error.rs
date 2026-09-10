use super::*;

#[test]
fn repository_failures_have_stable_semantic_classifications_and_sanitized_messages() {
    let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
    let worklog_id = WorklogId::from_uuid(uuid::Uuid::from_u128(2));
    let cases = [
        (
            RepositoryError::TaskNotFound { id: task_id },
            ApplicationFailureCategory::General,
            "Task not found",
        ),
        (
            RepositoryError::WorklogNotFound { id: worklog_id },
            ApplicationFailureCategory::WorklogNotFound,
            "Worklog not found",
        ),
        (
            RepositoryError::WorklogAlreadyStopped { id: worklog_id },
            ApplicationFailureCategory::General,
            "Worklog is already stopped",
        ),
        (
            RepositoryError::WorklogChanged { id: worklog_id },
            ApplicationFailureCategory::WorklogChanged,
            "Worklog changed in another client",
        ),
        (
            RepositoryError::WorklogIsActive { id: worklog_id },
            ApplicationFailureCategory::ActiveWorklog,
            "Running worklogs cannot be deleted",
        ),
        (
            RepositoryError::WorklogHistoryChanged { task_id },
            ApplicationFailureCategory::WorklogHistoryChanged,
            "Worklog history changed. Press r to refresh",
        ),
        (
            RepositoryError::SameTaskWorklogOverlap { id: worklog_id },
            ApplicationFailureCategory::WorklogOverlap,
            "The worklog overlaps another worklog",
        ),
        (
            RepositoryError::WorklogAlreadyExists { id: worklog_id },
            ApplicationFailureCategory::General,
            "Worklog already exists",
        ),
        (
            RepositoryError::TaskAlreadyExists { id: task_id },
            ApplicationFailureCategory::General,
            "Task already exists",
        ),
        (
            RepositoryError::ActiveWorklogExists,
            ApplicationFailureCategory::General,
            "Another worklog is active",
        ),
        (
            RepositoryError::TaskArchived { id: task_id },
            ApplicationFailureCategory::General,
            "Task is archived",
        ),
        (
            RepositoryError::TaskIsActive { id: task_id },
            ApplicationFailureCategory::ActiveTask,
            "Task has active work",
        ),
        (
            RepositoryError::Constraint {
                message: "private constraint".to_owned(),
            },
            ApplicationFailureCategory::General,
            "Storage rejected the change",
        ),
        (
            RepositoryError::CorruptData {
                field: "private field",
            },
            ApplicationFailureCategory::General,
            "Stored data is invalid",
        ),
        (
            RepositoryError::Backend {
                message: "private backend".to_owned(),
            },
            ApplicationFailureCategory::General,
            "Storage error",
        ),
    ];

    for (error, category, message) in cases {
        let failure = ApplicationError::Repository(error).failure();
        assert_eq!(failure.category(), category);
        assert_eq!(failure.message(), message);
        assert!(!failure.recovery_failed());
    }
}

#[test]
fn write_and_recovery_classification_sanitizes_both_causes() {
    let error = ApplicationError::WorklogCorrectionRecovery {
        write: RepositoryError::Backend {
            message: "postgres://writer:secret@host/tracker".to_owned(),
        },
        recovery: RepositoryError::CorruptData {
            field: "account token",
        },
    };

    let failure = error.failure();

    assert_eq!(failure.category(), ApplicationFailureCategory::General);
    assert!(failure.recovery_failed());
    assert_eq!(
        failure.recovery_category(),
        Some(ApplicationFailureCategory::General)
    );
    assert_eq!(failure.recovery_message(), Some("Stored data is invalid"));
    assert_eq!(
        failure.message(),
        "Correction failed: Storage error. State recovery failed: Stored data is invalid."
    );
    assert!(!failure.message().contains("secret"));
    assert!(!failure.message().contains("account token"));
}

#[test]
fn active_task_domain_failures_have_the_active_task_category() {
    let id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
    let failure = ApplicationError::Domain(TrackingError::TaskIsActive { id }).failure();

    assert_eq!(failure.category(), ApplicationFailureCategory::ActiveTask);
    assert_eq!(
        failure.message(),
        format!("task {id} is active and cannot be archived")
    );
}

#[test]
fn semantic_factory_errors_keep_diagnostic_identifiers() {
    let id = WorklogId::from_uuid(uuid::Uuid::from_u128(2));

    assert_eq!(
        ApplicationError::worklog_changed(id),
        ApplicationError::Repository(RepositoryError::WorklogChanged { id })
    );
    assert_eq!(
        ApplicationError::correction_overlap_with_recovery_failure(id, "reload failed"),
        ApplicationError::WorklogCorrectionRecovery {
            write: RepositoryError::SameTaskWorklogOverlap { id },
            recovery: RepositoryError::Backend {
                message: "reload failed".to_owned(),
            },
        }
    );
}

#[test]
fn recovery_errors_keep_the_repository_error_as_their_source() {
    use std::error::Error as _;

    let error = ApplicationError::TrackingWrite(RepositoryError::Backend {
        message: "write failed".to_owned(),
    });
    assert!(error.source().is_some());
    let error = ApplicationError::TrackingRecovery(RepositoryError::Backend {
        message: "reload failed".to_owned(),
    });
    assert!(error.source().is_some());
    let error = ApplicationError::TaskRecovery(RepositoryError::Backend {
        message: "task reload failed".to_owned(),
    });
    assert!(error.source().is_some());
    let error = ApplicationError::WorklogCorrectionWrite {
        write: RepositoryError::Backend {
            message: "correction failed".to_owned(),
        },
    };
    assert!(error.source().is_some());
    let error = ApplicationError::WorklogCorrectionRecovery {
        write: RepositoryError::Backend {
            message: "correction failed".to_owned(),
        },
        recovery: RepositoryError::Backend {
            message: "correction reload failed".to_owned(),
        },
    };
    assert!(error.source().is_some());
    let error = ApplicationError::WorklogDeletionWrite {
        write: RepositoryError::Backend {
            message: "deletion failed".to_owned(),
        },
    };
    assert!(error.source().is_some());
    let error = ApplicationError::WorklogDeletionRecovery {
        write: RepositoryError::Backend {
            message: "deletion failed".to_owned(),
        },
        recovery: RepositoryError::Backend {
            message: "deletion reload failed".to_owned(),
        },
    };
    assert!(error.source().is_some());
}
