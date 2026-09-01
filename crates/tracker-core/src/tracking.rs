//! Tracking state and commands.
//!
//! The tracker is either idle or running one active entry. Every command
//! takes its timestamps as parameters, so callers control the clock and
//! tests never depend on the wall clock. Commands either fully apply or
//! leave the state unchanged; timestamps are validated before the state
//! moves.

use chrono::{DateTime, Utc};

use crate::entry::{ActiveEntry, TimeEntry};
use crate::ids::{EntryId, TaskId};
use crate::task::Task;

/// Why a tracking command was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TrackingError {
    /// A start or switch command ran while another entry is already active.
    #[error("another time entry is already active")]
    AlreadyRunning { active: ActiveEntry },
    /// A stop or switch command ran while no entry is active.
    #[error("no time entry is running")]
    NotRunning,
    /// The target task is archived and cannot start tracking.
    #[error("task {id} is archived")]
    TaskArchived { id: TaskId },
    /// The task to archive is the one that is currently running.
    #[error("task {id} is active and cannot be archived")]
    TaskIsActive { id: TaskId },
    /// The stop time precedes the entry's start time.
    #[error("entry {entry_id} cannot stop at {stop_at} before starting at {start}")]
    StopBeforeStart {
        entry_id: EntryId,
        start: DateTime<Utc>,
        stop_at: DateTime<Utc>,
    },
    /// A switch would start the new entry before it stops the old one.
    #[error("switch start {start_at} must not precede its stop {stop_at}")]
    StartBeforeStop {
        stop_at: DateTime<Utc>,
        start_at: DateTime<Utc>,
    },
    /// A switch targeted the task that is already active.
    #[error("task {id} is already the active task")]
    TaskAlreadyActive { id: TaskId },
    /// A recovered entry was already stopped, so it cannot resume.
    #[error("time entry {id} is stopped and cannot resume")]
    EntryNotActive { id: EntryId },
}

/// The result of a successful switch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchedEntries {
    /// The entry that was stopped, with its end time set.
    pub stopped: TimeEntry,
    /// The new active entry, with no end time.
    pub started: TimeEntry,
}

/// What a successful toggle did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackingOutcome {
    /// Tracking started on the given task.
    Started { entry: TimeEntry },
    /// The active entry, which belonged to the given task, was stopped.
    Stopped { entry: TimeEntry },
    /// One task's entry was stopped and another task's entry started.
    Switched {
        stopped: TimeEntry,
        started: TimeEntry,
    },
}

/// Whether the tracker is idle or running an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackingState {
    /// No entry is active.
    Idle,
    /// One entry is active.
    Running { entry: ActiveEntry },
}

