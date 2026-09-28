//! Integration tests for the SQLite repository.
//!
//! These tests exercise the schema, the database-level invariants, and the
//! interplay between the domain tracker and persistence, including failure
//! states, rollback behavior, concurrent connections, migrations, and the
//! task-list read model with its latest-work aggregate.

use std::sync::{
    Arc, Barrier, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel},
};
use std::thread;
use std::time::Duration;

use crate::{SqliteRepository, StorageError};
use chrono::{DateTime, Utc};
use std::ffi::{CStr, c_char, c_int, c_uint, c_void};
use tempfile::TempDir;
use tracker_application::{
    ApplicationError, ClearActiveTaskOutcome, ReportRead, ReportRepository, RepositoryError,
    SetActiveTaskOutcome, TaskListItem, TaskOperations, TaskOrdering, TaskQueries, TaskRepository,
    TrackerApplication, TrackingOperations, TrackingRepository, WORKLOG_PAGE_SIZE, WorklogCursor,
    WorklogPage, WorklogQueries, WorklogRepository,
};
use tracker_domain::{
    ActiveWorklog, Task, TaskId, TaskName, Tracker, TrackingError, TrackingOutcome, TrackingState,
    Worklog, WorklogId, WorklogTimes,
};

fn at(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 0).unwrap()
}

struct WritePause {
    action: c_int,
    paused: AtomicBool,
    reached: SyncSender<()>,
    release: Mutex<Receiver<()>>,
}

unsafe extern "C" fn pause_worklog_write(
    context: *mut c_void,
    action: c_int,
    table: *const c_char,
    _: *const c_char,
    _: *const c_char,
    _: *const c_char,
) -> c_int {
    let pause = unsafe { &*(context.cast::<WritePause>()) };
    let is_target = action == pause.action
        && unsafe { CStr::from_ptr(table) }.to_bytes() == b"worklogs"
        && !pause.paused.swap(true, Ordering::SeqCst);
    if is_target
        && (pause.reached.send(()).is_err()
            || pause.release.lock().map_or(true, |release| {
                release.recv_timeout(Duration::from_secs(5)).is_err()
            }))
    {
        return rusqlite::ffi::SQLITE_DENY;
    }
    rusqlite::ffi::SQLITE_OK
}

struct BusyObservation {
    observed: AtomicBool,
    reached: SyncSender<()>,
}

unsafe extern "C" fn observe_busy(context: *mut c_void, previous_attempts: c_int) -> c_int {
    let observation = unsafe { &*(context.cast::<BusyObservation>()) };
    if !observation.observed.swap(true, Ordering::SeqCst) {
        let _ = observation.reached.try_send(());
    }
    thread::sleep(Duration::from_millis(1));
    c_int::from(previous_attempts < 2_000)
}

struct AggregateObservation {
    completed: SyncSender<()>,
}

unsafe extern "C" fn observe_aggregate(
    event: c_uint,
    context: *mut c_void,
    statement: *mut c_void,
    _: *mut c_void,
) -> c_int {
    if event == rusqlite::ffi::SQLITE_TRACE_PROFILE {
        let sql = unsafe { rusqlite::ffi::sqlite3_sql(statement.cast()) };
        if !sql.is_null()
            && unsafe { CStr::from_ptr(sql) }.to_bytes()
                == b"SELECT MAX(start_us) FROM worklogs WHERE task_id = ?1"
        {
            let observation = unsafe { &*(context.cast::<AggregateObservation>()) };
            let _ = observation.completed.try_send(());
        }
    }
    rusqlite::ffi::SQLITE_OK
}

unsafe extern "C" fn count_worklog_start_reads(
    context: *mut c_void,
    action: c_int,
    table: *const c_char,
    column: *const c_char,
    _: *const c_char,
    _: *const c_char,
) -> c_int {
    let is_worklog_start_read = action == rusqlite::ffi::SQLITE_READ
        && unsafe { CStr::from_ptr(table) }.to_bytes() == b"worklogs"
        && unsafe { CStr::from_ptr(column) }.to_bytes() == b"start_us";
    if is_worklog_start_read {
        unsafe { &*(context.cast::<AtomicUsize>()) }.fetch_add(1, Ordering::SeqCst);
    }
    rusqlite::ffi::SQLITE_OK
}

