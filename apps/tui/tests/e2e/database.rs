//! Probes of the real SQLite database a scenario ran against.

use std::path::Path;

use tracker_storage::SqliteRepository;

/// A read-only view of one test database.
pub(crate) struct Database {
    repository: SqliteRepository,
}

/// One task as stored: its name and whether it is archived.
pub(crate) struct StoredTask {
    pub(crate) name: String,
    pub(crate) archived: bool,
}

impl Database {
    pub(crate) fn open(path: &Path) -> Self {
        Self {
            repository: SqliteRepository::open(path).expect("the test database must open"),
        }
    }

    /// Every stored task, in storage order.
    pub(crate) fn tasks(&self) -> Vec<StoredTask> {
        self.repository
            .list_tasks()
            .expect("the tasks must list")
            .into_iter()
            .map(|task| StoredTask {
                name: task.name.to_string(),
                archived: task.archived,
            })
            .collect()
    }

    /// The names of the stored tasks, in storage order.
    pub(crate) fn task_names(&self) -> Vec<String> {
        self.tasks().into_iter().map(|task| task.name).collect()
    }

    /// The task an open worklog tracks, if tracking is on.
    pub(crate) fn active_worklog_task_name(&self) -> Option<String> {
        let worklog = self
            .repository
            .active_worklog()
            .expect("the active worklog must be readable")?;
        let task = self
            .repository
            .find_task(worklog.task_id)
            .expect("the task lookup must work")?;
        Some(task.name.to_string())
    }
}
