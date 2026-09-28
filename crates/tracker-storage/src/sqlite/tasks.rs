//! Task persistence and task-list queries.

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use tracker_application::{
    InactiveTaskArchive, InactiveTaskPreviewRead, TaskListItem, TrackerSnapshot,
};
use tracker_domain::{Task, TaskId, TaskName};

use super::{
    SqliteRepository, error,
    mapping::{raw_task, raw_task_item, task_from_stored, timestamp_to_us, us_to_timestamp},
    tracking::active_worklog_on,
};
use crate::StorageError;

pub(crate) fn task_by_id_on(conn: &Connection, id: TaskId) -> Result<Option<Task>, StorageError> {
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

/// The task list probes the latest start through the existing task/start
/// index once per task, instead of scanning every historical worklog.
pub(crate) const TASK_ITEMS_SQL: &str =
    "SELECT t.id, t.name, t.archived, t.created_at_us, t.updated_at_us,
            (SELECT MAX(w.start_us) FROM worklogs AS w WHERE w.task_id = t.id) AS latest_start_us
     FROM tasks AS t
     ORDER BY t.id";

pub(crate) fn list_task_items_on(conn: &Connection) -> Result<Vec<TaskListItem>, StorageError> {
    let mut statement = conn.prepare(TASK_ITEMS_SQL)?;
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

const INACTIVE_WINDOW_US: i64 = 14 * 24 * 60 * 60 * 1_000_000;

/// Half-open worklog intervals only overlap the window when they have
/// positive duration. Running worklogs always prevent archiving.
const INACTIVE_TASKS_SQL: &str = "SELECT t.id, t.name, t.archived, t.created_at_us, t.updated_at_us
     FROM tasks AS t
     WHERE t.archived = 0 AND t.created_at_us < ?1
       AND NOT EXISTS (
         SELECT 1 FROM worklogs AS w
         WHERE w.task_id = t.id
           AND (w.end_us IS NULL
                OR (w.start_us < ?2 AND w.end_us > ?1 AND w.end_us > w.start_us))
       )
     ORDER BY t.id";

fn inactive_task_bounds(as_of: DateTime<Utc>) -> Result<(i64, i64), StorageError> {
    let as_of_us = timestamp_to_us(as_of);
    let cutoff_us = as_of_us
        .checked_sub(INACTIVE_WINDOW_US)
        .ok_or(StorageError::InvalidInactiveTaskTime)?;
    Ok((cutoff_us, as_of_us))
}

fn inactive_tasks_on(conn: &Connection, as_of: DateTime<Utc>) -> Result<Vec<Task>, StorageError> {
    let (cutoff_us, as_of_us) = inactive_task_bounds(as_of)?;
    let mut statement = conn.prepare(INACTIVE_TASKS_SQL)?;
    let mut rows = statement.query(rusqlite::params![cutoff_us, as_of_us])?;
    let mut tasks = Vec::new();
    while let Some(row) = rows.next()? {
        let (id, name, archived, created_us, updated_us) = raw_task(row)?;
        tasks.push(task_from_stored(
            id, name, archived, created_us, updated_us,
        )?);
    }
    Ok(tasks)
}

impl SqliteRepository {
    fn task_by_id(&self, id: TaskId) -> Result<Option<Task>, StorageError> {
        task_by_id_on(&self.conn, id)
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
                        task.id().to_string(),
                        task.name().as_str(),
                        task.is_archived(),
                        timestamp_to_us(task.created_at()),
                        timestamp_to_us(task.updated_at()),
                    ],
                )
                .map(|_| ())
                .map_err(|error| error::create_task_error(error, task.id()))?;
        }
        Ok(true)
    }
}

impl SqliteRepository {
    pub fn create_task(&self, task: Task) -> Result<(), StorageError> {
        self.conn
            .execute(
                "INSERT INTO tasks (id, name, archived, created_at_us, updated_at_us)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    task.id().to_string(),
                    task.name().as_str(),
                    task.is_archived(),
                    timestamp_to_us(task.created_at()),
                    timestamp_to_us(task.updated_at()),
                ],
            )
            .map(|_| ())
            .map_err(|error| error::create_task_error(error, task.id()))
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
        list_task_items_on(&self.conn)
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

    /// Reads the current eligible set from one SQLite snapshot.
    pub fn preview_inactive_tasks(
        &self,
        as_of: DateTime<Utc>,
    ) -> Result<InactiveTaskPreviewRead, StorageError> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Deferred)?;
        let tasks = inactive_tasks_on(&transaction, as_of)?;
        let snapshot = TrackerSnapshot {
            task_items: list_task_items_on(&transaction)?,
            active_worklog: active_worklog_on(&transaction)?,
        };
        transaction.commit()?;
        Ok(InactiveTaskPreviewRead { tasks, snapshot })
    }

    /// Rechecks the previewed set and archives all rows under one write lock.
    pub fn archive_inactive_tasks(
        &self,
        expected_ids: &[TaskId],
        as_of: DateTime<Utc>,
    ) -> Result<InactiveTaskArchive, StorageError> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let mut tasks = inactive_tasks_on(&transaction, as_of)?;
        if tasks.iter().map(Task::id).collect::<Vec<_>>() != expected_ids {
            return Err(StorageError::InactiveTaskCandidatesChanged);
        }
        let as_of = us_to_timestamp(timestamp_to_us(as_of))?;
        for task in &mut tasks {
            task.archive(as_of);
            transaction
                .execute(
                    "UPDATE tasks SET archived = TRUE, updated_at_us = ?1 WHERE id = ?2",
                    rusqlite::params![timestamp_to_us(task.updated_at()), task.id().to_string()],
                )
                .map_err(|error| error::archive_task_error(error, task.id()))?;
        }
        let snapshot = TrackerSnapshot {
            task_items: list_task_items_on(&transaction)?,
            active_worklog: active_worklog_on(&transaction)?,
        };
        transaction.commit()?;
        Ok(InactiveTaskArchive { tasks, snapshot })
    }
}
