//! Active tracking persistence and tracking snapshots.

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use tracker_application::{ActiveTrackingRead, TaskListItem};
use tracker_domain::{Worklog, WorklogId};

use super::tasks::{list_task_items_on, task_item_by_id_on};
use super::worklogs::worklog_by_id_on;
use super::{
    SqliteRepository, error,
    mapping::{raw_worklog, timestamp_to_us, worklog_from_raw, worklog_from_stored},
};
use crate::StorageError;

pub(crate) fn active_worklog_on(conn: &Connection) -> Result<Option<Worklog>, StorageError> {
    let mut statement =
        conn.prepare("SELECT id, task_id, start_us, end_us FROM worklogs WHERE end_us IS NULL")?;
    statement
        .query_row([], raw_worklog)
        .optional()?
        .map(worklog_from_raw)
        .transpose()
}

pub(crate) fn active_tracking_read_on(
    conn: &Connection,
) -> Result<ActiveTrackingRead, StorageError> {
    let active_worklog = active_worklog_on(conn)?;
    let active_task_item = active_worklog
        .as_ref()
        .map(|active| task_item_by_id_on(conn, active.task_id()))
        .transpose()?
        .flatten();
    Ok(ActiveTrackingRead {
        active_worklog,
        active_task_item,
    })
}

impl SqliteRepository {
    /// Reads task aggregates and global tracking from one SQLite read transaction.
    pub fn load_task_tracking_resources(
        &self,
    ) -> Result<(Vec<TaskListItem>, Option<Worklog>), StorageError> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Deferred)?;
        let task_items = list_task_items_on(&transaction)?;
        let active_worklog = active_worklog_on(&transaction)?;
        transaction.commit()?;
        Ok((task_items, active_worklog))
    }

    pub fn insert_worklog(&self, worklog: &Worklog) -> Result<(), StorageError> {
        let end_us = worklog.end().map(timestamp_to_us);
        self.conn
            .execute(
                "INSERT INTO worklogs (id, task_id, start_us, end_us) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    worklog.id().to_string(),
                    worklog.task_id().to_string(),
                    timestamp_to_us(worklog.start()),
                    end_us,
                ],
            )
            .map(|_| ())
            .map_err(|error| error::insert_worklog_error(error, worklog))
    }

    pub fn stop_worklog(
        &self,
        id: WorklogId,
        expected_start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Worklog, StorageError> {
        let mut statement = self.conn.prepare(
            "UPDATE worklogs SET end_us = ?1
             WHERE id = ?2 AND start_us = ?3 AND end_us IS NULL
             RETURNING id, task_id, start_us, end_us",
        )?;
        match statement
            .query_row(
                rusqlite::params![
                    timestamp_to_us(end),
                    id.to_string(),
                    timestamp_to_us(expected_start)
                ],
                raw_worklog,
            )
            .optional()
        {
            Ok(Some(raw)) => Ok(worklog_from_stored(raw.0, raw.1, raw.2, raw.3)?),
            Ok(None) => match worklog_by_id_on(&self.conn, id)? {
                None => Err(StorageError::WorklogNotFound { id }),
                Some(worklog) if !worklog.is_active() => {
                    Err(StorageError::WorklogAlreadyStopped { id })
                }
                Some(_) => Err(StorageError::WorklogChanged { id }),
            },
            Err(error) => Err(error::classify_write_error(error)),
        }
    }

    pub fn active_worklog(&self) -> Result<Option<Worklog>, StorageError> {
        active_worklog_on(&self.conn)
    }

    pub fn switch_worklog(
        &self,
        id: WorklogId,
        expected_start: DateTime<Utc>,
        stop_at: DateTime<Utc>,
        next: &Worklog,
    ) -> Result<(), StorageError> {
        let transaction = self.conn.unchecked_transaction()?;
        let stopped = transaction.execute(
            "UPDATE worklogs SET end_us = ?1
             WHERE id = ?2 AND start_us = ?3 AND end_us IS NULL",
            rusqlite::params![
                timestamp_to_us(stop_at),
                id.to_string(),
                timestamp_to_us(expected_start)
            ],
        );
        match stopped {
            Ok(1) => {}
            Ok(_) => match worklog_by_id_on(&self.conn, id)? {
                None => return Err(StorageError::WorklogNotFound { id }),
                Some(worklog) if !worklog.is_active() => {
                    return Err(StorageError::WorklogAlreadyStopped { id });
                }
                Some(_) => return Err(StorageError::WorklogChanged { id }),
            },
            Err(error) => return Err(error::classify_write_error(error)),
        }
        let end_us = next.end().map(timestamp_to_us);
        if let Err(error) = transaction.execute(
            "INSERT INTO worklogs (id, task_id, start_us, end_us) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                next.id().to_string(),
                next.task_id().to_string(),
                timestamp_to_us(next.start()),
                end_us,
            ],
        ) {
            return Err(error::insert_worklog_error(error, next));
        }
        transaction.commit()?;
        Ok(())
    }
}
