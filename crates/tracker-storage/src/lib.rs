//! SQLite persistence for the Time Tracker.
//!
//! [`SqliteRepository`] implements the repository ports from
//! `tracker-application` with a bundled SQLite database. The schema is created and upgraded by
//! migrations, and database-level rules back the domain invariants: at most
//! one active worklog, no stopped worklog whose end precedes its start, no
//! worklogs on archived tasks, and no archiving of a task with an active
//! worklog. Task timestamps are stored as strict integer microseconds and
//! `updated_at` never moves backward. This crate depends on the application
//! and domain crates, never the other way around.

mod error;
mod migrate;
mod paths;

use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Row, Transaction, TransactionBehavior};
use tracker_application::{
    RepositoryError, TaskListItem, TaskRepository, TrackingRepository, WorklogRepository,
};
use tracker_domain::{Task, TaskError, TaskId, TaskName, Worklog, WorklogError, WorklogId};

pub use error::StorageError;
pub use paths::{app_data_dir, default_database_path, ensure_app_data_dir};

/// How long a connection waits for a database locked by another process
/// before giving up.
///
/// The bound keeps a stuck peer from hanging the application forever while
/// still letting simultaneous first opens and ordinary cross-process writes
/// complete.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Converts a UTC timestamp to stored microseconds.
pub(crate) fn timestamp_to_us(time: DateTime<Utc>) -> i64 {
    time.timestamp_micros()
}

/// Converts stored microseconds back to a UTC timestamp.
fn us_to_timestamp(us: i64) -> Result<DateTime<Utc>, StorageError> {
    DateTime::from_timestamp_micros(us).ok_or(StorageError::CorruptData("timestamp"))
}

fn task_id_from_stored(value: &str) -> Result<TaskId, StorageError> {
    TaskId::from_str(value).map_err(StorageError::InvalidId)
}

fn worklog_id_from_stored(value: &str) -> Result<WorklogId, StorageError> {
    WorklogId::from_str(value).map_err(StorageError::InvalidId)
}

/// Builds a task from stored columns, rejecting values that break domain
/// rules, including an `updated_at` that precedes `created_at`.
fn task_from_stored(
    id: String,
    name: String,
    archived: bool,
    created_us: i64,
    updated_us: i64,
) -> Result<Task, StorageError> {
    let created_at = us_to_timestamp(created_us)?;
    let updated_at = us_to_timestamp(updated_us)?;
    Task::rehydrate(
        task_id_from_stored(&id)?,
        TaskName::new(&name).map_err(|_| StorageError::CorruptData("task name"))?,
        archived,
        created_at,
        updated_at,
    )
    .map_err(|error| match error {
        TaskError::UpdatedBeforeCreated => StorageError::CorruptData("task timestamps"),
    })
}

/// Builds a worklog from stored columns, rejecting values that break
/// domain rules.
fn worklog_from_stored(
    id: String,
    task_id: String,
    start_us: i64,
    end_us: Option<i64>,
) -> Result<Worklog, StorageError> {
    let end = end_us.map(us_to_timestamp).transpose()?;
    Worklog::new(
        worklog_id_from_stored(&id)?,
        task_id_from_stored(&task_id)?,
        us_to_timestamp(start_us)?,
        end,
    )
    .map_err(|error| match error {
        WorklogError::EndBeforeStart => StorageError::CorruptData("worklog interval"),
    })
}

type RawTask = (String, String, bool, i64, i64);
type RawTaskItem = (RawTask, Option<i64>);

/// Extracts the raw task columns from a row.
fn raw_task(row: &Row<'_>) -> rusqlite::Result<RawTask> {
    Ok((
        row.get("id")?,
        row.get("name")?,
        row.get("archived")?,
        row.get("created_at_us")?,
        row.get("updated_at_us")?,
    ))
}

/// Extracts the raw worklog columns from a row.
fn raw_worklog(row: &Row<'_>) -> rusqlite::Result<(String, String, i64, Option<i64>)> {
    Ok((
        row.get("id")?,
        row.get("task_id")?,
        row.get("start_us")?,
        row.get("end_us")?,
    ))
}

