//! Errors reported by SQLite persistence.

use tracker_application::RepositoryError;
use tracker_domain::{TaskId, WorklogId};

/// Why a storage operation failed.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// No task exists with this identifier.
    #[error("task {id} not found")]
    TaskNotFound { id: TaskId },
    /// No worklog exists with this identifier.
    #[error("worklog {id} not found")]
    WorklogNotFound { id: WorklogId },
    /// The worklog already has an end time and cannot be stopped again.
    #[error("worklog {id} is already stopped")]
    WorklogAlreadyStopped { id: WorklogId },
    /// The stored task or timestamps no longer match the caller's expected values.
    #[error("worklog {id} changed since it was read")]
    WorklogChanged { id: WorklogId },
    /// The worklog is active and cannot be deleted.
    #[error("worklog {id} is active and cannot be deleted")]
    WorklogIsActive { id: WorklogId },
    /// A continuation cursor no longer matches the task's history order.
    #[error("worklog history for task {task_id} changed since this page was read")]
    WorklogHistoryChanged { task_id: TaskId },
    /// A continuation cursor no longer matches the global worklog feed.
    #[error("global worklog history changed since this page was read")]
    GlobalWorklogHistoryChanged,
    /// The proposed interval overlaps another worklog for the same task.
    #[error("worklog {id} overlaps another worklog for the same task")]
    SameTaskWorklogOverlap { id: WorklogId },
    /// A worklog with this identifier is already stored.
    #[error("worklog {id} already exists")]
    WorklogAlreadyExists { id: WorklogId },
    /// A task with this identifier already exists.
    #[error("task {id} already exists")]
    TaskAlreadyExists { id: TaskId },
    /// Another worklog is already active; the database permits only one.
    #[error("another worklog is already active")]
    ActiveWorklogExists,
    /// The task is archived, so it cannot receive worklogs.
    #[error("task {id} is archived and cannot receive worklogs")]
    TaskArchived { id: TaskId },
    /// The task has an active worklog, so it cannot be archived.
    #[error("task {id} has an active worklog and cannot be archived")]
    TaskIsActive { id: TaskId },
    /// The candidate set changed after the caller previewed it.
    #[error("inactive task candidates changed since preview")]
    InactiveTaskCandidatesChanged,
    /// Subtracting the 14-day window exceeded the supported timestamp range.
    #[error("inactive task cutoff is outside the supported timestamp range")]
    InvalidInactiveTaskTime,
    /// A database constraint rejected the write, such as an end time that
    /// precedes the start time.
    #[error("database constraint rejected the write: {0}")]
    Constraint(#[source] rusqlite::Error),
    /// Stored data breaks a domain rule, which points at tampering or a
    /// schema mismatch rather than at the caller.
    #[error("stored data is invalid: {0}")]
    CorruptData(&'static str),
    /// A report interval or sum cannot fit a signed microsecond duration.
    #[error("report duration exceeds the supported range")]
    ReportDurationOverflow,
    /// The database was written by a newer version of Time Tracker; this
    /// version cannot understand its schema.
    #[error(
        "database schema version {found} is newer than this version of \
         Time Tracker supports (latest known: {latest})"
    )]
    DatabaseTooNew {
        /// The schema version stored in the database.
        found: i64,
        /// The newest schema version this build can migrate to.
        latest: i64,
    },
    /// No platform application-data directory could be determined.
    #[error("cannot determine the application data directory")]
    NoDataDir,
    /// The application data directory is not a safe location: it is a
    /// symbolic link, not a directory, or not owned by the current user.
    #[error("unsafe application data directory: {0}")]
    UnsafeDataDir(&'static str),
    /// The database file is not a safe target: it is a symbolic link, not a
    /// regular file, or not owned by the current user.
    #[error("unsafe database file: {0}")]
    UnsafeDatabase(&'static str),
    /// An identifier in the database is not a valid UUID.
    #[error("stored identifier is invalid: {0}")]
    InvalidId(#[source] uuid::Error),
    /// A database error that has no more specific variant.
    #[error("database error: {0}")]
    Sql(#[from] rusqlite::Error),
    /// A filesystem error, such as creating the application data directory.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<StorageError> for RepositoryError {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::WorklogNotFound { id } => Self::WorklogNotFound { id },
            StorageError::WorklogAlreadyStopped { id } => Self::WorklogAlreadyStopped { id },
            StorageError::WorklogChanged { id } => Self::WorklogChanged { id },
            StorageError::WorklogIsActive { id } => Self::WorklogIsActive { id },
            StorageError::WorklogHistoryChanged { task_id } => {
                Self::WorklogHistoryChanged { task_id }
            }
            StorageError::GlobalWorklogHistoryChanged => Self::GlobalWorklogHistoryChanged,
            StorageError::SameTaskWorklogOverlap { id } => Self::SameTaskWorklogOverlap { id },
            StorageError::WorklogAlreadyExists { id } => Self::WorklogAlreadyExists { id },
            StorageError::ActiveWorklogExists => Self::ActiveWorklogExists,
            other => task_or_backend_error(other),
        }
    }
}

fn task_or_backend_error(error: StorageError) -> RepositoryError {
    match error {
        StorageError::TaskNotFound { id } => RepositoryError::TaskNotFound { id },
        StorageError::TaskAlreadyExists { id } => RepositoryError::TaskAlreadyExists { id },
        StorageError::TaskArchived { id } => RepositoryError::TaskArchived { id },
        StorageError::TaskIsActive { id } => RepositoryError::TaskIsActive { id },
        StorageError::InactiveTaskCandidatesChanged => {
            RepositoryError::InactiveTaskCandidatesChanged
        }
        StorageError::InvalidInactiveTaskTime => RepositoryError::Constraint {
            message: "inactive task cutoff is outside the supported timestamp range".to_owned(),
        },
        error @ StorageError::Constraint(_) => RepositoryError::Constraint {
            message: error.to_string(),
        },
        StorageError::CorruptData(field) => RepositoryError::CorruptData { field },
        StorageError::ReportDurationOverflow => RepositoryError::ReportDurationOverflow,
        other => RepositoryError::Backend {
            message: other.to_string(),
        },
    }
}

/// Returns the SQLite extended result code when the error is an SQLite
/// failure.
fn extended_code(error: &rusqlite::Error) -> Option<i32> {
    match error {
        rusqlite::Error::SqliteFailure(failure, _) => Some(failure.extended_code),
        _ => None,
    }
}

/// Returns the SQLite error message when one is attached.
pub(crate) fn failure_message(error: &rusqlite::Error) -> Option<&str> {
    match error {
        rusqlite::Error::SqliteFailure(_, message) => message.as_deref(),
        _ => None,
    }
}

/// Whether the error is a UNIQUE or PRIMARY KEY constraint violation.
pub(crate) fn is_unique_violation(error: &rusqlite::Error) -> bool {
    matches!(
        extended_code(error),
        Some(rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE | rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY)
    )
}

/// Whether the error is a CHECK constraint violation.
pub(crate) fn is_check_violation(error: &rusqlite::Error) -> bool {
    extended_code(error) == Some(rusqlite::ffi::SQLITE_CONSTRAINT_CHECK)
}

/// Whether the error is a foreign key violation.
pub(crate) fn is_foreign_key_violation(error: &rusqlite::Error) -> bool {
    extended_code(error) == Some(rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY)
}

/// Whether the error is a trigger `RAISE(ABORT)` violation.
pub(crate) fn is_trigger_violation(error: &rusqlite::Error) -> bool {
    extended_code(error) == Some(rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER)
}

/// Maps a write error to the most specific storage error.
pub(crate) fn classify_write_error(error: rusqlite::Error) -> StorageError {
    if is_check_violation(&error) {
        StorageError::Constraint(error)
    } else {
        StorageError::Sql(error)
    }
}

/// Builds a constraint error for a repository rule checked alongside a
/// conditional write rather than by a table constraint.
pub(crate) fn constraint(message: &str) -> StorageError {
    StorageError::Constraint(rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
        Some(message.to_owned()),
    ))
}