fn task_id(tag: u32) -> TaskId {
    // Fixed, ordered identifiers keep the ordering tests deterministic.
    TaskId::from_uuid(uuid::Uuid::from_u128(u128::from(tag)))
}

fn worklog_id(tag: u32) -> WorklogId {
    WorklogId::from_uuid(uuid::Uuid::from_u128(u128::from(tag)))
}

fn named_task(tag: u32, name: &str) -> Task {
    Task::create(task_id(tag), TaskName::new(name).unwrap(), at(100))
}

/// A task with explicit client-created timestamps, for ordering tests.
fn stamped_task(tag: u32, name: &str, created: i64, updated: i64) -> Task {
    Task::rehydrate(
        task_id(tag),
        TaskName::new(name).unwrap(),
        false,
        at(created),
        at(updated),
    )
    .unwrap()
}

fn repo() -> SqliteRepository {
    SqliteRepository::open_in_memory().unwrap()
}

fn file_repo(dir: &TempDir) -> SqliteRepository {
    SqliteRepository::open(dir.path().join("tracker.db")).unwrap()
}

/// The version 1 baseline schema, as this test file pins it: no task
/// timestamp columns. The real migration must upgrade exactly this shape.
const V1_SCHEMA: &str = "CREATE TABLE tasks (
        id TEXT PRIMARY KEY NOT NULL,
        name TEXT NOT NULL,
        archived INTEGER NOT NULL CHECK (archived IN (0, 1))
    ) STRICT;
    CREATE TABLE worklogs (
        id TEXT PRIMARY KEY NOT NULL,
        task_id TEXT NOT NULL REFERENCES tasks (id),
        start_us INTEGER NOT NULL,
        end_us INTEGER,
        CHECK (end_us IS NULL OR end_us >= start_us)
    ) STRICT;
    CREATE INDEX worklogs_task_start ON worklogs (task_id, start_us);
    CREATE UNIQUE INDEX worklogs_single_active
        ON worklogs (1) WHERE end_us IS NULL;";

/// The version 2 schema before overlap triggers were added.
const V2_SCHEMA: &str = "CREATE TABLE tasks (
        id TEXT PRIMARY KEY NOT NULL,
        name TEXT NOT NULL,
        archived INTEGER NOT NULL CHECK (archived IN (0, 1)),
        created_at_us INTEGER NOT NULL,
        updated_at_us INTEGER NOT NULL,
        CHECK (updated_at_us >= created_at_us)
    ) STRICT;
    CREATE TABLE worklogs (
        id TEXT PRIMARY KEY NOT NULL,
        task_id TEXT NOT NULL REFERENCES tasks (id),
        start_us INTEGER NOT NULL,
        end_us INTEGER,
        CHECK (end_us IS NULL OR end_us >= start_us)
    ) STRICT;
    CREATE INDEX worklogs_task_start ON worklogs (task_id, start_us);
    CREATE UNIQUE INDEX worklogs_single_active
        ON worklogs (1) WHERE end_us IS NULL;
    CREATE TRIGGER worklogs_reject_archived_task
    BEFORE INSERT ON worklogs
    WHEN NEW.task_id IN (SELECT id FROM tasks WHERE archived = 1)
    BEGIN SELECT RAISE(ABORT, 'task is archived'); END;
    CREATE TRIGGER tasks_reject_archive_while_active
    BEFORE UPDATE OF archived ON tasks
    WHEN NEW.archived = 1
      AND EXISTS (SELECT 1 FROM worklogs WHERE task_id = NEW.id AND end_us IS NULL)
    BEGIN SELECT RAISE(ABORT, 'task is active'); END;";