/// Extracts one task-list row: the task columns plus the latest worklog
/// start aggregate.
fn raw_task_item(row: &Row<'_>) -> rusqlite::Result<RawTaskItem> {
    Ok((raw_task(row)?, row.get("latest_start_us")?))
}

fn task_by_id_on(conn: &Connection, id: TaskId) -> Result<Option<Task>, StorageError> {
    let mut statement = conn.prepare(
        "SELECT id, name, archived, created_at_us, updated_at_us FROM tasks WHERE id = ?1",
    )?;
    statement
        .query_row([id.to_string()], raw_task)
        .optional()?
        .map(|(id, name, archived, created_us, updated_us)| {
            task_from_stored(id, name, archived, created_us, updated_us)
        })
        .transpose()
}

/// A tracker repository backed by a bundled SQLite database.
#[derive(Debug)]
pub struct SqliteRepository {
    conn: Connection,
}

impl SqliteRepository {
    /// Opens the database at the given path, creating the file and running
    /// pending migrations.
    ///
    /// A missing file is created with owner-only permissions. An existing
    /// file must be a regular file owned by the current user; a symbolic
    /// link is rejected, and permissions of an owned file are repaired to
    /// owner-only. The file itself is opened with `SQLITE_OPEN_NOFOLLOW`,
    /// so a link swapped in later cannot be followed. Parent directories
    /// must already exist; use [`ensure_app_data_dir`] for the standard
    /// database location.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let path = path.as_ref();
        paths::prepare_database_file(path)?;
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE.union(OpenFlags::SQLITE_OPEN_NOFOLLOW);
        let conn = Connection::open_with_flags(path, flags)?;
        Self::prepare(conn)
    }

    /// Opens a private in-memory database, mainly for tests.
    pub fn open_in_memory() -> Result<Self, StorageError> {
        let conn = Connection::open_in_memory()?;
        Self::prepare(conn)
    }

    fn prepare(conn: Connection) -> Result<Self, StorageError> {
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.pragma_update(None, "foreign_keys", true)?;
        migrate::migrate(&conn)?;
        Ok(Self { conn })
    }

    fn task_by_id(&self, id: TaskId) -> Result<Option<Task>, StorageError> {
        task_by_id_on(&self.conn, id)
    }

    fn worklog_by_id(&self, id: WorklogId) -> Result<Option<Worklog>, StorageError> {
        let mut statement = self
            .conn
            .prepare("SELECT id, task_id, start_us, end_us FROM worklogs WHERE id = ?1")?;
        statement
            .query_row([id.to_string()], raw_worklog)
            .optional()?
            .map(|(id, task_id, start_us, end_us)| {
                worklog_from_stored(id, task_id, start_us, end_us)
            })
            .transpose()
    }

    /// Seeds the given default tasks when the database has no tasks at all.
    ///
    /// Every seeded task shares the given creation timestamp: they are
    /// created together in one transaction, and a shared, ordered value keeps
    /// the seed deterministic. The emptiness check and the inserts run
    /// inside one immediate transaction: two processes calling this at the
    /// same time serialize on the write lock, the second one rechecks and
    /// finds the database no longer empty, and a failure anywhere rolls the
    /// whole seed back.
    ///
    /// Returns whether this call seeded the database.
    pub fn seed_default_tasks(
        &self,
        names: &[TaskName],
        created_at: DateTime<Utc>,
    ) -> Result<bool, StorageError> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let seeded = Self::seed_within(&transaction, names, created_at)?;
        transaction.commit()?;
        Ok(seeded)
    }

    /// The emptiness check and inserts, inside the caller's transaction.
    fn seed_within(
        transaction: &Transaction<'_>,
        names: &[TaskName],
        created_at: DateTime<Utc>,
    ) -> Result<bool, StorageError> {
        let created_at = us_to_timestamp(timestamp_to_us(created_at))?;
        let empty: bool =
            transaction.query_row("SELECT NOT EXISTS (SELECT 1 FROM tasks)", [], |row| {
                row.get(0)
            })?;
        if !empty {
            return Ok(false);
        }
        for name in names {
            let task = Task::create(TaskId::generate(), name.clone(), created_at);
            transaction
                .execute(
                    "INSERT INTO tasks (id, name, archived, created_at_us, updated_at_us)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![
                        task.id.to_string(),
                        task.name().as_str(),
                        task.is_archived(),
                        timestamp_to_us(task.created_at()),
                        timestamp_to_us(task.updated_at()),
                    ],
                )
                .map(|_| ())
                .map_err(|error| error::create_task_error(error, task.id))?;
        }
        Ok(true)
    }

    /// Gives direct access to the SQLite connection for diagnostics and
    /// schema-level tests.
    ///
    /// This is not part of the application repository ports. Callers that
    /// only use those ports never need it.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

