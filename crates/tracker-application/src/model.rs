//! Application read models and successful tracking outcomes.

use chrono::{DateTime, TimeDelta, Utc};
use tracker_domain::{ActiveWorklog, Task, TaskId, Worklog, WorklogId};

/// How the task list is ordered.
///
/// The ordering rules live here once, backend-neutral, per ADR 0002:
/// recently worked puts the latest worklog start first and tasks without
/// worklogs last; the alternatives are newest-first by `updated_at` and by
/// `created_at`. Every ordering ends in a deterministic `TaskId` ascending
/// tie-break.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskOrdering {
    /// Latest worklog start descending, tasks without worklogs last, then
    /// `created_at` descending, then `TaskId` ascending. The default.
    #[default]
    RecentlyWorked,
    /// `updated_at` descending, then `created_at` descending, then `TaskId`
    /// ascending.
    RecentlyUpdated,
    /// `created_at` descending, then `TaskId` ascending.
    RecentlyCreated,
}

impl TaskOrdering {
    /// Sorts task-list items in place by this ordering, exactly per ADR 0002.
    pub fn sort_items(self, items: &mut [TaskListItem]) {
        match self {
            TaskOrdering::RecentlyWorked => items.sort_by(|a, b| {
                b.latest_work_start
                    .cmp(&a.latest_work_start)
                    .then_with(|| b.task.created_at().cmp(&a.task.created_at()))
                    .then_with(|| a.task.id().cmp(&b.task.id()))
            }),
            TaskOrdering::RecentlyUpdated => items.sort_by(|a, b| {
                b.task
                    .updated_at()
                    .cmp(&a.task.updated_at())
                    .then_with(|| b.task.created_at().cmp(&a.task.created_at()))
                    .then_with(|| a.task.id().cmp(&b.task.id()))
            }),
            TaskOrdering::RecentlyCreated => items.sort_by(|a, b| {
                b.task
                    .created_at()
                    .cmp(&a.task.created_at())
                    .then_with(|| a.task.id().cmp(&b.task.id()))
            }),
        };
    }
}

/// One row of the task-list read model: the task plus its latest worklog
/// start, or `None` when the task has no worklogs.
///
/// The latest work start is derived, never stored on the task, so it stays
/// correct no matter which client wrote the worklog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskListItem {
    pub task: Task,
    pub latest_work_start: Option<DateTime<Utc>>,
}

/// One task's time inside the requested report interval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportRow {
    pub task: Task,
    pub duration: TimeDelta,
}

/// Positive task totals and their combined duration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportTotals {
    pub rows: Vec<ReportRow>,
    pub total: TimeDelta,
}

/// How many worklogs one history page carries.
///
/// The size is an application constant, not a caller argument, so no caller
/// can request an unbounded page.
pub const WORKLOG_PAGE_SIZE: usize = 50;

/// The position of one worklog in the history order, used to fetch the next
/// page after it.
///
/// History is ordered by start descending, then `WorklogId` ascending as the
/// deterministic tie-break, so a cursor carries both values. Rows that share
/// a start are ordered and bounded by identifier, which is what keeps page
/// boundaries from repeating or dropping them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorklogCursor {
    /// The task whose history produced this cursor.
    pub task_id: TaskId,
    /// The start time of the page's last worklog, in UTC.
    pub start: DateTime<Utc>,
    /// The identifier of the page's last worklog.
    pub id: WorklogId,
    /// The task history ordering revision that produced this cursor.
    pub revision: i64,
}

/// The bounded state read with one worklog-history page.
///
/// This contains only the aggregates that page adoption can change: the
/// requested task and, when tracking is active, the active task. It avoids
/// materializing every task and worklog aggregate for each history page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorklogPageSnapshot {
    /// The requested task's authoritative latest worklog start.
    pub requested_task_latest_work_start: Option<DateTime<Utc>>,
    /// The global active worklog, if one exists.
    pub active_worklog: Option<Worklog>,
    /// The active task's authoritative latest worklog start. It is present
    /// when `active_worklog` is present, including when it is the requested
    /// task.
    pub active_task_latest_work_start: Option<DateTime<Utc>>,
}

/// One bounded page of a task's worklog history.
///
/// Worklogs appear in history order: start descending, then `WorklogId`
/// ascending. Active worklogs are part of the history. `snapshot` comes from
/// the same backend read as the page. A `next_cursor` of `None` means the page
/// reached the end of the history; it does not mean the page was full.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorklogPage {
    /// The worklogs of this page, in history order.
    pub worklogs: Vec<Worklog>,
    /// Bounded aggregate and tracking state read with this page.
    pub snapshot: WorklogPageSnapshot,
    /// The cursor to pass for the next page, or `None` at the end of the
    /// history.
    pub next_cursor: Option<WorklogCursor>,
}

/// Position and ordering revision for the global worklog feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalWorklogCursor {
    pub start: DateTime<Utc>,
    pub id: WorklogId,
    /// Any committed worklog write changes this revision.
    pub revision: i64,
}

/// One page of worklogs across active and archived tasks.
///
/// Rows are ordered by start descending, then `WorklogId` ascending. The
/// tracker snapshot comes from the same backend read as the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalWorklogPage {
    pub worklogs: Vec<Worklog>,
    pub snapshot: crate::TrackerSnapshot,
    pub next_cursor: Option<GlobalWorklogCursor>,
}

/// The result of making one task active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetActiveTaskOutcome {
    Started { worklog: Worklog },
    Switched { stopped: Worklog, started: Worklog },
    AlreadyActive { worklog: ActiveWorklog },
}

/// The result of making the tracker idle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClearActiveTaskOutcome {
    Stopped { worklog: Worklog },
    AlreadyIdle,
}