/// Creates a genuine version 1 database at the given path, with tasks and
/// worklogs that carry no timestamps.
fn create_v1_database(path: &std::path::Path) {
    let connection = rusqlite::Connection::open(path).unwrap();
    connection.execute_batch(V1_SCHEMA).unwrap();
    connection.pragma_update(None, "user_version", 1).unwrap();
    connection
        .execute(
            "INSERT INTO tasks (id, name, archived) VALUES (?1, ?2, 0)",
            rusqlite::params![task_id(1).to_string(), "Write release notes"],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO tasks (id, name, archived) VALUES (?1, ?2, 1)",
            rusqlite::params![task_id(2).to_string(), "Fix the coffee machine"],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO tasks (id, name, archived) VALUES (?1, ?2, 0)",
            rusqlite::params![task_id(3).to_string(), "Plan Friday's demo"],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO worklogs (id, task_id, start_us, end_us) VALUES (?1, ?2, ?3, NULL)",
            rusqlite::params![
                worklog_id(10).to_string(),
                task_id(1).to_string(),
                at(100).timestamp_micros()
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO worklogs (id, task_id, start_us, end_us) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                worklog_id(11).to_string(),
                task_id(1).to_string(),
                at(50).timestamp_micros(),
                at(75).timestamp_micros()
            ],
        )
        .unwrap();
}

fn create_v2_database(path: &std::path::Path, intervals: &[(u32, i64, Option<i64>)]) {
    let connection = rusqlite::Connection::open(path).unwrap();
    connection.execute_batch(V2_SCHEMA).unwrap();
    connection.pragma_update(None, "user_version", 2).unwrap();
    connection
        .execute(
            "INSERT INTO tasks (id, name, archived, created_at_us, updated_at_us)
             VALUES (?1, 'preserved', 0, 1234567, 2345678)",
            [task_id(1).to_string()],
        )
        .unwrap();
    for &(tag, start_us, end_us) in intervals {
        connection
            .execute(
                "INSERT INTO worklogs (id, task_id, start_us, end_us)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    worklog_id(tag).to_string(),
                    task_id(1).to_string(),
                    start_us,
                    end_us,
                ],
            )
            .unwrap();
    }
}

struct SynchronizingRepository {
    repository: SqliteRepository,
    ready: SyncSender<()>,
    started: Receiver<()>,
    synchronize_active_read: Arc<AtomicBool>,
    fail_next_active_read: Arc<AtomicBool>,
}

impl TaskRepository for SynchronizingRepository {
    fn create_task(&self, task: Task) -> Result<(), RepositoryError> {
        self.repository.create_task(task).map_err(Into::into)
    }

    fn tracker_snapshot(&self) -> Result<tracker_application::TrackerSnapshot, RepositoryError> {
        if self.fail_next_active_read.swap(false, Ordering::SeqCst) {
            return Err(RepositoryError::Backend {
                message: "recovery read failed".to_owned(),
            });
        }
        let snapshot = self
            .repository
            .tracker_snapshot()
            .map_err(RepositoryError::from)?;
        if self.synchronize_active_read.swap(false, Ordering::SeqCst) {
            self.ready.send(()).map_err(|_| RepositoryError::Backend {
                message: "the competing client stopped before synchronization".to_owned(),
            })?;
            self.started
                .recv_timeout(Duration::from_secs(5))
                .map_err(|_| RepositoryError::Backend {
                    message: "the competing client did not start within five seconds".to_owned(),
                })?;
        }
        Ok(snapshot)
    }

