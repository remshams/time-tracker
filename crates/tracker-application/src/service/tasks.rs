//! Task queries and commands.

use chrono::{DateTime, Utc};
use tracker_domain::{InactivityPeriod, Task, TaskId, TaskName, TrackingError};

use super::{TrackerApplication, canonical_timestamp};
use crate::{ApplicationError, RepositoryError, TaskListItem, TaskOrdering, TrackerRepository};

impl<R: TrackerRepository> TrackerApplication<R> {
    pub fn refresh_task_list(&mut self) -> Result<(), ApplicationError> {
        self.refresh_authoritative_state()
    }

    pub fn tasks(&self, ordering: TaskOrdering) -> Vec<TaskListItem> {
        let mut items = self.tasks.clone();
        ordering.sort_items(&mut items);
        items
    }

    pub fn task(&self, id: TaskId) -> Option<&Task> {
        self.tasks
            .iter()
            .find(|item| item.task.id() == id)
            .map(|TaskListItem { task, .. }| task)
    }

    pub fn create_task(
        &mut self,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        self.create_task_with_id(TaskId::generate(), name, occurred_at)
    }

    pub fn rename_task(
        &mut self,
        id: TaskId,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        let occurred_at = canonical_timestamp(occurred_at);
        // The port changes only the name and its metadata timestamp, so a
        // concurrent archive state survives the write.
        let task = self.repository.rename_task(id, name, occurred_at)?;
        self.replace_task(task.clone());
        Ok(task)
    }

    /// Protects the active task, adopts the stored archive result, and refreshes tracking.
    /// Write failures retain their primary cause if state recovery also fails.
    pub fn archive_task(
        &mut self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        let occurred_at = canonical_timestamp(occurred_at);
        self.reload_tasks_and_tracking()?;
        self.tracker.ensure_archivable(id)?;
        // The operation preserves unrelated metadata, so a concurrent rename
        // survives the archive.
        match self.repository.archive_task(id, occurred_at) {
            Ok(task) => {
                self.replace_task(task.clone());
                self.refresh_after_task_operation()?;
                Ok(task)
            }
            Err(error) => {
                let primary = match error {
                    RepositoryError::TaskIsActive { id } => {
                        TrackingError::TaskIsActive { id }.into()
                    }
                    error => error.into(),
                };
                Err(self.task_failure_with_recovery(primary))
            }
        }
    }

    /// Passes the reviewed name to the atomic repository guard and adopts the stored task.
    /// On failure, refreshes authoritative state without discarding the original error.
    pub fn rename_task_with_expected_name(
        &mut self,
        id: TaskId,
        expected_name: &TaskName,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        match self.repository.compare_and_rename_task(
            id,
            expected_name,
            name,
            canonical_timestamp(occurred_at),
        ) {
            Ok(task) => {
                self.replace_task(task.clone());
                Ok(task)
            }
            Err(error) => Err(self.task_failure_with_recovery(error.into())),
        }
    }

    /// Archives under the repository name guard after a tracking preflight.
    /// A failed refresh after commit retains the archived task and reports recovery failure.
    pub fn archive_task_with_expected_name(
        &mut self,
        id: TaskId,
        expected_name: &TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        self.reload_tasks_and_tracking()?;
        match self.repository.compare_and_archive_task(
            id,
            expected_name,
            canonical_timestamp(occurred_at),
        ) {
            Ok(task) => {
                self.replace_task(task.clone());
                self.refresh_after_task_operation()?;
                Ok(task)
            }
            Err(error) => {
                let primary = match error {
                    RepositoryError::TaskIsActive { id } => {
                        TrackingError::TaskIsActive { id }.into()
                    }
                    error => error.into(),
                };
                Err(self.task_failure_with_recovery(primary))
            }
        }
    }

    pub fn unarchive_task(
        &mut self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        let occurred_at = canonical_timestamp(occurred_at);
        // Another client may have restored this task and started tracking it
        // since our last load. Refresh before the write so a read failure
        // prevents the unarchive and Idle is never reported over an active
        // worklog that survived the call.
        self.reload_tasks_and_tracking()?;
        match self.repository.unarchive_task(id, occurred_at) {
            Ok(task) => {
                self.replace_task(task.clone());
                self.refresh_after_task_operation()?;
                Ok(task)
            }
            Err(error) => Err(self.task_failure_with_recovery(error.into())),
        }
    }

    pub fn preview_inactive_tasks(
        &mut self,
        as_of: DateTime<Utc>,
    ) -> Result<Vec<Task>, ApplicationError> {
        self.preview_inactive_tasks_with_period(as_of, InactivityPeriod::default())
    }

    pub fn archive_inactive_tasks(
        &mut self,
        expected_ids: &[TaskId],
        as_of: DateTime<Utc>,
    ) -> Result<Vec<Task>, ApplicationError> {
        self.archive_inactive_tasks_with_period(expected_ids, as_of, InactivityPeriod::default())
    }

    pub fn preview_inactive_tasks_with_period(
        &mut self,
        as_of: DateTime<Utc>,
        period: InactivityPeriod,
    ) -> Result<Vec<Task>, ApplicationError> {
        let preview = self
            .repository
            .preview_inactive_tasks_with_period(canonical_timestamp(as_of), period)?;
        self.adopt_tracking_read(preview.tracking)?;
        for task in &preview.tasks {
            self.replace_task(task.clone());
        }
        Ok(preview.tasks)
    }

    pub fn archive_inactive_tasks_with_period(
        &mut self,
        expected_ids: &[TaskId],
        as_of: DateTime<Utc>,
        period: InactivityPeriod,
    ) -> Result<Vec<Task>, ApplicationError> {
        match self.repository.archive_inactive_tasks_with_period(
            expected_ids,
            canonical_timestamp(as_of),
            period,
        ) {
            Ok(archive) => {
                self.adopt_tracking_read(archive.tracking)?;
                for task in &archive.tasks {
                    self.replace_task(task.clone());
                }
                Ok(archive.tasks)
            }
            Err(error) => Err(self.task_failure_with_recovery(error.into())),
        }
    }

    fn task_failure_with_recovery(&mut self, primary: ApplicationError) -> ApplicationError {
        match self.refresh_after_task_operation() {
            Ok(()) => primary,
            Err(recovery) => primary.with_recovery_failure(recovery),
        }
    }

    /// Creates a task with a caller-owned identifier, which makes remote retries safe.
    pub fn create_task_with_id(
        &mut self,
        id: TaskId,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        let occurred_at = canonical_timestamp(occurred_at);
        let task = Task::create(id, name, occurred_at);
        self.repository.create_task(task.clone())?;
        self.replace_task(task.clone());
        Ok(task)
    }
}
