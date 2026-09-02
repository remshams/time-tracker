//! Worklogs.
//!
//! One task has many worklogs. An worklog that has no end time is the
//! active worklog; at most one worklog is active across the whole tracker.

use chrono::{DateTime, Utc};

use crate::ids::{TaskId, WorklogId};

/// A recorded interval spent on a task.
///
/// `end` is `None` while the worklog is active. A stopped worklog always has an
/// end time that is not earlier than its start time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worklog {
    /// The stable UUIDv7 identifier.
    pub id: WorklogId,
    /// The task the time was spent on.
    pub task_id: TaskId,
    /// When the worklog started, in UTC.
    pub start: DateTime<Utc>,
    /// When the worklog stopped, in UTC, or `None` while it is active.
    pub end: Option<DateTime<Utc>>,
}

/// Why a worklog could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WorklogError {
    /// The end time precedes the start time.
    #[error("worklog end must not precede its start")]
    EndBeforeStart,
}

impl Worklog {
    /// Builds a worklog from stored values, rejecting an end before the start.
    pub fn new(
        id: WorklogId,
        task_id: TaskId,
        start: DateTime<Utc>,
        end: Option<DateTime<Utc>>,
    ) -> Result<Self, WorklogError> {
        if let Some(end) = end
            && end < start
        {
            return Err(WorklogError::EndBeforeStart);
        }
        Ok(Self {
            id,
            task_id,
            start,
            end,
        })
    }

    /// Builds a new active worklog with no end time.
    pub fn begin(id: WorklogId, task_id: TaskId, start: DateTime<Utc>) -> Self {
        Self {
            id,
            task_id,
            start,
            end: None,
        }
    }
}

/// The worklog that is currently running.
///
/// This type cannot represent a stopped worklog, so the tracker's running state
/// keeps the "end is unset" invariant by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveWorklog {
    /// The identifier of the underlying worklog.
    pub id: WorklogId,
    /// The task being tracked.
    pub task_id: TaskId,
    /// When tracking started, in UTC.
    pub start: DateTime<Utc>,
}

impl ActiveWorklog {
    /// Starts tracking the given task at the given instant.
    pub fn begin(id: WorklogId, task_id: TaskId, start: DateTime<Utc>) -> Self {
        Self { id, task_id, start }
    }

    /// Stops the worklog at the given instant.
    ///
    /// Fails when `at` precedes the start time; the worklog stays untouched in
    /// that case.
    pub fn stop(self, at: DateTime<Utc>) -> Result<Worklog, crate::tracking::TrackingError> {
        if at < self.start {
            return Err(crate::tracking::TrackingError::StopBeforeStart {
                worklog_id: self.id,
                start: self.start,
                stop_at: at,
            });
        }
        Ok(Worklog {
            id: self.id,
            task_id: self.task_id,
            start: self.start,
            end: Some(at),
        })
    }

    /// Views the active worklog as a plain worklog with no end time.
    pub fn to_worklog(&self) -> Worklog {
        Worklog {
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

    fn worklog_id(tag: u32) -> WorklogId {
        WorklogId::from_uuid(uuid::Uuid::from_u128(tag as u128))
    }

    fn task_id(tag: u32) -> TaskId {
        TaskId::from_uuid(uuid::Uuid::from_u128(tag as u128))
    }

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).unwrap()
    }

    #[test]
    fn new_accepts_active_and_valid_stopped_worklogs() {
        let active = Worklog::new(worklog_id(1), task_id(1), at(100), None).unwrap();
        assert_eq!(active.end, None);
        let boundary = Worklog::new(worklog_id(2), task_id(1), at(100), Some(at(100))).unwrap();
        assert_eq!(boundary.end, Some(at(100)));
        let stopped = Worklog::new(worklog_id(3), task_id(1), at(100), Some(at(200))).unwrap();
        assert_eq!(stopped.end, Some(at(200)));
    }

    #[test]
    fn new_rejects_end_before_start() {
        let error = Worklog::new(worklog_id(1), task_id(1), at(200), Some(at(199)))
            .expect_err("backwards interval must fail");
        assert_eq!(error, WorklogError::EndBeforeStart);
    }

    #[test]
    fn begin_creates_an_active_worklog() {
        let worklog = Worklog::begin(worklog_id(1), task_id(2), at(100));
        assert_eq!(worklog.id, worklog_id(1));
        assert_eq!(worklog.task_id, task_id(2));
        assert_eq!(worklog.start, at(100));
        assert_eq!(worklog.end, None);
    }

    #[test]
    fn active_stop_accepts_start_and_later_instants() {
        let active = ActiveWorklog::begin(worklog_id(1), task_id(2), at(100));
        let stopped = active.clone().stop(at(100)).unwrap();
        assert_eq!(stopped.end, Some(at(100)));
        let active = ActiveWorklog::begin(worklog_id(1), task_id(2), at(100));
        let stopped = active.stop(at(150)).unwrap();
        assert_eq!(stopped.end, Some(at(150)));
    }

    #[test]
    fn active_stop_rejects_earlier_instants_and_reports_details() {
        let active = ActiveWorklog::begin(worklog_id(1), task_id(2), at(100));
        let error = active.stop(at(99)).expect_err("earlier stop must fail");
        assert_eq!(
            error,
            crate::tracking::TrackingError::StopBeforeStart {
                worklog_id: worklog_id(1),
                start: at(100),
                stop_at: at(99),
            }
        );
    }

    #[test]
    fn to_worklog_keeps_the_worklog_active() {
        let active = ActiveWorklog::begin(worklog_id(1), task_id(2), at(100));
        let worklog = active.to_worklog();
        assert_eq!(worklog.id, worklog_id(1));
        assert_eq!(worklog.task_id, task_id(2));
        assert_eq!(worklog.start, at(100));
        assert_eq!(worklog.end, None);
    }
}
