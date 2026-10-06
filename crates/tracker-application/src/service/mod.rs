//! Application services and client-facing use-case contracts.

mod reports;
mod tasks;
mod tracking;
mod worklogs;

pub use worklogs::move_candidates_for_tasks;

use chrono::{DateTime, Utc};
use tracker_domain::{
    Task, TaskId, TaskName, Tracker, TrackingState, Worklog, WorklogId, WorklogTimes,
};

use crate::{
    ApplicationError, ClearActiveTaskOutcome, GlobalWorklogCursor, GlobalWorklogPage, ReportTotals,
    RepositoryError, SetActiveTaskOutcome, TaskListItem, TaskOrdering, TrackerRepository,
    TrackerSnapshot, WorklogCorrection, WorklogCursor, WorklogDeletion, WorklogMove, WorklogPage,
    WorklogPageSnapshot,
};

fn canonical_timestamp(timestamp: DateTime<Utc>) -> DateTime<Utc> {
    DateTime::from_timestamp_micros(timestamp.timestamp_micros())
        .expect("every UTC timestamp fits the canonical microsecond range")
}

fn canonical_worklog_times(times: WorklogTimes) -> WorklogTimes {
    WorklogTimes::new(
        canonical_timestamp(times.start()),
        times.end().map(canonical_timestamp),
    )
}

/// Task queries available to presentation and transport layers.
///
/// The service keeps its task snapshot current on every committed write, so
/// querying never reads the backend again and cannot fail.
pub trait TaskQueries {
    /// The tasks of the selected backend, ordered by the given ordering.
    fn tasks(&self, ordering: TaskOrdering) -> Vec<TaskListItem>;
    fn task(&self, id: TaskId) -> Option<&Task>;
}

