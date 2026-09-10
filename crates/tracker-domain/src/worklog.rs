//! Worklogs.
//!
//! One task has many worklogs. A worklog without an end time is active; at
//! most one worklog is active across the whole tracker.

use chrono::{DateTime, Utc};

use crate::ids::{TaskId, WorklogId};

/// The editable timestamps of a worklog.
///
/// This value carries no identity or task identifier. Callers can use it as
/// an optimistic-concurrency version or as proposed replacement timestamps
/// without gaining a way to move a worklog to another task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorklogTimes {
    start: DateTime<Utc>,
    end: Option<DateTime<Utc>>,
}

impl WorklogTimes {
    /// Creates a timestamp pair. Domain validation happens when the pair is
    /// applied to a worklog.
    pub fn new(start: DateTime<Utc>, end: Option<DateTime<Utc>>) -> Self {
        Self { start, end }
    }

    /// Returns the start timestamp.
    pub fn start(&self) -> DateTime<Utc> {
        self.start
    }

    /// Returns the optional end timestamp.
    pub fn end(&self) -> Option<DateTime<Utc>> {
        self.end
    }

    /// Whether these timestamps describe an active worklog.
    pub fn is_active(&self) -> bool {
        self.end.is_none()
    }
}

/// A recorded interval spent on a task.
///
/// `end` is `None` while the worklog is active. A completed worklog always
/// has an end time that is not earlier than its start time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worklog {
    id: WorklogId,
    task_id: TaskId,
    start: DateTime<Utc>,
    end: Option<DateTime<Utc>>,
}

/// Why a worklog could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WorklogError {
    /// The end time precedes the start time.
    #[error("worklog end must not precede its start")]
    EndBeforeStart,
}

/// Why a worklog correction was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WorklogCorrectionError {
    /// The corrected end time precedes the corrected start time.
    #[error("corrected worklog end must not precede its start")]
    EndBeforeStart,
    /// A correction tried to complete an active worklog or reactivate a
    /// completed one.
    #[error("a correction must preserve whether the worklog is active or completed")]
    CompletionStateChanged,
    /// The corrected start is later than the operation timestamp.
    #[error("corrected worklog start must not be later than occurred_at")]
    StartAfterOccurredAt,
    /// The corrected end is later than the operation timestamp.
    #[error("corrected worklog end must not be later than occurred_at")]
    EndAfterOccurredAt,
}

impl Worklog {
    /// Builds a worklog from stored values, rejecting an end before the start.
    pub fn new(
        id: WorklogId,
        task_id: TaskId,
        start: DateTime<Utc>,
        end: Option<DateTime<Utc>>,
    ) -> Result<Self, WorklogError> {
        Self::validate_order(start, end)?;
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

    /// Returns the stable worklog identifier.
    pub fn id(&self) -> WorklogId {
        self.id
    }

    /// Returns the task this time belongs to.
    pub fn task_id(&self) -> TaskId {
        self.task_id
    }

    /// Returns the start timestamp.
    pub fn start(&self) -> DateTime<Utc> {
        self.start
    }

    /// Returns the optional end timestamp.
    pub fn end(&self) -> Option<DateTime<Utc>> {
        self.end
    }

    /// Whether this worklog is active.
    pub fn is_active(&self) -> bool {
        self.end.is_none()
    }

    /// Returns the timestamps used for optimistic concurrency.
    pub fn times(&self) -> WorklogTimes {
        WorklogTimes::new(self.start, self.end)
    }

    /// Whether this worklog overlaps another worklog for the same task.
    ///
    /// Intervals are half-open. Touching boundaries do not overlap, completed
    /// zero-duration intervals overlap nothing, and active intervals extend
    /// indefinitely. Worklogs for different tasks never overlap.
    pub fn overlaps(&self, other: &Self) -> bool {
        if self.task_id != other.task_id
            || self.end == Some(self.start)
            || other.end == Some(other.start)
        {
            return false;
        }

        let self_starts_before_other_ends = other.end.is_none_or(|end| self.start < end);
        let other_starts_before_self_ends = self.end.is_none_or(|end| other.start < end);
        self_starts_before_other_ends && other_starts_before_self_ends
    }

    /// Returns a corrected copy while preserving identity, task, and active
    /// state.
    ///
    /// Completed worklogs may change both timestamps. Active worklogs may
    /// change only their start because a correction cannot stop or reactivate
    /// a worklog. Corrected timestamps cannot be later than `occurred_at`.
    pub fn corrected(
        &self,
        replacement: WorklogTimes,
        occurred_at: DateTime<Utc>,
    ) -> Result<Self, WorklogCorrectionError> {
        if self.is_active() != replacement.is_active() {
            return Err(WorklogCorrectionError::CompletionStateChanged);
        }
        if replacement
            .end()
            .is_some_and(|end| end < replacement.start())
        {
            return Err(WorklogCorrectionError::EndBeforeStart);
        }
        if replacement.start() > occurred_at {
            return Err(WorklogCorrectionError::StartAfterOccurredAt);
        }
        if replacement.end().is_some_and(|end| end > occurred_at) {
            return Err(WorklogCorrectionError::EndAfterOccurredAt);
        }
        Ok(Self {
            id: self.id,
            task_id: self.task_id,
            start: replacement.start(),
            end: replacement.end(),
        })
    }

    fn validate_order(
        start: DateTime<Utc>,
        end: Option<DateTime<Utc>>,
    ) -> Result<(), WorklogError> {
        if end.is_some_and(|end| end < start) {
            return Err(WorklogError::EndBeforeStart);
        }
        Ok(())
    }
}

/// The worklog that is currently running.
///
/// This type cannot represent a stopped worklog, so the tracker's running
/// state keeps the "end is unset" invariant by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveWorklog {
    id: WorklogId,
    task_id: TaskId,
    start: DateTime<Utc>,
}