impl SqliteRepository {
    pub fn create_task(&self, task: Task) -> Result<(), StorageError> {
        self.conn
            .execute(
                "INSERT INTO tasks (id, name, archived, created_at_us, updated_at_us)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    task.id.to_string(),
                    task.name().as_str(),
                    task.is_archived(),
                    timestamp_to_us(task.created_at()),
                    timestamp_to_us(task.updated_at()),
                ],
            )
            .map(|_| ())
            .map_err(|error| error::create_task_error(error, task.id))
    }

    pub fn find_task(&self, id: TaskId) -> Result<Option<Task>, StorageError> {
        self.task_by_id(id)
    }

    /// Lists every task, ordered by identifier. This plain listing is not
    /// part of the application ports; the port's task-list read model is
    /// [`SqliteRepository::list_task_items`].
    pub fn list_tasks(&self) -> Result<Vec<Task>, StorageError> {
        let mut statement = self.conn.prepare(
            "SELECT id, name, archived, created_at_us, updated_at_us FROM tasks ORDER BY id",
        )?;
        let mut rows = statement.query([])?;
        let mut tasks = Vec::new();
        while let Some(row) = rows.next()? {
            let (id, name, archived, created_us, updated_us) = raw_task(row)?;
            tasks.push(task_from_stored(
                id, name, archived, created_us, updated_us,
            )?);
        }
        Ok(tasks)
    }

    /// Lists every task with its latest worklog start.
    ///
    /// The latest work start is derived as a per-task `MAX(start_us)`
    /// aggregate in this query, backed by the `worklogs_task_start` index;
    /// full worklogs are never loaded for the listing.
    pub fn list_task_items(&self) -> Result<Vec<TaskListItem>, StorageError> {
        let mut statement = self.conn.prepare(
            "SELECT t.id, t.name, t.archived, t.created_at_us, t.updated_at_us, w.latest_start_us
             FROM tasks AS t
             LEFT JOIN (
                 SELECT task_id, MAX(start_us) AS latest_start_us
                 FROM worklogs
                 GROUP BY task_id
             ) AS w ON w.task_id = t.id
             ORDER BY t.id",
        )?;
        let mut rows = statement.query([])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            let ((id, name, archived, created_us, updated_us), latest_us) = raw_task_item(row)?;
            items.push(TaskListItem {
                task: task_from_stored(id, name, archived, created_us, updated_us)?,
                latest_work_start: latest_us.map(us_to_timestamp).transpose()?,
            });
        }
        Ok(items)
    }

    /// Renames a task atomically while preserving concurrent archive state.
    pub fn rename_task(
        &self,
        id: TaskId,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, StorageError> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let mut task = task_by_id_on(&transaction, id)?.ok_or(StorageError::TaskNotFound { id })?;
        let occurred_at = us_to_timestamp(timestamp_to_us(occurred_at))?;
        if task.rename(name, occurred_at) {
            transaction
                .execute(
                    "UPDATE tasks SET name = ?1, updated_at_us = ?2 WHERE id = ?3",
                    rusqlite::params![
                        task.name().as_str(),
                        timestamp_to_us(task.updated_at()),
                        id.to_string(),
                    ],
                )
                .map_err(error::classify_write_error)?;
        }
        transaction.commit()?;
        Ok(task)
    }

    /// Archives a task atomically while preserving concurrent name changes.
    pub fn archive_task(
        &self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, StorageError> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let mut task = task_by_id_on(&transaction, id)?.ok_or(StorageError::TaskNotFound { id })?;
        let occurred_at = us_to_timestamp(timestamp_to_us(occurred_at))?;
        if task.archive(occurred_at)
            && let Err(error) = transaction.execute(
                "UPDATE tasks SET archived = TRUE, updated_at_us = ?1 WHERE id = ?2",
                rusqlite::params![timestamp_to_us(task.updated_at()), id.to_string()],
            )
        {
            return Err(error::archive_task_error(error, id));
        }
        transaction.commit()?;
        Ok(task)
    }

    /// Restores a task atomically without changing its worklogs or name.
    pub fn unarchive_task(
        &self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, StorageError> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let mut task = task_by_id_on(&transaction, id)?.ok_or(StorageError::TaskNotFound { id })?;
        let occurred_at = us_to_timestamp(timestamp_to_us(occurred_at))?;
        if task.restore(occurred_at) {
            transaction
                .execute(
                    "UPDATE tasks SET archived = FALSE, updated_at_us = ?1 WHERE id = ?2",
                    rusqlite::params![timestamp_to_us(task.updated_at()), id.to_string()],
                )
                .map_err(error::classify_write_error)?;
        }
        transaction.commit()?;
        Ok(task)
    }

    pub fn insert_worklog(&self, worklog: &Worklog) -> Result<(), StorageError> {
        let end_us = worklog.end.map(timestamp_to_us);
        self.conn
            .execute(
                "INSERT INTO worklogs (id, task_id, start_us, end_us) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    worklog.id.to_string(),
                    worklog.task_id.to_string(),
                    timestamp_to_us(worklog.start),
                    end_us,
                ],
            )
            .map(|_| ())
            .map_err(|error| error::insert_worklog_error(error, worklog))
    }

    pub fn stop_worklog(&self, id: WorklogId, end: DateTime<Utc>) -> Result<Worklog, StorageError> {
        let mut statement = self.conn.prepare(
            "UPDATE worklogs SET end_us = ?1 WHERE id = ?2 AND end_us IS NULL
             RETURNING id, task_id, start_us, end_us",
        )?;
        match statement
            .query_row(
                rusqlite::params![timestamp_to_us(end), id.to_string()],
                raw_worklog,
            )
            .optional()
        {
            Ok(Some(raw)) => Ok(worklog_from_stored(raw.0, raw.1, raw.2, raw.3)?),
            Ok(None) => {
                if self.worklog_by_id(id)?.is_some() {
                    Err(StorageError::WorklogAlreadyStopped { id })
                } else {
                    Err(StorageError::WorklogNotFound { id })
                }
            }
            Err(error) => Err(error::classify_write_error(error)),
        }
    }

    pub fn active_worklog(&self) -> Result<Option<Worklog>, StorageError> {
        let mut statement = self
            .conn
            .prepare("SELECT id, task_id, start_us, end_us FROM worklogs WHERE end_us IS NULL")?;
        statement
            .query_row([], raw_worklog)
            .optional()?
            .map(|(id, task_id, start_us, end_us)| {
                worklog_from_stored(id, task_id, start_us, end_us)
            })
            .transpose()
    }

    pub fn list_worklogs(&self, task_id: TaskId) -> Result<Vec<Worklog>, StorageError> {
        let mut statement = self.conn.prepare(
            "SELECT id, task_id, start_us, end_us FROM worklogs
             WHERE task_id = ?1
             ORDER BY start_us, id",
        )?;
        let mut rows = statement.query([task_id.to_string()])?;
        let mut worklogs = Vec::new();
        while let Some(row) = rows.next()? {
            let (id, task_id, start_us, end_us) = raw_worklog(row)?;
            worklogs.push(worklog_from_stored(id, task_id, start_us, end_us)?);
        }
        Ok(worklogs)
    }

    pub fn switch_worklog(
        &self,
        id: WorklogId,
        stop_at: DateTime<Utc>,
        next: &Worklog,
    ) -> Result<(), StorageError> {
        let transaction = self.conn.unchecked_transaction()?;
        let stopped = transaction.execute(
            "UPDATE worklogs SET end_us = ?1 WHERE id = ?2 AND end_us IS NULL",
            rusqlite::params![timestamp_to_us(stop_at), id.to_string()],
        );
        match stopped {
            Ok(1) => {}
            Ok(_) => {
                if self.worklog_by_id(id)?.is_some() {
                    return Err(StorageError::WorklogAlreadyStopped { id });
                }
                return Err(StorageError::WorklogNotFound { id });
            }
            Err(error) => return Err(error::classify_write_error(error)),
        }
        let end_us = next.end.map(timestamp_to_us);
        if let Err(error) = transaction.execute(
            "INSERT INTO worklogs (id, task_id, start_us, end_us) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                next.id.to_string(),
                next.task_id.to_string(),
                timestamp_to_us(next.start),
                end_us,
            ],
        ) {
            return Err(error::insert_worklog_error(error, next));
        }
        transaction.commit()?;
        Ok(())
    }
}

