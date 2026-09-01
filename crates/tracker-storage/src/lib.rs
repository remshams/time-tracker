//! SQLite persistence for the Time Tracker.
//!
//! [`SqliteRepository`] implements [`TrackerRepository`] from `tracker-core`
//! with a bundled SQLite database. The schema is created and upgraded by
//! migrations, and database-level rules back the domain invariants: at most
//! one active time entry, and no stopped entry whose end precedes its start.
//! This crate depends on `tracker-core` and on SQLite, never the other way
//! around.

mod error;
mod migrate;
mod paths;

use std::path::Path;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Row};
use tracker_core::{EntryId, Task, TaskId, TaskName, TimeEntry, TimeEntryError, TrackerRepository};

pub use error::StorageError;
pub use paths::{app_data_dir, default_database_path, ensure_app_data_dir};

/// Converts a UTC timestamp to stored microseconds.
fn timestamp_to_us(time: DateTime<Utc>) -> i64 {
    time.timestamp_micros()
}

/// Converts stored microseconds back to a UTC timestamp.
fn us_to_timestamp(us: i64) -> Result<DateTime<Utc>, StorageError> {
    DateTime::from_timestamp_micros(us).ok_or(StorageError::CorruptData("timestamp"))
}

fn task_id_from_stored(value: &str) -> Result<TaskId, StorageError> {
    TaskId::from_str(value).map_err(StorageError::InvalidId)
}

fn entry_id_from_stored(value: &str) -> Result<EntryId, StorageError> {
    EntryId::from_str(value).map_err(StorageError::InvalidId)
}

/// Builds a task from stored columns, rejecting values that break domain
/// rules.
fn task_from_stored(id: String, name: String, archived: bool) -> Result<Task, StorageError> {
    Ok(Task {
        id: task_id_from_stored(&id)?,
        name: TaskName::new(&name).map_err(|_| StorageError::CorruptData("task name"))?,
        archived,
    })
}

/// Builds a time entry from stored columns, rejecting values that break
/// domain rules.
fn entry_from_stored(
    id: String,
    task_id: String,
    start_us: i64,
    end_us: Option<i64>,
) -> Result<TimeEntry, StorageError> {
    let end = end_us.map(us_to_timestamp).transpose()?;
    TimeEntry::new(
        entry_id_from_stored(&id)?,
        task_id_from_stored(&task_id)?,
        us_to_timestamp(start_us)?,
        end,
    )
    .map_err(|error| match error {
        TimeEntryError::EndBeforeStart => StorageError::CorruptData("entry interval"),
    })
}

/// Extracts the raw task columns from a row.
fn raw_task(row: &Row<'_>) -> rusqlite::Result<(String, String, bool)> {
    Ok((row.get("id")?, row.get("name")?, row.get("archived")?))
}

/// Extracts the raw time entry columns from a row.
fn raw_entry(row: &Row<'_>) -> rusqlite::Result<(String, String, i64, Option<i64>)> {
    Ok((
        row.get("id")?,
        row.get("task_id")?,
        row.get("start_us")?,
        row.get("end_us")?,
    ))
}

/// A tracker repository backed by a bundled SQLite database.
#[derive(Debug)]
pub struct SqliteRepository {
    conn: Connection,
}

impl SqliteRepository {
    /// Opens the database at the given path, creating the file and running
    /// pending migrations. Parent directories must already exist; use
    /// [`ensure_app_data_dir`] for the standard database location.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let conn = Connection::open(path)?;
        Self::prepare(conn)
    }

    /// Opens a private in-memory database, mainly for tests.
    pub fn open_in_memory() -> Result<Self, StorageError> {
        let conn = Connection::open_in_memory()?;
        Self::prepare(conn)
    }

    fn prepare(conn: Connection) -> Result<Self, StorageError> {
        conn.pragma_update(None, "foreign_keys", true)?;
        migrate::migrate(&conn)?;
        Ok(Self { conn })
    }

    fn task_by_id(&self, id: TaskId) -> Result<Option<Task>, StorageError> {
        let mut statement = self
            .conn
            .prepare("SELECT id, name, archived FROM tasks WHERE id = ?1")?;
        statement
            .query_row([id.to_string()], raw_task)
            .optional()?
            .map(|(id, name, archived)| task_from_stored(id, name, archived))
            .transpose()
    }

    fn entry_by_id(&self, id: EntryId) -> Result<Option<TimeEntry>, StorageError> {
        let mut statement = self
            .conn
            .prepare("SELECT id, task_id, start_us, end_us FROM time_entries WHERE id = ?1")?;
        statement
            .query_row([id.to_string()], raw_entry)
            .optional()?
            .map(|(id, task_id, start_us, end_us)| entry_from_stored(id, task_id, start_us, end_us))
            .transpose()
    }

    /// Gives direct access to the SQLite connection for diagnostics and
    /// schema-level tests.
    ///
    /// This is not part of the [`TrackerRepository`] contract. Callers that
    /// only read or write through the trait methods never need it.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

