//! Application services and client-facing use-case contracts.

mod reports;
mod tasks;
mod tracking;
mod worklogs;

pub use worklogs::move_candidates_for_tasks;

use chrono::{DateTime, Utc};
use tracker_domain::{
    Task, TaskId, TaskName, Tracker, TrackingError, TrackingState, Worklog, WorklogId, WorklogTimes,
};

use crate::{
    ActiveTrackingRead, ApplicationError, ClearActiveTaskOutcome, GlobalWorklogCursor,
    GlobalWorklogPage, ReportTotals, RepositoryError, SetActiveTaskOutcome, TaskListItem,
    TaskOrdering, TrackerRepository, WorklogCorrection, WorklogCursor, WorklogDeletion,
    WorklogMove, WorklogPage, WorklogPageAdoption,
};

use crate::timestamps::{canonical_timestamp, canonical_worklog_times};

/// Synchronous application use cases consumed by the TUI and its test service.
///
/// Concrete clients call inherent methods on [`TrackerApplication`].
pub trait TrackerApplicationService {
    /// The tasks of the selected backend, ordered by the given ordering.
    fn tasks(&self, ordering: TaskOrdering) -> Vec<TaskListItem>;

    /// Refreshes the task list and its active-task indication together.
    fn refresh_task_list(&mut self) -> Result<(), ApplicationError>;