/// Maps an error from a write to an existing worklog.
pub(crate) fn update_worklog_error(error: rusqlite::Error, id: WorklogId) -> StorageError {
    if is_trigger_violation(&error)
        && failure_message(&error) == Some(crate::migrate::TRIGGER_WORKLOG_OVERLAP)
    {
        StorageError::SameTaskWorklogOverlap { id }
    } else {
        classify_write_error(error)
    }
}

/// Maps an error raised while moving a worklog to another task.
pub(crate) fn move_worklog_error(
    error: rusqlite::Error,
    id: WorklogId,
    destination_task_id: TaskId,
) -> StorageError {
    if is_trigger_violation(&error)
        && failure_message(&error) == Some(crate::migrate::TRIGGER_TASK_ARCHIVED)
    {
        return StorageError::TaskArchived {
            id: destination_task_id,
        };
    }
    if is_foreign_key_violation(&error) {
        return StorageError::TaskNotFound {
            id: destination_task_id,
        };
    }
    update_worklog_error(error, id)
}

/// Maps an error raised while deleting a worklog.
pub(crate) fn delete_worklog_error(error: rusqlite::Error, id: WorklogId) -> StorageError {
    if is_trigger_violation(&error)
        && failure_message(&error) == Some(crate::migrate::TRIGGER_WORKLOG_ACTIVE_DELETE)
    {
        StorageError::WorklogIsActive { id }
    } else {
        classify_write_error(error)
    }
}

