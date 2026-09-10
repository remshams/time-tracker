//! Tracking state queries and commands.

use chrono::{DateTime, Utc};
use tracker_domain::{SwitchedWorklogs, TaskId, TrackingState, WorklogId};

use super::{TrackerApplication, TrackingOperations, canonical_timestamp};
use crate::{
    ApplicationError, ClearActiveTaskOutcome, RepositoryError, SetActiveTaskOutcome, TaskListItem,
    TrackerRepository,
};

impl<R: TrackerRepository> TrackingOperations for TrackerApplication<R> {
    fn current_tracking(&self) -> &TrackingState {
        self.tracker.state()
    }

    fn set_active_task(
        &mut self,
        task_id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, ApplicationError> {
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
        if let Some(active) = self.tracker.active().cloned()
            && active.task_id() == task_id
        {
            // Another client may have started this worklog after our task
            // snapshot was loaded. Keep the derived latest-work value current
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
        let mut candidate = self.tracker.clone();
        let outcome = match candidate.active() {
            None => {
                let worklog = candidate.start(&task, occurred_at)?;
                match self.repository.insert_worklog(&worklog) {
                    Ok(()) => SetActiveTaskOutcome::Started { worklog },
                    Err(error) => return Err(self.recover_after_tracking_write(error)),
                }
            }
            Some(_) => {
                let SwitchedWorklogs { stopped, started } =
                    candidate.switch(&task, occurred_at, occurred_at)?;
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
        self.tracker = candidate;
        // Starting or switching work makes this task the latest work on the
        // snapshot; stopping never touches it.
        self.note_work_start(task_id, occurred_at);
        Ok(outcome)
    }

    fn clear_active_task(
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
}
