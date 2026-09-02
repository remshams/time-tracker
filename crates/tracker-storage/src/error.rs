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
    /// A database constraint rejected the write, such as an end time that
    /// precedes the start time.
    #[error("database constraint rejected the write: {0}")]
    Constraint(#[source] rusqlite::Error),
    /// Stored data breaks a domain rule, which points at tampering or a
    /// schema mismatch rather than at the caller.
    #[error("stored data is invalid: {0}")]
    CorruptData(&'static str),
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
            StorageError::TaskNotFound { id } => Self::TaskNotFound { id },
            StorageError::WorklogNotFound { id } => Self::WorklogNotFound { id },
            StorageError::WorklogAlreadyStopped { id } => Self::WorklogAlreadyStopped { id },
            StorageError::WorklogAlreadyExists { id } => Self::WorklogAlreadyExists { id },
            StorageError::TaskAlreadyExists { id } => Self::TaskAlreadyExists { id },
            StorageError::ActiveWorklogExists => Self::ActiveWorklogExists,
            StorageError::TaskArchived { id } => Self::TaskArchived { id },
            StorageError::TaskIsActive { id } => Self::TaskIsActive { id },
            error @ StorageError::Constraint(_) => Self::Constraint {
                message: error.to_string(),
            },
            StorageError::CorruptData(field) => Self::CorruptData { field },
            other => Self::Backend {
                message: other.to_string(),
            },
        }
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
    if is_trigger_violation(&error)
        && failure_message(&error) == Some(crate::migrate::TRIGGER_TASK_ARCHIVED)
    {
        return StorageError::TaskArchived {
            id: worklog.task_id,
        };
    }
    if is_foreign_key_violation(&error) {
        return StorageError::TaskNotFound {
            id: worklog.task_id,
        };
    }
    if is_unique_violation(&error) {
        return match failure_message(&error) {
            Some(message) if message.contains("worklogs.id") => {
                StorageError::WorklogAlreadyExists { id: worklog.id }
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
            StorageError::TaskArchived { id } if id == worklog.task_id
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
            StorageError::TaskNotFound { id } if id == worklog.task_id
        ));
        assert!(matches!(
            insert_worklog_error(
                failure(
                    rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY,
                    Some("UNIQUE constraint failed: worklogs.id")
                ),
                &worklog
            ),
            StorageError::WorklogAlreadyExists { id } if id == worklog.id
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
                latest: 2
            }
            .to_string(),
            "database schema version 9 is newer than this version of Time Tracker supports (latest known: 2)"
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
