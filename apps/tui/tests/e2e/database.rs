//! Probes and fixtures for the real SQLite database a scenario ran against.
//!
//! Seeding and reading go through the `tt-cli` binary, so the suite drives
//! storage only through public interfaces. The one exception is the
//! injected task-create failure, which has no public interface and stays a
//! plain SQL trigger on the database file.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::Value;

/// The name of the trigger a scenario installs to make task creates fail.
const INJECTED_FAILURE_TRIGGER: &str = "e2e_injected_task_create_failure";

/// A fixture and probe view of one test database. It seeds tasks and
/// worklogs through `tt-cli`, injects deterministic storage failures, and
/// reads stored state back for postconditions.
pub(crate) struct Database {
    path: PathBuf,
}

/// One task as stored, including its stable identity and metadata timestamps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredTask {
    pub(crate) id: String,
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
    pub(crate) id: String,
    pub(crate) task_id: String,
    pub(crate) task_name: String,
    pub(crate) start: DateTime<Utc>,
    pub(crate) end: Option<DateTime<Utc>>,
}

fn fixture_time(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 0).expect("the fixture timestamp must be valid")
}

fn timestamp_argument(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

fn text(value: &Value, field: &str) -> String {
    value[field]
        .as_str()
        .unwrap_or_else(|| panic!("tt-cli output must contain the text field {field:?}: {value}"))
        .to_owned()
}

fn timestamp(value: &Value, field: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&text(value, field))
        .unwrap_or_else(|_| panic!("tt-cli output must contain an RFC3339 {field:?}: {value}"))
        .with_timezone(&Utc)
}

fn optional_timestamp(value: &Value, field: &str) -> Option<DateTime<Utc>> {
    (!value[field].is_null()).then(|| timestamp(value, field))
}

fn stored_task(value: &Value) -> StoredTask {
    StoredTask {
        id: text(value, "id"),
        name: text(value, "name"),
        archived: value["archived"]
            .as_bool()
            .unwrap_or_else(|| panic!("tt-cli task output must contain archived: {value}")),
        created_at: timestamp(value, "created_at"),
        updated_at: timestamp(value, "updated_at"),
    }
}

fn stored_worklog(value: &Value, task_name: &str) -> StoredWorklog {
    StoredWorklog {
        id: text(value, "id"),
        task_id: text(value, "task_id"),
        task_name: task_name.to_owned(),
        start: timestamp(value, "start"),
        end: optional_timestamp(value, "end"),
    }
}

impl Database {
    /// Opens the database through `tt-cli`, which creates and migrates it
    /// when it does not exist yet.
    pub(crate) fn open(path: &Path) -> Self {
        let database = Self {
            path: path.to_path_buf(),
        };
        database.tasks();
        database
    }