impl TaskRepository for SqliteRepository {
    fn create_task(&self, task: Task) -> Result<(), RepositoryError> {
        SqliteRepository::create_task(self, task).map_err(Into::into)
    }

    fn find_task(&self, id: TaskId) -> Result<Option<Task>, RepositoryError> {
        SqliteRepository::find_task(self, id).map_err(Into::into)
    }

    fn list_task_items(&self) -> Result<Vec<TaskListItem>, RepositoryError> {
        SqliteRepository::list_task_items(self).map_err(Into::into)
    }

    fn rename_task(
        &self,
        id: TaskId,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, RepositoryError> {
        SqliteRepository::rename_task(self, id, name, occurred_at).map_err(Into::into)
    }

    fn archive_task(
        &self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, RepositoryError> {
        SqliteRepository::archive_task(self, id, occurred_at).map_err(Into::into)
    }

    fn unarchive_task(
        &self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, RepositoryError> {
        SqliteRepository::unarchive_task(self, id, occurred_at).map_err(Into::into)
    }
}

impl TrackingRepository for SqliteRepository {
    fn insert_worklog(&self, worklog: &Worklog) -> Result<(), RepositoryError> {
        SqliteRepository::insert_worklog(self, worklog).map_err(Into::into)
    }

    fn stop_worklog(&self, id: WorklogId, end: DateTime<Utc>) -> Result<Worklog, RepositoryError> {
        SqliteRepository::stop_worklog(self, id, end).map_err(Into::into)
    }

    fn active_worklog(&self) -> Result<Option<Worklog>, RepositoryError> {
        SqliteRepository::active_worklog(self).map_err(Into::into)
    }

    fn switch_worklog(
        &self,
        id: WorklogId,
        stop_at: DateTime<Utc>,
        next: &Worklog,
    ) -> Result<(), RepositoryError> {
        SqliteRepository::switch_worklog(self, id, stop_at, next).map_err(Into::into)
    }
}

impl WorklogRepository for SqliteRepository {
    fn list_worklogs(&self, task_id: TaskId) -> Result<Vec<Worklog>, RepositoryError> {
        SqliteRepository::list_worklogs(self, task_id).map_err(Into::into)
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
        let worklog_id = WorklogId::generate();
        let restored = worklog_id_from_stored(&worklog_id.to_string()).unwrap();
        assert_eq!(restored, worklog_id);
        assert!(matches!(
            task_id_from_stored("not-a-uuid"),
            Err(StorageError::InvalidId(_))
        ));
    }

    #[test]
    fn stored_task_values_reject_an_empty_name() {
        let error = task_from_stored(
            TaskId::generate().to_string(),
            "   ".to_owned(),
            false,
            0,
            0,
        )
        .expect_err("whitespace name is corrupt");
        assert!(matches!(error, StorageError::CorruptData("task name")));
    }

    #[test]
    fn stored_task_values_reject_control_and_oversized_names() {
        let error = task_from_stored(
            TaskId::generate().to_string(),
            "bad \u{1b} name".to_owned(),
            false,
            0,
            0,
        )
        .expect_err("control characters make a name corrupt");
        assert!(matches!(error, StorageError::CorruptData("task name")));

        let error = task_from_stored(
            TaskId::generate().to_string(),
            "a".repeat(TaskName::MAX_LEN + 1),
            false,
            0,
            0,
        )
        .expect_err("an oversized name is corrupt");
        assert!(matches!(error, StorageError::CorruptData("task name")));
    }

    #[test]
    fn stored_task_values_reject_updated_before_created() {
        let error = task_from_stored(
            TaskId::generate().to_string(),
            "backwards".to_owned(),
            false,
            200,
            199,
        )
        .expect_err("updated before created is corrupt");
        assert!(matches!(
            error,
            StorageError::CorruptData("task timestamps")
        ));
    }

    #[test]
    fn stored_task_values_reject_out_of_range_timestamps() {
        let error = task_from_stored(
            TaskId::generate().to_string(),
            "huge".to_owned(),
            false,
            i64::MAX,
            i64::MAX,
        )
        .expect_err("an unrepresentable timestamp is corrupt");
        assert!(matches!(error, StorageError::CorruptData("timestamp")));
    }

    #[test]
    fn stored_worklog_values_reject_a_backwards_interval() {
        let error = worklog_from_stored(
            WorklogId::generate().to_string(),
            TaskId::generate().to_string(),
            200,
            Some(100),
        )
        .expect_err("end before start is corrupt");
        assert!(matches!(
            error,
            StorageError::CorruptData("worklog interval")
        ));
    }

    #[test]
    fn seeding_an_empty_database_inserts_every_name_with_one_shared_timestamp() {
        let repository = SqliteRepository::open_in_memory().unwrap();
        let names: Vec<TaskName> = ["one", "two"]
            .iter()
            .map(|name| TaskName::new(name).unwrap())
            .collect();
        let created_at = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        assert!(repository.seed_default_tasks(&names, created_at).unwrap());
        let tasks = repository.list_tasks().unwrap();
        let stored: Vec<String> = tasks.iter().map(|task| task.name().to_string()).collect();
        assert_eq!(stored, ["one".to_owned(), "two".to_owned()]);
        for task in &tasks {
            assert_eq!(task.created_at(), created_at);
            assert_eq!(task.updated_at(), created_at);
        }

        // A second call sees a non-empty database and changes nothing.
        assert!(!repository.seed_default_tasks(&names, created_at).unwrap());
        assert_eq!(repository.list_tasks().unwrap().len(), 2);
    }

    #[test]
    fn seeding_canonicalizes_the_timestamp_before_creating_tasks() {
        let repository = SqliteRepository::open_in_memory().unwrap();
        let names = vec![TaskName::new("precise").unwrap()];
        let timestamp = DateTime::from_timestamp(100, 123_456_789).unwrap();

        assert!(repository.seed_default_tasks(&names, timestamp).unwrap());

        let task = repository.list_tasks().unwrap().pop().unwrap();
        let canonical = DateTime::from_timestamp(100, 123_456_000).unwrap();
        assert_eq!(task.created_at(), canonical);
        assert_eq!(task.updated_at(), canonical);
    }

    #[test]
    fn seeding_skips_a_database_that_has_only_archived_tasks() {
        let repository = SqliteRepository::open_in_memory().unwrap();
        let mut task = Task::create(
            TaskId::generate(),
            TaskName::new("mine").unwrap(),
            DateTime::from_timestamp(100, 0).unwrap(),
        );
        assert!(task.archive(DateTime::from_timestamp(100, 0).unwrap()));
        repository.create_task(task).unwrap();
        let names = vec![TaskName::new("default").unwrap()];
        assert!(
            !repository
                .seed_default_tasks(&names, DateTime::from_timestamp(100, 0).unwrap())
                .unwrap()
        );
        assert_eq!(repository.list_tasks().unwrap().len(), 1);
    }

    #[test]
    fn a_failed_seed_leaves_no_partial_tasks() {
        let repository = SqliteRepository::open_in_memory().unwrap();
        // A trigger aborts the insert of the middle name.
        repository
            .connection()
            .execute_batch(
                "CREATE TRIGGER reject_boom BEFORE INSERT ON tasks
                 WHEN NEW.name = 'boom'
                 BEGIN SELECT RAISE(ABORT, 'boom'); END;",
            )
            .unwrap();
        let names: Vec<TaskName> = ["first", "boom", "third"]
            .iter()
            .map(|name| TaskName::new(name).unwrap())
            .collect();
        let error = repository
            .seed_default_tasks(&names, DateTime::from_timestamp(0, 0).unwrap())
            .expect_err("the aborted insert fails the seed");
        assert!(matches!(error, StorageError::Sql(_)));
        assert!(
            repository.list_tasks().unwrap().is_empty(),
            "no partial seed may remain"
        );
        // The connection is usable afterwards: the transaction was rolled
        // back, not left open.
        let fresh = vec![TaskName::new("works").unwrap()];
        assert!(
            repository
                .seed_default_tasks(&fresh, DateTime::from_timestamp(0, 0).unwrap())
                .unwrap()
        );
        assert_eq!(repository.list_tasks().unwrap().len(), 1);
    }

    #[test]
    fn seeding_serializes_between_two_connections() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tracker.db");
        let names: Vec<TaskName> = ["alpha", "beta"]
            .iter()
            .map(|name| TaskName::new(name).unwrap())
            .collect();
        let created_at = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let first = std::thread::spawn({
            let path = path.clone();
            let names = names.clone();
            move || -> Result<bool, StorageError> {
                SqliteRepository::open(&path)?.seed_default_tasks(&names, created_at)
            }
        });
        let second = {
            let path = path.clone();
            let names = names.clone();
            std::thread::spawn(move || -> Result<bool, StorageError> {
                SqliteRepository::open(&path)?.seed_default_tasks(&names, created_at)
            })
        };
        let first = first.join().unwrap().unwrap();
        let second = second.join().unwrap().unwrap();
        assert!(
            first ^ second,
            "exactly one of two concurrent seeds must seed, got {first} and {second}"
        );
        let repository = SqliteRepository::open(&path).unwrap();
        let stored: Vec<String> = repository
            .list_tasks()
            .unwrap()
            .into_iter()
            .map(|task| task.name().to_string())
            .collect();
        assert_eq!(stored, ["alpha".to_owned(), "beta".to_owned()]);
    }

    #[test]
    fn connections_wait_for_each_other_within_the_busy_timeout() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tracker.db");
        let writer = SqliteRepository::open(&path).unwrap();
        writer
            .create_task(Task::create(
                TaskId::generate(),
                TaskName::new("held").unwrap(),
                DateTime::from_timestamp(100, 0).unwrap(),
            ))
            .unwrap();
        // A second connection on the same file can read and write while the
        // first is open; SQLite serializes through the busy timeout.
        let reader = SqliteRepository::open(&path).unwrap();
        assert_eq!(reader.list_tasks().unwrap().len(), 1);
        reader
            .create_task(Task::create(
                TaskId::generate(),
                TaskName::new("next").unwrap(),
                DateTime::from_timestamp(100, 0).unwrap(),
            ))
            .unwrap();
        assert_eq!(writer.list_tasks().unwrap().len(), 2);
    }
}
