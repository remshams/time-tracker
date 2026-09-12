use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use tracker_domain::{ActiveWorklog, TaskId, TrackingState, Worklog, WorklogId};

use crate::support::clock::ElapsedClock;

/// Active tracking state paired with its monotonic elapsed-time anchor.
pub(crate) struct TrackingSession {
    tracking: TrackingState,
    elapsed_clock: Option<ElapsedClock>,
    clock: Clock,
}

impl TrackingSession {
    pub(crate) fn new(tracking: TrackingState) -> Self {
        Self::with_clock(tracking, Clock::system())
    }

    fn with_clock(tracking: TrackingState, clock: Clock) -> Self {
        let elapsed_clock = match &tracking {
            TrackingState::Idle => None,
            TrackingState::Running { worklog } => Some(ElapsedClock::at_anchor(
                ElapsedClock::base_since(worklog.start(), clock.wall_clock()),
                clock.monotonic_clock(),
            )),
        };
        Self {
            tracking,
            elapsed_clock,
            clock,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_test_clock(
        tracking: TrackingState,
        wall_clock: DateTime<Utc>,
    ) -> (Self, TestClock) {
        let (clock, handle) = Clock::controlled(wall_clock);
        (Self::with_clock(tracking, clock), handle)
    }

    pub(crate) fn active_worklog(&self) -> Option<&ActiveWorklog> {
        match &self.tracking {
            TrackingState::Idle => None,
            TrackingState::Running { worklog } => Some(worklog),
        }
    }

    pub(crate) fn active_worklog_id(&self) -> Option<WorklogId> {
        self.active_worklog().map(ActiveWorklog::id)
    }

    pub(crate) fn active_task_id(&self) -> Option<TaskId> {
        self.active_worklog().map(ActiveWorklog::task_id)
    }

    pub(crate) fn elapsed(&self) -> Option<Duration> {
        self.elapsed_clock
            .as_ref()
            .map(|clock| clock.elapsed_at(self.clock.monotonic_clock()))
    }

    pub(crate) fn row_duration(&self, worklog: &Worklog) -> Duration {
        let Some(end) = worklog.end() else {
            return if self.active_worklog_id() == Some(worklog.id()) {
                self.elapsed().unwrap_or(Duration::ZERO)
            } else {
                Duration::ZERO
            };
        };
        (end - worklog.start()).to_std().unwrap_or(Duration::ZERO)
    }

    pub(crate) fn sync(&mut self, tracking: TrackingState, fresh_active: bool) {
        let unchanged = tracking == self.tracking;
        if fresh_active {
            self.elapsed_clock = match &tracking {
                TrackingState::Idle => None,
                TrackingState::Running { .. } => Some(ElapsedClock::at_anchor(
                    Duration::ZERO,
                    self.clock.monotonic_clock(),
                )),
            };
        } else if !unchanged {
            self.elapsed_clock = match &tracking {
                TrackingState::Idle => None,
                TrackingState::Running { worklog } => Some(ElapsedClock::at_anchor(
                    ElapsedClock::base_since(worklog.start(), self.clock.wall_clock()),
                    self.clock.monotonic_clock(),
                )),
            };
        }
        self.tracking = tracking;
    }

    pub(crate) fn sync_after_history_reload(&mut self, tracking: TrackingState) {
        match (&self.tracking, &tracking) {
            (
                TrackingState::Running { worklog: current },
                TrackingState::Running {
                    worklog: final_worklog,
                },
            ) if current.id() == final_worklog.id() && current.start() == final_worklog.start() => {
            }
            (_, TrackingState::Idle) => self.elapsed_clock = None,
            (_, TrackingState::Running { worklog }) => {
                self.elapsed_clock = Some(ElapsedClock::at_anchor(
                    ElapsedClock::base_since(worklog.start(), self.clock.wall_clock()),
                    self.clock.monotonic_clock(),
                ));
            }
        }
        self.tracking = tracking;
    }
}

enum Clock {
    System,
    #[cfg(test)]
    Controlled(std::rc::Rc<std::cell::RefCell<ControlledClock>>),
}

impl Clock {
    fn system() -> Self {
        Self::System
    }

    fn wall_clock(&self) -> DateTime<Utc> {
        match self {
            Self::System => Utc::now(),
            #[cfg(test)]
            Self::Controlled(clock) => clock.borrow().wall_clock,
        }
    }

    fn monotonic_clock(&self) -> Instant {
        match self {
            Self::System => Instant::now(),
            #[cfg(test)]
            Self::Controlled(clock) => clock.borrow().monotonic_clock,
        }
    }

    #[cfg(test)]
    fn controlled(wall_clock: DateTime<Utc>) -> (Self, TestClock) {
        let clock = std::rc::Rc::new(std::cell::RefCell::new(ControlledClock {
            wall_clock,
            monotonic_clock: Instant::now(),
        }));
        (Self::Controlled(clock.clone()), TestClock { clock })
    }
}

#[cfg(test)]
struct ControlledClock {
    wall_clock: DateTime<Utc>,
    monotonic_clock: Instant,
}

/// Test-only handle for advancing a tracking session's construction-time clock.
#[cfg(test)]
#[derive(Clone)]
pub(crate) struct TestClock {
    clock: std::rc::Rc<std::cell::RefCell<ControlledClock>>,
}

#[cfg(test)]
impl TestClock {
    pub(crate) fn advance_monotonic(&self, elapsed: Duration) {
        let mut clock = self.clock.borrow_mut();
        clock.monotonic_clock += elapsed;
    }

    pub(crate) fn set_wall_clock(&self, wall_clock: DateTime<Utc>) {
        self.clock.borrow_mut().wall_clock = wall_clock;
    }
}
