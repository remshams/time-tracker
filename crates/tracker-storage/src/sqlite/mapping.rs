//! Conversion between SQLite rows and domain values.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use rusqlite::Row;
use tracker_domain::{Task, TaskError, TaskId, TaskName, Worklog, WorklogError, WorklogId};

use crate::StorageError;

/// Converts a UTC timestamp to stored microseconds.
pub(crate) fn timestamp_to_us(time: DateTime<Utc>) -> i64 {
    time.timestamp_micros()
}

/// Converts stored microseconds back to a UTC timestamp.
pub(crate) fn us_to_timestamp(us: i64) -> Result<DateTime<Utc>, StorageError> {
    DateTime::from_timestamp_micros(us).ok_or(StorageError::CorruptData("timestamp"))
}

pub(crate) fn task_id_from_stored(value: &str) -> Result<TaskId, StorageError> {
    TaskId::from_str(value).map_err(StorageError::InvalidId)
}

pub(crate) fn worklog_id_from_stored(value: &str) -> Result<WorklogId, StorageError> {
    WorklogId::from_str(value).map_err(StorageError::InvalidId)
}

/// Builds a task from stored columns, rejecting values that break domain
/// rules, including an `updated_at` that precedes `created_at`.
pub(crate) fn task_from_stored(
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
pub(crate) fn worklog_from_stored(
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

pub(crate) type RawTask = (String, String, bool, i64, i64);
pub(crate) type RawTaskItem = (RawTask, Option<i64>);
/// One worklog's raw stored columns.
pub(crate) type RawWorklog = (String, String, i64, Option<i64>);

/// Extracts the raw task columns from a row.
pub(crate) fn raw_task(row: &Row<'_>) -> rusqlite::Result<RawTask> {
    Ok((
        row.get("id")?,
        row.get("name")?,
        row.get("archived")?,
        row.get("created_at_us")?,
        row.get("updated_at_us")?,
    ))
}

/// Extracts the raw worklog columns from a row.
pub(crate) fn raw_worklog(row: &Row<'_>) -> rusqlite::Result<RawWorklog> {
    Ok((
        row.get("id")?,
        row.get("task_id")?,
        row.get("start_us")?,
        row.get("end_us")?,
    ))
}

/// Builds a worklog from raw stored columns, rejecting domain-rule breaks.
pub(crate) fn worklog_from_raw(raw: RawWorklog) -> Result<Worklog, StorageError> {
    worklog_from_stored(raw.0, raw.1, raw.2, raw.3)
}

/// Extracts one task-list row: the task columns plus the latest worklog
/// start aggregate.
pub(crate) fn raw_task_item(row: &Row<'_>) -> rusqlite::Result<RawTaskItem> {
    Ok((raw_task(row)?, row.get("latest_start_us")?))
}
