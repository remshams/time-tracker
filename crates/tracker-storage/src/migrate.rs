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

/// The message the same-task overlap triggers abort with.
pub(crate) const TRIGGER_WORKLOG_OVERLAP: &str = "same-task worklog overlap";

/// The message the active-worklog deletion trigger aborts with.
pub(crate) const TRIGGER_WORKLOG_ACTIVE_DELETE: &str = "active worklog cannot be deleted";

/// The version 2 tasks table, before history ordering revisions existed.
const TASKS_TABLE_V2: &str = "CREATE TABLE tasks (
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
    "CREATE INDEX worklogs_task_start ON worklogs (task_id, start_us, id, end_us);
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

/// Version 3 triggers that reject same-task overlap with a linear scan.
const WORKLOG_OVERLAP_TRIGGERS_V3: &str = "CREATE TRIGGER worklogs_reject_same_task_overlap_insert
     BEFORE INSERT ON worklogs
     WHEN (NEW.end_us IS NULL OR NEW.end_us > NEW.start_us)
      AND EXISTS (SELECT 1 FROM worklogs AS stored WHERE stored.task_id = NEW.task_id
        AND stored.id <> NEW.id AND (stored.end_us IS NULL OR stored.end_us > stored.start_us)
        AND (stored.end_us IS NULL OR NEW.start_us < stored.end_us)
        AND (NEW.end_us IS NULL OR stored.start_us < NEW.end_us))
     BEGIN SELECT RAISE(ABORT, 'same-task worklog overlap'); END;
     CREATE TRIGGER worklogs_reject_same_task_overlap_update
     BEFORE UPDATE OF task_id, start_us, end_us ON worklogs
     WHEN (NEW.end_us IS NULL OR NEW.end_us > NEW.start_us)
      AND EXISTS (SELECT 1 FROM worklogs AS stored WHERE stored.task_id = NEW.task_id
        AND stored.id <> OLD.id AND (stored.end_us IS NULL OR stored.end_us > stored.start_us)
        AND (stored.end_us IS NULL OR NEW.start_us < stored.end_us)
        AND (NEW.end_us IS NULL OR stored.start_us < NEW.end_us))
     BEGIN SELECT RAISE(ABORT, 'same-task worklog overlap'); END;
     CREATE TRIGGER worklogs_bump_history_order_revision
     AFTER UPDATE OF start_us ON worklogs
     WHEN NEW.start_us <> OLD.start_us
     BEGIN UPDATE tasks SET history_order_revision = history_order_revision + 1 WHERE id = NEW.task_id; END;";