/// Maps a `create_task` error, where only uniqueness failures are expected
/// beyond generic database errors.
pub(crate) fn create_task_error(error: rusqlite::Error, id: TaskId) -> StorageError {
    if is_unique_violation(&error) {
        StorageError::TaskAlreadyExists { id }
    } else {
        classify_write_error(error)
    }
}

/// Maps an `insert_worklog` error.
///
/// Duplicate worklog keys, the single-active-worklog rule, the task foreign key,
/// and the archived-task trigger each produce their own violations;
/// everything else stays generic.
pub(crate) fn insert_worklog_error(
    error: rusqlite::Error,
    worklog: &tracker_domain::Worklog,
) -> StorageError {
    if is_trigger_violation(&error) {
        return match failure_message(&error) {
            Some(crate::migrate::TRIGGER_TASK_ARCHIVED) => StorageError::TaskArchived {
                id: worklog.task_id(),
            },
            Some(crate::migrate::TRIGGER_WORKLOG_OVERLAP) => {
                StorageError::SameTaskWorklogOverlap { id: worklog.id() }
            }
            _ => StorageError::Sql(error),
        };
    }
    if is_foreign_key_violation(&error) {
        return StorageError::TaskNotFound {
            id: worklog.task_id(),
        };
    }
    if is_unique_violation(&error) {
        return match failure_message(&error) {
            Some(message) if message.contains("worklogs.id") => {
                StorageError::WorklogAlreadyExists { id: worklog.id() }
            }
            _ => StorageError::ActiveWorklogExists,
        };
    }
    classify_write_error(error)
}

