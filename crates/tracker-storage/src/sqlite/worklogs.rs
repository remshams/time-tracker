//! Worklog lookup, history, move, correction, and deletion persistence.

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use tracker_application::{
    GlobalWorklogCursor, GlobalWorklogPage, WORKLOG_PAGE_SIZE, WorklogCorrection, WorklogCursor,
    WorklogDeletion, WorklogMove, WorklogPage, WorklogPageAdoption,
};
use tracker_domain::{TaskId, Worklog, WorklogId, WorklogTimes};

use super::tasks::task_item_by_id_on;
use super::tracking::{active_tracking_read_on, active_worklog_on};
use super::{
    SqliteRepository, error,
    mapping::{RawWorklog, raw_worklog, timestamp_to_us, us_to_timestamp, worklog_from_raw},
};
use crate::StorageError;

pub(crate) fn worklog_by_id_on(
    conn: &Connection,
    id: WorklogId,
) -> Result<Option<Worklog>, StorageError> {
    let mut statement =
        conn.prepare("SELECT id, task_id, start_us, end_us FROM worklogs WHERE id = ?1")?;
    statement
        .query_row([id.to_string()], raw_worklog)
        .optional()?
        .map(worklog_from_raw)
        .transpose()
}
/// Reads one task's latest work start through `worklogs_task_start`.
fn latest_work_start_on(
    conn: &Connection,
    task_id: TaskId,
) -> Result<Option<DateTime<Utc>>, StorageError> {
    conn.query_row(
        "SELECT MAX(start_us) FROM worklogs WHERE task_id = ?1",
        [task_id.to_string()],
        |row| row.get::<_, Option<i64>>(0),
    )?
    .map(us_to_timestamp)
    .transpose()
}

/// Reads the active worklog and its task's latest work start for clients
/// that adopt both after a worklog read or write.
fn active_adoption_on(
    conn: &Connection,
) -> Result<(Option<Worklog>, Option<DateTime<Utc>>), StorageError> {
    let active_worklog = active_worklog_on(conn)?;
    let active_task_latest_work_start = active_worklog
        .as_ref()
        .map(|active| latest_work_start_on(conn, active.task_id()))
        .transpose()?
        .flatten();
    Ok((active_worklog, active_task_latest_work_start))
}

fn moved_worklog_result(
    transaction: Transaction<'_>,
    id: WorklogId,
    source_task_id: TaskId,
    destination_task_id: TaskId,
) -> Result<WorklogMove, StorageError> {
    let worklog =
        worklog_by_id_on(&transaction, id)?.ok_or(StorageError::WorklogNotFound { id })?;
    let source_task_latest_work_start = latest_work_start_on(&transaction, source_task_id)?;
    let destination_task_latest_work_start =
        latest_work_start_on(&transaction, destination_task_id)?;
    let (active_worklog, active_task_latest_work_start) = active_adoption_on(&transaction)?;
    transaction.commit()?;
    Ok(WorklogMove {
        worklog,
        source_task_latest_work_start,
        destination_task_latest_work_start,
        active_worklog,
        active_task_latest_work_start,
    })
}

fn corrected_worklog_result(
    transaction: Transaction<'_>,
    id: WorklogId,
) -> Result<WorklogCorrection, StorageError> {
    let worklog =
        worklog_by_id_on(&transaction, id)?.ok_or(StorageError::WorklogNotFound { id })?;
    let task_latest_work_start = latest_work_start_on(&transaction, worklog.task_id())?;
    let (active_worklog, active_task_latest_work_start) = active_adoption_on(&transaction)?;
    transaction.commit()?;
    Ok(WorklogCorrection {
        worklog,
        task_latest_work_start,
        active_worklog,
        active_task_latest_work_start,
    })
}

