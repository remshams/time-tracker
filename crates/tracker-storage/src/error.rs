//! Errors reported by SQLite persistence.

use tracker_core::{EntryId, TaskId};

/// Why a storage operation failed.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// No task exists with this identifier.
    #[error("task {id} not found")]
    TaskNotFound { id: TaskId },
    /// No time entry exists with this identifier.
    #[error("time entry {id} not found")]
    EntryNotFound { id: EntryId },
    /// The entry already has an end time and cannot be stopped again.
    #[error("time entry {id} is already stopped")]
    EntryAlreadyStopped { id: EntryId },
    /// A task with this identifier already exists.
    #[error("task {id} already exists")]
    TaskAlreadyExists { id: TaskId },
    /// Another entry is already active; the database permits only one.
    #[error("another time entry is already active")]
    ActiveEntryExists,
    /// A database constraint rejected the write, such as an end time that
    /// precedes the start time.
    #[error("database constraint rejected the write: {0}")]
    Constraint(#[source] rusqlite::Error),
    /// Stored data breaks a domain rule, which points at tampering or a
    /// schema mismatch rather than at the caller.
    #[error("stored data is invalid: {0}")]
    CorruptData(&'static str),
    /// No platform application-data directory could be determined.
    #[error("cannot determine the application data directory")]
    NoDataDir,
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

/// Returns the SQLite extended result code when the error is an SQLite
/// failure.
fn extended_code(error: &rusqlite::Error) -> Option<i32> {
    match error {
        rusqlite::Error::SqliteFailure(failure, _) => Some(failure.extended_code),
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

/// Maps an `insert_entry` error. The active-entry rule and the task foreign
/// key produce their own violations; everything else stays generic.
pub(crate) fn insert_entry_error(error: rusqlite::Error, task_id: TaskId) -> StorageError {
    if is_unique_violation(&error) {
        StorageError::ActiveEntryExists
    } else if is_foreign_key_violation(&error) {
        StorageError::TaskNotFound { id: task_id }
    } else {
        classify_write_error(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;

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
        let other = rusqlite::Error::InvalidQuery;
        assert!(is_check_violation(&check));
        assert!(!is_check_violation(&unique));
        assert!(is_unique_violation(&unique));
        assert!(is_unique_violation(&primary_key));
        assert!(!is_unique_violation(&foreign));
        assert!(is_foreign_key_violation(&foreign));
        assert!(!is_foreign_key_violation(&other));
        assert!(!is_check_violation(&other));
        assert!(!is_unique_violation(&other));
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

    fn failure(code: i32) -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(code), None)
    }

    #[test]
    fn create_task_errors_map_uniqueness_and_keep_the_rest() {
        let id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        assert!(matches!(
            create_task_error(failure(rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY), id),
            StorageError::TaskAlreadyExists { id: existing } if existing == id
        ));
        assert!(matches!(
            create_task_error(failure(rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE), id),
            StorageError::TaskAlreadyExists { id: existing } if existing == id
        ));
        assert!(matches!(
            create_task_error(rusqlite::Error::InvalidQuery, id),
            StorageError::Sql(_)
        ));
        assert!(matches!(
            create_task_error(failure(rusqlite::ffi::SQLITE_CONSTRAINT_CHECK), id),
            StorageError::Constraint(_)
        ));
    }

    #[test]
    fn insert_entry_errors_map_each_expected_violation() {
        let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(2));
        assert!(matches!(
            insert_entry_error(failure(rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE), task_id),
            StorageError::ActiveEntryExists
        ));
        assert!(matches!(
            insert_entry_error(failure(rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY), task_id),
            StorageError::TaskNotFound { id: existing } if existing == task_id
        ));
        assert!(matches!(
            insert_entry_error(rusqlite::Error::InvalidQuery, task_id),
            StorageError::Sql(_)
        ));
        assert!(matches!(
            insert_entry_error(failure(rusqlite::ffi::SQLITE_CONSTRAINT_CHECK), task_id),
            StorageError::Constraint(_)
        ));
    }
}
