//! Probes and fixtures for the real SQLite database a scenario ran against.

use std::path::Path;

use chrono::{DateTime, Utc};
use tracker_domain::{Task, TaskId, TaskName, Worklog, WorklogId, WorklogTimes};
use tracker_storage::SqliteRepository;

/// The name of the trigger a scenario installs to make task creates fail.
const INJECTED_FAILURE_TRIGGER: &str = "e2e_injected_task_create_failure";

/// A fixture and probe view of one test database. It seeds tasks and
/// worklogs through the real adapter, injects deterministic storage
/// failures, and reads stored state back for postconditions.
pub(crate) struct Database {
    repository: SqliteRepository,
}

/// One task as stored, including its stable identity and metadata timestamps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredTask {
    pub(crate) id: TaskId,
    pub(crate) name: String,
    pub(crate) archived: bool,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
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

fn fixture_time(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 0).expect("the fixture timestamp must be valid")
}

fn stored_task(task: Task) -> StoredTask {
    StoredTask {
        id: task.id(),
        name: task.name().to_string(),
        archived: task.is_archived(),
        created_at: task.created_at(),
        updated_at: task.updated_at(),
    }
}

impl Database {
    pub(crate) fn open(path: &Path) -> Self {
        Self {
            repository: SqliteRepository::open(path).expect("the test database must open"),
        }
    }

    /// Adds a task through the real adapter, so a scenario can launch `tt`
    /// against a database that already has content. Existing scenarios use
    /// one shared timestamp, which preserves their identifier-based order.
    pub(crate) fn create_task(&self, name: &str) -> StoredTask {
        self.create_task_at(name, fixture_time(0), fixture_time(0))
    }

    pub(crate) fn create_fixture_tasks(&self) {
        let now = Utc::now();
        for name in [
            "Write release notes",
            "Fix the coffee machine",
            "Plan Friday's demo",
        ] {
            self.create_task_at(name, now, now);
        }
    }

    /// Adds a task with explicit metadata timestamps for ordering scenarios.
    pub(crate) fn create_task_at(
        &self,
        name: &str,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> StoredTask {
        let task = Task::rehydrate(
            TaskId::generate(),
            TaskName::new(name).expect("the seeded task name must be valid"),
            false,
            created_at,
            updated_at,
        )
        .expect("the fixture task timestamps must be valid");
        self.repository
            .create_task(task.clone())
            .expect("the task must be created");
        stored_task(task)
    }

    /// Marks the named stored task archived through the real adapter and
    /// returns the archived record.
    pub(crate) fn archive_task(&self, name: &str) -> StoredTask {
        let task = self
            .task_by_name(name)
            .unwrap_or_else(|| panic!("the task {name:?} must be stored"));
        let archived = self
            .repository
            .archive_task(task.id, fixture_time(0))
            .expect("the task must be archived");
        stored_task(archived)
    }

    /// Adds a worklog through the real adapter for an existing task.
    pub(crate) fn create_worklog(
        &self,
        task_name: &str,
        start: DateTime<Utc>,
        end: Option<DateTime<Utc>>,
    ) -> StoredWorklog {
        let task = self
            .task_by_name(task_name)
            .unwrap_or_else(|| panic!("the task {task_name:?} must be stored"));
        let worklog = Worklog::new(WorklogId::generate(), task.id, start, end)
            .expect("the fixture worklog interval must be valid");
        self.repository
            .insert_worklog(&worklog)
            .expect("the worklog must be created");
        StoredWorklog {
            id: worklog.id(),
            task_id: worklog.task_id(),
            task_name: task.name,
            start: worklog.start(),
            end: worklog.end(),
        }
    }

    /// Every stored task, in storage order.
    pub(crate) fn tasks(&self) -> Vec<StoredTask> {
        self.repository
            .list_tasks()
            .expect("the tasks must list")
            .into_iter()
            .map(stored_task)
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

    /// Corrects one stored worklog start through the real SQLite adapter.
    ///
    /// This is used only to model a second client's write while another
    /// `tt` process is open.
    pub(crate) fn correct_worklog_start(&self, worklog: &StoredWorklog, start: DateTime<Utc>) {
        let end = worklog.end.map(|end| start + (end - worklog.start));
        self.repository
            .compare_and_set_worklog_times(
                worklog.id,
                WorklogTimes::new(worklog.start, worklog.end),
                WorklogTimes::new(start, end),
            )
            .expect("the worklog correction must be stored");
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
            for worklog in self.worklogs_of(task.id(), task.name().as_str()) {
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
                    .find_task(worklog.task_id())
                    .expect("the task lookup must work")
                    .unwrap_or_else(|| panic!("the active worklog's task must be stored"));
                StoredWorklog {
                    id: worklog.id(),
                    task_id: worklog.task_id(),
                    task_name: task.name().to_string(),
                    start: worklog.start(),
                    end: worklog.end(),
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
                id: worklog.id(),
                task_id: worklog.task_id(),
                task_name: task_name.to_owned(),
                start: worklog.start(),
                end: worklog.end(),
            })
            .collect()
    }
}
