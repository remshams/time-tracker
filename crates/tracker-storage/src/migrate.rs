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

use chrono::Utc;
use rusqlite::{Connection, Transaction, TransactionBehavior};

use crate::{error::StorageError, timestamp_to_us};

/// The message the archived-task trigger aborts with. Kept in one place so
/// the trigger text and the error mapping cannot drift apart.
pub(crate) const TRIGGER_TASK_ARCHIVED: &str = "task is archived";

/// The message the active-task archive trigger aborts with.
pub(crate) const TRIGGER_TASK_ACTIVE: &str = "task is active";

/// The tasks table of the current schema.
///
/// Version 2 added the client-created timestamp columns and the invariant
/// that `updated_at` never precedes `created_at`.
const TASKS_TABLE: &str = "CREATE TABLE tasks (
        id TEXT PRIMARY KEY NOT NULL,
        name TEXT NOT NULL,
        archived INTEGER NOT NULL CHECK (archived IN (0, 1)),
        created_at_us INTEGER NOT NULL,
        updated_at_us INTEGER NOT NULL,
        CHECK (updated_at_us >= created_at_us)
    ) STRICT;";

/// The worklogs table of the current schema.
const WORKLOGS_TABLE: &str = "CREATE TABLE worklogs (
        id TEXT PRIMARY KEY NOT NULL,
        task_id TEXT NOT NULL REFERENCES tasks (id),
        start_us INTEGER NOT NULL,
        end_us INTEGER,
        CHECK (end_us IS NULL OR end_us >= start_us)
    ) STRICT;";

/// The indexes and triggers of the current schema.
const WORKLOGS_INDEXES_AND_TRIGGERS: &str =
    "CREATE INDEX worklogs_task_start ON worklogs (task_id, start_us);
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
    END;";

/// The baseline schema, exactly as version 1 created it.
///
/// A fresh database runs this script first and then the version 2 upgrade,
/// so migrated and freshly created databases end up byte-for-byte the same
/// schema.
const BASELINE_V1: &str = "CREATE TABLE tasks (
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
    END;";

/// Creates the version 1 baseline schema.
fn apply_baseline(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    transaction.execute_batch(BASELINE_V1)?;
    Ok(())
}

/// Upgrades a version 1 schema to version 2: task timestamps.
///
/// SQLite cannot add a CHECK constraint with `ALTER TABLE`, so the tasks
/// table is rebuilt. The old tables are renamed first, so the worklogs
/// foreign key follows its parent to the renamed table, and the rebuilt
/// tables are populated before the old ones are dropped: dropping the
/// child first keeps the drop legal with foreign keys enforced. Every
/// version 1 task id, name, archive flag, and worklog survives; each task
/// receives one timestamp captured here, at migration time, so every
/// migrated row shares it.
fn apply_task_timestamps(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    let backfill_us = timestamp_to_us(Utc::now());
    transaction.execute_batch(&format!(
        "ALTER TABLE worklogs RENAME TO worklogs_old;
         ALTER TABLE tasks RENAME TO tasks_old;
         {TASKS_TABLE}
         {WORKLOGS_TABLE}"
    ))?;
    transaction.execute(
        "INSERT INTO tasks (id, name, archived, created_at_us, updated_at_us)
         SELECT id, name, archived, ?1, ?1 FROM tasks_old",
        rusqlite::params![backfill_us],
    )?;
    transaction.execute_batch(&format!(
        "INSERT INTO worklogs (id, task_id, start_us, end_us)
             SELECT id, task_id, start_us, end_us FROM worklogs_old;
         DROP TABLE worklogs_old;
         DROP TABLE tasks_old;
         {WORKLOGS_INDEXES_AND_TRIGGERS}"
    ))?;
    Ok(())
}

type Migration = fn(&Transaction<'_>) -> Result<(), StorageError>;

/// The migrations in order; index plus one is the version each one produces.
const MIGRATIONS: &[Migration] = &[apply_baseline, apply_task_timestamps];

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
    for (index, apply) in MIGRATIONS.iter().enumerate() {
        let version = index as i64 + 1;
        if version <= current {
            continue;
        }
        apply(&transaction)?;
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
        assert_eq!(LATEST_VERSION, 2);
    }

    #[test]
    fn trigger_messages_are_the_ones_the_error_mapping_expects() {
        for script in [BASELINE_V1, WORKLOGS_INDEXES_AND_TRIGGERS] {
            assert!(script.contains(&format!("'{TRIGGER_TASK_ARCHIVED}'")));
            assert!(script.contains(&format!("'{TRIGGER_TASK_ACTIVE}'")));
        }
    }

    #[test]
    fn the_version_2_schema_has_both_timestamp_columns() {
        assert!(TASKS_TABLE.contains("created_at_us INTEGER NOT NULL"));
        assert!(TASKS_TABLE.contains("updated_at_us INTEGER NOT NULL"));
        assert!(TASKS_TABLE.contains("CHECK (updated_at_us >= created_at_us)"));
        assert!(TASKS_TABLE.contains(") STRICT;"));
    }

    #[test]
    fn the_backfill_is_one_migration_time_shared_by_every_row() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(BASELINE_V1).unwrap();
        connection.pragma_update(None, "user_version", 1).unwrap();
        for tag in 1..=3 {
            connection
                .execute(
                    "INSERT INTO tasks (id, name, archived) VALUES (?1, ?2, 0)",
                    rusqlite::params![
                        format!("00000000-0000-7000-8000-00000000000{tag}"),
                        format!("task {tag}"),
                    ],
                )
                .unwrap();
        }
        let before_us = timestamp_to_us(Utc::now());
        migrate(&connection).unwrap();
        let after_us = timestamp_to_us(Utc::now());

        let stamps: Vec<(i64, i64)> = connection
            .prepare("SELECT created_at_us, updated_at_us FROM tasks")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert_eq!(stamps.len(), 3);
        let (first_created, _) = stamps[0];
        for (created, updated) in &stamps {
            assert_eq!(
                *created, first_created,
                "every migrated row shares one value"
            );
            assert_eq!(*updated, first_created, "created_at and updated_at agree");
        }
        assert!(
            first_created >= before_us && first_created <= after_us,
            "the backfill is the migration time, captured once: {first_created} \
             must fall between {before_us} and {after_us}"
        );
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

    #[test]
    fn a_failed_version_2_migration_rolls_back_completely() {
        // A database at version 1 whose target name is already occupied:
        // renaming worklogs to worklogs_old fails, and the whole
        // transaction, including the version bump, must roll back.
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(BASELINE_V1).unwrap();
        connection.pragma_update(None, "user_version", 1).unwrap();
        connection
            .execute_batch("CREATE TABLE worklogs_old (id TEXT)")
            .unwrap();

        let error = migrate(&connection).expect_err("the occupied name must fail");
        assert!(matches!(error, StorageError::Sql(_)));
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 1, "the version did not move");
        let columns: Vec<String> = connection
            .prepare("PRAGMA table_info(tasks)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert_eq!(
            columns,
            vec!["id".to_owned(), "name".to_owned(), "archived".to_owned()],
            "the v1 schema is untouched"
        );
        let worklogs_exists: bool = connection
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'worklogs')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(worklogs_exists, "the rename rolled back");
        let trigger_exists: bool = connection
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'trigger' AND name = 'worklogs_reject_archived_task')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(trigger_exists, "the v1 trigger survived the rollback");
    }
}
