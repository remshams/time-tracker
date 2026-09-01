//! Domain model for the Time Tracker.
//!
//! This crate holds the task and tracking types, the tracking commands, and
//! the persistence contract. It stays independent of Ratatui, Crossterm,
//! storage engines, and networking: SQLite support lives in
//! `tracker-storage`, which implements [`TrackerRepository`].

mod entry;
mod ids;
mod repository;
mod task;
mod tracking;

pub use entry::{ActiveEntry, TimeEntry, TimeEntryError};
pub use ids::{EntryId, TaskId};
pub use repository::TrackerRepository;
pub use task::{Task, TaskName, TaskNameError};
pub use tracking::{SwitchedEntries, Tracker, TrackingError, TrackingOutcome, TrackingState};
