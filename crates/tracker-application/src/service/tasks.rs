//! Task queries and commands.

use chrono::{DateTime, Utc};
use tracker_domain::{Task, TaskId, TaskName, TrackingError};

use super::{TaskOperations, TaskQueries, TrackerApplication, canonical_timestamp};
use crate::{ApplicationError, RepositoryError, TaskListItem, TaskOrdering, TrackerRepository};

impl<R: TrackerRepository> TaskQueries for TrackerApplication<R> {
    fn tasks(&self, ordering: TaskOrdering) -> Vec<TaskListItem> {
        let mut items = self.tasks.clone();
        ordering.sort_items(&mut items);
        items
    }

    fn task(&self, id: TaskId) -> Option<&Task> {
        self.tasks
            .iter()
            .find(|item| item.task.id() == id)
            .map(|TaskListItem { task, .. }| task)
    }
}

impl<R: TrackerRepository> TaskOperations for TrackerApplication<R> {
    fn create_task(
        &mut self,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        self.create_task_with_id(TaskId::generate(), name, occurred_at)
    }

    fn rename_task(
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

    fn archive_task(
        &mut self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        let occurred_at = canonical_timestamp(occurred_at);
        self.refresh_tracking()?;
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

    fn unarchive_task(
        &mut self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        let occurred_at = canonical_timestamp(occurred_at);
        // Another client may have restored this task and started tracking it
        // since our last load. Refresh before the write so a read failure
        // prevents the unarchive and Idle is never reported over an active
        // worklog that survived the call.
        self.refresh_tracking()?;
        match self.repository.unarchive_task(id, occurred_at) {
            Ok(task) => {
                self.replace_task(task.clone());
                self.refresh_after_task_operation()?;
                Ok(task)
            }
            Err(error) => Err(self.task_failure_with_recovery(error.into())),
        }
    }

    fn preview_inactive_tasks(
        &mut self,
        as_of: DateTime<Utc>,
    ) -> Result<Vec<Task>, ApplicationError> {
        let preview = self
            .repository
            .preview_inactive_tasks(canonical_timestamp(as_of))?;
        self.adopt_snapshot(preview.snapshot)?;
        Ok(preview.tasks)
    }

    fn archive_inactive_tasks(
        &mut self,
        expected_ids: &[TaskId],
        as_of: DateTime<Utc>,
    ) -> Result<Vec<Task>, ApplicationError> {
        let result = self
            .repository
            .archive_inactive_tasks(expected_ids, canonical_timestamp(as_of));
        match result {
            Ok(archive) => {
                self.adopt_snapshot(archive.snapshot)?;
                Ok(archive.tasks)
            }
            Err(error) => Err(self.task_failure_with_recovery(error.into())),
        }
    }
}

impl<R: TrackerRepository> TrackerApplication<R> {
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
