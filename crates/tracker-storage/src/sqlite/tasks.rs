//! Task persistence and task-list queries.

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use tracker_application::{InactiveTaskArchive, InactiveTaskPreviewRead, TaskListItem};
use tracker_domain::{InactivityPeriod, Task, TaskId, TaskName};

use super::{
    SqliteRepository, error,
    mapping::{raw_task, raw_task_item, task_from_stored, timestamp_to_us, us_to_timestamp},
    tracking::active_tracking_read_on,
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

/// Reads one task and its latest start without scanning the task catalog.
pub(crate) fn task_item_by_id_on(
    conn: &Connection,
    id: TaskId,
) -> Result<Option<TaskListItem>, StorageError> {
    let mut statement = conn.prepare(
        "SELECT t.id, t.name, t.archived, t.created_at_us, t.updated_at_us,
                (SELECT MAX(w.start_us) FROM worklogs AS w WHERE w.task_id = t.id) AS latest_start_us
         FROM tasks AS t WHERE t.id = ?1",
    )?;
    statement
        .query_row([id.to_string()], raw_task_item)
        .optional()?
        .map(
            |((id, name, archived, created_us, updated_us), latest_us)| {
                Ok(TaskListItem {
                    task: task_from_stored(id, name, archived, created_us, updated_us)?,
                    latest_work_start: latest_us.map(us_to_timestamp).transpose()?,
                })
            },
        )
        .transpose()
}

const DAY_US: i64 = 24 * 60 * 60 * 1_000_000;

/// Running worklogs and positive-duration work after the cutoff prevent
/// archiving, including work recorded after the preview's original time.
const INACTIVE_TASKS_SQL: &str = "SELECT t.id, t.name, t.archived, t.created_at_us, t.updated_at_us
     FROM tasks AS t
     WHERE t.archived = 0 AND t.created_at_us < ?1 AND t.updated_at_us < ?1
       AND NOT EXISTS (
         SELECT 1 FROM worklogs AS w
         WHERE w.task_id = t.id
           AND (w.end_us IS NULL
                OR (w.end_us > ?1 AND w.end_us > w.start_us))
       )
     ORDER BY t.id";

fn inactive_task_cutoff(
    as_of: DateTime<Utc>,
    period: InactivityPeriod,
) -> Result<i64, StorageError> {
    let window_us = i64::from(period.days())
        .checked_mul(DAY_US)
        .ok_or(StorageError::InvalidInactiveTaskTime)?;
    timestamp_to_us(as_of)
        .checked_sub(window_us)
        .ok_or(StorageError::InvalidInactiveTaskTime)
}

fn inactive_tasks_on(
    conn: &Connection,
    as_of: DateTime<Utc>,
    period: InactivityPeriod,
) -> Result<Vec<Task>, StorageError> {
    let cutoff_us = inactive_task_cutoff(as_of, period)?;
    let mut statement = conn.prepare(INACTIVE_TASKS_SQL)?;
    let mut rows = statement.query([cutoff_us])?;
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
        self.preview_inactive_tasks_with_period(as_of, InactivityPeriod::default())
    }

    pub fn preview_inactive_tasks_with_period(
        &self,
        as_of: DateTime<Utc>,
        period: InactivityPeriod,
    ) -> Result<InactiveTaskPreviewRead, StorageError> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Deferred)?;
        let tasks = inactive_tasks_on(&transaction, as_of, period)?;
        let tracking = active_tracking_read_on(&transaction)?;
        transaction.commit()?;
        Ok(InactiveTaskPreviewRead { tasks, tracking })
    }

    /// Rechecks the previewed set and archives all rows under one write lock.
    pub fn archive_inactive_tasks(
        &self,
        expected_ids: &[TaskId],
        as_of: DateTime<Utc>,
    ) -> Result<InactiveTaskArchive, StorageError> {
        self.archive_inactive_tasks_with_period(expected_ids, as_of, InactivityPeriod::default())
    }

    pub fn archive_inactive_tasks_with_period(
        &self,
        expected_ids: &[TaskId],
        as_of: DateTime<Utc>,
        period: InactivityPeriod,
    ) -> Result<InactiveTaskArchive, StorageError> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let mut tasks = inactive_tasks_on(&transaction, as_of, period)?;
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
        let tracking = active_tracking_read_on(&transaction)?;
        transaction.commit()?;
        Ok(InactiveTaskArchive { tasks, tracking })
    }
}