/// Reports time by task for a half-open UTC interval.
pub trait ReportQueries {
    /// Clips completed work to the interval and ends open work at `now`.
    /// A successful read also refreshes task and tracking queries from the
    /// same backend snapshot.
    fn report_totals(
        &mut self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<ReportTotals, ApplicationError>;
}

/// Commands that create or change tasks.
///
/// Every operation takes the client-created instant it occurred at; the
/// domain decides how that instant moves `updated_at`.
pub trait TaskOperations {
    fn create_task(
        &mut self,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError>;
    fn rename_task(
        &mut self,
        id: TaskId,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError>;
    fn archive_task(
        &mut self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError>;
    fn unarchive_task(
        &mut self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError>;

    /// Lists active tasks created more than 14 days before `as_of` with no
    /// worklog overlapping the preceding 14 days.
    fn preview_inactive_tasks(
        &mut self,
        as_of: DateTime<Utc>,
    ) -> Result<Vec<Task>, ApplicationError>;

    /// Archives the previewed set only if it is still the complete eligible set.
    fn archive_inactive_tasks(
        &mut self,
        expected_ids: &[TaskId],
        as_of: DateTime<Utc>,
    ) -> Result<Vec<Task>, ApplicationError>;
}

/// Current tracking state and desired-state tracking commands.
pub trait TrackingOperations {
    fn current_tracking(&self) -> &TrackingState;
    fn set_active_task(
        &mut self,
        task_id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, ApplicationError>;
    fn clear_active_task(
        &mut self,
        expected_active: WorklogId,
        occurred_at: DateTime<Utc>,
    ) -> Result<ClearActiveTaskOutcome, ApplicationError>;
}

/// Queries for worklog history and move destinations. History page reads
/// adopt the task and tracking snapshot read with the page before returning.
pub trait WorklogQueries: TaskQueries {
    /// Eligible destinations from the current task snapshot, fuzzy matched and
    /// ordered by recent activity, creation time, then task identity.
    fn move_candidates(&self, source_task_id: TaskId, query: &str) -> Vec<crate::MoveCandidate> {
        move_candidates_for_tasks(
            &self.tasks(TaskOrdering::RecentlyWorked),
            source_task_id,
            query,
        )
    }

    /// One bounded page of the task's worklog history, continuing strictly
    /// after the optional cursor. The page size is the fixed
    /// [`crate::WORKLOG_PAGE_SIZE`]; callers cannot request more.
    fn worklogs_for_task(
        &mut self,
        task_id: TaskId,
        after: Option<&WorklogCursor>,
    ) -> Result<WorklogPage, ApplicationError>;

    /// One bounded page across every task, including archived tasks.
    fn all_worklogs(
        &mut self,
        after: Option<&GlobalWorklogCursor>,
    ) -> Result<GlobalWorklogPage, ApplicationError>;
}

/// Commands that change existing worklog history.
pub trait WorklogOperations {
    /// Moves a worklog to a different task if its selected task and timestamps
    /// still match the stored row.
    ///
    /// The operation canonicalizes timestamps to UTC microseconds. It
    /// preserves the worklog's identity, timestamps, and active state. Stale
    /// and failed writes reload authoritative state before returning an error.
    fn move_worklog(
        &mut self,
        id: WorklogId,
        expected_source_task_id: TaskId,
        expected: WorklogTimes,
        destination_task_id: TaskId,
    ) -> Result<Worklog, ApplicationError>;

    /// Replaces a worklog's timestamps if the stored timestamps still match
    /// `expected`.
    ///
    /// The operation canonicalizes expected, replacement, and operation
    /// timestamps to UTC microseconds. It cannot change the worklog's
    /// identity, task, or active state. Stale and failed writes return an
    /// error after an authoritative state reload, so callers can keep their
    /// own replacement draft.
    fn correct_worklog(
        &mut self,
        id: WorklogId,
        expected: WorklogTimes,
        replacement: WorklogTimes,
        occurred_at: DateTime<Utc>,
    ) -> Result<Worklog, ApplicationError>;

    /// Permanently deletes a completed worklog if its task and timestamps
    /// still match the selected row. The timestamps are canonicalized to UTC
    /// microseconds before comparison. An expected active worklog is rejected
    /// before the repository call. Active worklogs are never deleted.
    fn delete_completed_worklog(
        &mut self,
        id: WorklogId,
        expected_task_id: TaskId,
        expected: WorklogTimes,
    ) -> Result<Worklog, ApplicationError>;
}

/// The application service required by presentation and transport clients.
///
/// This keeps repository ports behind the application boundary.
pub trait TrackerApplicationService:
    TaskQueries
    + TaskOperations
    + TrackingOperations
    + WorklogQueries
    + WorklogOperations
    + ReportQueries
{
}

impl<T> TrackerApplicationService for T where
    T: TaskQueries
        + TaskOperations
        + TrackingOperations
        + WorklogQueries
        + WorklogOperations
        + ReportQueries
{
}

/// Stateful application service backed by one repository.
///
/// The service owns persistence sequencing. It keeps the task snapshot and
/// current tracking queries ready for a synchronous client, updates the
/// snapshot itself on every committed write instead of reading the backend
/// again behind an ambiguous result, commits switches through the
/// repository's atomic operation, and reloads authoritative state after any
/// tracking write conflict.
pub struct TrackerApplication<R> {
    repository: R,
    /// The snapshot in canonical `TaskId` order; queries sort it per the
    /// requested ordering.
    tasks: Vec<TaskListItem>,
    tracker: Tracker,
}

impl<R: TrackerRepository> TrackerApplication<R> {
    /// Loads task and tracking state from one coherent backend snapshot.
    pub fn load(repository: R) -> Result<Self, ApplicationError> {
        let snapshot = repository.tracker_snapshot()?;
        let (tasks, tracker) = Self::state_from_snapshot(snapshot)?;
        Ok(Self {
            repository,
            tasks,
            tracker,
        })
    }

    /// Replaces cached task and tracking state with one authoritative read.
    /// The server uses this after an uncertain write outcome.
    pub fn refresh_authoritative_state(&mut self) -> Result<(), ApplicationError> {
        self.refresh_tracking()
    }

    pub(crate) fn state_from_snapshot(
        mut snapshot: TrackerSnapshot,
    ) -> Result<(Vec<TaskListItem>, Tracker), ApplicationError> {
        snapshot.task_items.sort_by_key(|item| item.task.id());
        let tracker = match snapshot.active_worklog {
            Some(worklog) => Tracker::resume(worklog)?,
            None => Tracker::idle(),
        };
        Self::align_active_work(&mut snapshot.task_items, &tracker);
        Ok((snapshot.task_items, tracker))
    }

    fn align_active_work(items: &mut [TaskListItem], tracker: &Tracker) {
        if let Some(active) = tracker.active()
            && let Some(item) = items
                .iter_mut()
                .find(|item| item.task.id() == active.task_id())
        {
            item.latest_work_start = Some(
                item.latest_work_start
                    .map_or(active.start(), |old| old.max(active.start())),
            );
        }
    }

    fn set_latest_work_start(&mut self, task_id: TaskId, latest_work_start: Option<DateTime<Utc>>) {
        if let Some(item) = self.tasks.iter_mut().find(|item| item.task.id() == task_id) {
            item.latest_work_start = latest_work_start;
        }
    }

    fn adopt_active_worklog_snapshot(
        &mut self,
        active_worklog: Option<Worklog>,
        active_task_latest_work_start: Option<DateTime<Utc>>,
    ) -> Result<(), ApplicationError> {
        self.tracker = match active_worklog {
            Some(worklog) => Tracker::resume(worklog)?,
            None => Tracker::idle(),
        };
        if let Some(active) = self.tracker.active() {
            self.set_latest_work_start(active.task_id(), active_task_latest_work_start);
        }
        Ok(())
    }

    fn adopt_worklog_page_snapshot(
        &mut self,
        task_id: TaskId,
        snapshot: WorklogPageSnapshot,
    ) -> Result<(), ApplicationError> {
        self.set_latest_work_start(task_id, snapshot.requested_task_latest_work_start);
        self.adopt_active_worklog_snapshot(
            snapshot.active_worklog,
            snapshot.active_task_latest_work_start,
        )
    }

    fn adopt_snapshot(&mut self, snapshot: TrackerSnapshot) -> Result<(), ApplicationError> {
        let (tasks, tracker) = Self::state_from_snapshot(snapshot)?;
        self.tasks = tasks;
        self.tracker = tracker;
        Ok(())
    }

    fn refresh_tracking(&mut self) -> Result<(), ApplicationError> {
        self.adopt_snapshot(self.repository.tracker_snapshot()?)
    }

    fn reload_authoritative_state(&mut self) -> Result<(), RepositoryError> {
        let snapshot = self.repository.tracker_snapshot()?;
        self.adopt_snapshot(snapshot)
            .map_err(|_| RepositoryError::CorruptData {
                field: "active worklog",
            })
    }

    fn recover_after_tracking_write(&mut self, write_error: RepositoryError) -> ApplicationError {
        match self.reload_authoritative_state() {
            Ok(()) if matches!(write_error, RepositoryError::WorklogChanged { .. }) => {
                ApplicationError::TrackingStateChanged
            }
            Ok(()) => ApplicationError::TrackingWrite(write_error),
            Err(error) => ApplicationError::TrackingRecovery(error),
        }
    }

    fn recover_after_worklog_correction(&mut self, write: RepositoryError) -> ApplicationError {
        match self.reload_authoritative_state() {
            Ok(()) => ApplicationError::WorklogCorrectionWrite { write },
            Err(recovery) => ApplicationError::WorklogCorrectionRecovery { write, recovery },
        }
    }

    fn recover_after_worklog_move(&mut self, write: RepositoryError) -> ApplicationError {
        match self.reload_authoritative_state() {
            Ok(()) => ApplicationError::WorklogMoveWrite { write },
            Err(recovery) => ApplicationError::WorklogMoveRecovery { write, recovery },
        }
    }

    fn recover_after_worklog_deletion(&mut self, write: RepositoryError) -> ApplicationError {
        match self.reload_authoritative_state() {
            Ok(()) => ApplicationError::WorklogDeletionWrite { write },
            Err(recovery) => ApplicationError::WorklogDeletionRecovery { write, recovery },
        }
    }

    fn adopt_worklog_correction(
        &mut self,
        correction: WorklogCorrection,
    ) -> Result<Worklog, ApplicationError> {
        self.set_latest_work_start(
            correction.worklog.task_id(),
            correction.task_latest_work_start,
        );
        let worklog = correction.worklog;
        self.adopt_active_worklog_snapshot(
            correction.active_worklog,
            correction.active_task_latest_work_start,
        )?;
        Ok(worklog)
    }

    fn adopt_worklog_move(
        &mut self,
        source_task_id: TaskId,
        movement: WorklogMove,
    ) -> Result<Worklog, ApplicationError> {
        self.set_latest_work_start(source_task_id, movement.source_task_latest_work_start);
        self.set_latest_work_start(
            movement.worklog.task_id(),
            movement.destination_task_latest_work_start,
        );
        let worklog = movement.worklog;
        self.adopt_active_worklog_snapshot(
            movement.active_worklog,
            movement.active_task_latest_work_start,
        )?;
        Ok(worklog)
    }

    fn adopt_worklog_deletion(&mut self, deletion: WorklogDeletion) -> Worklog {
        self.set_latest_work_start(deletion.worklog.task_id(), deletion.task_latest_work_start);
        deletion.worklog
    }

    fn refresh_after_task_operation(&mut self) -> Result<(), ApplicationError> {
        match self.refresh_tracking() {
            Err(ApplicationError::Repository(error)) => Err(ApplicationError::TaskRecovery(error)),
            result => result,
        }
    }

    fn replace_task(&mut self, changed: Task) {
        if let Some(item) = self
            .tasks
            .iter_mut()
            .find(|item| item.task.id() == changed.id())
        {
            item.task = changed;
        } else {
            self.tasks.push(TaskListItem {
                task: changed,
                latest_work_start: None,
            });
            self.tasks.sort_by_key(|a| a.task.id());
        }
    }

    /// Records that a worklog started on a task at `at`, keeping the
    /// snapshot's latest-work value equal to the stored `MAX(start)`
    /// without reading the backend again.
    fn note_work_start(&mut self, task_id: TaskId, at: DateTime<Utc>) {
        if let Some(item) = self.tasks.iter_mut().find(|item| item.task.id() == task_id) {
            item.latest_work_start = Some(item.latest_work_start.map_or(at, |old| old.max(at)));
        }
    }
}

#[cfg(test)]
mod timestamp_range_tests {
    use chrono::{DateTime, Utc};

    use super::canonical_timestamp;

    #[test]
    fn chrono_utc_extremes_fit_the_canonical_microsecond_range() {
        for instant in [DateTime::<Utc>::MIN_UTC, DateTime::<Utc>::MAX_UTC] {
            assert_eq!(
                canonical_timestamp(instant).timestamp_micros(),
                instant.timestamp_micros()
            );
        }
    }
}
