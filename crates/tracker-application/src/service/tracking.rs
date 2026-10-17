//! Tracking state queries and commands.

use chrono::{DateTime, Utc};
use tracker_domain::{SwitchedWorklogs, TaskId, TrackingState, WorklogId};

use super::{TrackerApplication, canonical_timestamp};
use crate::{
    ApplicationError, ClearActiveTaskOutcome, RepositoryError, SetActiveTaskOutcome, TaskListItem,
    TrackerRepository,
};

impl<R: TrackerRepository> TrackerApplication<R> {
    pub fn current_tracking(&self) -> &TrackingState {
        self.tracker.state()
    }

    pub fn set_active_task(
        &mut self,
        task_id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, ApplicationError> {
        self.set_active_task_guarded(task_id, WorklogId::generate(), None, occurred_at)
    }

    /// Generates an identifier for a new worklog while preserving the reviewed active state.
    pub fn set_active_task_with_expected_active(
        &mut self,
        task_id: TaskId,
        expected_active: Option<WorklogId>,
        occurred_at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, ApplicationError> {
        self.set_active_task_guarded(
            task_id,
            WorklogId::generate(),
            Some(expected_active),
            occurred_at,
        )
    }

    /// Stops only the reviewed active worklog after refreshing its persisted start time.
    /// An already idle tracker succeeds without writing; a changed worklog requires a retry.
    pub fn clear_active_task(
        &mut self,
        expected_active: WorklogId,
        occurred_at: DateTime<Utc>,
    ) -> Result<ClearActiveTaskOutcome, ApplicationError> {
        let occurred_at = canonical_timestamp(occurred_at);
        let cached_active = self.tracker.active().cloned();
        self.refresh_tracking()?;
        if cached_active.as_ref().is_some_and(|cached| {
            self.tracker.active().is_some_and(|active| {
                active.id() == cached.id() && active.start() != cached.start()
            })
        }) {
            return Err(ApplicationError::TrackingStateChanged);
        }
        let Some(active) = self.tracker.active() else {
            return Ok(ClearActiveTaskOutcome::AlreadyIdle);
        };
        if active.id() != expected_active {
            return Err(ApplicationError::TrackingStateChanged);
        }

        let mut candidate = self.tracker.clone();
        let stopped = candidate.stop(occurred_at)?;
        match self
            .repository
            .stop_worklog(stopped.id(), stopped.start(), occurred_at)
        {
            Ok(_) => {
                self.tracker = candidate;
                Ok(ClearActiveTaskOutcome::Stopped { worklog: stopped })
            }
            Err(error) => Err(self.recover_after_tracking_write(error)),
        }
    }

    /// Preserves both the caller's worklog identifier and reviewed active state.
    pub fn set_active_task_with_id_and_expected_active(
        &mut self,
        task_id: TaskId,
        worklog_id: WorklogId,
        expected_active: Option<WorklogId>,
        occurred_at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, ApplicationError> {
        self.set_active_task_guarded(task_id, worklog_id, Some(expected_active), occurred_at)
    }

    /// Refreshes tracking before checking the optional reviewed state and applying the transition.
    /// The outer option distinguishes an unguarded request from a request that reviewed idle tracking.
    fn set_active_task_guarded(
        &mut self,
        task_id: TaskId,
        worklog_id: WorklogId,
        expected_active: Option<Option<WorklogId>>,
        occurred_at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, ApplicationError> {
        let occurred_at = canonical_timestamp(occurred_at);
        let cached_active = self.tracker.active().cloned();
        self.refresh_tracking()?;
        if expected_active
            .is_some_and(|expected| self.tracker.active().map(|active| active.id()) != expected)
        {
            return Err(ApplicationError::TrackingStateChanged);
        }
        if cached_active.as_ref().is_some_and(|cached| {
            self.tracker.active().is_some_and(|active| {
                active.id() == cached.id() && active.start() != cached.start()
            })
        }) {
            return Err(ApplicationError::TrackingStateChanged);
        }
        if let Some(active) = self.tracker.active().cloned()
            && active.task_id() == task_id
        {
            // Another client may have started this worklog after our task
            // catalog was loaded. Keep the derived latest-work value current
            // even though the desired tracking state already exists.
            self.note_work_start(task_id, active.start());
            return Ok(SetActiveTaskOutcome::AlreadyActive { worklog: active });
        }

        let task = self
            .tasks
            .iter()
            .find(|item| item.task.id() == task_id)
            .map(|TaskListItem { task, .. }| task.clone())
            .ok_or_else(|| ApplicationError::from(RepositoryError::TaskNotFound { id: task_id }))?;
        let (candidate, outcome) = self.persist_tracking_change(&task, worklog_id, occurred_at)?;
        self.tracker = candidate;
        // Starting or switching work makes this task the latest work in the
        // task catalog; stopping never touches it.
        self.note_work_start(task_id, occurred_at);
        Ok(outcome)
    }

    /// Persists the domain transition before the caller adopts its candidate tracker.
    /// A failed insert or atomic switch recovers authoritative state instead of adopting the candidate.
    fn persist_tracking_change(
        &mut self,
        task: &tracker_domain::Task,
        worklog_id: WorklogId,
        occurred_at: DateTime<Utc>,
    ) -> Result<(tracker_domain::Tracker, SetActiveTaskOutcome), ApplicationError> {
        let mut candidate = self.tracker.clone();
        let outcome = match candidate.active() {
            None => {
                let worklog = candidate.start(task, worklog_id, occurred_at)?;
                match self.repository.insert_worklog(&worklog) {
                    Ok(()) => SetActiveTaskOutcome::Started { worklog },
                    Err(error) => return Err(self.recover_after_tracking_write(error)),
                }
            }
            Some(_) => {
                let SwitchedWorklogs { stopped, started } =
                    candidate.switch(task, worklog_id, occurred_at, occurred_at)?;
                match self.repository.switch_worklog(
                    stopped.id(),
                    stopped.start(),
                    occurred_at,
                    &started,
                ) {
                    Ok(()) => SetActiveTaskOutcome::Switched { stopped, started },
                    Err(error) => return Err(self.recover_after_tracking_write(error)),
                }
            }
        };
        Ok((candidate, outcome))
    }
}
