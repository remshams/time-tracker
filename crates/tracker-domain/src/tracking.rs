//! Tracking state and commands.
//!
//! The tracker is either idle or running one active worklog. Callers supply
//! identifiers for new worklogs and timestamps for commands, so tests never
//! depend on the wall clock. Commands either fully apply or
//! leave the state unchanged; timestamps are validated before the state
//! moves.

use chrono::{DateTime, Utc};

use crate::ids::{TaskId, WorklogId};
use crate::task::Task;
use crate::worklog::{ActiveWorklog, Worklog};

/// Why a tracking command was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TrackingError {
    /// A start or switch command ran while another worklog is already active.
    #[error("another worklog is already active")]
    AlreadyRunning { active: ActiveWorklog },
    /// A stop or switch command ran while no worklog is active.
    #[error("no worklog is running")]
    NotRunning,
    /// The target task is archived and cannot start tracking.
    #[error("task {id} is archived")]
    TaskArchived { id: TaskId },
    /// The task to archive is the one that is currently running.
    #[error("task {id} is active and cannot be archived")]
    TaskIsActive { id: TaskId },
    /// The stop time precedes the worklog's start time.
    #[error("worklog {worklog_id} cannot stop at {stop_at} before starting at {start}")]
    StopBeforeStart {
        worklog_id: WorklogId,
        start: DateTime<Utc>,
        stop_at: DateTime<Utc>,
    },
    /// The stop time equals the worklog start.
    #[error("worklog {worklog_id} must stop later than its start at {start}")]
    StopAtStart {
        worklog_id: WorklogId,
        start: DateTime<Utc>,
    },
    /// A switch targeted the task that is already active.
    #[error("task {id} is already the active task")]
    TaskAlreadyActive { id: TaskId },
    /// A recovered worklog was already stopped, so it cannot resume.
    #[error("worklog {id} is stopped and cannot resume")]
    WorklogNotActive { id: WorklogId },
}

/// The result of a successful switch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchedWorklogs {
    /// The worklog that was stopped, with its end time set.
    pub stopped: Worklog,
    /// The new active worklog, with no end time.
    pub started: Worklog,
}

/// Whether the tracker is idle or running a worklog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackingState {
    /// No worklog is active.
    Idle,
    /// One worklog is active.
    Running { worklog: ActiveWorklog },
}

/// The single tracker's state plus the commands that change it.
///
/// The tracker holds at most one active worklog. Restarting a stopped task
/// creates a new worklog; stopped worklogs are never resumed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tracker {
    state: TrackingState,
}

impl Tracker {
    /// Creates an idle tracker.
    pub fn idle() -> Self {
        Self {
            state: TrackingState::Idle,
        }
    }

    /// Rebuilds a tracker from a worklog recovered after a process restart.
    ///
    /// Fails when the worklog is already stopped, because a stopped worklog is
    /// never resumed.
    pub fn resume(worklog: Worklog) -> Result<Self, TrackingError> {
        if !worklog.is_active() {
            return Err(TrackingError::WorklogNotActive { id: worklog.id() });
        }
        Ok(Self {
            state: TrackingState::Running {
                worklog: ActiveWorklog::begin(worklog.id(), worklog.task_id(), worklog.start()),
            },
        })
    }

    /// Returns the current tracking state.
    pub fn state(&self) -> &TrackingState {
        &self.state
    }

    /// Returns the active worklog, if any.
    pub fn active(&self) -> Option<&ActiveWorklog> {
        match &self.state {
            TrackingState::Idle => None,
            TrackingState::Running { worklog } => Some(worklog),
        }
    }

    /// Starts a new worklog with the supplied identifier at the given instant.
    ///
    /// Fails when another worklog is active or the task is archived. Restarting
    /// a task that has stopped worklogs still creates a new worklog.
    pub fn start(
        &mut self,
        task: &Task,
        worklog_id: WorklogId,
        at: DateTime<Utc>,
    ) -> Result<Worklog, TrackingError> {
        if let TrackingState::Running { worklog } = &self.state {
            return Err(TrackingError::AlreadyRunning {
                active: worklog.clone(),
            });
        }
        if task.is_archived() {
            return Err(TrackingError::TaskArchived { id: task.id() });
        }
        let active = ActiveWorklog::begin(worklog_id, task.id(), at);
        let worklog = active.to_worklog();
        self.state = TrackingState::Running { worklog: active };
        Ok(worklog)
    }

