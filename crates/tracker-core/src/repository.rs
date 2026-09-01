//! The persistence contract for the tracker.
//!
//! `tracker-core` owns this trait; storage backends such as
//! `tracker-storage` implement it. The core crate never depends on a
//! specific backend.

use chrono::{DateTime, Utc};

use crate::entry::TimeEntry;
use crate::ids::{EntryId, TaskId};
use crate::task::{Task, TaskName};

/// Stores tasks and time entries.
///
/// Contracts for implementations:
///
/// - Tasks are ordered by identifier, which approximates creation order
///   because identifiers are UUIDv7.
/// - Entries of one task are ordered by start time, then identifier.
/// - At most one entry may be active across the whole tracker; backends must
///   enforce this themselves, not just rely on callers.
/// - `switch_entry` must stop the old entry and insert the new one
///   atomically: on failure neither change may be visible.
/// - `insert_entry` and `switch_entry` receive entries the caller validated
///   with `Tracker`; backends still enforce their own constraints, such as
///   an end time not preceding the start time.
pub trait TrackerRepository {
    /// The error type this backend reports.
    type Error: std::error::Error;

    /// Stores a new task. Fails when a task with the same id already exists.
    fn create_task(&self, task: Task) -> Result<(), Self::Error>;

    /// Returns the task with the given id, or `None` when it does not exist.
    fn find_task(&self, id: TaskId) -> Result<Option<Task>, Self::Error>;

    /// Returns all tasks ordered by id.
    fn list_tasks(&self) -> Result<Vec<Task>, Self::Error>;

    /// Renames the task and returns the updated task.
    fn rename_task(&self, id: TaskId, name: TaskName) -> Result<Task, Self::Error>;

    /// Marks the task archived and returns the updated task.
    ///
    /// Callers must reject archiving the active task through
    /// `Tracker::ensure_archivable` first.
    fn archive_task(&self, id: TaskId) -> Result<Task, Self::Error>;

    /// Stores an entry, active or stopped.
    ///
    /// Fails when the task does not exist or another entry is already active.
    fn insert_entry(&self, entry: &TimeEntry) -> Result<(), Self::Error>;

    /// Sets the end time of an active entry and returns the stopped entry.
    fn stop_entry(&self, id: EntryId, end: DateTime<Utc>) -> Result<TimeEntry, Self::Error>;

    /// Returns the active entry, or `None` when the tracker is idle.
    fn active_entry(&self) -> Result<Option<TimeEntry>, Self::Error>;

    /// Returns the task's entries ordered by start time, then id.
    fn list_entries(&self, task_id: TaskId) -> Result<Vec<TimeEntry>, Self::Error>;

    /// Stops the entry with the given id at `stop_at` and inserts `next` as
    /// the new active entry, in one atomic step.
    fn switch_entry(
        &self,
        id: EntryId,
        stop_at: DateTime<Utc>,
        next: &TimeEntry,
    ) -> Result<(), Self::Error>;
}