impl TrackerRepository for SqliteRepository {
    type Error = StorageError;

    fn create_task(&self, task: Task) -> Result<(), Self::Error> {
        self.conn
            .execute(
                "INSERT INTO tasks (id, name, archived) VALUES (?1, ?2, ?3)",
                rusqlite::params![task.id.to_string(), task.name.as_str(), task.archived],
            )
            .map(|_| ())
            .map_err(|error| error::create_task_error(error, task.id))
    }

    fn find_task(&self, id: TaskId) -> Result<Option<Task>, Self::Error> {
        self.task_by_id(id)
    }

    fn list_tasks(&self) -> Result<Vec<Task>, Self::Error> {
        let mut statement = self
            .conn
            .prepare("SELECT id, name, archived FROM tasks ORDER BY id")?;
        let mut rows = statement.query([])?;
        let mut tasks = Vec::new();
        while let Some(row) = rows.next()? {
            let (id, name, archived) = raw_task(row)?;
            tasks.push(task_from_stored(id, name, archived)?);
        }
        Ok(tasks)
    }

    fn rename_task(&self, id: TaskId, name: TaskName) -> Result<Task, Self::Error> {
        let mut statement = self.conn.prepare(
            "UPDATE tasks SET name = ?1 WHERE id = ?2
             RETURNING id, name, archived",
        )?;
        statement
            .query_row([name.as_str().to_owned(), id.to_string()], raw_task)
            .optional()?
            .map(|(id, name, archived)| task_from_stored(id, name, archived))
            .transpose()?
            .ok_or(StorageError::TaskNotFound { id })
    }

    fn archive_task(&self, id: TaskId) -> Result<Task, Self::Error> {
        let mut statement = self.conn.prepare(
            "UPDATE tasks SET archived = TRUE WHERE id = ?1
             RETURNING id, name, archived",
        )?;
        statement
            .query_row([id.to_string()], raw_task)
            .optional()?
            .map(|(id, name, archived)| task_from_stored(id, name, archived))
            .transpose()?
            .ok_or(StorageError::TaskNotFound { id })
    }