    /// Stops the active worklog at the given instant.
    ///
    /// Fails when nothing is running or when `at` is not later than the worklog's start.
    /// On failure the worklog stays active.
    pub fn stop(&mut self, at: DateTime<Utc>) -> Result<Worklog, TrackingError> {
        let active = match &self.state {
            TrackingState::Idle => return Err(TrackingError::NotRunning),
            TrackingState::Running { worklog } => worklog.clone(),
        };
        let stopped = active.stop(at)?;
        self.state = TrackingState::Idle;
        Ok(stopped)
    }

    /// Stops the active worklog and starts a new worklog for the given task at
    /// the same instant, using the supplied worklog identifier.
    ///
    /// Fails, leaving the current worklog active, when nothing is running, when
    /// the target is the active task, when the target is archived, or when `at`
    /// is not later than the active worklog's start. Callers persist both halves
    /// of the switch in one transaction.
    pub fn switch(
        &mut self,
        task: &Task,
        worklog_id: WorklogId,
        at: DateTime<Utc>,
    ) -> Result<SwitchedWorklogs, TrackingError> {
        let active = match &self.state {
            TrackingState::Idle => return Err(TrackingError::NotRunning),
            TrackingState::Running { worklog } => worklog.clone(),
        };
        if task.id() == active.task_id() {
            return Err(TrackingError::TaskAlreadyActive { id: task.id() });
        }
        if task.is_archived() {
            return Err(TrackingError::TaskArchived { id: task.id() });
        }
        let stopped = active.stop(at)?;
        let started = ActiveWorklog::begin(worklog_id, task.id(), at);
        self.state = TrackingState::Running {
            worklog: started.clone(),
        };
        Ok(SwitchedWorklogs {
            stopped,
            started: started.to_worklog(),
        })
    }

    /// Checks that the task may be archived.
    ///
    /// Archiving the active task is rejected because it would leave a timer
    /// running on a task that is no longer usable.
    pub fn ensure_archivable(&self, task_id: TaskId) -> Result<(), TrackingError> {
        match &self.state {
            TrackingState::Idle => Ok(()),
            TrackingState::Running { worklog } if worklog.task_id() == task_id => {
                Err(TrackingError::TaskIsActive { id: task_id })
            }
            TrackingState::Running { .. } => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::TaskName;

    fn task_id(tag: u32) -> TaskId {
        TaskId::from_uuid(uuid::Uuid::from_u128(tag as u128))
    }

    fn worklog_id(tag: u32) -> WorklogId {
        WorklogId::from_uuid(uuid::Uuid::from_u128(tag as u128))
    }

    fn task(tag: u32) -> Task {
        Task::create(task_id(tag), TaskName::new("task").unwrap(), at(100))
    }

    fn archived_task(tag: u32) -> Task {
        let mut task = task(tag);
        assert!(task.archive(at(100)));
        task
    }

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).unwrap()
    }

    #[test]
    fn idle_tracker_has_no_active_worklog() {
        let tracker = Tracker::idle();
        assert_eq!(tracker.state(), &TrackingState::Idle);
        assert!(tracker.active().is_none());
    }

    /// The supplied identity must survive in both the returned worklog and active state.
    #[test]
    fn start_creates_one_active_worklog_for_the_task() {
        let mut tracker = Tracker::idle();
        let worklog = tracker.start(&task(1), worklog_id(1), at(100)).unwrap();
        assert_eq!(worklog.id(), worklog_id(1));
        assert_eq!(worklog.task_id(), task_id(1));
        assert_eq!(worklog.start(), at(100));
        assert_eq!(worklog.end(), None);
        assert_eq!(tracker.active().map(|e| e.id()), Some(worklog.id()));
        assert_eq!(
            tracker.state(),
            &TrackingState::Running {
                worklog: ActiveWorklog::begin(worklog.id(), task_id(1), at(100))
            }
        );
    }

    /// Rejected starts retain the identity and state of the worklog already running.
    #[test]
    fn start_while_running_is_rejected_with_the_active_worklog() {
        let mut tracker = Tracker::idle();
        let first = tracker.start(&task(1), worklog_id(1), at(100)).unwrap();
        let error = tracker
            .start(&task(2), worklog_id(1), at(110))
            .expect_err("already running");
        assert_eq!(
            error,
            TrackingError::AlreadyRunning {
                active: ActiveWorklog::begin(first.id(), task_id(1), at(100))
            }
        );
        assert_eq!(tracker.active().map(|e| e.id()), Some(first.id()));
    }

