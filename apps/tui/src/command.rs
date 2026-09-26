//! Commands crossing from key handling into the event loop.
//!
//! The wrapper carries one screen-owned payload or the global quit request.
//! [`crate::app::App`] dispatches a payload only when its screen is active, so
//! the single mismatch guard is the boundary between active and dormant state.

use crate::screens::{AllWorklogsCommand, ReportCommand, TaskListCommand, WorklogHistoryCommand};

/// A command accepted by the event loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Command {
    /// A command owned by the task-list screen.
    TaskList(TaskListCommand),
    /// A command owned by the worklog-history screen.
    WorklogHistory(WorklogHistoryCommand),
    AllWorklogs(AllWorklogsCommand),
    Reports(ReportCommand),
    /// Leave the application without changing active tracking.
    Quit,
}