    fn rename_task(
        &self,
        id: TaskId,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, RepositoryError> {
        TaskRepository::rename_task(&self.repository, id, name, occurred_at)
    }

    fn archive_task(
        &self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, RepositoryError> {
        TaskRepository::archive_task(&self.repository, id, occurred_at)
    }

    fn unarchive_task(
        &self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, RepositoryError> {
        TaskRepository::unarchive_task(&self.repository, id, occurred_at)
    }

    fn preview_inactive_tasks(
        &self,
        as_of: DateTime<Utc>,
    ) -> Result<tracker_application::InactiveTaskPreviewRead, RepositoryError> {
        TaskRepository::preview_inactive_tasks(&self.repository, as_of)
    }

    fn archive_inactive_tasks(
        &self,
        expected_ids: &[TaskId],
        as_of: DateTime<Utc>,
    ) -> Result<tracker_application::InactiveTaskArchive, RepositoryError> {
        TaskRepository::archive_inactive_tasks(&self.repository, expected_ids, as_of)
    }
}

impl ReportRepository for SynchronizingRepository {
    fn report_read(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<ReportRead, RepositoryError> {
        self.repository
            .report_read(start, end, now)
            .map_err(Into::into)
    }
}

impl WorklogRepository for SynchronizingRepository {
    fn global_worklog_page(
        &self,
        after: Option<&tracker_application::GlobalWorklogCursor>,
    ) -> Result<tracker_application::GlobalWorklogPage, RepositoryError> {
        self.repository
            .global_worklog_page(after)
            .map_err(Into::into)
    }

    fn find_worklog(&self, id: WorklogId) -> Result<Option<Worklog>, RepositoryError> {
        self.repository.find_worklog(id).map_err(Into::into)
    }

    fn compare_and_move_worklog(
        &self,
        id: WorklogId,
        expected_source_task_id: TaskId,
        expected: WorklogTimes,
        destination_task_id: TaskId,
    ) -> Result<tracker_application::WorklogMove, RepositoryError> {
        self.repository
            .compare_and_move_worklog(id, expected_source_task_id, expected, destination_task_id)
            .map_err(Into::into)
    }

    fn compare_and_set_worklog_times(
        &self,
        id: WorklogId,
        expected: WorklogTimes,
        replacement: WorklogTimes,
    ) -> Result<tracker_application::WorklogCorrection, RepositoryError> {
        self.repository
            .compare_and_set_worklog_times(id, expected, replacement)
            .map_err(Into::into)
    }

    fn compare_and_delete_completed_worklog(
        &self,
        id: WorklogId,
        expected_task_id: TaskId,
        expected: WorklogTimes,
    ) -> Result<tracker_application::WorklogDeletion, RepositoryError> {
        self.repository
            .compare_and_delete_completed_worklog(id, expected_task_id, expected)
            .map_err(Into::into)
    }

    fn worklog_page(
        &self,
        task_id: TaskId,
        after: Option<&WorklogCursor>,
    ) -> Result<WorklogPage, RepositoryError> {
        self.repository
            .worklog_page(task_id, after)
            .map_err(Into::into)
    }
}

impl TrackingRepository for SynchronizingRepository {
    fn insert_worklog(&self, worklog: &Worklog) -> Result<(), RepositoryError> {
        self.repository.insert_worklog(worklog).map_err(Into::into)
    }

    fn stop_worklog(
        &self,
        id: WorklogId,
        expected_start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Worklog, RepositoryError> {
        self.repository
            .stop_worklog(id, expected_start, end)
            .map_err(Into::into)
    }

    fn switch_worklog(
        &self,
        id: WorklogId,
        expected_start: DateTime<Utc>,
        stop_at: DateTime<Utc>,
        next: &Worklog,
    ) -> Result<(), RepositoryError> {
        self.repository
            .switch_worklog(id, expected_start, stop_at, next)
            .map_err(Into::into)
    }
}

fn user_version(repository: &SqliteRepository) -> i64 {
    repository
        .connection()
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap()
}

fn history_revision(repository: &SqliteRepository, task_id: TaskId) -> i64 {
    repository
        .connection()
        .query_row(
            "SELECT history_order_revision FROM tasks WHERE id = ?1",
            [task_id.to_string()],
            |row| row.get(0),
        )
        .unwrap()
}

/// Inserts `worklogs` worklogs for the task, one per tag, started at
/// `at(tag)`. The last one is active and the others stop one second later.
fn insert_numbered_worklogs(repository: &SqliteRepository, task: &Task, worklogs: u32) {
    for tag in 1..=worklogs {
        let worklog = if tag == worklogs {
            Worklog::begin(worklog_id(tag), task.id(), at(i64::from(tag)))
        } else {
            Worklog::new(
                worklog_id(tag),
                task.id(),
                at(i64::from(tag)),
                Some(at(i64::from(tag) + 1)),
            )
            .unwrap()
        };
        repository.insert_worklog(&worklog).unwrap();
    }
}

mod concurrency;
mod correction;
mod deletion;
mod global_history;
mod history;
mod migrations;
mod moves;
mod reports;
mod tasks;
mod tracking;