    /// An archived task cannot create active tracking state.
    #[test]
    fn start_on_archived_task_is_rejected() {
        let mut tracker = Tracker::idle();
        let error = tracker
            .start(&archived_task(1), worklog_id(1), at(100))
            .expect_err("archived task must not start");
        assert_eq!(error, TrackingError::TaskArchived { id: task_id(1) });
        assert_eq!(tracker.state(), &TrackingState::Idle);
    }

    #[test]
    fn stop_when_idle_is_rejected() {
        let mut tracker = Tracker::idle();
        assert_eq!(tracker.stop(at(100)), Err(TrackingError::NotRunning));
    }

    /// Stopping completes the same worklog rather than assigning another identity.
    #[test]
    fn stop_sets_the_end_time_and_goes_idle() {
        let mut tracker = Tracker::idle();
        let started = tracker.start(&task(1), worklog_id(1), at(100)).unwrap();
        let stopped = tracker.stop(at(150)).unwrap();
        assert_eq!(stopped.id(), started.id());
        assert_eq!(stopped.start(), at(100));
        assert_eq!(stopped.end(), Some(at(150)));
        assert_eq!(tracker.state(), &TrackingState::Idle);
    }

    /// Rejecting a zero-duration stop keeps the same worklog active.
    #[test]
    fn stop_at_exactly_the_start_time_is_rejected_without_changing_state() {
        let mut tracker = Tracker::idle();
        let started = tracker.start(&task(1), worklog_id(1), at(100)).unwrap();
        let state = tracker.state().clone();
        assert_eq!(
            tracker.stop(at(100)),
            Err(TrackingError::StopAtStart {
                worklog_id: started.id(),
                start: at(100)
            })
        );
        assert_eq!(tracker.state(), &state);
    }

    /// A backwards stop must leave the original worklog active.
    #[test]
    fn stop_before_start_is_rejected_and_the_worklog_stays_active() {
        let mut tracker = Tracker::idle();
        let started = tracker.start(&task(1), worklog_id(1), at(100)).unwrap();
        let error = tracker.stop(at(99)).expect_err("backwards stop must fail");
        assert_eq!(
            error,
            TrackingError::StopBeforeStart {
                worklog_id: started.id(),
                start: at(100),
                stop_at: at(99),
            }
        );
        assert_eq!(tracker.active().map(|e| e.id()), Some(started.id()));
    }

    /// Restarting uses a separate identity so completed history is never resumed.
    #[test]
    fn restarting_a_stopped_task_creates_a_new_worklog() {
        let mut tracker = Tracker::idle();
        let first = tracker.start(&task(1), worklog_id(1), at(100)).unwrap();
        let first_stopped = tracker.stop(at(150)).unwrap();
        let second = tracker.start(&task(1), worklog_id(2), at(200)).unwrap();
        assert_ne!(first.id(), second.id());
        assert_eq!(first_stopped.id(), first.id());
        assert_eq!(first_stopped.end(), Some(at(150)));
        assert_eq!(second.start(), at(200));
        assert_eq!(second.end(), None);
        assert_eq!(tracker.active().map(|e| e.id()), Some(second.id()));
    }

    /// A switch requires an existing worklog to complete and cannot act as an idle start.
    #[test]
    fn switch_when_idle_is_rejected() {
        let mut tracker = Tracker::idle();
        let error = tracker
            .switch(&task(1), worklog_id(2), at(100))
            .expect_err("nothing runs");
        assert_eq!(error, TrackingError::NotRunning);
    }

    /// Same-task switches fail instead of splitting the current worklog.
    #[test]
    fn switch_to_the_active_task_is_rejected() {
        let mut tracker = Tracker::idle();
        let started = tracker.start(&task(1), worklog_id(1), at(100)).unwrap();
        let error = tracker
            .switch(&task(1), worklog_id(2), at(110))
            .expect_err("same task switch must fail");
        assert_eq!(error, TrackingError::TaskAlreadyActive { id: task_id(1) });
        assert_eq!(tracker.active().map(|e| e.id()), Some(started.id()));
    }