/// Version 4 archived-task and overlap triggers. The overlap check needs only
/// the predecessor because stored nonzero intervals are already disjoint.
const WORKLOG_TRIGGERS_V4: &str = "CREATE TRIGGER worklogs_reject_archived_task
     BEFORE INSERT ON worklogs
     WHEN NEW.task_id IN (SELECT id FROM tasks WHERE archived = 1)
     BEGIN SELECT RAISE(ABORT, 'task is archived'); END;
     CREATE TRIGGER worklogs_reject_archived_task_move
     BEFORE UPDATE OF task_id ON worklogs
     WHEN NEW.task_id <> OLD.task_id
      AND NEW.task_id IN (SELECT id FROM tasks WHERE archived = 1)
     BEGIN SELECT RAISE(ABORT, 'task is archived'); END;
     CREATE TRIGGER worklogs_reject_same_task_overlap_insert
     BEFORE INSERT ON worklogs
     WHEN (NEW.end_us IS NULL OR NEW.end_us > NEW.start_us)
      AND EXISTS (SELECT 1 FROM (SELECT end_us FROM worklogs AS stored
          WHERE stored.task_id = NEW.task_id AND stored.id <> NEW.id
            AND (stored.end_us IS NULL OR stored.end_us > stored.start_us)
            AND (NEW.end_us IS NULL OR stored.start_us < NEW.end_us)
          ORDER BY stored.start_us DESC LIMIT 1) AS predecessor
          WHERE predecessor.end_us IS NULL OR predecessor.end_us > NEW.start_us)
     BEGIN SELECT RAISE(ABORT, 'same-task worklog overlap'); END;
     CREATE TRIGGER worklogs_reject_same_task_overlap_update
     BEFORE UPDATE OF task_id, start_us, end_us ON worklogs
     WHEN (NEW.end_us IS NULL OR NEW.end_us > NEW.start_us)
      AND EXISTS (SELECT 1 FROM (SELECT end_us FROM worklogs AS stored
          WHERE stored.task_id = NEW.task_id AND stored.id <> OLD.id
            AND (stored.end_us IS NULL OR stored.end_us > stored.start_us)
            AND (NEW.end_us IS NULL OR stored.start_us < NEW.end_us)
          ORDER BY stored.start_us DESC LIMIT 1) AS predecessor
          WHERE predecessor.end_us IS NULL OR predecessor.end_us > NEW.start_us)
     BEGIN SELECT RAISE(ABORT, 'same-task worklog overlap'); END;
     CREATE TRIGGER worklogs_bump_history_order_revision
     AFTER UPDATE OF task_id, start_us ON worklogs
     WHEN NEW.task_id <> OLD.task_id OR NEW.start_us <> OLD.start_us
     BEGIN
         UPDATE tasks
         SET history_order_revision = history_order_revision + 1
         WHERE id = OLD.task_id
            OR (NEW.task_id <> OLD.task_id AND id = NEW.task_id);
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
         {TASKS_TABLE_V2}
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

/// Adds database-level same-task overlap enforcement.
///
/// A zero-duration completed interval is excluded before the half-open
/// comparisons. A NULL end has no upper bound. The preflight query uses the
/// same predicates as the triggers and rejects corrupt version 2 data without
/// changing any row.
fn apply_worklog_overlap(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    // Replace the old pagination index before validating. If validation
    // fails, the migration transaction restores the old index with the rest
    // of the schema. The new order serves the window sweep, overlap trigger,
    // and keyset pagination.
    transaction.execute_batch(
        "DROP INDEX worklogs_task_start;
         CREATE INDEX worklogs_task_start ON worklogs (task_id, start_us, id, end_us);",
    )?;

    // The running maximum end of prior nonempty intervals detects every
    // overlap in start order. This avoids the quadratic pairwise self-join.
    let overlaps: bool = transaction.query_row(
        "SELECT EXISTS (
             SELECT 1 FROM (
                 SELECT start_us,
                        MAX(CASE WHEN end_us IS NULL THEN 9223372036854775807
                                 ELSE end_us END) OVER (
                            PARTITION BY task_id
                            ORDER BY start_us, id
                            ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING
                        ) AS prior_end_us
                 FROM worklogs
                 WHERE end_us IS NULL OR end_us > start_us
             )
             WHERE prior_end_us > start_us
         )",
        [],
        |row| row.get(0),
    )?;
    if overlaps {
        return Err(StorageError::CorruptData("same-task worklog overlap"));
    }

    transaction.execute_batch(
        "ALTER TABLE tasks ADD COLUMN history_order_revision INTEGER NOT NULL DEFAULT 0;",
    )?;
    transaction.execute_batch(WORKLOG_OVERLAP_TRIGGERS_V3)?;
    Ok(())
}

/// Replaces version 3's scan-based overlap triggers with predecessor checks
/// backed by a partial covering index. It also rejects direct moves onto an
/// archived task while leaving timestamp-only archived-history corrections valid.
fn apply_indexed_worklog_predecessor(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    transaction.execute_batch(
        "DROP TRIGGER worklogs_reject_archived_task;
         DROP TRIGGER worklogs_reject_same_task_overlap_insert;
         DROP TRIGGER worklogs_reject_same_task_overlap_update;
         DROP TRIGGER worklogs_bump_history_order_revision;
         CREATE INDEX worklogs_nonzero_task_start
             ON worklogs (task_id, start_us, id, end_us)
             WHERE end_us IS NULL OR end_us > start_us;",
    )?;
    transaction.execute_batch(WORKLOG_TRIGGERS_V4)?;
    Ok(())
}