/// Maps an `archive_task` error, where only the active-task trigger is
/// expected beyond generic database errors.
pub(crate) fn archive_task_error(error: rusqlite::Error, id: TaskId) -> StorageError {
    if is_trigger_violation(&error)
        && failure_message(&error) == Some(crate::migrate::TRIGGER_TASK_ACTIVE)
    {
        return StorageError::TaskIsActive { id };
    }
    classify_write_error(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;
    use tracker_domain::Worklog;

    #[test]
    fn constraint_error_keeps_its_source_and_displays_plain_text() {
        let source = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT_CHECK),
            Some("check".to_owned()),
        );
        let error = StorageError::Constraint(source);
        let message = error.to_string();
        assert_eq!(
            message,
            "database constraint rejected the write: check".to_owned()
        );
        assert!(error.source().is_some());
    }

    #[test]
    fn violation_checks_only_match_their_own_code() {
        let check = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT_CHECK),
            None,
        );
        let unique = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE),
            None,
        );
        let primary_key = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY),
            None,
        );
        let foreign = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY),
            None,
        );
        let trigger = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER),
            None,
        );
        let other = rusqlite::Error::InvalidQuery;
        assert!(is_check_violation(&check));
        assert!(!is_check_violation(&unique));
        assert!(is_unique_violation(&unique));
        assert!(is_unique_violation(&primary_key));
        assert!(!is_unique_violation(&foreign));
        assert!(is_foreign_key_violation(&foreign));
        assert!(is_trigger_violation(&trigger));
        assert!(!is_trigger_violation(&foreign));
        assert!(!is_check_violation(&other));
        assert!(!is_unique_violation(&other));
        assert!(!is_trigger_violation(&other));
    }

    #[test]
    fn write_errors_classify_checks_and_keep_the_rest() {
        let check = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT_CHECK),
            None,
        );
        assert!(matches!(
            classify_write_error(check),
            StorageError::Constraint(_)
        ));
        let other = rusqlite::Error::InvalidQuery;
        assert!(matches!(classify_write_error(other), StorageError::Sql(_)));
    }

    fn failure(code: i32, message: Option<&str>) -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(code), message.map(str::to_owned))
    }

    fn worklog(worklog_tag: u32, task_tag: u32) -> Worklog {
        Worklog::begin(
            WorklogId::from_uuid(uuid::Uuid::from_u128(u128::from(worklog_tag))),
            TaskId::from_uuid(uuid::Uuid::from_u128(u128::from(task_tag))),
            chrono::DateTime::from_timestamp(0, 0).unwrap(),
        )
    }

    #[test]
    fn create_task_errors_map_uniqueness_and_keep_the_rest() {
        let id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        assert!(matches!(
            create_task_error(failure(rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY, None), id),
            StorageError::TaskAlreadyExists { id: existing } if existing == id
        ));
        assert!(matches!(
            create_task_error(failure(rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE, None), id),
            StorageError::TaskAlreadyExists { id: existing } if existing == id
        ));
        assert!(matches!(
            create_task_error(rusqlite::Error::InvalidQuery, id),
            StorageError::Sql(_)
        ));
        assert!(matches!(
            create_task_error(failure(rusqlite::ffi::SQLITE_CONSTRAINT_CHECK, None), id),
            StorageError::Constraint(_)
        ));
    }

    #[test]
    fn insert_worklog_errors_map_each_expected_violation() {
        let worklog = worklog(10, 2);
        assert!(matches!(
            insert_worklog_error(
                failure(
                    rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER,
                    Some(crate::migrate::TRIGGER_TASK_ARCHIVED)
                ),
                &worklog
            ),
            StorageError::TaskArchived { id } if id == worklog.task_id()
        ));
        assert!(matches!(
            insert_worklog_error(
                failure(
                    rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER,
                    Some(crate::migrate::TRIGGER_WORKLOG_OVERLAP)
                ),
                &worklog
            ),
            StorageError::SameTaskWorklogOverlap { id } if id == worklog.id()
        ));
        assert!(matches!(
            insert_worklog_error(
                failure(
                    rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER,
                    Some("something else")
                ),
                &worklog
            ),
            StorageError::Sql(_)
        ));
        assert!(matches!(
            insert_worklog_error(
                failure(rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY, None),
                &worklog
            ),
            StorageError::TaskNotFound { id } if id == worklog.task_id()
        ));
        assert!(matches!(
            insert_worklog_error(
                failure(
                    rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY,
                    Some("UNIQUE constraint failed: worklogs.id")
                ),
                &worklog
            ),
            StorageError::WorklogAlreadyExists { id } if id == worklog.id()
        ));
        assert!(matches!(
            insert_worklog_error(
                failure(
                    rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE,
                    Some("UNIQUE constraint failed: index 'worklogs_single_active'")
                ),
                &worklog
            ),
            StorageError::ActiveWorklogExists
        ));
        // A unique violation without a parseable message keeps the
        // single-active-worklog meaning the insert path always had.
        assert!(matches!(
            insert_worklog_error(
                failure(rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE, None),
                &worklog
            ),
            StorageError::ActiveWorklogExists
        ));
        assert!(matches!(
            insert_worklog_error(rusqlite::Error::InvalidQuery, &worklog),
            StorageError::Sql(_)
        ));
        assert!(matches!(
            insert_worklog_error(
                failure(rusqlite::ffi::SQLITE_CONSTRAINT_CHECK, None),
                &worklog
            ),
            StorageError::Constraint(_)
        ));
    }

    #[test]
    fn update_worklog_errors_map_only_the_overlap_trigger() {
        let id = WorklogId::from_uuid(uuid::Uuid::from_u128(2));
        assert!(matches!(
            update_worklog_error(
                failure(
                    rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER,
                    Some(crate::migrate::TRIGGER_WORKLOG_OVERLAP)
                ),
                id
            ),
            StorageError::SameTaskWorklogOverlap { id: existing } if existing == id
        ));
        assert!(matches!(
            update_worklog_error(
                failure(rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER, Some("other")),
                id
            ),
            StorageError::Sql(_)
        ));
        assert!(matches!(
            update_worklog_error(failure(rusqlite::ffi::SQLITE_CONSTRAINT_CHECK, None), id),
            StorageError::Constraint(_)
        ));
    }

    #[test]
    fn delete_worklog_errors_map_only_the_active_delete_trigger() {
        let id = WorklogId::from_uuid(uuid::Uuid::from_u128(2));
        assert!(matches!(
            delete_worklog_error(
                failure(
                    rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER,
                    Some(crate::migrate::TRIGGER_WORKLOG_ACTIVE_DELETE)
                ),
                id
            ),
            StorageError::WorklogIsActive { id: existing } if existing == id
        ));
        assert!(matches!(
            delete_worklog_error(
                failure(rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER, Some("other")),
                id
            ),
            StorageError::Sql(_)
        ));
        assert!(matches!(
            delete_worklog_error(rusqlite::Error::InvalidQuery, id),
            StorageError::Sql(_)
        ));
    }

    #[test]
    fn archive_task_errors_map_the_active_task_trigger() {
        let id = TaskId::from_uuid(uuid::Uuid::from_u128(2));
        assert!(matches!(
            archive_task_error(
                failure(
                    rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER,
                    Some(crate::migrate::TRIGGER_TASK_ACTIVE)
                ),
                id
            ),
            StorageError::TaskIsActive { id: existing } if existing == id
        ));
        assert!(matches!(
            archive_task_error(
                failure(
                    rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER,
                    Some(crate::migrate::TRIGGER_TASK_ARCHIVED)
                ),
                id
            ),
            StorageError::Sql(_)
        ));
        assert!(matches!(
            archive_task_error(rusqlite::Error::InvalidQuery, id),
            StorageError::Sql(_)
        ));
        assert!(matches!(
            archive_task_error(failure(rusqlite::ffi::SQLITE_CONSTRAINT_CHECK, None), id),
            StorageError::Constraint(_)
        ));
    }

    #[test]
    fn storage_errors_map_to_backend_neutral_repository_errors() {
        let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        let worklog_id = WorklogId::from_uuid(uuid::Uuid::from_u128(2));
        assert_eq!(
            RepositoryError::from(StorageError::TaskNotFound { id: task_id }),
            RepositoryError::TaskNotFound { id: task_id }
        );
        assert_eq!(
            RepositoryError::from(StorageError::WorklogNotFound { id: worklog_id }),
            RepositoryError::WorklogNotFound { id: worklog_id }
        );
        assert_eq!(
            RepositoryError::from(StorageError::WorklogAlreadyStopped { id: worklog_id }),
            RepositoryError::WorklogAlreadyStopped { id: worklog_id }
        );
        assert_eq!(
            RepositoryError::from(StorageError::WorklogChanged { id: worklog_id }),
            RepositoryError::WorklogChanged { id: worklog_id }
        );
        assert_eq!(
            RepositoryError::from(StorageError::WorklogIsActive { id: worklog_id }),
            RepositoryError::WorklogIsActive { id: worklog_id }
        );
        assert_eq!(
            RepositoryError::from(StorageError::SameTaskWorklogOverlap { id: worklog_id }),
            RepositoryError::SameTaskWorklogOverlap { id: worklog_id }
        );
        assert_eq!(
            RepositoryError::from(StorageError::WorklogAlreadyExists { id: worklog_id }),
            RepositoryError::WorklogAlreadyExists { id: worklog_id }
        );
        assert_eq!(
            RepositoryError::from(StorageError::TaskAlreadyExists { id: task_id }),
            RepositoryError::TaskAlreadyExists { id: task_id }
        );
        assert_eq!(
            RepositoryError::from(StorageError::ActiveWorklogExists),
            RepositoryError::ActiveWorklogExists
        );
        assert_eq!(
            RepositoryError::from(StorageError::TaskArchived { id: task_id }),
            RepositoryError::TaskArchived { id: task_id }
        );
        assert_eq!(
            RepositoryError::from(StorageError::TaskIsActive { id: task_id }),
            RepositoryError::TaskIsActive { id: task_id }
        );

        let constraint = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT_CHECK),
            Some("check failed".to_owned()),
        );
        assert_eq!(
            RepositoryError::from(StorageError::Constraint(constraint)),
            RepositoryError::Constraint {
                message: "database constraint rejected the write: check failed".to_owned()
            }
        );
        assert_eq!(
            RepositoryError::from(StorageError::CorruptData("worklog")),
            RepositoryError::CorruptData { field: "worklog" }
        );
        assert_eq!(
            RepositoryError::from(StorageError::NoDataDir),
            RepositoryError::Backend {
                message: "cannot determine the application data directory".to_owned()
            }
        );
    }

    #[test]
    fn new_error_variants_display_concise_text() {
        let worklog_id = WorklogId::from_uuid(uuid::Uuid::from_u128(5));
        let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(6));
        assert_eq!(
            StorageError::WorklogAlreadyExists { id: worklog_id }.to_string(),
            format!("worklog {worklog_id} already exists")
        );
        assert_eq!(
            StorageError::TaskArchived { id: task_id }.to_string(),
            format!("task {task_id} is archived and cannot receive worklogs")
        );
        assert_eq!(
            StorageError::TaskIsActive { id: task_id }.to_string(),
            format!("task {task_id} has an active worklog and cannot be archived")
        );
        assert_eq!(
            StorageError::DatabaseTooNew {
                found: 9,
                latest: 1
            }
            .to_string(),
            "database schema version 9 is newer than this version of Time Tracker supports (latest known: 1)"
                .to_owned()
        );
        assert_eq!(
            StorageError::UnsafeDataDir("is a symbolic link").to_string(),
            "unsafe application data directory: is a symbolic link".to_owned()
        );
        assert_eq!(
            StorageError::UnsafeDatabase("is not a regular file").to_string(),
            "unsafe database file: is not a regular file".to_owned()
        );
    }
}