    /// An archived destination must leave the original worklog running.
    #[test]
    fn switch_to_an_archived_task_is_rejected() {
        let mut tracker = Tracker::idle();
        let started = tracker.start(&task(1), worklog_id(1), at(100)).unwrap();
        let error = tracker
            .switch(&archived_task(2), worklog_id(2), at(110))
            .expect_err("archived target must fail");
        assert_eq!(error, TrackingError::TaskArchived { id: task_id(2) });
        assert_eq!(tracker.active().map(|e| e.id()), Some(started.id()));
    }

    /// A switch preserves the completed identity and uses the caller identity for new work.
    #[test]
    fn switch_stops_the_old_worklog_and_starts_the_new_one() {
        let mut tracker = Tracker::idle();
        let first = tracker.start(&task(1), worklog_id(1), at(100)).unwrap();
        let switched = tracker.switch(&task(2), worklog_id(2), at(150)).unwrap();
        assert_eq!(switched.stopped.id(), first.id());
        assert_eq!(switched.stopped.task_id(), task_id(1));
        assert_eq!(switched.stopped.end(), Some(at(150)));
        assert_eq!(switched.started.id(), worklog_id(2));
        assert_eq!(switched.started.task_id(), task_id(2));
        assert_eq!(switched.started.start(), at(150));
        assert_eq!(switched.started.end(), None);
        assert_ne!(switched.stopped.id(), switched.started.id());
        assert_eq!(
            tracker.active().map(|e| e.id()),
            Some(switched.started.id())
        );
        assert_eq!(tracker.active().map(|e| e.task_id()), Some(task_id(2)));
    }

    /// A switch permits adjacent half-open intervals with positive completed duration.
    #[test]
    fn switch_accepts_matching_boundaries() {
        let mut tracker = Tracker::idle();
        tracker.start(&task(1), worklog_id(1), at(100)).unwrap();
        let switched = tracker.switch(&task(2), worklog_id(2), at(101)).unwrap();
        assert_eq!(switched.stopped.end(), Some(at(101)));
        assert_eq!(switched.started.start(), at(101));
    }

    #[test]
    fn zero_duration_switches_preserve_the_active_worklog() {
        let mut tracker = Tracker::idle();
        let started = tracker.start(&task(1), worklog_id(1), at(100)).unwrap();
        let state = tracker.state().clone();
        let expected = TrackingError::StopAtStart {
            worklog_id: started.id(),
            start: at(100),
        };
        assert_eq!(
            tracker.switch(&task(2), worklog_id(2), at(100)),
            Err(expected)
        );
        assert_eq!(tracker.state(), &state);
    }

    /// Invalid completion times must not replace the active worklog during a switch.
    #[test]
    fn switch_with_stop_before_the_active_start_is_rejected_and_rolls_back() {
        let mut tracker = Tracker::idle();
        let started = tracker.start(&task(1), worklog_id(1), at(100)).unwrap();
        let error = tracker
            .switch(&task(2), worklog_id(2), at(99))
            .expect_err("stop precedes start");
        assert_eq!(
            error,
            TrackingError::StopBeforeStart {
                worklog_id: started.id(),
                start: at(100),
                stop_at: at(99),
            }
        );
        assert_eq!(tracker.active().map(|e| e.id()), Some(started.id()));
    }

    /// Only the task that owns the active timer receives tracking-based archive protection.
    #[test]
    fn ensure_archivable_only_rejects_the_active_task() {
        let mut tracker = Tracker::idle();
        assert_eq!(tracker.ensure_archivable(task_id(1)), Ok(()));
        tracker.start(&task(1), worklog_id(1), at(100)).unwrap();
        assert_eq!(
            tracker.ensure_archivable(task_id(1)),
            Err(TrackingError::TaskIsActive { id: task_id(1) })
        );
        assert_eq!(tracker.ensure_archivable(task_id(2)), Ok(()));
    }

    #[test]
    fn resume_accepts_an_active_worklog() {
        let worklog = Worklog::begin(worklog_id(1), task_id(2), at(100));
        let tracker = Tracker::resume(worklog).unwrap();
        assert_eq!(
            tracker.state(),
            &TrackingState::Running {
                worklog: ActiveWorklog::begin(worklog_id(1), task_id(2), at(100))
            }
        );
    }

    #[test]
    fn resume_rejects_a_stopped_worklog() {
        let worklog = Worklog::new(worklog_id(1), task_id(2), at(100), Some(at(150))).unwrap();
        let error = Tracker::resume(worklog).expect_err("stopped worklogs do not resume");
        assert_eq!(error, TrackingError::WorklogNotActive { id: worklog_id(1) });
    }
}