/// Prevents every SQL client from deleting an active worklog. Completed rows
/// remain deletable, and deletion does not change history ordering revisions.
fn apply_active_worklog_delete_guard(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    transaction.execute_batch(
        "CREATE TRIGGER worklogs_reject_active_delete
         BEFORE DELETE ON worklogs
         WHEN OLD.end_us IS NULL
         BEGIN SELECT RAISE(ABORT, 'active worklog cannot be deleted'); END;",
    )?;
    Ok(())
}

/// Tracks writes that can invalidate a global worklog page cursor.
fn apply_global_worklog_revision(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    transaction.execute_batch(
        "CREATE TABLE global_worklog_revision (
             id INTEGER PRIMARY KEY CHECK (id = 1),
             revision INTEGER NOT NULL
         ) STRICT;
         INSERT INTO global_worklog_revision (id, revision) VALUES (1, 0);
         CREATE INDEX worklogs_global_start ON worklogs (start_us DESC, id ASC);
         CREATE TRIGGER worklogs_bump_global_revision_insert
         AFTER INSERT ON worklogs
         BEGIN UPDATE global_worklog_revision SET revision = revision + 1 WHERE id = 1; END;
         CREATE TRIGGER worklogs_bump_global_revision_update
         AFTER UPDATE ON worklogs
         BEGIN UPDATE global_worklog_revision SET revision = revision + 1 WHERE id = 1; END;
         CREATE TRIGGER worklogs_bump_global_revision_delete
         AFTER DELETE ON worklogs
         BEGIN UPDATE global_worklog_revision SET revision = revision + 1 WHERE id = 1; END;",
    )?;
    Ok(())
}

type Migration = fn(&Transaction<'_>) -> Result<(), StorageError>;

/// The migrations in order; index plus one is the version each one produces.
const MIGRATIONS: &[Migration] = &[
    apply_baseline,
    apply_task_timestamps,
    apply_worklog_overlap,
    apply_indexed_worklog_predecessor,
    apply_active_worklog_delete_guard,
    apply_global_worklog_revision,
];

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
        assert_eq!(LATEST_VERSION, 6);
    }

    #[test]
    fn trigger_messages_are_the_ones_the_error_mapping_expects() {
        for script in [BASELINE_V1, WORKLOGS_INDEXES_AND_TRIGGERS] {
            assert!(script.contains(&format!("'{TRIGGER_TASK_ARCHIVED}'")));
            assert!(script.contains(&format!("'{TRIGGER_TASK_ACTIVE}'")));
        }
        assert!(WORKLOG_OVERLAP_TRIGGERS_V3.contains(&format!("'{TRIGGER_WORKLOG_OVERLAP}'")));
        assert!(WORKLOG_TRIGGERS_V4.contains(&format!("'{TRIGGER_WORKLOG_OVERLAP}'")));

        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(BASELINE_V1).unwrap();
        let transaction = connection.unchecked_transaction().unwrap();
        apply_active_worklog_delete_guard(&transaction).unwrap();
        let trigger: String = transaction
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'worklogs_reject_active_delete'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(trigger.contains(&format!("'{TRIGGER_WORKLOG_ACTIVE_DELETE}'")));
    }

    #[test]
    fn the_version_2_schema_has_both_timestamp_columns() {
        assert!(TASKS_TABLE_V2.contains("created_at_us INTEGER NOT NULL"));
        assert!(TASKS_TABLE_V2.contains("updated_at_us INTEGER NOT NULL"));
        assert!(TASKS_TABLE_V2.contains("CHECK (updated_at_us >= created_at_us)"));
        assert!(TASKS_TABLE_V2.contains(") STRICT;"));
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
