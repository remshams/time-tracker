//! One query for period totals across all tasks.

use chrono::{DateTime, TimeDelta, Utc};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, Transaction, TransactionBehavior};
use tracker_application::{ReportRead, ReportRow, TaskListItem};

use super::{
    SqliteRepository,
    mapping::{task_from_stored, timestamp_to_us},
    tasks::list_task_items_on,
    tracking::active_tracking_read_on,
};
use crate::StorageError;

impl SqliteRepository {
    /// Reads task-list display resources and totals in one SQLite transaction.
    pub fn task_list_report_read(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<(Vec<TaskListItem>, ReportRead), StorageError> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Deferred)?;
        let items = list_task_items_on(&transaction)?;
        let rows = report_rows_on(&transaction, start, end, now)?;
        let tracking = active_tracking_read_on(&transaction)?;
        transaction.commit()?;
        Ok((items, ReportRead { rows, tracking }))
    }

    /// Reads report rows and active tracking in one SQLite read transaction.
    pub fn report_read(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<ReportRead, StorageError> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Deferred)?;
        let rows = report_rows_on(&transaction, start, end, now)?;
        let tracking = active_tracking_read_on(&transaction)?;
        transaction.commit()?;
        Ok(ReportRead { rows, tracking })
    }
}

/// Aggregates all tasks in one query. Archived tasks remain eligible because
/// report intervals belong to worklogs.
fn report_rows_on(
    conn: &Connection,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<Vec<ReportRow>, StorageError> {
    let start_us = timestamp_to_us(start);
    let end_us = timestamp_to_us(end);
    let now_us = timestamp_to_us(now);
    if end_us <= start_us {
        return Ok(Vec::new());
    }
    let mut statement = conn.prepare(
        "SELECT t.id, t.name, t.archived, t.created_at_us, t.updated_at_us,
                    SUM(MIN(COALESCE(w.end_us, ?3), ?2) - MAX(w.start_us, ?1)) AS duration_us
             FROM worklogs AS w
             JOIN tasks AS t ON t.id = w.task_id
             WHERE w.start_us < ?2 AND (w.end_us IS NULL OR w.end_us > ?1)
               AND MIN(COALESCE(w.end_us, ?3), ?2) > MAX(w.start_us, ?1)
             GROUP BY t.id
             ORDER BY t.id",
    )?;
    let mut rows = statement.query(rusqlite::params![start_us, end_us, now_us])?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().map_err(report_query_error)? {
        result.push(report_row(row)?);
    }
    Ok(result)
}

fn report_row(row: &rusqlite::Row<'_>) -> Result<ReportRow, StorageError> {
    let duration_us = match row.get_ref("duration_us")? {
        ValueRef::Integer(value) => value,
        _ => return Err(StorageError::ReportDurationOverflow),
    };
    Ok(ReportRow {
        task: task_from_stored(
            row.get("id")?,
            row.get("name")?,
            row.get("archived")?,
            row.get("created_at_us")?,
            row.get("updated_at_us")?,
        )?,
        duration: TimeDelta::microseconds(duration_us),
    })
}

fn report_query_error(error: rusqlite::Error) -> StorageError {
    match &error {
        rusqlite::Error::SqliteFailure(_, Some(message)) if message == "integer overflow" => {
            StorageError::ReportDurationOverflow
        }
        _ => StorageError::Sql(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflow_mapping_preserves_unrelated_sqlite_errors() {
        let sqlite_failure = |message| {
            rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
                Some(message),
            )
        };
        assert!(matches!(
            report_query_error(sqlite_failure("integer overflow".to_owned())),
            StorageError::ReportDurationOverflow
        ));
        assert!(matches!(
            report_query_error(sqlite_failure(
                "database disk image is malformed".to_owned()
            )),
            StorageError::Sql(_)
        ));
    }
}