/// Resolves a history cursor to its `(start_us, id)` continuation key.
///
/// With a task scope, an anchor that belongs to another task counts as
/// missing. Returns `None` when the anchor is missing so each caller can
/// report its own history-changed error.
fn cursor_anchor_on(
    conn: &Connection,
    id: WorklogId,
    task_id: Option<TaskId>,
) -> Result<Option<(i64, WorklogId)>, StorageError> {
    let start = conn
        .query_row(
            "SELECT start_us FROM worklogs WHERE id = ?1 AND (?2 IS NULL OR task_id = ?2)",
            rusqlite::params![id.to_string(), task_id.map(|task_id| task_id.to_string())],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    Ok(start.map(|start| (start, id)))
}

fn global_worklog_rows_on(
    conn: &Connection,
    after: Option<(i64, WorklogId)>,
) -> Result<Vec<RawWorklog>, StorageError> {
    let limit = i64::try_from(WORKLOG_PAGE_SIZE + 1).expect("the page size plus one fits i64");
    let rows = match after {
        None => {
            let mut statement = conn.prepare(
                "SELECT id, task_id, start_us, end_us FROM worklogs
                 ORDER BY start_us DESC, id ASC LIMIT ?1",
            )?;
            statement
                .query_map([limit], raw_worklog)?
                .collect::<rusqlite::Result<Vec<RawWorklog>>>()?
        }
        Some((start_us, id)) => {
            let mut statement = conn.prepare(
                "SELECT id, task_id, start_us, end_us FROM worklogs
                 WHERE start_us < ?1 OR (start_us = ?1 AND id > ?2)
                 ORDER BY start_us DESC, id ASC LIMIT ?3",
            )?;
            statement
                .query_map(
                    rusqlite::params![start_us, id.to_string(), limit],
                    raw_worklog,
                )?
                .collect::<rusqlite::Result<Vec<RawWorklog>>>()?
        }
    };
    Ok(rows)
}

fn task_worklog_rows_on(
    conn: &Connection,
    task_id: TaskId,
    after: Option<(i64, WorklogId)>,
) -> Result<Vec<RawWorklog>, StorageError> {
    let limit = i64::try_from(WORKLOG_PAGE_SIZE + 1).expect("the page size plus one fits i64");
    let rows = match after {
        None => {
            let mut statement = conn.prepare(
                "SELECT id, task_id, start_us, end_us FROM worklogs
                 WHERE task_id = ?1
                 ORDER BY start_us DESC, id
                 LIMIT ?2",
            )?;
            statement
                .query_map(rusqlite::params![task_id.to_string(), limit], raw_worklog)?
                .collect::<rusqlite::Result<Vec<RawWorklog>>>()?
        }
        Some((start_us, id)) => {
            let mut statement = conn.prepare(
                "SELECT id, task_id, start_us, end_us FROM worklogs
                 WHERE task_id = ?1
                   AND (start_us < ?2 OR (start_us = ?2 AND id > ?3))
                 ORDER BY start_us DESC, id
                 LIMIT ?4",
            )?;
            statement
                .query_map(
                    rusqlite::params![task_id.to_string(), start_us, id.to_string(), limit],
                    raw_worklog,
                )?
                .collect::<rusqlite::Result<Vec<RawWorklog>>>()?
        }
    };
    Ok(rows)
}

impl SqliteRepository {
    /// Reads a global page, its task metadata, and active tracking together.
    ///
    /// A write to any worklog invalidates an earlier cursor. This makes a
    /// continuation safe when another client changes ordering or page rows.
    pub fn global_worklog_page(
        &self,
        after: Option<&GlobalWorklogCursor>,
    ) -> Result<GlobalWorklogPage, StorageError> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Deferred)?;
        let revision: i64 = transaction.query_row(
            "SELECT revision FROM global_worklog_revision WHERE id = 1",
            [],
            |row| row.get(0),
        )?;
        if after.is_some_and(|cursor| cursor.revision != revision) {
            return Err(StorageError::GlobalWorklogHistoryChanged);
        }
        let anchor = after
            .map(|cursor| {
                cursor_anchor_on(&transaction, cursor.id, None)?
                    .ok_or(StorageError::GlobalWorklogHistoryChanged)
            })
            .transpose()?;
        let raw = global_worklog_rows_on(&transaction, anchor)?;
        let has_next = raw.len() > WORKLOG_PAGE_SIZE;
        let worklogs = raw
            .into_iter()
            .take(WORKLOG_PAGE_SIZE)
            .map(worklog_from_raw)
            .collect::<Result<Vec<_>, _>>()?;
        let next_cursor = has_next.then(|| {
            let last = worklogs.last().expect("a full page has a last worklog");
            GlobalWorklogCursor {
                id: last.id(),
                revision,
            }
        });
        let task_ids = worklogs
            .iter()
            .map(Worklog::task_id)
            .collect::<std::collections::BTreeSet<_>>();
        let task_items = task_ids
            .into_iter()
            .filter_map(|id| task_item_by_id_on(&transaction, id).transpose())
            .collect::<Result<Vec<_>, _>>()?;
        let tracking = active_tracking_read_on(&transaction)?;
        transaction.commit()?;
        Ok(GlobalWorklogPage {
            worklogs,
            task_items,
            tracking: Some(tracking),
            next_cursor,
        })
    }

    pub fn find_worklog(&self, id: WorklogId) -> Result<Option<Worklog>, StorageError> {
        worklog_by_id_on(&self.conn, id)
    }

    /// Moves a worklog when its stored task and timestamps still match the
    /// caller's selected row.
    ///
    /// The immediate transaction holds the write lock through the conditional
    /// update and all returned aggregates. SQLite triggers reject archived
    /// destinations, overlaps, and update both affected history revisions.
    pub fn compare_and_move_worklog(
        &self,
        id: WorklogId,
        expected_source_task_id: TaskId,
        expected: WorklogTimes,
        destination_task_id: TaskId,
    ) -> Result<WorklogMove, StorageError> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let expected_start_us = timestamp_to_us(expected.start());
        let expected_end_us = expected.end().map(timestamp_to_us);
        let update = transaction.execute(
            "UPDATE worklogs
             SET task_id = ?1
             WHERE id = ?2
               AND task_id = ?3
               AND start_us = ?4
               AND end_us IS ?5
               AND task_id <> ?1",
            rusqlite::params![
                destination_task_id.to_string(),
                id.to_string(),
                expected_source_task_id.to_string(),
                expected_start_us,
                expected_end_us,
            ],
        );

        match update {
            Ok(1) => moved_worklog_result(
                transaction,
                id,
                expected_source_task_id,
                destination_task_id,
            ),
            Ok(0) => {
                let current = worklog_by_id_on(&transaction, id)?
                    .ok_or(StorageError::WorklogNotFound { id })?;
                let stored_expected = stored_worklog_times(expected_start_us, expected_end_us)?;
                if current.task_id() != expected_source_task_id
                    || current.times() != stored_expected
                {
                    return Err(StorageError::WorklogChanged { id });
                }
                Err(error::constraint("a worklog must move to a different task"))
            }
            Ok(_) => Err(StorageError::CorruptData("worklog update count")),
            Err(sql_error) => Err(error::move_worklog_error(
                sql_error,
                id,
                destination_task_id,
            )),
        }
    }

    /// Replaces timestamps when the stored pair still matches `expected`.
    ///
    /// The immediate transaction serializes this read-modify-write sequence
    /// with other processes. The conditional UPDATE remains the concurrency
    /// check, while the follow-up read distinguishes a missing row, stale
    /// timestamps, and a forbidden completion-state change.
    pub fn compare_and_set_worklog_times(
        &self,
        id: WorklogId,
        expected: WorklogTimes,
        replacement: WorklogTimes,
    ) -> Result<WorklogCorrection, StorageError> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let expected_start_us = timestamp_to_us(expected.start());
        let expected_end_us = expected.end().map(timestamp_to_us);
        let replacement_start_us = timestamp_to_us(replacement.start());
        let replacement_end_us = replacement.end().map(timestamp_to_us);

        let update = transaction.execute(
            "UPDATE worklogs
             SET start_us = ?1, end_us = ?2
             WHERE id = ?3
               AND start_us = ?4
               AND end_us IS ?5
               AND ((end_us IS NULL AND ?2 IS NULL)
                    OR (end_us IS NOT NULL AND ?2 IS NOT NULL))",
            rusqlite::params![
                replacement_start_us,
                replacement_end_us,
                id.to_string(),
                expected_start_us,
                expected_end_us,
            ],
        );

        match update {
            Ok(1) => corrected_worklog_result(transaction, id),
            Ok(0) => {
                let current = worklog_by_id_on(&transaction, id)?
                    .ok_or(StorageError::WorklogNotFound { id })?;
                if current.times() != stored_worklog_times(expected_start_us, expected_end_us)? {
                    return Err(StorageError::WorklogChanged { id });
                }
                Err(error::constraint(
                    "a worklog correction must preserve completion state",
                ))
            }
            Ok(_) => Err(StorageError::CorruptData("worklog update count")),
            Err(sql_error) => Err(error::update_worklog_error(sql_error, id)),
        }
    }

    /// Deletes a completed worklog only when every selected value still
    /// matches, then reads the affected task's latest-work aggregate before
    /// committing.
    pub fn compare_and_delete_completed_worklog(
        &self,
        id: WorklogId,
        expected_task_id: TaskId,
        expected: WorklogTimes,
    ) -> Result<WorklogDeletion, StorageError> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let deleted_raw = {
            let mut statement = transaction.prepare(
                "DELETE FROM worklogs
                 WHERE id = ?1
                   AND task_id = ?2
                   AND start_us = ?3
                   AND end_us = ?4
                   AND end_us IS NOT NULL
                 RETURNING id, task_id, start_us, end_us",
            )?;
            match statement
                .query_row(
                    rusqlite::params![
                        id.to_string(),
                        expected_task_id.to_string(),
                        timestamp_to_us(expected.start()),
                        expected.end().map(timestamp_to_us),
                    ],
                    raw_worklog,
                )
                .optional()
            {
                Ok(raw) => raw,
                Err(error) => return Err(error::delete_worklog_error(error, id)),
            }
        };

        let Some(raw) = deleted_raw else {
            return Err(deletion_conflict_on(&transaction, id)?);
        };
        let worklog = worklog_from_raw(raw)?;
        let task_latest_work_start = latest_work_start_on(&transaction, worklog.task_id())?;
        transaction.commit()?;
        Ok(WorklogDeletion {
            worklog,
            task_latest_work_start,
        })
    }

    /// Reads one bounded page of the task's worklog history.
    ///
    /// The cursor identifies the previous page's last row. Its start is read
    /// in the same transaction as the revision check and page query, then the
    /// query continues in `start_us DESC, id ASC` order. A missing anchor or
    /// one outside this task requires a fresh first page. Active worklogs are
    /// included. One extra row determines whether another page follows.
    pub fn worklog_page(
        &self,
        task_id: TaskId,
        after: Option<&WorklogCursor>,
    ) -> Result<WorklogPage, StorageError> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Deferred)?;
        let revision = transaction
            .query_row(
                "SELECT history_order_revision FROM tasks WHERE id = ?1",
                [task_id.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0);
        if after.is_some_and(|cursor| cursor.revision != revision) {
            return Err(StorageError::WorklogHistoryChanged { task_id });
        }
        let anchor = after
            .map(|cursor| {
                cursor_anchor_on(&transaction, cursor.id, Some(task_id))?
                    .ok_or(StorageError::WorklogHistoryChanged { task_id })
            })
            .transpose()?;
        let raw = task_worklog_rows_on(&transaction, task_id, anchor)?;
        let has_next = raw.len() > WORKLOG_PAGE_SIZE;
        let worklogs = raw
            .into_iter()
            .take(WORKLOG_PAGE_SIZE)
            .map(worklog_from_raw)
            .collect::<Result<Vec<_>, _>>()?;
        let next_cursor = has_next.then(|| {
            let last = worklogs.last().expect("a full page has a last worklog");
            WorklogCursor {
                id: last.id(),
                revision,
            }
        });
        let requested_task_latest_work_start = latest_work_start_on(&transaction, task_id)?;
        let (active_worklog, active_task_latest_work_start) = active_adoption_on(&transaction)?;
        transaction.commit()?;
        Ok(WorklogPage {
            worklogs,
            adoption: Some(WorklogPageAdoption {
                requested_task_latest_work_start,
                active_worklog,
                active_task_latest_work_start,
            }),
            next_cursor,
        })
    }

    /// Lists every worklog of the task, oldest first.
    ///
    /// This test-support query is not part of the application repository
    /// ports. Application history reads bounded pages instead.
    #[cfg(any(test, feature = "test-support"))]
    pub fn list_worklogs(&self, task_id: TaskId) -> Result<Vec<Worklog>, StorageError> {
        let mut statement = self.conn.prepare(
            "SELECT id, task_id, start_us, end_us FROM worklogs
             WHERE task_id = ?1
             ORDER BY start_us, id",
        )?;
        let mut rows = statement.query([task_id.to_string()])?;
        let mut worklogs = Vec::new();
        while let Some(row) = rows.next()? {
            let raw = raw_worklog(row)?;
            worklogs.push(worklog_from_raw(raw)?);
        }
        Ok(worklogs)
    }
}

fn stored_worklog_times(start_us: i64, end_us: Option<i64>) -> Result<WorklogTimes, StorageError> {
    Ok(WorklogTimes::new(
        us_to_timestamp(start_us)?,
        end_us.map(us_to_timestamp).transpose()?,
    ))
}

fn deletion_conflict_on(conn: &Connection, id: WorklogId) -> Result<StorageError, StorageError> {
    Ok(match worklog_by_id_on(conn, id)? {
        None => StorageError::WorklogNotFound { id },
        Some(worklog) if worklog.is_active() => StorageError::WorklogIsActive { id },
        Some(_) => StorageError::WorklogChanged { id },
    })
}
