//! Time entries.
//!
//! One task has many time entries. An entry that has no end time is the
//! active entry; at most one entry is active across the whole tracker.

use chrono::{DateTime, Utc};

use crate::ids::{EntryId, TaskId};

/// A recorded interval spent on a task.
///
/// `end` is `None` while the entry is active. A stopped entry always has an
/// end time that is not earlier than its start time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeEntry {
    /// The stable UUIDv7 identifier.
    pub id: EntryId,
    /// The task the time was spent on.
    pub task_id: TaskId,
    /// When the entry started, in UTC.
    pub start: DateTime<Utc>,
    /// When the entry stopped, in UTC, or `None` while it is active.
    pub end: Option<DateTime<Utc>>,
}

/// Why a time entry could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TimeEntryError {
    /// The end time precedes the start time.
    #[error("time entry end must not precede its start")]
    EndBeforeStart,
}

impl TimeEntry {
    /// Builds an entry from stored values, rejecting an end before the start.
    pub fn new(
        id: EntryId,
        task_id: TaskId,
        start: DateTime<Utc>,
        end: Option<DateTime<Utc>>,
    ) -> Result<Self, TimeEntryError> {
        if let Some(end) = end
            && end < start
        {
            return Err(TimeEntryError::EndBeforeStart);
        }
        Ok(Self {
            id,
            task_id,
            start,
            end,
        })
    }

    /// Builds a new active entry with no end time.
    pub fn begin(id: EntryId, task_id: TaskId, start: DateTime<Utc>) -> Self {
        Self {
            id,
            task_id,
            start,
            end: None,
        }
    }
}

/// The entry that is currently running.
///
/// This type cannot represent a stopped entry, so the tracker's running state
/// keeps the "end is unset" invariant by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveEntry {
    /// The identifier of the underlying time entry.
    pub id: EntryId,
    /// The task being tracked.
    pub task_id: TaskId,
    /// When tracking started, in UTC.
    pub start: DateTime<Utc>,
}

impl ActiveEntry {
    /// Starts tracking the given task at the given instant.
    pub fn begin(id: EntryId, task_id: TaskId, start: DateTime<Utc>) -> Self {
        Self { id, task_id, start }
    }

    /// Stops the entry at the given instant.
    ///
    /// Fails when `at` precedes the start time; the entry stays untouched in
    /// that case.
    pub fn stop(self, at: DateTime<Utc>) -> Result<TimeEntry, crate::tracking::TrackingError> {
        if at < self.start {
            return Err(crate::tracking::TrackingError::StopBeforeStart {
                entry_id: self.id,
                start: self.start,
                stop_at: at,
            });
        }
        Ok(TimeEntry {
            id: self.id,
            task_id: self.task_id,
            start: self.start,
            end: Some(at),
        })
    }

    /// Views the active entry as a plain time entry with no end time.
    pub fn to_time_entry(&self) -> TimeEntry {
        TimeEntry {
            id: self.id,
            task_id: self.task_id,
            start: self.start,
            end: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry_id(tag: u32) -> EntryId {
        EntryId::from_uuid(uuid::Uuid::from_u128(tag as u128))
    }

    fn task_id(tag: u32) -> TaskId {
        TaskId::from_uuid(uuid::Uuid::from_u128(tag as u128))
    }

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).unwrap()
    }

    #[test]
    fn new_accepts_active_and_valid_stopped_entries() {
        let active = TimeEntry::new(entry_id(1), task_id(1), at(100), None).unwrap();
        assert_eq!(active.end, None);
        let boundary = TimeEntry::new(entry_id(2), task_id(1), at(100), Some(at(100))).unwrap();
        assert_eq!(boundary.end, Some(at(100)));
        let stopped = TimeEntry::new(entry_id(3), task_id(1), at(100), Some(at(200))).unwrap();
        assert_eq!(stopped.end, Some(at(200)));
    }

    #[test]
    fn new_rejects_end_before_start() {
        let error = TimeEntry::new(entry_id(1), task_id(1), at(200), Some(at(199)))
            .expect_err("backwards interval must fail");
        assert_eq!(error, TimeEntryError::EndBeforeStart);
    }

    #[test]
    fn begin_creates_an_active_entry() {
        let entry = TimeEntry::begin(entry_id(1), task_id(2), at(100));
        assert_eq!(entry.id, entry_id(1));
        assert_eq!(entry.task_id, task_id(2));
        assert_eq!(entry.start, at(100));
        assert_eq!(entry.end, None);
    }

    #[test]
    fn active_stop_accepts_start_and_later_instants() {
        let active = ActiveEntry::begin(entry_id(1), task_id(2), at(100));
        let stopped = active.clone().stop(at(100)).unwrap();
        assert_eq!(stopped.end, Some(at(100)));
        let active = ActiveEntry::begin(entry_id(1), task_id(2), at(100));
        let stopped = active.stop(at(150)).unwrap();
        assert_eq!(stopped.end, Some(at(150)));
    }

    #[test]
    fn active_stop_rejects_earlier_instants_and_reports_details() {
        let active = ActiveEntry::begin(entry_id(1), task_id(2), at(100));
        let error = active.stop(at(99)).expect_err("earlier stop must fail");
        assert_eq!(
            error,
            crate::tracking::TrackingError::StopBeforeStart {
                entry_id: entry_id(1),
                start: at(100),
                stop_at: at(99),
            }
        );
    }

    #[test]
    fn to_time_entry_keeps_the_entry_active() {
        let active = ActiveEntry::begin(entry_id(1), task_id(2), at(100));
        let entry = active.to_time_entry();
        assert_eq!(entry.id, entry_id(1));
        assert_eq!(entry.task_id, task_id(2));
        assert_eq!(entry.start, at(100));
        assert_eq!(entry.end, None);
    }
}
