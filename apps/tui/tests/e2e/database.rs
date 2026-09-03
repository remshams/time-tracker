//! Probes of the real SQLite database a scenario ran against.

use std::path::Path;

use chrono::{DateTime, Utc};
use tracker_domain::{TaskId, WorklogId};
use tracker_storage::SqliteRepository;

/// A read-only view of one test database.
pub(crate) struct Database {
    repository: SqliteRepository,
}

/// One task as stored: its stable identifier, its name, and whether it is
/// archived.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct StoredTask {
    pub(crate) id: TaskId,
    pub(crate) name: String,
    pub(crate) archived: bool,
}

/// One worklog as stored. `end` is `None` while the worklog is active.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct StoredWorklog {
    pub(crate) id: WorklogId,
    pub(crate) task_name: String,
    pub(crate) start: DateTime<Utc>,
    pub(crate) end: Option<DateTime<Utc>>,
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
                id: task.id,
                name: task.name.to_string(),
                archived: task.archived,
            })
            .collect()
    }

    /// The names of the stored tasks, in storage order.
    pub(crate) fn task_names(&self) -> Vec<String> {
        self.tasks().into_iter().map(|task| task.name).collect()
    }

    /// The named task, if it is stored.
    pub(crate) fn task_by_name(&self, name: &str) -> Option<StoredTask> {
        self.tasks().into_iter().find(|task| task.name == name)
    }

    /// Every worklog of the named task, oldest first.
    ///
    /// Panics when the task is not stored; scenarios call this for tasks
    /// they have already seen on the screen.
    pub(crate) fn worklogs_for_task(&self, name: &str) -> Vec<StoredWorklog> {
        let task = self
            .task_by_name(name)
            .unwrap_or_else(|| panic!("the task {name:?} must be stored"));
        self.repository
            .list_worklogs(task.id)
            .expect("the worklogs must list")
            .into_iter()
            .map(|worklog| StoredWorklog {
                id: worklog.id,
                task_name: name.to_owned(),
                start: worklog.start,
                end: worklog.end,
            })
            .collect()
    }

    /// The one active worklog, if tracking is on.
    pub(crate) fn active_worklog(&self) -> Option<StoredWorklog> {
        let worklog = self
            .repository
            .active_worklog()
            .expect("the active worklog must be readable")?;
        let task = self
            .repository
            .find_task(worklog.task_id)
            .expect("the task lookup must work")?;
        Some(StoredWorklog {
            id: worklog.id,
            task_name: task.name.to_string(),
            start: worklog.start,
            end: worklog.end,
        })
    }

    /// The task an open worklog tracks, if tracking is on.
    pub(crate) fn active_worklog_task_name(&self) -> Option<String> {
        self.active_worklog().map(|worklog| worklog.task_name)
    }
}