    /// Clips completed work to the interval and ends open work at `now`.
    /// Adopts active tracking and metadata for reported tasks from the same
    /// backend read. Full task-list refreshes are explicit client operations.
    fn report_totals(
        &mut self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<ReportTotals, ApplicationError>;

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
    /// Renames only if the persisted name still matches the reviewed name.
    fn rename_task_with_expected_name(
        &mut self,
        id: TaskId,
        expected_name: &TaskName,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError>;
    /// Archives only if the persisted name still matches the reviewed name.
    fn archive_task_with_expected_name(
        &mut self,
        id: TaskId,
        expected_name: &TaskName,
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

    fn current_tracking(&self) -> &TrackingState;
    fn set_active_task(
        &mut self,
        task_id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, ApplicationError>;
    /// Starts or switches only while the reviewed worklog remains active.
    /// `None` requires idle tracking.
    fn set_active_task_with_expected_active(
        &mut self,
        task_id: TaskId,
        expected_active: Option<WorklogId>,
        occurred_at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, ApplicationError>;
    fn clear_active_task(
        &mut self,
        expected_active: WorklogId,
        occurred_at: DateTime<Utc>,
    ) -> Result<ClearActiveTaskOutcome, ApplicationError>;

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

impl<R: TrackerRepository> TrackerApplicationService for TrackerApplication<R> {
    fn rename_task_with_expected_name(
        &mut self,
        id: TaskId,
        expected_name: &TaskName,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        TrackerApplication::rename_task_with_expected_name(
            self,
            id,
            expected_name,
            name,
            occurred_at,
        )
    }

    fn archive_task_with_expected_name(
        &mut self,
        id: TaskId,
        expected_name: &TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        TrackerApplication::archive_task_with_expected_name(self, id, expected_name, occurred_at)
    }

    fn set_active_task_with_expected_active(
        &mut self,
        task_id: TaskId,
        expected_active: Option<WorklogId>,
        occurred_at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, ApplicationError> {
        TrackerApplication::set_active_task_with_expected_active(
            self,
            task_id,
            expected_active,
            occurred_at,
        )
    }

    fn tasks(&self, ordering: TaskOrdering) -> Vec<TaskListItem> {
        TrackerApplication::tasks(self, ordering)
    }

    fn refresh_task_list(&mut self) -> Result<(), ApplicationError> {
        TrackerApplication::refresh_task_list(self)
    }

    fn report_totals(
        &mut self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<ReportTotals, ApplicationError> {
        TrackerApplication::report_totals(self, start, end, now)
    }

    fn create_task(
        &mut self,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        TrackerApplication::create_task(self, name, occurred_at)
    }

    fn rename_task(
        &mut self,
        id: TaskId,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        TrackerApplication::rename_task(self, id, name, occurred_at)
    }

    fn archive_task(
        &mut self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        TrackerApplication::archive_task(self, id, occurred_at)
    }

    fn unarchive_task(
        &mut self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        TrackerApplication::unarchive_task(self, id, occurred_at)
    }

    fn preview_inactive_tasks(
        &mut self,
        as_of: DateTime<Utc>,
    ) -> Result<Vec<Task>, ApplicationError> {
        TrackerApplication::preview_inactive_tasks(self, as_of)
    }

    fn archive_inactive_tasks(
        &mut self,
        expected_ids: &[TaskId],
        as_of: DateTime<Utc>,
    ) -> Result<Vec<Task>, ApplicationError> {
        TrackerApplication::archive_inactive_tasks(self, expected_ids, as_of)
    }

    fn current_tracking(&self) -> &TrackingState {
        TrackerApplication::current_tracking(self)
    }

    fn set_active_task(
        &mut self,
        task_id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, ApplicationError> {
        TrackerApplication::set_active_task(self, task_id, occurred_at)
    }

    fn clear_active_task(
        &mut self,
        expected_active: WorklogId,
        occurred_at: DateTime<Utc>,
    ) -> Result<ClearActiveTaskOutcome, ApplicationError> {
        TrackerApplication::clear_active_task(self, expected_active, occurred_at)
    }

    fn worklogs_for_task(
        &mut self,
        task_id: TaskId,
        after: Option<&WorklogCursor>,
    ) -> Result<WorklogPage, ApplicationError> {
        TrackerApplication::worklogs_for_task(self, task_id, after)
    }

    fn all_worklogs(
        &mut self,
        after: Option<&GlobalWorklogCursor>,
    ) -> Result<GlobalWorklogPage, ApplicationError> {
        TrackerApplication::all_worklogs(self, after)
    }

    fn move_worklog(
        &mut self,
        id: WorklogId,
        expected_source_task_id: TaskId,
        expected: WorklogTimes,
        destination_task_id: TaskId,
    ) -> Result<Worklog, ApplicationError> {
        TrackerApplication::move_worklog(
            self,
            id,
            expected_source_task_id,
            expected,
            destination_task_id,
        )
    }

    fn correct_worklog(
        &mut self,
        id: WorklogId,
        expected: WorklogTimes,
        replacement: WorklogTimes,
        occurred_at: DateTime<Utc>,
    ) -> Result<Worklog, ApplicationError> {
        TrackerApplication::correct_worklog(self, id, expected, replacement, occurred_at)
    }

    fn delete_completed_worklog(
        &mut self,
        id: WorklogId,
        expected_task_id: TaskId,
        expected: WorklogTimes,
    ) -> Result<Worklog, ApplicationError> {
        TrackerApplication::delete_completed_worklog(self, id, expected_task_id, expected)
    }
}

/// Stateful application service backed by one repository.
///
/// The service owns persistence sequencing. It keeps the task catalog and
/// current tracking queries ready for a synchronous client. Most committed
/// writes update the catalog from the stored result instead of reading the
/// backend again. Archive and restore adopt the stored task and then reload
/// task and tracking state. Switches commit through the repository's atomic
/// operation, and any tracking write conflict reloads authoritative state.
pub struct TrackerApplication<R> {
    repository: R,
    /// The task catalog in canonical `TaskId` order; queries sort it per the
    /// requested ordering.
    tasks: Vec<TaskListItem>,
    tracker: Tracker,
}

impl<R: TrackerRepository> TrackerApplication<R> {
    /// Loads task and tracking state from one coherent backend read.
    pub fn load(repository: R) -> Result<Self, ApplicationError> {
        let (tasks, active_worklog) = repository.load_task_tracking_resources()?;
        let (tasks, tracker) = Self::state_from_resources(tasks, active_worklog)?;
        Ok(Self {
            repository,
            tasks,
            tracker,
        })
    }

    /// Reads and adopts one task resource, preserving an authoritative absence.
    pub fn read_task_item(&mut self, id: TaskId) -> Result<Option<TaskListItem>, ApplicationError> {
        let item = self.repository.load_task_item(id)?;
        match &item {
            Some(item) => self.adopt_task_item(item.clone()),
            None => self.tasks.retain(|item| item.task.id() != id),
        }
        Ok(item)
    }

    /// Refreshes task metadata and activity without reading active tracking.
    pub fn refresh_task_catalog(&mut self) -> Result<(), ApplicationError> {
        let mut tasks = self.repository.load_task_catalog()?;
        tasks.sort_by_key(|item| item.task.id());
        self.tasks = tasks;
        Ok(())
    }

    /// Refreshes active tracking without reading task metadata.
    pub fn refresh_tracking_resource(&mut self) -> Result<(), ApplicationError> {
        self.tracker = Self::tracker_from_active_worklog(self.repository.active_worklog()?)?;
        Ok(())
    }

    /// Replaces task and tracking caches together for a coherent workflow.
    pub fn refresh_task_list(&mut self) -> Result<(), ApplicationError> {
        let (tasks, active_worklog) = self.repository.load_task_tracking_resources()?;
        self.adopt_task_tracking_resources(tasks, active_worklog)
    }

    fn state_from_resources(
        mut tasks: Vec<TaskListItem>,
        active_worklog: Option<Worklog>,
    ) -> Result<(Vec<TaskListItem>, Tracker), ApplicationError> {
        tasks.sort_by_key(|item| item.task.id());
        let tracker = Self::tracker_from_active_worklog(active_worklog)?;
        Ok((tasks, tracker))
    }

    /// Resumes the stored active worklog, or returns an idle tracker when none runs.
    fn tracker_from_active_worklog(
        active_worklog: Option<Worklog>,
    ) -> Result<Tracker, TrackingError> {
        match active_worklog {
            Some(worklog) => Tracker::resume(worklog),
            None => Ok(Tracker::idle()),
        }
    }

    fn set_latest_work_start(&mut self, task_id: TaskId, latest_work_start: Option<DateTime<Utc>>) {
        if let Some(item) = self.tasks.iter_mut().find(|item| item.task.id() == task_id) {
            item.latest_work_start = latest_work_start;
        }
    }

    fn adopt_active_worklog(
        &mut self,
        active_worklog: Option<Worklog>,
        active_task_latest_work_start: Option<DateTime<Utc>>,
    ) -> Result<(), ApplicationError> {
        self.tracker = Self::tracker_from_active_worklog(active_worklog)?;
        if let Some(active) = self.tracker.active() {
            self.set_latest_work_start(active.task_id(), active_task_latest_work_start);
        }
        Ok(())
    }

    fn adopt_worklog_page(
        &mut self,
        task_id: TaskId,
        adoption: WorklogPageAdoption,
    ) -> Result<(), ApplicationError> {
        self.set_latest_work_start(task_id, adoption.requested_task_latest_work_start);
        self.adopt_active_worklog(
            adoption.active_worklog,
            adoption.active_task_latest_work_start,
        )
    }

    fn adopt_task_tracking_resources(
        &mut self,
        tasks: Vec<TaskListItem>,
        active_worklog: Option<Worklog>,
    ) -> Result<(), ApplicationError> {
        let (tasks, tracker) = Self::state_from_resources(tasks, active_worklog)?;
        self.tasks = tasks;
        self.tracker = tracker;
        Ok(())
    }

    fn adopt_tracking_read(&mut self, read: ActiveTrackingRead) -> Result<(), ApplicationError> {
        let tracker = Self::tracker_from_active_worklog(read.active_worklog)?;
        if let Some(item) = read.active_task_item {
            self.adopt_task_item(item);
        }
        self.tracker = tracker;
        Ok(())
    }

    fn adopt_task_item(&mut self, item: TaskListItem) {
        let id = item.task.id();
        self.replace_task(item.task);
        self.set_latest_work_start(id, item.latest_work_start);
    }

    /// Reloads like `refresh_task_list`, but reports a stored active
    /// worklog that cannot resume as corrupt data.
    fn reload_authoritative_state(&mut self) -> Result<(), RepositoryError> {
        self.refresh_task_list().map_err(|error| match error {
            ApplicationError::Repository(error) => error,
            _ => RepositoryError::CorruptData {
                field: "active worklog",
            },
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
        self.adopt_active_worklog(
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
        self.adopt_active_worklog(
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
        match self.refresh_task_list() {
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
    /// task catalog's latest-work value equal to the stored `MAX(start)`
    /// without reading the backend again.
    fn note_work_start(&mut self, task_id: TaskId, at: DateTime<Utc>) {
        if let Some(item) = self.tasks.iter_mut().find(|item| item.task.id() == task_id) {
            item.latest_work_start = Some(item.latest_work_start.map_or(at, |old| old.max(at)));
        }
    }
}
