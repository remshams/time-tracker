//! Domain model for the Time Tracker.
//!
//! This crate holds tasks, worklogs, tracking state, identifiers, and domain
//! invariants. It has no application, presentation, storage, or transport code.

mod ids;
mod task;
mod tracking;
mod worklog;

pub use ids::{TaskId, WorklogId};
pub use task::{Task, TaskError, TaskName, TaskNameError};
pub use tracking::{SwitchedWorklogs, Tracker, TrackingError, TrackingOutcome, TrackingState};
pub use worklog::{
    ActiveWorklog, Worklog, WorklogCorrectionError, WorklogError, WorklogMoveError, WorklogTimes,
};