/// The single tracker's state plus the commands that change it.
///
/// The tracker holds at most one active entry. Restarting a stopped task
/// creates a new entry; stopped entries are never resumed.
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

    /// Rebuilds a tracker from an entry recovered after a process restart.
    ///
    /// Fails when the entry is already stopped, because a stopped entry is
    /// never resumed.
    pub fn resume(entry: TimeEntry) -> Result<Self, TrackingError> {
        if entry.end.is_some() {
            return Err(TrackingError::EntryNotActive { id: entry.id });
        }
        Ok(Self {
            state: TrackingState::Running {
                entry: ActiveEntry {
                    id: entry.id,
                    task_id: entry.task_id,
                    start: entry.start,
                },
            },
        })
    }

    /// Returns the current tracking state.
    pub fn state(&self) -> &TrackingState {
        &self.state
    }

    /// Returns the active entry, if any.
    pub fn active(&self) -> Option<&ActiveEntry> {
        match &self.state {
            TrackingState::Idle => None,
            TrackingState::Running { entry } => Some(entry),
        }
    }

    /// Starts a new entry for the given task at the given instant.
    ///
    /// Fails when another entry is active or the task is archived. Restarting
    /// a task that has stopped entries still creates a new entry.
    pub fn start(&mut self, task: &Task, at: DateTime<Utc>) -> Result<TimeEntry, TrackingError> {
        if let TrackingState::Running { entry } = &self.state {
            return Err(TrackingError::AlreadyRunning {
                active: entry.clone(),
            });
        }
        if task.archived {
            return Err(TrackingError::TaskArchived { id: task.id });
        }
        let active = ActiveEntry::begin(EntryId::generate(), task.id, at);
        let entry = active.to_time_entry();
        self.state = TrackingState::Running { entry: active };
        Ok(entry)
    }

    /// Stops the active entry at the given instant.
    ///
    /// Fails when nothing is running or when `at` precedes the entry's start.
    /// On failure the entry stays active.
    pub fn stop(&mut self, at: DateTime<Utc>) -> Result<TimeEntry, TrackingError> {
        let active = match &self.state {
            TrackingState::Idle => return Err(TrackingError::NotRunning),
            TrackingState::Running { entry } => entry.clone(),
        };
        let stopped = active.stop(at)?;
        self.state = TrackingState::Idle;
        Ok(stopped)
    }

    /// Stops the active entry at `stop_at` and starts a new entry for the
    /// given task at `start_at`.
    ///
    /// Fails, leaving the current entry active, when nothing is running, when
    /// the target is the active task, when the target is archived, when
    /// `stop_at` precedes the active entry's start, or when `start_at`
    /// precedes `stop_at`. Callers persist both halves of the switch in one
    /// transaction.
    pub fn switch(
        &mut self,
        task: &Task,
        stop_at: DateTime<Utc>,
        start_at: DateTime<Utc>,
    ) -> Result<SwitchedEntries, TrackingError> {
        let active = match &self.state {
            TrackingState::Idle => return Err(TrackingError::NotRunning),
            TrackingState::Running { entry } => entry.clone(),
        };
        if task.id == active.task_id {
            return Err(TrackingError::TaskAlreadyActive { id: task.id });
        }
        if task.archived {
            return Err(TrackingError::TaskArchived { id: task.id });
        }
        let stopped = active.stop(stop_at)?;
        if start_at < stop_at {
            return Err(TrackingError::StartBeforeStop { stop_at, start_at });
        }
        let started = ActiveEntry::begin(EntryId::generate(), task.id, start_at);
        self.state = TrackingState::Running {
            entry: started.clone(),
        };
        Ok(SwitchedEntries {
            stopped,
            started: started.to_time_entry(),
        })
    }

    /// Starts or stops tracking for the given task.
    ///
    /// Toggling the active task stops it. Toggling another task while an
    /// entry runs switches to it, stopping the old entry and starting the new
    /// one at the same instant. Toggling while idle starts the task.
    pub fn toggle(
        &mut self,
        task: &Task,
        at: DateTime<Utc>,
    ) -> Result<TrackingOutcome, TrackingError> {
        let active = match &self.state {
            TrackingState::Idle => {
                return self
                    .start(task, at)
                    .map(|entry| TrackingOutcome::Started { entry });
            }
            TrackingState::Running { entry } => entry.clone(),
        };
        if active.task_id == task.id {
            return self
                .stop(at)
                .map(|entry| TrackingOutcome::Stopped { entry });
        }
        self.switch(task, at, at)
            .map(|switched| TrackingOutcome::Switched {
                stopped: switched.stopped,
                started: switched.started,
            })
    }

    /// Checks that the task may be archived.
    ///
    /// Archiving the active task is rejected because it would leave a timer
    /// running on a task that is no longer usable.
    pub fn ensure_archivable(&self, task_id: TaskId) -> Result<(), TrackingError> {
        match &self.state {
            TrackingState::Idle => Ok(()),
            TrackingState::Running { entry } if entry.task_id == task_id => {
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

    fn entry_id(tag: u32) -> EntryId {
        EntryId::from_uuid(uuid::Uuid::from_u128(tag as u128))
    }

    fn task(tag: u32) -> Task {
        Task::new(task_id(tag), TaskName::new("task").unwrap())
    }

    fn archived_task(tag: u32) -> Task {
        Task {
            archived: true,
            ..task(tag)
        }
    }

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).unwrap()
    }

    #[test]
    fn idle_tracker_has_no_active_entry() {
        let tracker = Tracker::idle();
        assert_eq!(tracker.state(), &TrackingState::Idle);
        assert!(tracker.active().is_none());
    }

    #[test]
    fn start_creates_one_active_entry_for_the_task() {
        let mut tracker = Tracker::idle();
        let entry = tracker.start(&task(1), at(100)).unwrap();
        assert_eq!(entry.task_id, task_id(1));
        assert_eq!(entry.start, at(100));
        assert_eq!(entry.end, None);
        assert_eq!(tracker.active().map(|e| e.id), Some(entry.id));
        assert_eq!(
            tracker.state(),
            &TrackingState::Running {
                entry: ActiveEntry {
                    id: entry.id,
                    task_id: task_id(1),
                    start: at(100),
                }
            }
        );
    }

    #[test]
    fn start_while_running_is_rejected_with_the_active_entry() {
        let mut tracker = Tracker::idle();
        let first = tracker.start(&task(1), at(100)).unwrap();
        let error = tracker
            .start(&task(2), at(110))
            .expect_err("already running");
        assert_eq!(
            error,
            TrackingError::AlreadyRunning {
                active: ActiveEntry {
                    id: first.id,
                    task_id: task_id(1),
                    start: at(100),
                }
            }
        );
        assert_eq!(tracker.active().map(|e| e.id), Some(first.id));
    }

    #[test]
    fn start_on_archived_task_is_rejected() {
        let mut tracker = Tracker::idle();
        let error = tracker
            .start(&archived_task(1), at(100))
            .expect_err("archived task must not start");
        assert_eq!(error, TrackingError::TaskArchived { id: task_id(1) });
        assert_eq!(tracker.state(), &TrackingState::Idle);
    }

    #[test]
    fn stop_when_idle_is_rejected() {
        let mut tracker = Tracker::idle();
        assert_eq!(tracker.stop(at(100)), Err(TrackingError::NotRunning));
    }

    #[test]
    fn stop_sets_the_end_time_and_goes_idle() {
        let mut tracker = Tracker::idle();
        let started = tracker.start(&task(1), at(100)).unwrap();
        let stopped = tracker.stop(at(150)).unwrap();
        assert_eq!(stopped.id, started.id);
        assert_eq!(stopped.start, at(100));
        assert_eq!(stopped.end, Some(at(150)));
        assert_eq!(tracker.state(), &TrackingState::Idle);
    }

    #[test]
    fn stop_at_exactly_the_start_time_is_accepted() {
        let mut tracker = Tracker::idle();
        tracker.start(&task(1), at(100)).unwrap();
        let stopped = tracker.stop(at(100)).unwrap();
        assert_eq!(stopped.end, Some(at(100)));
    }

    #[test]
    fn stop_before_start_is_rejected_and_the_entry_stays_active() {
        let mut tracker = Tracker::idle();
        let started = tracker.start(&task(1), at(100)).unwrap();
        let error = tracker.stop(at(99)).expect_err("backwards stop must fail");
        assert_eq!(
            error,
            TrackingError::StopBeforeStart {
                entry_id: started.id,
                start: at(100),
                stop_at: at(99),
            }
        );
        assert_eq!(tracker.active().map(|e| e.id), Some(started.id));
    }

    #[test]
    fn restarting_a_stopped_task_creates_a_new_entry() {
        let mut tracker = Tracker::idle();
        let first = tracker.start(&task(1), at(100)).unwrap();
        let first_stopped = tracker.stop(at(150)).unwrap();
        let second = tracker.start(&task(1), at(200)).unwrap();
        assert_ne!(first.id, second.id);
        assert_eq!(first_stopped.id, first.id);
        assert_eq!(first_stopped.end, Some(at(150)));
        assert_eq!(second.start, at(200));
        assert_eq!(second.end, None);
        assert_eq!(tracker.active().map(|e| e.id), Some(second.id));
    }

    #[test]
    fn switch_when_idle_is_rejected() {
        let mut tracker = Tracker::idle();
        let error = tracker
            .switch(&task(1), at(100), at(100))
            .expect_err("nothing runs");
        assert_eq!(error, TrackingError::NotRunning);
    }

    #[test]
    fn switch_to_the_active_task_is_rejected() {
        let mut tracker = Tracker::idle();
        let started = tracker.start(&task(1), at(100)).unwrap();
        let error = tracker
            .switch(&task(1), at(110), at(110))
            .expect_err("same task switch must fail");
        assert_eq!(error, TrackingError::TaskAlreadyActive { id: task_id(1) });
        assert_eq!(tracker.active().map(|e| e.id), Some(started.id));
    }

    #[test]
    fn switch_to_an_archived_task_is_rejected() {
        let mut tracker = Tracker::idle();
        let started = tracker.start(&task(1), at(100)).unwrap();
        let error = tracker
            .switch(&archived_task(2), at(110), at(110))
            .expect_err("archived target must fail");
        assert_eq!(error, TrackingError::TaskArchived { id: task_id(2) });
        assert_eq!(tracker.active().map(|e| e.id), Some(started.id));
    }

    #[test]
    fn switch_stops_the_old_entry_and_starts_the_new_one() {
        let mut tracker = Tracker::idle();
        let first = tracker.start(&task(1), at(100)).unwrap();
        let switched = tracker.switch(&task(2), at(150), at(160)).unwrap();
        assert_eq!(switched.stopped.id, first.id);
        assert_eq!(switched.stopped.task_id, task_id(1));
        assert_eq!(switched.stopped.end, Some(at(150)));
        assert_eq!(switched.started.task_id, task_id(2));
        assert_eq!(switched.started.start, at(160));
        assert_eq!(switched.started.end, None);
        assert_ne!(switched.stopped.id, switched.started.id);
        assert_eq!(tracker.active().map(|e| e.id), Some(switched.started.id));
        assert_eq!(tracker.active().map(|e| e.task_id), Some(task_id(2)));
    }

    #[test]
    fn switch_accepts_matching_boundaries() {
        let mut tracker = Tracker::idle();
        tracker.start(&task(1), at(100)).unwrap();
        let switched = tracker.switch(&task(2), at(100), at(100)).unwrap();
        assert_eq!(switched.stopped.end, Some(at(100)));
        assert_eq!(switched.started.start, at(100));
    }

    #[test]
    fn switch_with_stop_before_the_active_start_is_rejected_and_rolls_back() {
        let mut tracker = Tracker::idle();
        let started = tracker.start(&task(1), at(100)).unwrap();
        let error = tracker
            .switch(&task(2), at(99), at(150))
            .expect_err("stop precedes start");
        assert_eq!(
            error,
            TrackingError::StopBeforeStart {
                entry_id: started.id,
                start: at(100),
                stop_at: at(99),
            }
        );
        assert_eq!(tracker.active().map(|e| e.id), Some(started.id));
    }

    #[test]
    fn switch_with_start_before_stop_is_rejected_and_rolls_back() {
        let mut tracker = Tracker::idle();
        let started = tracker.start(&task(1), at(100)).unwrap();
        let error = tracker
            .switch(&task(2), at(150), at(149))
            .expect_err("start precedes stop");
        assert_eq!(
            error,
            TrackingError::StartBeforeStop {
                stop_at: at(150),
                start_at: at(149),
            }
        );
        assert_eq!(tracker.active().map(|e| e.id), Some(started.id));
        assert_eq!(tracker.active().map(|e| e.task_id), Some(task_id(1)));
    }

    #[test]
    fn toggle_while_idle_starts_the_task() {
        let mut tracker = Tracker::idle();
        let outcome = tracker.toggle(&task(1), at(100)).unwrap();
        match outcome {
            TrackingOutcome::Started { entry } => {
                assert_eq!(entry.task_id, task_id(1));
                assert_eq!(entry.start, at(100));
                assert_eq!(entry.end, None);
            }
            other => panic!("expected Started, got {other:?}"),
        }
        assert!(tracker.active().is_some());
    }

    #[test]
    fn toggling_the_active_task_stops_it() {
        let mut tracker = Tracker::idle();
        let started = tracker.start(&task(1), at(100)).unwrap();
        let outcome = tracker.toggle(&task(1), at(150)).unwrap();
        match outcome {
            TrackingOutcome::Stopped { entry } => {
                assert_eq!(entry.id, started.id);
                assert_eq!(entry.end, Some(at(150)));
            }
            other => panic!("expected Stopped, got {other:?}"),
        }
        assert_eq!(tracker.state(), &TrackingState::Idle);
    }

    #[test]
    fn toggling_another_task_switches_at_one_instant() {
        let mut tracker = Tracker::idle();
        let started = tracker.start(&task(1), at(100)).unwrap();
        let outcome = tracker.toggle(&task(2), at(150)).unwrap();
        match outcome {
            TrackingOutcome::Switched { stopped, started } => {
                assert_eq!(stopped.end, Some(at(150)));
                assert_eq!(started.start, at(150));
                assert_eq!(started.end, None);
            }
            other => panic!("expected Switched, got {other:?}"),
        }
        assert_eq!(tracker.active().map(|e| e.task_id), Some(task_id(2)));
        assert_ne!(tracker.active().map(|e| e.id), Some(started.id));
    }

    #[test]
    fn toggling_an_archived_task_while_idle_is_rejected() {
        let mut tracker = Tracker::idle();
        let error = tracker
            .toggle(&archived_task(1), at(100))
            .expect_err("archived task must not start");
        assert_eq!(error, TrackingError::TaskArchived { id: task_id(1) });
    }

    #[test]
    fn ensure_archivable_only_rejects_the_active_task() {
        let mut tracker = Tracker::idle();
        assert_eq!(tracker.ensure_archivable(task_id(1)), Ok(()));
        tracker.start(&task(1), at(100)).unwrap();
        assert_eq!(
            tracker.ensure_archivable(task_id(1)),
            Err(TrackingError::TaskIsActive { id: task_id(1) })
        );
        assert_eq!(tracker.ensure_archivable(task_id(2)), Ok(()));
    }

    #[test]
    fn resume_accepts_an_active_entry() {
        let entry = TimeEntry::begin(entry_id(1), task_id(2), at(100));
        let tracker = Tracker::resume(entry).unwrap();
        assert_eq!(
            tracker.state(),
            &TrackingState::Running {
                entry: ActiveEntry {
                    id: entry_id(1),
                    task_id: task_id(2),
                    start: at(100),
                }
            }
        );
    }

    #[test]
    fn resume_rejects_a_stopped_entry() {
        let entry = TimeEntry::new(entry_id(1), task_id(2), at(100), Some(at(150))).unwrap();
        let error = Tracker::resume(entry).expect_err("stopped entries do not resume");
        assert_eq!(error, TrackingError::EntryNotActive { id: entry_id(1) });
    }
}
