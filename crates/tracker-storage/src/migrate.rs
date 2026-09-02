//! Schema migrations.
//!
//! All pending migrations run inside one `BEGIN IMMEDIATE` transaction that
//! is taken before the applied version is read and held until the commit.
//! Two processes opening the same new database therefore serialize: the
//! second one waits on the write lock, then sees the first one's
//! `user_version` and applies nothing. A failed migration rolls back
//! completely, leaving the version and the schema untouched.
//!
//! A database whose `user_version` is newer than the newest migration here
//! was written by a newer Time Tracker; it is rejected instead of being
//! opened with a half-understood schema.

use rusqlite::{Connection, Transaction, TransactionBehavior};

use crate::error::StorageError;

/// The message the archived-task trigger aborts with. Kept in one place so
/// the trigger text and the error mapping cannot drift apart.
pub(crate) const TRIGGER_TASK_ARCHIVED: &str = "task is archived";

/// The message the active-task archive trigger aborts with.
pub(crate) const TRIGGER_TASK_ACTIVE: &str = "task is active";

/// The baseline schema. There is no production schema to migrate, so tasks,
/// worklogs, indexes, and archive triggers share version 1.
const MIGRATIONS: &[&str] = &["CREATE TABLE tasks (
        id TEXT PRIMARY KEY NOT NULL,
        name TEXT NOT NULL,
        archived INTEGER NOT NULL CHECK (archived IN (0, 1))
    ) STRICT;
    CREATE TABLE worklogs (
        id TEXT PRIMARY KEY NOT NULL,
        task_id TEXT NOT NULL REFERENCES tasks (id),
        start_us INTEGER NOT NULL,
        end_us INTEGER,
        CHECK (end_us IS NULL OR end_us >= start_us)
    ) STRICT;
    CREATE INDEX worklogs_task_start ON worklogs (task_id, start_us);
    CREATE UNIQUE INDEX worklogs_single_active
        ON worklogs (1) WHERE end_us IS NULL;
    CREATE TRIGGER worklogs_reject_archived_task
    BEFORE INSERT ON worklogs
    WHEN NEW.task_id IN (SELECT id FROM tasks WHERE archived = 1)
    BEGIN
        SELECT RAISE(ABORT, 'task is archived');
    END;
    CREATE TRIGGER tasks_reject_archive_while_active
    BEFORE UPDATE OF archived ON tasks
    WHEN NEW.archived = 1
        AND EXISTS (SELECT 1 FROM worklogs
                    WHERE task_id = NEW.id AND end_us IS NULL)
    BEGIN
        SELECT RAISE(ABORT, 'task is active');
    END;"];

/// The newest schema version this build understands.
pub(crate) const LATEST_VERSION: i64 = MIGRATIONS.len() as i64;

/// Applies all pending migrations inside one immediate transaction.
pub(crate) fn migrate(conn: &Connection) -> Result<(), StorageError> {
    // IMMEDIATE takes the write lock before the version is read, so two
    // simultaneous first opens cannot both try to create the schema.
    let transaction = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let current: i64 = transaction.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if current > LATEST_VERSION {
        return Err(StorageError::DatabaseTooNew {
            found: current,
            latest: LATEST_VERSION,
        });
    }
    for (index, script) in MIGRATIONS.iter().enumerate() {
        let version = index as i64 + 1;
        if version <= current {
            continue;
        }
        transaction.execute_batch(script)?;
        transaction.pragma_update(None, "user_version", version)?;
    }
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_version_counts_the_scripts() {
        assert_eq!(LATEST_VERSION, 1);
    }

    #[test]
    fn trigger_messages_are_the_ones_the_error_mapping_expects() {
        assert!(MIGRATIONS[0].contains(&format!("'{TRIGGER_TASK_ARCHIVED}'")));
        assert!(MIGRATIONS[0].contains(&format!("'{TRIGGER_TASK_ACTIVE}'")));
    }

    #[test]
    fn a_failed_baseline_migration_rolls_back_the_partial_schema() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch("CREATE TABLE worklogs (id TEXT)")
            .unwrap();

        let error = migrate(&connection).expect_err("the occupied table name must fail");
        assert!(matches!(error, StorageError::Sql(_)));
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 0);
        let tasks_exists: bool = connection
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'tasks')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let worklogs_exists: bool = connection
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'worklogs')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!tasks_exists);
        assert!(worklogs_exists, "the pre-existing table remains untouched");
    }
}
