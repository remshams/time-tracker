//! Schema migrations.
//!
//! Migrations run in order, each in its own transaction, and the applied
//! version is tracked in the `user_version` pragma. Reopening a migrated
//! database applies nothing.

use rusqlite::Connection;

use crate::error::StorageError;

/// The schema migration scripts, in order. Version 1 is the first script.
const MIGRATIONS: &[&str] = &[
    // Version 1: tasks and time entries.
    //
    // Identifiers are UUIDv7 text; ordering by task id approximates creation
    // order. Timestamps are microseconds since the Unix epoch in UTC. A
    // partial unique index allows at most one active entry (end_us IS NULL)
    // across the whole tracker, and a CHECK constraint rejects stopped
    // entries whose end precedes their start.
    "CREATE TABLE tasks (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        archived INTEGER NOT NULL CHECK (archived IN (0, 1))
    );
    CREATE TABLE time_entries (
        id TEXT PRIMARY KEY,
        task_id TEXT NOT NULL REFERENCES tasks (id),
        start_us INTEGER NOT NULL,
        end_us INTEGER,
        CHECK (end_us IS NULL OR end_us >= start_us)
    );
    CREATE INDEX time_entries_task_start ON time_entries (task_id, start_us);
    CREATE UNIQUE INDEX time_entries_single_active
        ON time_entries (1) WHERE end_us IS NULL;",
];

/// Applies all pending migrations.
pub(crate) fn migrate(conn: &Connection) -> Result<(), StorageError> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    for (index, script) in MIGRATIONS.iter().enumerate() {
        let version = index as i64 + 1;
        if version <= current {
            continue;
        }
        let transaction = conn.unchecked_transaction()?;
        transaction.execute_batch(script)?;
        transaction.pragma_update(None, "user_version", version)?;
        transaction.commit()?;
    }
    Ok(())
}