impl ActiveWorklog {
    /// Starts tracking the given task at the given instant.
    pub fn begin(id: WorklogId, task_id: TaskId, start: DateTime<Utc>) -> Self {
        Self { id, task_id, start }
    }

    /// Returns the identifier of the underlying worklog.
    pub fn id(&self) -> WorklogId {
        self.id
    }

    /// Returns the task being tracked.
    pub fn task_id(&self) -> TaskId {
        self.task_id
    }

    /// Returns when tracking started.
    pub fn start(&self) -> DateTime<Utc> {
        self.start
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
    fn new_accepts_active_completed_and_zero_duration_worklogs() {
        let active = Worklog::new(worklog_id(1), task_id(1), at(100), None).unwrap();
        assert_eq!(active.end(), None);
        let boundary = Worklog::new(worklog_id(2), task_id(1), at(100), Some(at(100))).unwrap();
        assert_eq!(boundary.end(), Some(at(100)));
        let completed = Worklog::new(worklog_id(3), task_id(1), at(100), Some(at(200))).unwrap();
        assert_eq!(completed.end(), Some(at(200)));
    }

    #[test]
    fn new_rejects_end_before_start() {
        let error = Worklog::new(worklog_id(1), task_id(1), at(200), Some(at(199)))
            .expect_err("backwards interval must fail");
        assert_eq!(error, WorklogError::EndBeforeStart);
    }

    #[test]
    fn accessors_expose_an_active_worklog_without_allowing_mutation() {
        let worklog = Worklog::begin(worklog_id(1), task_id(2), at(100));
        assert_eq!(worklog.id(), worklog_id(1));
        assert_eq!(worklog.task_id(), task_id(2));
        assert_eq!(worklog.start(), at(100));
        assert_eq!(worklog.end(), None);
        assert!(worklog.is_active());
        assert_eq!(worklog.times(), WorklogTimes::new(at(100), None));
    }

    #[test]
    fn overlapping_same_task_intervals_are_detected_in_both_directions() {
        let first = Worklog::new(worklog_id(1), task_id(1), at(100), Some(at(200))).unwrap();
        let second = Worklog::new(worklog_id(2), task_id(1), at(150), Some(at(250))).unwrap();

        assert!(first.overlaps(&second));
        assert!(second.overlaps(&first));
    }

    #[test]
    fn touching_same_task_intervals_do_not_overlap() {
        let first = Worklog::new(worklog_id(1), task_id(1), at(100), Some(at(200))).unwrap();
        let second = Worklog::new(worklog_id(2), task_id(1), at(200), Some(at(300))).unwrap();

        assert!(!first.overlaps(&second));
        assert!(!second.overlaps(&first));
    }

    #[test]
    fn completed_zero_duration_intervals_do_not_overlap() {
        let interval = Worklog::new(worklog_id(1), task_id(1), at(100), Some(at(200))).unwrap();
        let inside = Worklog::new(worklog_id(2), task_id(1), at(150), Some(at(150))).unwrap();
        let same_start = Worklog::new(worklog_id(3), task_id(1), at(100), Some(at(100))).unwrap();

        assert!(!interval.overlaps(&inside));
        assert!(!inside.overlaps(&interval));
        assert!(!interval.overlaps(&same_start));
        assert!(!same_start.overlaps(&interval));
    }

    #[test]
    fn worklogs_for_different_tasks_do_not_overlap() {
        let first = Worklog::new(worklog_id(1), task_id(1), at(100), Some(at(200))).unwrap();
        let second = Worklog::begin(worklog_id(2), task_id(2), at(150));

        assert!(!first.overlaps(&second));
        assert!(!second.overlaps(&first));
    }

    #[test]
    fn active_same_task_intervals_extend_indefinitely() {
        let active = Worklog::begin(worklog_id(1), task_id(1), at(100));
        let later = Worklog::new(worklog_id(2), task_id(1), at(10_000), Some(at(10_001))).unwrap();
        let earlier = Worklog::new(worklog_id(3), task_id(1), at(50), Some(at(100))).unwrap();

        assert!(active.overlaps(&later));
        assert!(later.overlaps(&active));
        assert!(!active.overlaps(&earlier));
        assert!(!earlier.overlaps(&active));
    }

    #[test]
    fn completed_correction_changes_both_times_and_preserves_identity() {
        let original = Worklog::new(worklog_id(1), task_id(2), at(100), Some(at(200))).unwrap();
        let corrected = original
            .corrected(WorklogTimes::new(at(120), Some(at(220))), at(300))
            .unwrap();

        assert_eq!(corrected.id(), original.id());
        assert_eq!(corrected.task_id(), original.task_id());
        assert_eq!(corrected.start(), at(120));
        assert_eq!(corrected.end(), Some(at(220)));
    }

    #[test]
    fn active_correction_changes_only_the_start_and_preserves_identity() {
        let original = Worklog::begin(worklog_id(1), task_id(2), at(100));
        let corrected = original
            .corrected(WorklogTimes::new(at(120), None), at(300))
            .unwrap();

        assert_eq!(corrected.id(), original.id());
        assert_eq!(corrected.task_id(), original.task_id());
        assert_eq!(corrected.start(), at(120));
        assert_eq!(corrected.end(), None);
    }

    #[test]
    fn correction_cannot_change_active_or_completed_shape() {
        let active = Worklog::begin(worklog_id(1), task_id(1), at(100));
        assert_eq!(
            active.corrected(WorklogTimes::new(at(100), Some(at(200))), at(300)),
            Err(WorklogCorrectionError::CompletionStateChanged)
        );

        let completed = Worklog::new(worklog_id(2), task_id(1), at(100), Some(at(200))).unwrap();
        assert_eq!(
            completed.corrected(WorklogTimes::new(at(100), None), at(300)),
            Err(WorklogCorrectionError::CompletionStateChanged)
        );
    }

    #[test]
    fn correction_accepts_zero_duration_and_rejects_end_before_start() {
        let completed = Worklog::new(worklog_id(1), task_id(1), at(100), Some(at(200))).unwrap();
        assert!(
            completed
                .corrected(WorklogTimes::new(at(150), Some(at(150))), at(300))
                .is_ok()
        );
        assert_eq!(
            completed.corrected(WorklogTimes::new(at(151), Some(at(150))), at(300)),
            Err(WorklogCorrectionError::EndBeforeStart)
        );
    }

    #[test]
    fn correction_accepts_timestamps_equal_to_occurred_at() {
        let occurred_at = at(300);
        let completed = Worklog::new(worklog_id(1), task_id(1), at(100), Some(at(200))).unwrap();

        let corrected = completed
            .corrected(
                WorklogTimes::new(occurred_at, Some(occurred_at)),
                occurred_at,
            )
            .unwrap();

        assert_eq!(corrected.start(), occurred_at);
        assert_eq!(corrected.end(), Some(occurred_at));
    }

    #[test]
    fn correction_rejects_each_future_timestamp() {
        let active = Worklog::begin(worklog_id(1), task_id(1), at(100));
        assert_eq!(
            active.corrected(WorklogTimes::new(at(301), None), at(300)),
            Err(WorklogCorrectionError::StartAfterOccurredAt)
        );

        let completed = Worklog::new(worklog_id(2), task_id(1), at(100), Some(at(200))).unwrap();
        assert_eq!(
            completed.corrected(WorklogTimes::new(at(301), Some(at(301))), at(300)),
            Err(WorklogCorrectionError::StartAfterOccurredAt)
        );
        assert_eq!(
            completed.corrected(WorklogTimes::new(at(100), Some(at(301))), at(300)),
            Err(WorklogCorrectionError::EndAfterOccurredAt)
        );
    }

    #[test]
    fn active_stop_accepts_start_and_later_instants() {
        let active = ActiveWorklog::begin(worklog_id(1), task_id(2), at(100));
        assert_eq!(active.id(), worklog_id(1));
        assert_eq!(active.task_id(), task_id(2));
        assert_eq!(active.start(), at(100));
        let stopped = active.clone().stop(at(100)).unwrap();
        assert_eq!(stopped.end(), Some(at(100)));
        let stopped = active.stop(at(150)).unwrap();
        assert_eq!(stopped.end(), Some(at(150)));
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
        assert_eq!(worklog.id(), worklog_id(1));
        assert_eq!(worklog.task_id(), task_id(2));
        assert_eq!(worklog.start(), at(100));
        assert_eq!(worklog.end(), None);
    }
}
