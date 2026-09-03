//! Probes and fixtures for the real SQLite database a scenario ran against.

use std::path::Path;

use chrono::{DateTime, Utc};
use tracker_domain::{Task, TaskId, TaskName, WorklogId};
use tracker_storage::SqliteRepository;

/// The name of the trigger a scenario installs to make task creates fail.
const INJECTED_FAILURE_TRIGGER: &str = "e2e_injected_task_create_failure";

/// A fixture and probe view of one test database. It seeds tasks and
/// worklogs through the real adapter, injects deterministic storage
/// failures, and reads stored state back for postconditions.
pub(crate) struct Database {
    repository: SqliteRepository,
}

/// One task as stored: its stable identifier, its name, and whether it is
/// archived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredTask {
    pub(crate) id: TaskId,
    pub(crate) name: String,
    pub(crate) archived: bool,
}

/// One worklog as stored: its stable identifier, the task it tracks, the
/// task's name, and its interval. `end` is `None` while the worklog is
/// active.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct StoredWorklog {
    pub(crate) id: WorklogId,
    pub(crate) task_id: TaskId,
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

    /// Adds a task through the real adapter, so a scenario can launch `tt`
    /// against a database that already has content.
    pub(crate) fn create_task(&self, name: &str) -> StoredTask {
        let task = Task::new(
            TaskId::generate(),
            TaskName::new(name).expect("the seeded task name must be valid"),
        );
        self.repository
            .create_task(task.clone())
            .expect("the task must be created");
        StoredTask {
            id: task.id,
            name: task.name.to_string(),
            archived: task.archived,
        }
    }

    /// Marks the named stored task archived through the real adapter and
    /// returns the archived record.
    pub(crate) fn archive_task(&self, name: &str) -> StoredTask {
        let task = self
            .task_by_name(name)
            .unwrap_or_else(|| panic!("the task {name:?} must be stored"));
        let archived = self
            .repository
            .archive_task(task.id)
            .expect("the task must be archived");
        StoredTask {
            id: archived.id,
            name: archived.name.to_string(),
            archived: archived.archived,
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

    /// Installs a persistent BEFORE INSERT trigger on tasks that aborts
    /// every insert with a fixed English message, so the next task create
    /// through any connection fails deterministically.
    ///
    /// The trigger lives in this scenario's temporary database only, and a
    /// trigger fires no matter what user runs the test, so the injection
    /// works under root too, where a permissions-based block would not.
    pub(crate) fn inject_task_create_failure(&self) {
        self.repository
            .connection()
            .execute_batch(&format!(
                "CREATE TRIGGER {INJECTED_FAILURE_TRIGGER}
                 BEFORE INSERT ON tasks
                 BEGIN
                     SELECT RAISE(ABORT, 'injected failure: the e2e fixture blocked this task create');
                 END;"
            ))
            .expect("the failure trigger must install");
    }

    /// Drops the injected failure trigger again, so task creates succeed.
    pub(crate) fn remove_injected_task_create_failure(&self) {
        self.repository
            .connection()
            .execute_batch(&format!(
                "DROP TRIGGER IF EXISTS {INJECTED_FAILURE_TRIGGER};"
            ))
            .expect("the failure trigger must be removable");
    }

    /// Every worklog of the named task, oldest first.
    ///
    /// Panics when the task is not stored; scenarios call this for tasks
    /// they have already seen on the screen.
    pub(crate) fn worklogs_for_task(&self, name: &str) -> Vec<StoredWorklog> {
        let task = self
            .task_by_name(name)
            .unwrap_or_else(|| panic!("the task {name:?} must be stored"));
        self.worklogs_of(task.id, &task.name)
    }

    /// Every active worklog across all tasks, as an exact set: one element
    /// while tracking is on, empty while it is off.
    ///
    /// The set is built by listing every task's worklogs through the
    /// repository APIs and keeping the ones without an end, so no raw SQL
    /// is involved.
    pub(crate) fn active_worklogs(&self) -> Vec<StoredWorklog> {
        let mut active = Vec::new();
        for task in self.repository.list_tasks().expect("the tasks must list") {
            for worklog in self.worklogs_of(task.id, task.name.as_str()) {
                if worklog.end.is_none() {
                    active.push(worklog);
                }
            }
        }
        active
    }

    /// The one active worklog, if tracking is on.
    pub(crate) fn active_worklog(&self) -> Option<StoredWorklog> {
        self.repository
            .active_worklog()
            .expect("the active worklog must be readable")
            .map(|worklog| {
                let task = self
                    .repository
                    .find_task(worklog.task_id)
                    .expect("the task lookup must work")
                    .unwrap_or_else(|| panic!("the active worklog's task must be stored"));
                StoredWorklog {
                    id: worklog.id,
                    task_id: worklog.task_id,
                    task_name: task.name.to_string(),
                    start: worklog.start,
                    end: worklog.end,
                }
            })
    }

    /// The task an open worklog tracks, if tracking is on.
    pub(crate) fn active_worklog_task_name(&self) -> Option<String> {
        self.active_worklog().map(|worklog| worklog.task_name)
    }

    /// The stored worklogs of one task id, with the task's name attached.
    fn worklogs_of(&self, task_id: TaskId, task_name: &str) -> Vec<StoredWorklog> {
        self.repository
            .list_worklogs(task_id)
            .expect("the worklogs must list")
            .into_iter()
            .map(|worklog| StoredWorklog {
                id: worklog.id,
                task_id: worklog.task_id,
                task_name: task_name.to_owned(),
                start: worklog.start,
                end: worklog.end,
            })
            .collect()
    }
}