    fn insert_entry(&self, entry: &TimeEntry) -> Result<(), Self::Error> {
        let end_us = entry.end.map(timestamp_to_us);
        self.conn
            .execute(
                "INSERT INTO time_entries (id, task_id, start_us, end_us) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    entry.id.to_string(),
                    entry.task_id.to_string(),
                    timestamp_to_us(entry.start),
                    end_us,
                ],
            )
            .map(|_| ())
            .map_err(|error| error::insert_entry_error(error, entry.task_id))
    }

    fn stop_entry(&self, id: EntryId, end: DateTime<Utc>) -> Result<TimeEntry, Self::Error> {
        let mut statement = self.conn.prepare(
            "UPDATE time_entries SET end_us = ?1 WHERE id = ?2 AND end_us IS NULL
             RETURNING id, task_id, start_us, end_us",
        )?;
        match statement
            .query_row(
                rusqlite::params![timestamp_to_us(end), id.to_string()],
                raw_entry,
            )
            .optional()
        {
            Ok(Some(raw)) => Ok(entry_from_stored(raw.0, raw.1, raw.2, raw.3)?),
            Ok(None) => {
                if self.entry_by_id(id)?.is_some() {
                    Err(StorageError::EntryAlreadyStopped { id })
                } else {
                    Err(StorageError::EntryNotFound { id })
                }
            }
            Err(error) => Err(error::classify_write_error(error)),
        }
    }

    fn active_entry(&self) -> Result<Option<TimeEntry>, Self::Error> {
        let mut statement = self.conn.prepare(
            "SELECT id, task_id, start_us, end_us FROM time_entries WHERE end_us IS NULL",
        )?;
        statement
            .query_row([], raw_entry)
            .optional()?
            .map(|(id, task_id, start_us, end_us)| entry_from_stored(id, task_id, start_us, end_us))
            .transpose()
    }

    fn list_entries(&self, task_id: TaskId) -> Result<Vec<TimeEntry>, Self::Error> {
        let mut statement = self.conn.prepare(
            "SELECT id, task_id, start_us, end_us FROM time_entries
             WHERE task_id = ?1
             ORDER BY start_us, id",
        )?;
        let mut rows = statement.query([task_id.to_string()])?;
        let mut entries = Vec::new();
        while let Some(row) = rows.next()? {
            let (id, task_id, start_us, end_us) = raw_entry(row)?;
            entries.push(entry_from_stored(id, task_id, start_us, end_us)?);
        }
        Ok(entries)
    }

    fn switch_entry(
        &self,
        id: EntryId,
        stop_at: DateTime<Utc>,
        next: &TimeEntry,
    ) -> Result<(), Self::Error> {
        let transaction = self.conn.unchecked_transaction()?;
        let stopped = transaction.execute(
            "UPDATE time_entries SET end_us = ?1 WHERE id = ?2 AND end_us IS NULL",
            rusqlite::params![timestamp_to_us(stop_at), id.to_string()],
        );
        match stopped {
            Ok(1) => {}
            Ok(_) => {
                if self.entry_by_id(id)?.is_some() {
                    return Err(StorageError::EntryAlreadyStopped { id });
                }
                return Err(StorageError::EntryNotFound { id });
            }
            Err(error) => return Err(error::classify_write_error(error)),
        }
        let end_us = next.end.map(timestamp_to_us);
        if let Err(error) = transaction.execute(
            "INSERT INTO time_entries (id, task_id, start_us, end_us) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                next.id.to_string(),
                next.task_id.to_string(),
                timestamp_to_us(next.start),
                end_us,
            ],
        ) {
            return Err(error::insert_entry_error(error, next.task_id));
        }
        transaction.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_round_trips_through_microseconds() {
        let time = DateTime::from_timestamp(1_700_000_000, 123_456_000).unwrap();
        assert_eq!(us_to_timestamp(timestamp_to_us(time)).unwrap(), time);
    }

    #[test]
    fn out_of_range_microseconds_are_corrupt_data() {
        assert!(matches!(
            us_to_timestamp(i64::MAX),
            Err(StorageError::CorruptData("timestamp"))
        ));
    }

    #[test]
    fn identifiers_round_trip_through_text() {
        let task_id = TaskId::generate();
        let restored = task_id_from_stored(&task_id.to_string()).unwrap();
        assert_eq!(restored, task_id);
        let entry_id = EntryId::generate();
        let restored = entry_id_from_stored(&entry_id.to_string()).unwrap();
        assert_eq!(restored, entry_id);
        assert!(matches!(
            task_id_from_stored("not-a-uuid"),
            Err(StorageError::InvalidId(_))
        ));
    }

    #[test]
    fn stored_task_values_reject_an_empty_name() {
        let error = task_from_stored(TaskId::generate().to_string(), "   ".to_owned(), false)
            .expect_err("whitespace name is corrupt");
        assert!(matches!(error, StorageError::CorruptData("task name")));
    }

    #[test]
    fn stored_entry_values_reject_a_backwards_interval() {
        let error = entry_from_stored(
            EntryId::generate().to_string(),
            TaskId::generate().to_string(),
            200,
            Some(100),
        )
        .expect_err("end before start is corrupt");
        assert!(matches!(error, StorageError::CorruptData("entry interval")));
    }
}