    /// Runs one `tt-cli` command against this database and returns its
    /// `data` value. `at` sets the operation timestamp. A failed command
    /// fails the scenario with the command's error output.
    fn run(&self, at: Option<DateTime<Utc>>, arguments: &[&str]) -> Value {
        let mut command = Command::new(crate::binaries::cli());
        command
            .env_clear()
            .env("TZ", "UTC")
            .arg("--db")
            .arg(&self.path)
            .stdin(Stdio::null());
        if let Some(at) = at {
            command.arg("--at").arg(timestamp_argument(at));
        }
        if let Ok(profile) = std::env::var("LLVM_PROFILE_FILE") {
            command.env("LLVM_PROFILE_FILE", profile);
        }
        let output = command.args(arguments).output().expect("tt-cli must run");
        assert!(
            output.status.success(),
            "tt-cli {arguments:?} failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        let mut response: Value =
            serde_json::from_slice(&output.stdout).expect("tt-cli must print one JSON document");
        assert_eq!(
            response["ok"], true,
            "tt-cli {arguments:?} answered {response}"
        );
        response["data"].take()
    }

    /// Adds a task through `tt-cli`, so a scenario can launch `tt` against
    /// a database that already has content. Existing scenarios use one
    /// shared timestamp, which preserves their identifier-based order.
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
    ///
    /// `tt-cli` stamps a new task with one timestamp. A later `updated_at`
    /// comes from renaming the task away and back at that instant, since a
    /// rename to the same name leaves the metadata unchanged.
    pub(crate) fn create_task_at(
        &self,
        name: &str,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> StoredTask {
        assert!(
            updated_at >= created_at,
            "the fixture task {name:?} must not be updated before it was created"
        );
        let task = stored_task(&self.run(Some(created_at), &["tasks", "create", name]));
        if updated_at == created_at {
            return task;
        }
        let interim = format!("{name} (seeding)");
        self.run(Some(updated_at), &["tasks", "rename", &task.id, &interim]);
        stored_task(&self.run(Some(updated_at), &["tasks", "rename", &task.id, name]))
    }

    /// Marks the named stored task archived through `tt-cli` and returns the
    /// archived record. The archive uses the shared fixture timestamp, so it
    /// leaves the task's `updated_at` unchanged.
    pub(crate) fn archive_task(&self, name: &str) -> StoredTask {
        let task = self
            .task_by_name(name)
            .unwrap_or_else(|| panic!("the task {name:?} must be stored"));
        stored_task(&self.run(Some(fixture_time(0)), &["tasks", "archive", &task.id]))
    }

    /// Adds a worklog for an existing task by starting tracking at `start`
    /// and, for a completed worklog, stopping it at `end`.
    ///
    /// Tracking must be idle, so a scenario seeds its completed worklogs
    /// before an active one, and its worklogs before archiving their task.
    pub(crate) fn create_worklog(
        &self,
        task_name: &str,
        start: DateTime<Utc>,
        end: Option<DateTime<Utc>>,
    ) -> StoredWorklog {
        let task = self
            .task_by_name(task_name)
            .unwrap_or_else(|| panic!("the task {task_name:?} must be stored"));
        let started = self.run(Some(start), &["tracking", "start", &task.id]);
        assert_eq!(
            started["outcome"], "started",
            "seeding a worklog for {task_name:?} must start from idle tracking: {started}"
        );
        let worklog = stored_worklog(&started["worklog"], &task.name);
        let Some(end) = end else {
            return worklog;
        };
        let stopped = self.run(
            Some(end),
            &["tracking", "stop", "--expected-active", &worklog.id],
        );
        assert_eq!(
            stopped["outcome"], "stopped",
            "seeding a worklog for {task_name:?} must stop it: {stopped}"
        );
        stored_worklog(&stopped["worklog"], &task.name)
    }

    /// Every stored task, in storage order, which is identifier order.
    pub(crate) fn tasks(&self) -> Vec<StoredTask> {
        let listed = self.run(
            None,
            &["tasks", "list", "--state", "all", "--sort", "created"],
        );
        let mut tasks: Vec<StoredTask> = listed["tasks"]
            .as_array()
            .unwrap_or_else(|| panic!("tt-cli must list tasks: {listed}"))
            .iter()
            .map(|item| stored_task(&item["task"]))
            .collect();
        tasks.sort_by(|left, right| left.id.cmp(&right.id));
        tasks
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
    /// No public interface can make a create fail, so this is plain SQL on
    /// the database file. The trigger lives in this scenario's temporary
    /// database only, and a trigger fires no matter what user runs the
    /// test, so the injection works under root too, where a
    /// permissions-based block would not.
    pub(crate) fn inject_task_create_failure(&self) {
        self.execute_sql(&format!(
            "CREATE TRIGGER {INJECTED_FAILURE_TRIGGER}
             BEFORE INSERT ON tasks
             BEGIN
                 SELECT RAISE(ABORT, 'injected failure: the e2e fixture blocked this task create');
             END;"
        ));
    }

    /// Drops the injected failure trigger again, so task creates succeed.
    pub(crate) fn remove_injected_task_create_failure(&self) {
        self.execute_sql(&format!(
            "DROP TRIGGER IF EXISTS {INJECTED_FAILURE_TRIGGER};"
        ));
    }

    fn execute_sql(&self, sql: &str) {
        let connection =
            rusqlite::Connection::open(&self.path).expect("the test database file must open");
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .expect("the busy timeout must be settable");
        connection
            .execute_batch(sql)
            .expect("the fault injection SQL must run");
    }

    /// Corrects one stored worklog start through `tt-cli`, keeping its
    /// duration.
    ///
    /// This is used only to model a second client's write while another
    /// `tt` process is open.
    pub(crate) fn correct_worklog_start(&self, worklog: &StoredWorklog, start: DateTime<Utc>) {
        let end = worklog.end.map(|end| start + (end - worklog.start));
        let format_end = |end: Option<DateTime<Utc>>| {
            end.map_or_else(|| "running".to_owned(), timestamp_argument)
        };
        self.run(
            None,
            &[
                "worklogs",
                "correct",
                &worklog.id,
                "--expected-start",
                &timestamp_argument(worklog.start),
                "--expected-end",
                &format_end(worklog.end),
                "--start",
                &timestamp_argument(start),
                "--end",
                &format_end(end),
            ],
        );
    }

    /// Every worklog of the named task, oldest first.
    ///
    /// Panics when the task is not stored; scenarios call this for tasks
    /// they have already seen on the screen.
    pub(crate) fn worklogs_for_task(&self, name: &str) -> Vec<StoredWorklog> {
        let task = self
            .task_by_name(name)
            .unwrap_or_else(|| panic!("the task {name:?} must be stored"));
        let mut worklogs: Vec<StoredWorklog> = self
            .worklog_pages(Some(&task.id))
            .iter()
            .map(|worklog| stored_worklog(worklog, &task.name))
            .collect();
        worklogs.sort_by(|left, right| (left.start, &left.id).cmp(&(right.start, &right.id)));
        worklogs
    }

    /// Every active worklog across all tasks, as an exact set: one element
    /// while tracking is on, empty while it is off.
    ///
    /// The set comes from the full worklog history, which includes archived
    /// tasks, rather than from the tracking status, so it would also reveal
    /// a second open worklog.
    pub(crate) fn active_worklogs(&self) -> Vec<StoredWorklog> {
        let tasks = self.tasks();
        self.worklog_pages(None)
            .iter()
            .filter(|worklog| worklog["end"].is_null())
            .map(|worklog| {
                let task_id = text(worklog, "task_id");
                let task = tasks
                    .iter()
                    .find(|task| task.id == task_id)
                    .unwrap_or_else(|| panic!("the worklog's task {task_id} must be stored"));
                stored_worklog(worklog, &task.name)
            })
            .collect()
    }

    /// The one active worklog, if tracking is on.
    pub(crate) fn active_worklog(&self) -> Option<StoredWorklog> {
        let status = self.run(None, &["tracking", "status"]);
        if status["state"] != "running" {
            return None;
        }
        let worklog = &status["active_worklog"];
        let task_id = text(worklog, "task_id");
        let task = self
            .tasks()
            .into_iter()
            .find(|task| task.id == task_id)
            .unwrap_or_else(|| panic!("the active worklog's task must be stored"));
        Some(stored_worklog(worklog, &task.name))
    }

    /// The task an open worklog tracks, if tracking is on.
    pub(crate) fn active_worklog_task_name(&self) -> Option<String> {
        self.active_worklog().map(|worklog| worklog.task_name)
    }

    /// Every worklog of one task, or of all tasks, across all history pages.
    fn worklog_pages(&self, task_id: Option<&str>) -> Vec<Value> {
        let mut worklogs = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let mut arguments = vec!["worklogs", "list"];
            if let Some(task_id) = task_id {
                arguments.extend(["--task", task_id]);
            }
            if let Some(cursor) = cursor.as_deref() {
                arguments.extend(["--cursor", cursor]);
            }
            let mut page = self.run(None, &arguments);
            worklogs.append(
                page["worklogs"]
                    .as_array_mut()
                    .unwrap_or_else(|| panic!("tt-cli must list worklogs")),
            );
            if page["next_cursor"].is_null() {
                return worklogs;
            }
            cursor = Some(page["next_cursor"].to_string());
        }
    }
}
