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

use chrono::{DateTime, Utc};
use std::ffi::{CStr, c_char, c_int, c_uint, c_void};
use tempfile::TempDir;
use tracker_application::{
    ApplicationError, ClearActiveTaskOutcome, RepositoryError, SetActiveTaskOutcome, TaskListItem,
    TaskOperations, TaskOrdering, TaskOutcome, TaskQueries, TaskRepository, TrackerApplication,
    TrackingOperations, TrackingRepository, WORKLOG_PAGE_SIZE, WorklogCursor, WorklogPage,
    WorklogQueries, WorklogRepository,
};
use tracker_domain::{
    ActiveWorklog, Task, TaskId, TaskName, Tracker, TrackingError, TrackingOutcome, TrackingState,
    Worklog, WorklogId, WorklogTimes,
};
use tracker_storage::{SqliteRepository, StorageError};

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
            || pause
                .release
                .lock()
                .map_or(true, |release| release.recv().is_err()))
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

    fn find_task(&self, id: TaskId) -> Result<Option<Task>, RepositoryError> {
        self.repository.find_task(id).map_err(Into::into)
    }

    fn list_task_items(&self) -> Result<Vec<TaskListItem>, RepositoryError> {
        self.repository.list_task_items().map_err(Into::into)
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
}

impl WorklogRepository for SynchronizingRepository {
    fn find_worklog(&self, id: WorklogId) -> Result<Option<Worklog>, RepositoryError> {
        self.repository.find_worklog(id).map_err(Into::into)
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

    fn active_worklog(&self) -> Result<Option<Worklog>, RepositoryError> {
        if self.fail_next_active_read.swap(false, Ordering::SeqCst) {
            return Err(RepositoryError::Backend {
                message: "recovery read failed".to_owned(),
            });
        }
        let active = self
            .repository
            .active_worklog()
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
        Ok(active)
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

#[test]
fn migrations_create_the_schema_triggers_and_are_idempotent() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("nested").join("tracker.db");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();

    {
        let repository = SqliteRepository::open(&path).unwrap();
        repository
            .create_task(stamped_task(1, "first", 100, 100))
            .unwrap();
    }
    // Reopening applies no migration again and keeps the data.
    let reopened = SqliteRepository::open(&path).unwrap();
    assert_eq!(user_version(&reopened), 5);
    let tasks = reopened.list_tasks().unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].name().as_str(), "first");
    let objects: Vec<(String, String)> = reopened
        .connection()
        .prepare("SELECT name, type FROM sqlite_master WHERE name LIKE 'worklogs%' ORDER BY name")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(objects.contains(&("worklogs".to_owned(), "table".to_owned())));
    assert!(objects.contains(&("worklogs_task_start".to_owned(), "index".to_owned())));
    assert!(objects.contains(&(
        "worklogs_reject_archived_task".to_owned(),
        "trigger".to_owned()
    )));
    assert!(objects.contains(&(
        "worklogs_reject_same_task_overlap_insert".to_owned(),
        "trigger".to_owned()
    )));
    assert!(objects.contains(&(
        "worklogs_reject_same_task_overlap_update".to_owned(),
        "trigger".to_owned()
    )));
    assert!(objects.contains(&(
        "worklogs_reject_active_delete".to_owned(),
        "trigger".to_owned()
    )));
    let active_delete_trigger: String = reopened
        .connection()
        .query_row(
            "SELECT sql FROM sqlite_master
             WHERE type = 'trigger' AND name = 'worklogs_reject_active_delete'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(active_delete_trigger.contains("BEFORE DELETE ON worklogs"));
    assert!(active_delete_trigger.contains("OLD.end_us IS NULL"));
    let revision_trigger: String = reopened
        .connection()
        .query_row(
            "SELECT sql FROM sqlite_master
             WHERE type = 'trigger' AND name = 'worklogs_bump_history_order_revision'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(revision_trigger.contains("AFTER UPDATE OF task_id, start_us"));
    assert!(revision_trigger.contains("id = OLD.task_id"));
    assert!(revision_trigger.contains("id = NEW.task_id"));
}

#[test]
fn a_version_1_database_migrates_to_version_5_and_keeps_every_record() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    create_v1_database(&path);

    // The backfill timestamp is captured between these two readings.
    let before = Utc::now();
    let repository = SqliteRepository::open(&path).unwrap();
    let after = Utc::now();
    assert_eq!(user_version(&repository), 5);
    let backfill = repository.list_tasks().unwrap()[0].created_at();

    // Every task id, name, and archive flag survived.
    let tasks = repository.list_tasks().unwrap();
    let names: Vec<(String, bool)> = tasks
        .iter()
        .map(|task| (task.name().to_string(), task.is_archived()))
        .collect();
    assert_eq!(
        names,
        [
            ("Write release notes".to_owned(), false),
            ("Fix the coffee machine".to_owned(), true),
            ("Plan Friday's demo".to_owned(), false),
        ]
    );
    assert_eq!(tasks[0].id, task_id(1));
    assert_eq!(tasks[1].id, task_id(2));
    assert_eq!(tasks[2].id, task_id(3));

    // Every v1 task is backfilled with one timestamp captured at migration
    // time: the same value on every row, created_at equal to updated_at,
    // inside the window this open observed.
    for task in &tasks {
        assert_eq!(task.created_at(), task.updated_at());
        assert_eq!(task.created_at(), backfill, "every migrated row shares it");
    }
    assert!(backfill >= before && backfill <= after, "got {backfill}");

    // Every worklog survived, including the active one.
    let worklogs = repository.list_worklogs(task_id(1)).unwrap();
    assert_eq!(worklogs.len(), 2);
    assert_eq!(worklogs[0].id(), worklog_id(11));
    assert_eq!(worklogs[0].start(), at(50));
    assert_eq!(worklogs[0].end(), Some(at(75)));
    assert_eq!(worklogs[1].id(), worklog_id(10));
    assert_eq!(worklogs[1].start(), at(100));
    assert_eq!(worklogs[1].end(), None);
    assert_eq!(
        repository.active_worklog().unwrap().unwrap().id(),
        worklog_id(10)
    );

    // The read model derives the latest work start from the migrated rows.
    let items = repository.list_task_items().unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].latest_work_start, Some(at(100)));
    assert_eq!(items[1].latest_work_start, None);
    assert_eq!(items[2].latest_work_start, None);

    // The rebuilt schema enforces its invariants again: a second active
    // worklog is rejected, and so is archiving the task that runs one.
    let second = Worklog::begin(worklog_id(42), task_id(3), at(300));
    assert!(matches!(
        repository.insert_worklog(&second),
        Err(StorageError::ActiveWorklogExists)
    ));
    assert!(matches!(
        repository.archive_task(task_id(1), at(300)),
        Err(StorageError::TaskIsActive { id }) if id == task_id(1)
    ));
}

#[test]
fn version_2_migration_preserves_identity_and_timestamp_values_exactly() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let intervals = [
        (10, 1_000_001, Some(1_000_007)),
        (11, 1_000_007, None),
        (12, 1_000_004, Some(1_000_004)),
    ];
    create_v2_database(&path, &intervals);

    let repository = SqliteRepository::open(&path).unwrap();

    assert_eq!(user_version(&repository), 5);
    let task_values: (String, i64, i64) = repository
        .connection()
        .query_row(
            "SELECT id, created_at_us, updated_at_us FROM tasks WHERE id = ?1",
            [task_id(1).to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(task_values, (task_id(1).to_string(), 1_234_567, 2_345_678));
    let worklog_values: Vec<(String, String, i64, Option<i64>)> = repository
        .connection()
        .prepare("SELECT id, task_id, start_us, end_us FROM worklogs ORDER BY id")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        worklog_values,
        vec![
            (
                worklog_id(10).to_string(),
                task_id(1).to_string(),
                1_000_001,
                Some(1_000_007)
            ),
            (
                worklog_id(11).to_string(),
                task_id(1).to_string(),
                1_000_007,
                None
            ),
            (
                worklog_id(12).to_string(),
                task_id(1).to_string(),
                1_000_004,
                Some(1_000_004)
            ),
        ]
    );
}

#[test]
fn version_2_migration_rejects_overlap_and_rolls_back_every_schema_change() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    create_v2_database(&path, &[(10, 100, Some(200)), (11, 150, Some(250))]);

    let error = SqliteRepository::open(&path).expect_err("overlapping v2 rows must fail migration");
    assert!(matches!(
        error,
        StorageError::CorruptData("same-task worklog overlap")
    ));

    let connection = rusqlite::Connection::open(&path).unwrap();
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 2);
    let rows: Vec<(String, i64, Option<i64>)> = connection
        .prepare("SELECT id, start_us, end_us FROM worklogs ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        rows,
        vec![
            (worklog_id(10).to_string(), 100, Some(200)),
            (worklog_id(11).to_string(), 150, Some(250)),
        ]
    );
    let overlap_triggers: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'trigger' AND name LIKE 'worklogs_reject_same_task_overlap_%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(overlap_triggers, 0);
}

#[test]
fn a_migration_is_not_applied_twice() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    create_v1_database(&path);
    let first = SqliteRepository::open(&path).unwrap();
    let task = first
        .find_task(task_id(1))
        .unwrap()
        .expect("the migrated task is stored");
    drop(first);

    let second = SqliteRepository::open(&path).unwrap();
    assert_eq!(user_version(&second), 5);
    assert_eq!(second.find_task(task_id(1)).unwrap(), Some(task));
}

#[test]
fn strict_tables_reject_null_ids_and_non_integer_timestamps() {
    let repository = repo();
    let conn = repository.connection();
    assert!(
        conn.execute(
            "INSERT INTO tasks (id, name, archived) VALUES (NULL, 'name', 0)",
            []
        )
        .is_err()
    );
    // The timestamp columns are strict integers and cannot be missing.
    assert!(
        conn.execute(
            "INSERT INTO tasks (id, name, archived) VALUES
             ('00000000-0000-7000-8000-000000000001', 'name', 0)",
            [],
        )
        .is_err(),
        "the NOT NULL timestamp columns must reject a timestamp-less insert"
    );
    assert!(
        conn.execute(
            "INSERT INTO tasks (id, name, archived, created_at_us, updated_at_us) VALUES
             ('00000000-0000-7000-8000-000000000001', 'name', 0, 'not an integer', 0)",
            [],
        )
        .is_err()
    );
    // updated_at can never precede created_at.
    assert!(
        conn.execute(
            "INSERT INTO tasks (id, name, archived, created_at_us, updated_at_us) VALUES
             ('00000000-0000-7000-8000-000000000001', 'name', 0, 200, 199)",
            [],
        )
        .is_err(),
        "the updated >= created CHECK must reject a backwards pair"
    );
    conn.execute(
        "INSERT INTO tasks (id, name, archived, created_at_us, updated_at_us) VALUES
         ('00000000-0000-7000-8000-000000000001', 'name', 0, 0, 0)",
        [],
    )
    .unwrap();
    assert!(
        conn.execute(
            "INSERT INTO worklogs (id, task_id, start_us, end_us)
             VALUES (NULL, '00000000-0000-7000-8000-000000000001', 1, NULL)",
            [],
        )
        .is_err()
    );
    assert!(
        conn.execute(
            "INSERT INTO worklogs (id, task_id, start_us, end_us)
             VALUES ('00000000-0000-7000-8000-000000000002',
                     '00000000-0000-7000-8000-000000000001', 'not an integer', NULL)",
            [],
        )
        .is_err()
    );
}

#[test]
fn a_database_from_a_newer_version_is_rejected_without_changes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    SqliteRepository::open(&path).unwrap();
    {
        let repository = SqliteRepository::open(&path).unwrap();
        repository
            .connection()
            .pragma_update(None, "user_version", 99)
            .unwrap();
    }
    let error = SqliteRepository::open(&path).expect_err("a future schema must be rejected");
    match error {
        StorageError::DatabaseTooNew { found, latest } => {
            assert_eq!(found, 99);
            assert_eq!(latest, 5);
            assert!(error.to_string().contains("newer"));
        }
        other => panic!("expected DatabaseTooNew, got {other:?}"),
    }
    // The file is untouched: the version is still the future one.
    let raw = rusqlite::Connection::open(&path).unwrap();
    let version: i64 = raw
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 99);
}

#[test]
fn simultaneous_first_opens_of_one_database_all_complete() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let path = path.clone();
            thread::spawn(move || -> Result<(), StorageError> {
                let repository = SqliteRepository::open(&path)?;
                let name = TaskName::new("racer").expect("seed names are valid");
                repository.create_task(Task::create(TaskId::generate(), name, at(100)))?;
                Ok(())
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap().expect("every first open completes");
    }
    let repository = SqliteRepository::open(&path).unwrap();
    assert_eq!(user_version(&repository), 5, "migrations ran exactly once");
    assert_eq!(repository.list_tasks().unwrap().len(), 8);
}

#[test]
fn application_tracking_operations_use_the_sqlite_ports() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let alpha = named_task(1, "alpha");
    let beta = named_task(2, "beta");
    let setup = SqliteRepository::open(&path).unwrap();
    setup.create_task(alpha.clone()).unwrap();
    setup.create_task(beta.clone()).unwrap();
    drop(setup);

    let repository = SqliteRepository::open(&path).unwrap();
    let mut application = TrackerApplication::load(repository).unwrap();
    assert!(matches!(
        application.set_active_task(alpha.id, at(100)).unwrap(),
        SetActiveTaskOutcome::Started { worklog } if worklog.start() == at(100)
    ));
    assert!(matches!(
        application.set_active_task(alpha.id, at(110)).unwrap(),
        SetActiveTaskOutcome::AlreadyActive { .. }
    ));
    assert!(matches!(
        application.set_active_task(beta.id, at(150)).unwrap(),
        SetActiveTaskOutcome::Switched { stopped, started }
            if stopped.end() == Some(at(150)) && started.start() == at(150)
    ));
    let TrackingState::Running { worklog: active } = application.current_tracking() else {
        panic!("the worklog must be active");
    };
    let active = active.id();
    assert!(matches!(
        application.clear_active_task(active, at(200)).unwrap(),
        ClearActiveTaskOutcome::Stopped { worklog } if worklog.end() == Some(at(200))
    ));
    assert_eq!(
        application.clear_active_task(active, at(250)).unwrap(),
        ClearActiveTaskOutcome::AlreadyIdle
    );

    let stored = SqliteRepository::open(&path).unwrap();
    assert_eq!(stored.active_worklog().unwrap(), None);
    assert_eq!(stored.list_worklogs(alpha.id).unwrap().len(), 1);
    assert_eq!(stored.list_worklogs(beta.id).unwrap().len(), 1);
}

#[test]
fn sqlite_application_ports_delegate_task_and_worklog_queries() {
    let repository = repo();
    let task = stamped_task(1, "alpha", 100, 100);
    TaskRepository::create_task(&repository, task.clone()).unwrap();
    assert_eq!(
        TaskRepository::find_task(&repository, task.id).unwrap(),
        Some(task.clone())
    );
    let items = TaskRepository::list_task_items(&repository).unwrap();
    assert_eq!(
        items,
        vec![TaskListItem {
            task: task.clone(),
            latest_work_start: None,
        }]
    );

    let worklog = Worklog::begin(worklog_id(1), task.id, at(100));
    TrackingRepository::insert_worklog(&repository, &worklog).unwrap();
    assert_eq!(
        WorklogRepository::find_worklog(&repository, worklog.id()).unwrap(),
        Some(worklog.clone())
    );
    let corrected = WorklogRepository::compare_and_set_worklog_times(
        &repository,
        worklog.id(),
        worklog.times(),
        WorklogTimes::new(at(110), None),
    )
    .unwrap()
    .worklog;
    assert_eq!(
        WorklogRepository::worklog_page(&repository, task.id, None)
            .unwrap()
            .worklogs,
        vec![corrected]
    );

    let saved = TaskRepository::rename_task(
        &repository,
        task.id,
        TaskName::new("renamed").unwrap(),
        at(150),
    )
    .unwrap();
    assert_eq!(saved.name().as_str(), "renamed");
    assert_eq!(saved.updated_at(), at(150));
    assert_eq!(
        TaskRepository::find_task(&repository, task.id).unwrap(),
        Some(saved)
    );
}

#[test]
fn the_task_list_query_derives_the_latest_work_start_per_task() {
    let repository = repo();
    repository
        .create_task(stamped_task(1, "one", 100, 100))
        .unwrap();
    repository
        .create_task(stamped_task(2, "two", 200, 200))
        .unwrap();
    // Several worklogs on task one, out of order, one still active.
    repository
        .insert_worklog(&Worklog::new(worklog_id(1), task_id(1), at(300), Some(at(350))).unwrap())
        .unwrap();
    repository
        .insert_worklog(&Worklog::begin(worklog_id(2), task_id(1), at(500)))
        .unwrap();
    repository
        .insert_worklog(&Worklog::new(worklog_id(3), task_id(1), at(200), Some(at(250))).unwrap())
        .unwrap();

    let items = repository.list_task_items().unwrap();
    assert_eq!(items.len(), 2);
    // The aggregate is the MAX over all of the task's worklogs, active or
    // not, without loading any full worklog.
    assert_eq!(items[0].task.id, task_id(1));
    assert_eq!(items[0].latest_work_start, Some(at(500)));
    assert_eq!(items[1].task.id, task_id(2));
    assert_eq!(items[1].latest_work_start, None, "no worklogs means none");
}

#[test]
fn two_clients_recover_lost_start_switch_and_stale_clear_without_stopping_the_other_timer() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let alpha = named_task(1, "alpha");
    let beta = named_task(2, "beta");
    let setup = SqliteRepository::open(&path).unwrap();
    setup.create_task(alpha.clone()).unwrap();
    setup.create_task(beta.clone()).unwrap();
    drop(setup);

    let (ready_send, ready) = sync_channel(1);
    let (started, started_receive) = sync_channel(1);
    let synchronize_active_read = Arc::new(AtomicBool::new(false));
    let repository = SynchronizingRepository {
        repository: SqliteRepository::open(&path).unwrap(),
        ready: ready_send,
        started: started_receive,
        synchronize_active_read: synchronize_active_read.clone(),
        fail_next_active_read: Arc::new(AtomicBool::new(false)),
    };
    let first = TrackerApplication::load(repository).unwrap();
    synchronize_active_read.store(true, Ordering::SeqCst);
    let second = thread::spawn({
        let path = path.clone();
        move || {
            ready
                .recv_timeout(Duration::from_secs(5))
                .expect("the first client must finish its authoritative read");
            let mut second =
                TrackerApplication::load(SqliteRepository::open(path).unwrap()).unwrap();
            second.set_active_task(beta.id, at(100)).unwrap();
            started
                .send(())
                .expect("the first client must still await the competing start");
            second
        }
    });
    let first = thread::spawn(move || {
        let mut first = first;
        let outcome = first.set_active_task(alpha.id, at(110));
        (first, outcome)
    });
    let (mut first, lost_start) = first.join().unwrap();
    let mut second = second.join().unwrap();
    assert!(matches!(
        lost_start,
        Err(ApplicationError::TrackingWrite(
            RepositoryError::ActiveWorklogExists
        ))
    ));
    assert!(matches!(
        first.current_tracking(),
        TrackingState::Running { worklog } if worklog.task_id() == beta.id
    ));

    assert!(matches!(
        first.set_active_task(alpha.id, at(120)),
        Ok(SetActiveTaskOutcome::Switched { stopped, started })
            if stopped.task_id() == beta.id
                && stopped.end() == Some(at(120))
                && started.task_id() == alpha.id
                && started.start() == at(120)
    ));
    let stale_beta = match second.current_tracking() {
        TrackingState::Running { worklog } => worklog.id(),
        TrackingState::Idle => panic!("beta must be active in the stale client"),
    };
    assert_eq!(
        second.clear_active_task(stale_beta, at(130)),
        Err(ApplicationError::TrackingStateChanged)
    );
    assert!(matches!(
        second.current_tracking(),
        TrackingState::Running { worklog } if worklog.task_id() == alpha.id
    ));

    let other = SqliteRepository::open(&path).unwrap();
    other.archive_task(beta.id, at(130)).unwrap();
    assert!(matches!(
        first.set_active_task(beta.id, at(140)),
        Err(ApplicationError::Domain(TrackingError::TaskArchived { id })) if id == beta.id
    ));
    assert!(matches!(
        first.current_tracking(),
        TrackingState::Running { worklog } if worklog.task_id() == alpha.id
    ));
    assert!(first.task(beta.id).unwrap().is_archived());
}

#[test]
fn a_stale_client_unarchive_adopts_the_tracking_another_client_started() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let task = named_task(1, "restored");
    let setup = SqliteRepository::open(&path).unwrap();
    setup.create_task(task.clone()).unwrap();
    setup.archive_task(task.id, at(120)).unwrap();
    drop(setup);

    // Client A loads while the task is archived and nothing is tracked.
    let mut stale = TrackerApplication::load(SqliteRepository::open(&path).unwrap()).unwrap();
    // Client B loads on the same file and restores the task.
    let mut restoring = TrackerApplication::load(SqliteRepository::open(&path).unwrap()).unwrap();
    assert!(matches!(
        restoring.unarchive_task(task.id, at(150)).unwrap(),
        TaskOutcome::Unarchived(restored) if !restored.is_archived()
    ));
    let started = match restoring.set_active_task(task.id, at(200)).unwrap() {
        SetActiveTaskOutcome::Started { worklog } => worklog,
        other => panic!("expected Started, got {other:?}"),
    };

    // Client A unarchives from its stale archived row. The refresh before
    // the write must pick up the tracking B started, not report Idle over
    // an active worklog that survived the call.
    assert!(matches!(
        stale.unarchive_task(task.id, at(250)).unwrap(),
        TaskOutcome::Unarchived(restored) if !restored.is_archived()
    ));
    assert_eq!(
        stale.current_tracking(),
        &TrackingState::Running {
            worklog: ActiveWorklog::begin(started.id(), task.id, at(200))
        }
    );

    // One active worklog, and the restored task kept its history.
    let stored = SqliteRepository::open(&path).unwrap();
    let active = stored
        .active_worklog()
        .unwrap()
        .expect("one active worklog");
    assert_eq!(active.id(), started.id());
    assert_eq!(active.task_id(), task.id);
    assert_eq!(active.start(), at(200));
    assert_eq!(
        stored.list_worklogs(task.id).unwrap(),
        vec![started.clone()]
    );

    // Client A stops the tracking it adopted, leaving a clean stopped row.
    let cleared = stale.clear_active_task(started.id(), at(300)).unwrap();
    assert!(matches!(
        cleared,
        ClearActiveTaskOutcome::Stopped { worklog } if worklog.end() == Some(at(300))
    ));
    assert_eq!(stale.current_tracking(), &TrackingState::Idle);
    assert_eq!(
        SqliteRepository::open(&path)
            .unwrap()
            .active_worklog()
            .unwrap(),
        None
    );
}

#[test]
fn restore_refreshes_tracking_started_between_its_initial_read_and_write() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let task = named_task(1, "restored");
    let setup = SqliteRepository::open(&path).unwrap();
    setup.create_task(task.clone()).unwrap();
    setup.archive_task(task.id, at(120)).unwrap();
    drop(setup);

    let (ready_send, ready) = sync_channel(1);
    let (started, started_receive) = sync_channel(1);
    let synchronize_active_read = Arc::new(AtomicBool::new(false));
    let repository = SynchronizingRepository {
        repository: SqliteRepository::open(&path).unwrap(),
        ready: ready_send,
        started: started_receive,
        synchronize_active_read: synchronize_active_read.clone(),
        fail_next_active_read: Arc::new(AtomicBool::new(false)),
    };
    let application = TrackerApplication::load(repository).unwrap();
    synchronize_active_read.store(true, Ordering::SeqCst);

    let competing = thread::spawn({
        let path = path.clone();
        move || {
            ready
                .recv_timeout(Duration::from_secs(5))
                .expect("the stale client must finish its initial tracking read");
            let mut application =
                TrackerApplication::load(SqliteRepository::open(path).unwrap()).unwrap();
            application.unarchive_task(task.id, at(150)).unwrap();
            let worklog = match application.set_active_task(task.id, at(200)).unwrap() {
                SetActiveTaskOutcome::Started { worklog } => worklog,
                other => panic!("expected start, got {other:?}"),
            };
            started
                .send(())
                .expect("the stale client must still await the competing start");
            worklog
        }
    });
    let (application, outcome) = thread::spawn(move || {
        let mut application = application;
        let outcome = application.unarchive_task(task.id, at(250));
        (application, outcome)
    })
    .join()
    .unwrap();
    let worklog = competing.join().unwrap();

    assert!(
        matches!(outcome, Ok(TaskOutcome::Unarchived(ref restored)) if !restored.is_archived())
    );
    assert!(matches!(
        application.current_tracking(),
        TrackingState::Running { worklog: active } if active.id() == worklog.id()
    ));
}

#[test]
fn archive_adopts_tracking_started_between_its_initial_read_and_write() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let task = named_task(1, "contended");
    SqliteRepository::open(&path)
        .unwrap()
        .create_task(task.clone())
        .unwrap();

    let (ready_send, ready) = sync_channel(1);
    let (started, started_receive) = sync_channel(1);
    let synchronize_active_read = Arc::new(AtomicBool::new(false));
    let repository = SynchronizingRepository {
        repository: SqliteRepository::open(&path).unwrap(),
        ready: ready_send,
        started: started_receive,
        synchronize_active_read: synchronize_active_read.clone(),
        fail_next_active_read: Arc::new(AtomicBool::new(false)),
    };
    let application = TrackerApplication::load(repository).unwrap();
    synchronize_active_read.store(true, Ordering::SeqCst);

    let competing = thread::spawn({
        let path = path.clone();
        move || {
            ready
                .recv_timeout(Duration::from_secs(5))
                .expect("the stale client must finish its initial tracking read");
            let mut application =
                TrackerApplication::load(SqliteRepository::open(path).unwrap()).unwrap();
            let worklog = match application.set_active_task(task.id, at(200)).unwrap() {
                SetActiveTaskOutcome::Started { worklog } => worklog,
                other => panic!("expected start, got {other:?}"),
            };
            started
                .send(())
                .expect("the stale client must still await the competing start");
            worklog
        }
    });
    let (application, outcome) = thread::spawn(move || {
        let mut application = application;
        let outcome = application.archive_task(task.id, at(250));
        (application, outcome)
    })
    .join()
    .unwrap();
    let worklog = competing.join().unwrap();

    assert_eq!(
        outcome,
        Err(ApplicationError::Domain(TrackingError::TaskIsActive {
            id: task.id
        }))
    );
    assert!(matches!(
        application.current_tracking(),
        TrackingState::Running { worklog: active } if active.id() == worklog.id()
    ));
    assert!(
        !SqliteRepository::open(&path)
            .unwrap()
            .find_task(task.id)
            .unwrap()
            .unwrap()
            .is_archived()
    );
}

#[test]
fn active_start_correction_between_snapshot_and_stop_cas_recovers_authoritative_state() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let alpha = named_task(1, "alpha");
    let setup = SqliteRepository::open(&path).unwrap();
    setup.create_task(alpha.clone()).unwrap();
    let active = Worklog::begin(worklog_id(1), alpha.id, at(100));
    setup.insert_worklog(&active).unwrap();
    drop(setup);

    let (ready_send, ready) = sync_channel(1);
    let (continue_send, continue_receive) = sync_channel(1);
    let synchronize_active_read = Arc::new(AtomicBool::new(false));
    let repository = SynchronizingRepository {
        repository: SqliteRepository::open(&path).unwrap(),
        ready: ready_send,
        started: continue_receive,
        synchronize_active_read: synchronize_active_read.clone(),
        fail_next_active_read: Arc::new(AtomicBool::new(false)),
    };
    let application = TrackerApplication::load(repository).unwrap();
    synchronize_active_read.store(true, Ordering::SeqCst);

    let correction = thread::spawn({
        let path = path.clone();
        let active = active.clone();
        move || {
            ready
                .recv_timeout(Duration::from_secs(5))
                .expect("the stop must read its pre-write snapshot");
            SqliteRepository::open(path)
                .unwrap()
                .compare_and_set_worklog_times(
                    active.id(),
                    active.times(),
                    WorklogTimes::new(at(120), None),
                )
                .unwrap();
            continue_send
                .send(())
                .expect("the stop must still await the correction");
        }
    });
    let active_id = active.id();
    let (mut application, result) = thread::spawn(move || {
        let mut application = application;
        let result = application.clear_active_task(active_id, at(200));
        (application, result)
    })
    .join()
    .unwrap();
    correction.join().unwrap();

    assert_eq!(result, Err(ApplicationError::TrackingStateChanged));
    let corrected = Worklog::begin(active.id(), alpha.id, at(120));
    assert_eq!(
        application.current_tracking(),
        &TrackingState::Running {
            worklog: ActiveWorklog::begin(active.id(), alpha.id, at(120))
        }
    );
    assert_eq!(
        application
            .tasks(TaskOrdering::RecentlyWorked)
            .into_iter()
            .find(|item| item.task.id() == alpha.id)
            .unwrap()
            .latest_work_start,
        Some(at(120))
    );
    let stored = SqliteRepository::open(&path).unwrap();
    assert_eq!(stored.list_worklogs(alpha.id).unwrap(), vec![corrected]);
    assert_eq!(
        stored.active_worklog().unwrap(),
        Some(Worklog::begin(active.id(), alpha.id, at(120)))
    );

    assert!(matches!(
        application.clear_active_task(active.id(), at(200)),
        Ok(ClearActiveTaskOutcome::Stopped { worklog }) if worklog.start() == at(120) && worklog.end() == Some(at(200))
    ));
}

#[test]
fn active_start_correction_between_snapshot_and_switch_cas_recovers_authoritative_state() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let alpha = named_task(1, "alpha");
    let beta = named_task(2, "beta");
    let setup = SqliteRepository::open(&path).unwrap();
    setup.create_task(alpha.clone()).unwrap();
    setup.create_task(beta.clone()).unwrap();
    let active = Worklog::begin(worklog_id(1), alpha.id, at(100));
    setup.insert_worklog(&active).unwrap();
    drop(setup);

    let (ready_send, ready) = sync_channel(1);
    let (continue_send, continue_receive) = sync_channel(1);
    let synchronize_active_read = Arc::new(AtomicBool::new(false));
    let repository = SynchronizingRepository {
        repository: SqliteRepository::open(&path).unwrap(),
        ready: ready_send,
        started: continue_receive,
        synchronize_active_read: synchronize_active_read.clone(),
        fail_next_active_read: Arc::new(AtomicBool::new(false)),
    };
    let application = TrackerApplication::load(repository).unwrap();
    synchronize_active_read.store(true, Ordering::SeqCst);

    let correction = thread::spawn({
        let path = path.clone();
        let active = active.clone();
        move || {
            ready
                .recv_timeout(Duration::from_secs(5))
                .expect("the switch must read its pre-write snapshot");
            SqliteRepository::open(path)
                .unwrap()
                .compare_and_set_worklog_times(
                    active.id(),
                    active.times(),
                    WorklogTimes::new(at(120), None),
                )
                .unwrap();
            continue_send
                .send(())
                .expect("the switch must still await the correction");
        }
    });
    let beta_id = beta.id;
    let (mut application, result) = thread::spawn(move || {
        let mut application = application;
        let result = application.set_active_task(beta_id, at(200));
        (application, result)
    })
    .join()
    .unwrap();
    correction.join().unwrap();

    assert_eq!(result, Err(ApplicationError::TrackingStateChanged));
    assert_eq!(
        application.current_tracking(),
        &TrackingState::Running {
            worklog: ActiveWorklog::begin(active.id(), alpha.id, at(120))
        }
    );
    let items = application.tasks(TaskOrdering::RecentlyWorked);
    assert_eq!(
        items
            .iter()
            .find(|item| item.task.id() == alpha.id)
            .unwrap()
            .latest_work_start,
        Some(at(120))
    );
    assert_eq!(
        items
            .iter()
            .find(|item| item.task.id() == beta.id)
            .unwrap()
            .latest_work_start,
        None
    );
    let stored = SqliteRepository::open(&path).unwrap();
    assert_eq!(
        stored.list_worklogs(alpha.id).unwrap(),
        vec![Worklog::begin(active.id(), alpha.id, at(120))]
    );
    assert_eq!(stored.list_worklogs(beta.id).unwrap(), Vec::new());

    assert!(matches!(
        application.set_active_task(beta.id, at(200)),
        Ok(SetActiveTaskOutcome::Switched { stopped, started })
            if stopped.start() == at(120)
                && stopped.end() == Some(at(200))
                && started.task_id() == beta.id
                && started.start() == at(200)
    ));
}

#[test]
fn a_real_sqlite_write_conflict_reports_a_failed_recovery_separately() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let alpha = named_task(1, "alpha");
    let beta = named_task(2, "beta");
    let setup = SqliteRepository::open(&path).unwrap();
    setup.create_task(alpha.clone()).unwrap();
    setup.create_task(beta.clone()).unwrap();
    drop(setup);

    let (ready_send, ready) = sync_channel(1);
    let (started, started_receive) = sync_channel(1);
    let synchronize_active_read = Arc::new(AtomicBool::new(false));
    let fail_next_active_read = Arc::new(AtomicBool::new(false));
    let repository = SynchronizingRepository {
        repository: SqliteRepository::open(&path).unwrap(),
        ready: ready_send,
        started: started_receive,
        synchronize_active_read: synchronize_active_read.clone(),
        fail_next_active_read: fail_next_active_read.clone(),
    };
    let application = TrackerApplication::load(repository).unwrap();
    synchronize_active_read.store(true, Ordering::SeqCst);
    let competing = thread::spawn({
        let path = path.clone();
        move || {
            ready
                .recv_timeout(Duration::from_secs(5))
                .expect("the first client must finish its authoritative read");
            let mut competing =
                TrackerApplication::load(SqliteRepository::open(path).unwrap()).unwrap();
            competing.set_active_task(beta.id, at(100)).unwrap();
            fail_next_active_read.store(true, Ordering::SeqCst);
            started
                .send(())
                .expect("the first client must still await the competing start");
        }
    });
    let (application, error) = thread::spawn(move || {
        let mut application = application;
        let error = application
            .set_active_task(alpha.id, at(110))
            .expect_err("the competing active worklog must reject this start");
        (application, error)
    })
    .join()
    .unwrap();
    competing.join().unwrap();

    assert_eq!(
        error,
        ApplicationError::TrackingRecovery(RepositoryError::Backend {
            message: "recovery read failed".to_owned(),
        })
    );
    assert_eq!(application.current_tracking(), &TrackingState::Idle);
    assert!(matches!(
        SqliteRepository::open(&path).unwrap().active_worklog(),
        Ok(Some(worklog)) if worklog.task_id() == beta.id
    ));
}

#[test]
fn task_create_find_list_rename_and_missing_errors() {
    let repository = repo();
    // Insert out of id order; listing orders by id, not insertion order.
    repository
        .create_task(stamped_task(3, "third", 300, 300))
        .unwrap();
    repository
        .create_task(stamped_task(1, "first", 100, 100))
        .unwrap();
    repository
        .create_task(stamped_task(2, "second", 200, 200))
        .unwrap();

    let tasks = repository.list_tasks().unwrap();
    let names: Vec<&str> = tasks.iter().map(|task| task.name().as_str()).collect();
    assert_eq!(names, ["first", "second", "third"]);

    let found = repository.find_task(task_id(2)).unwrap().unwrap();
    assert_eq!(found.name().as_str(), "second");
    assert!(!found.is_archived());
    assert!(repository.find_task(task_id(9)).unwrap().is_none());

    // A rename goes through the field-specific port.
    let renamed = TaskRepository::rename_task(
        &repository,
        task_id(2),
        TaskName::new("renamed").unwrap(),
        at(350),
    )
    .unwrap();
    assert_eq!(renamed.name().as_str(), "renamed");
    assert_eq!(renamed.updated_at(), at(350));
    assert_eq!(renamed.created_at(), at(200), "creation is never rewritten");
    assert_eq!(
        repository.find_task(task_id(2)).unwrap().unwrap().name(),
        renamed.name()
    );
    // The list order is untouched by the rename.
    let tasks = repository.list_tasks().unwrap();
    let names: Vec<&str> = tasks.iter().map(|task| task.name().as_str()).collect();
    assert_eq!(names, ["first", "renamed", "third"]);

    // Renaming a missing task reports which one.
    assert!(matches!(
        TaskRepository::rename_task(&repository, task_id(9), TaskName::new("missing").unwrap(), at(100)),
        Err(RepositoryError::TaskNotFound { id }) if id == task_id(9)
    ));

    assert!(matches!(
        repository.create_task(stamped_task(1, "duplicate", 0, 0)),
        Err(StorageError::TaskAlreadyExists { id }) if id == task_id(1)
    ));
}

#[test]
fn rename_task_keeps_stored_updated_at_from_moving_backward() {
    let repository = repo();
    repository
        .create_task(stamped_task(1, "one", 100, 500))
        .unwrap();
    // A caller with a late client clock, for example one that computed its
    // timestamp earlier, cannot rewind the stored value.
    let saved = TaskRepository::rename_task(
        &repository,
        task_id(1),
        TaskName::new("older clock").unwrap(),
        at(200),
    )
    .unwrap();
    assert_eq!(saved.name().as_str(), "older clock");
    assert_eq!(saved.updated_at(), at(500), "the stored value stays");
    assert_eq!(saved.created_at(), at(100));
}

#[test]
fn a_stale_rename_cannot_overwrite_a_concurrent_archive_state() {
    let repository = repo();
    repository
        .create_task(stamped_task(1, "alpha", 100, 100))
        .unwrap();
    // A second client archives the task after this client last read it.
    let archived = TaskRepository::archive_task(&repository, task_id(1), at(150)).unwrap();
    assert!(archived.is_archived());

    // The stale client renames from its unarchived view: the rename lands,
    // and the concurrent archive state survives it.
    let renamed = TaskRepository::rename_task(
        &repository,
        task_id(1),
        TaskName::new("beta").unwrap(),
        at(200),
    )
    .unwrap();
    assert_eq!(renamed.name().as_str(), "beta");
    assert!(
        renamed.is_archived(),
        "the rename did not resurrect the task"
    );
    let stored = repository.find_task(task_id(1)).unwrap().unwrap();
    assert!(stored.is_archived());
    assert_eq!(stored.name().as_str(), "beta");
    assert_eq!(stored.updated_at(), at(200));
}

#[test]
fn a_stale_archive_cannot_overwrite_a_concurrent_rename() {
    let repository = repo();
    repository
        .create_task(stamped_task(1, "alpha", 100, 100))
        .unwrap();
    // A second client renames the task after this client last read it.
    let renamed = TaskRepository::rename_task(
        &repository,
        task_id(1),
        TaskName::new("beta").unwrap(),
        at(150),
    )
    .unwrap();
    assert_eq!(renamed.name().as_str(), "beta");

    // The stale client archives: the archive lands and keeps the rename.
    let archived = TaskRepository::archive_task(&repository, task_id(1), at(200)).unwrap();
    assert!(archived.is_archived());
    assert_eq!(
        archived.name().as_str(),
        "beta",
        "the archive kept the rename"
    );
    assert_eq!(archived.updated_at(), at(200));
    let stored = repository.find_task(task_id(1)).unwrap().unwrap();
    assert_eq!(stored.name().as_str(), "beta");
}

#[test]
fn idempotent_metadata_operations_leave_updated_at_unchanged() {
    let repository = repo();
    repository
        .create_task(stamped_task(1, "alpha", 100, 100))
        .unwrap();
    repository
        .create_task(stamped_task(2, "gone", 100, 100))
        .unwrap();
    TaskRepository::archive_task(&repository, task_id(2), at(150)).unwrap();

    // A real rename advances updated_at ...
    let renamed = TaskRepository::rename_task(
        &repository,
        task_id(1),
        TaskName::new("beta").unwrap(),
        at(200),
    )
    .unwrap();
    assert_eq!(renamed.updated_at(), at(200));
    // ... and a late client clock cannot move it backward.
    let renamed = TaskRepository::rename_task(
        &repository,
        task_id(1),
        TaskName::new("gamma").unwrap(),
        at(150),
    )
    .unwrap();
    assert_eq!(renamed.updated_at(), at(200), "MAX keeps the stored value");

    // Renaming to the stored name is a no-op for the timestamp, even at a
    // much later occurred_at.
    let again = TaskRepository::rename_task(
        &repository,
        task_id(1),
        TaskName::new("gamma").unwrap(),
        at(900),
    )
    .unwrap();
    assert_eq!(again.updated_at(), at(200));
    // So are archiving an archived task and restoring an active task.
    let again = TaskRepository::archive_task(&repository, task_id(2), at(900)).unwrap();
    assert_eq!(again.updated_at(), at(150));
    let again = TaskRepository::unarchive_task(&repository, task_id(1), at(900)).unwrap();
    assert_eq!(again.updated_at(), at(200));

    // A real restore still advances the timestamp.
    let restored = TaskRepository::unarchive_task(&repository, task_id(2), at(900)).unwrap();
    assert!(!restored.is_archived());
    assert_eq!(restored.updated_at(), at(900));
}

#[test]
fn archiving_a_task_with_an_active_worklog_is_rejected_until_it_stops() {
    let repository = repo();
    let task = stamped_task(1, "running", 100, 100);
    repository.create_task(task.clone()).unwrap();
    let started = Worklog::begin(worklog_id(1), task.id, at(200));
    repository.insert_worklog(&started).unwrap();

    let error = TaskRepository::archive_task(&repository, task.id, at(300))
        .expect_err("the active task must not archive");
    assert!(matches!(
        error,
        RepositoryError::TaskIsActive { id } if id == task.id
    ));
    assert!(
        !repository
            .find_task(task.id)
            .unwrap()
            .unwrap()
            .is_archived()
    );
    assert_eq!(
        repository.active_worklog().unwrap().unwrap().id(),
        started.id()
    );

    // After the worklog is stopped, the same archive succeeds.
    repository
        .stop_worklog(started.id(), started.start(), at(400))
        .unwrap();
    let archived = TaskRepository::archive_task(&repository, task.id, at(300)).unwrap();
    assert!(archived.is_archived());
    assert_eq!(archived.updated_at(), at(300));
}

#[test]
fn start_and_stop_persist_and_report_failures() {
    let repository = repo();
    let mut tracker = Tracker::idle();
    let task = named_task(1, "work");
    repository.create_task(task.clone()).unwrap();

    let started = tracker.start(&task, at(100)).unwrap();
    repository.insert_worklog(&started).unwrap();
    assert_eq!(repository.active_worklog().unwrap(), Some(started.clone()));

    let stopped = tracker.stop(at(150)).unwrap();
    let stored = repository
        .stop_worklog(stopped.id(), at(100), at(150))
        .unwrap();
    assert_eq!(stored.id(), started.id());
    assert_eq!(stored.start(), at(100));
    assert_eq!(stored.end(), Some(at(150)));
    assert_eq!(repository.active_worklog().unwrap(), None);
    assert_eq!(repository.list_worklogs(task_id(1)).unwrap(), vec![stored]);

    // Stopping a missing worklog reports which one.
    let missing = worklog_id(77);
    assert!(matches!(
        repository.stop_worklog(missing, at(100), at(200)),
        Err(StorageError::WorklogNotFound { id }) if id == missing
    ));
    // Stopping a stopped worklog reports the conflict.
    assert!(matches!(
        repository.stop_worklog(started.id(), started.start(), at(200)),
        Err(StorageError::WorklogAlreadyStopped { id }) if id == started.id()
    ));
    // A backwards end time is rejected by the database.
    let mut tracker = Tracker::idle();
    let worklog = tracker.start(&task, at(300)).unwrap();
    repository.insert_worklog(&worklog).unwrap();
    let error = repository
        .stop_worklog(worklog.id(), worklog.start(), at(299))
        .expect_err("end before start must fail");
    assert!(matches!(error, StorageError::Constraint(_)));
    assert_eq!(
        repository.active_worklog().unwrap().unwrap().id(),
        worklog.id()
    );
}

#[test]
fn inserting_a_second_active_worklog_is_rejected_by_the_database() {
    let repository = repo();
    let task = named_task(1, "work");
    repository.create_task(task.clone()).unwrap();
    let mut tracker = Tracker::idle();
    let first = tracker.start(&task, at(100)).unwrap();
    repository.insert_worklog(&first).unwrap();

    let second = Worklog::begin(worklog_id(42), task.id, at(200));
    let error = repository
        .insert_worklog(&second)
        .expect_err("second active worklog must fail");
    assert!(matches!(
        error,
        StorageError::SameTaskWorklogOverlap { id } if id == second.id()
    ));
    // The first worklog is still the only active one.
    assert_eq!(repository.active_worklog().unwrap(), Some(first.clone()));

    // An worklog for a missing task is rejected as a missing task. The first
    // worklog is stopped first so the active-worklog rule cannot mask the
    // foreign-key failure.
    tracker.stop(at(200)).unwrap();
    repository
        .stop_worklog(first.id(), first.start(), at(200))
        .unwrap();
    let orphan = Worklog::begin(worklog_id(43), task_id(99), at(300));
    assert!(matches!(
        repository.insert_worklog(&orphan),
        Err(StorageError::TaskNotFound { id }) if id == task_id(99)
    ));
}

#[test]
fn a_duplicate_worklog_id_is_distinct_from_the_active_worklog_conflict() {
    let repository = repo();
    let task = named_task(1, "work");
    repository.create_task(task.clone()).unwrap();
    let mut tracker = Tracker::idle();
    let first = tracker.start(&task, at(100)).unwrap();
    repository.insert_worklog(&first).unwrap();

    // A stopped worklog reusing a stored id hits the primary key, not the
    // single-active rule.
    let duplicate = Worklog::new(first.id(), task.id, at(500), Some(at(600))).unwrap();
    let error = repository
        .insert_worklog(&duplicate)
        .expect_err("a stored id must not be inserted twice");
    assert!(matches!(
        error,
        StorageError::WorklogAlreadyExists { id } if id == first.id()
    ));

    // A fresh id on the same task is reported as an interval overlap.
    let second_active = Worklog::begin(worklog_id(42), task.id, at(200));
    let error = repository
        .insert_worklog(&second_active)
        .expect_err("two active worklogs cannot coexist");
    assert!(matches!(
        error,
        StorageError::SameTaskWorklogOverlap { id } if id == second_active.id()
    ));
    assert_eq!(repository.active_worklog().unwrap(), Some(first.clone()));
    assert_eq!(repository.list_worklogs(task.id).unwrap().len(), 1);
}

#[test]
fn same_task_overlap_insert_uses_half_open_and_zero_duration_rules() {
    let repository = repo();
    let first_task = named_task(1, "first");
    let second_task = named_task(2, "second");
    repository.create_task(first_task.clone()).unwrap();
    repository.create_task(second_task.clone()).unwrap();
    repository
        .insert_worklog(
            &Worklog::new(worklog_id(1), first_task.id, at(100), Some(at(200))).unwrap(),
        )
        .unwrap();

    let overlap = Worklog::new(worklog_id(2), first_task.id, at(150), Some(at(250))).unwrap();
    assert!(matches!(
        repository.insert_worklog(&overlap),
        Err(StorageError::SameTaskWorklogOverlap { id }) if id == overlap.id()
    ));

    for worklog in [
        Worklog::new(worklog_id(3), first_task.id, at(50), Some(at(100))).unwrap(),
        Worklog::new(worklog_id(4), first_task.id, at(200), Some(at(250))).unwrap(),
        Worklog::new(worklog_id(5), first_task.id, at(150), Some(at(150))).unwrap(),
        Worklog::new(worklog_id(6), second_task.id, at(150), Some(at(250))).unwrap(),
    ] {
        repository.insert_worklog(&worklog).unwrap();
    }
    assert_eq!(repository.list_worklogs(first_task.id).unwrap().len(), 4);
    assert_eq!(repository.list_worklogs(second_task.id).unwrap().len(), 1);
}

#[test]
fn an_active_interval_overlaps_every_later_interval_on_the_same_task() {
    let repository = repo();
    let task = named_task(1, "running");
    repository.create_task(task.clone()).unwrap();
    let active = Worklog::begin(worklog_id(1), task.id, at(100));
    repository.insert_worklog(&active).unwrap();

    let later = Worklog::new(worklog_id(2), task.id, at(200), Some(at(300))).unwrap();
    assert!(matches!(
        repository.insert_worklog(&later),
        Err(StorageError::SameTaskWorklogOverlap { id }) if id == later.id()
    ));
    assert_eq!(repository.active_worklog().unwrap(), Some(active));
}

#[test]
fn concurrent_clients_cannot_insert_overlapping_worklogs_for_one_task() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let setup = SqliteRepository::open(&path).unwrap();
    let task = named_task(1, "shared");
    setup.create_task(task.clone()).unwrap();
    drop(setup);

    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = [(1, 100, 200), (2, 150, 250)]
        .into_iter()
        .map(|(tag, start, end)| {
            let path = path.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                let repository = SqliteRepository::open(path).unwrap();
                let worklog =
                    Worklog::new(worklog_id(tag), task.id, at(start), Some(at(end))).unwrap();
                barrier.wait();
                repository.insert_worklog(&worklog)
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(StorageError::SameTaskWorklogOverlap { .. })))
            .count(),
        1
    );
    assert_eq!(
        SqliteRepository::open(&path)
            .unwrap()
            .list_worklogs(task.id)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn find_and_compare_and_set_correct_a_completed_worklog_without_changing_identity() {
    let repository = repo();
    let task = named_task(1, "editable");
    repository.create_task(task.clone()).unwrap();
    let original = Worklog::new(worklog_id(1), task.id, at(100), Some(at(200))).unwrap();
    repository.insert_worklog(&original).unwrap();

    assert_eq!(
        repository.find_worklog(original.id()).unwrap(),
        Some(original.clone())
    );
    assert_eq!(repository.find_worklog(worklog_id(99)).unwrap(), None);
    let corrected = repository
        .compare_and_set_worklog_times(
            original.id(),
            original.times(),
            WorklogTimes::new(at(120), Some(at(220))),
        )
        .unwrap()
        .worklog;

    assert_eq!(corrected.id(), original.id());
    assert_eq!(corrected.task_id(), task.id);
    assert_eq!(corrected.start(), at(120));
    assert_eq!(corrected.end(), Some(at(220)));
    assert_eq!(
        repository.find_worklog(original.id()).unwrap(),
        Some(corrected)
    );

    let active = Worklog::begin(worklog_id(2), task.id, at(300));
    repository.insert_worklog(&active).unwrap();
    let corrected_active = repository
        .compare_and_set_worklog_times(
            active.id(),
            active.times(),
            WorklogTimes::new(at(250), None),
        )
        .unwrap()
        .worklog;
    assert_eq!(corrected_active.id(), active.id());
    assert_eq!(corrected_active.task_id(), active.task_id());
    assert_eq!(corrected_active.start(), at(250));
    assert_eq!(corrected_active.end(), None);
}

#[test]
fn correction_returns_the_active_tasks_exact_aggregate_for_another_task() {
    let repository = repo();
    let active_task = named_task(1, "active");
    let corrected_task = named_task(2, "corrected");
    repository.create_task(active_task.clone()).unwrap();
    repository.create_task(corrected_task.clone()).unwrap();
    let active = Worklog::begin(worklog_id(1), active_task.id, at(600));
    let corrected = Worklog::new(worklog_id(2), corrected_task.id, at(400), Some(at(410))).unwrap();
    repository.insert_worklog(&active).unwrap();
    repository.insert_worklog(&corrected).unwrap();

    let externally_corrected_active = repository
        .compare_and_set_worklog_times(
            active.id(),
            active.times(),
            WorklogTimes::new(at(540), None),
        )
        .unwrap()
        .worklog;
    let correction = repository
        .compare_and_set_worklog_times(
            corrected.id(),
            corrected.times(),
            WorklogTimes::new(at(350), Some(at(360))),
        )
        .unwrap();

    assert_eq!(correction.task_latest_work_start, Some(at(350)));
    assert_eq!(correction.active_worklog, Some(externally_corrected_active));
    assert_eq!(correction.active_task_latest_work_start, Some(at(540)));
}

#[test]
fn same_task_active_correction_reuses_the_corrected_task_aggregate() {
    let repository = repo();
    let task = named_task(1, "active");
    repository.create_task(task.clone()).unwrap();
    let active = Worklog::begin(worklog_id(1), task.id, at(600));
    repository.insert_worklog(&active).unwrap();
    let start_reads = Box::into_raw(Box::new(AtomicUsize::new(0)));
    assert_eq!(
        unsafe {
            rusqlite::ffi::sqlite3_set_authorizer(
                repository.connection().handle(),
                Some(count_worklog_start_reads),
                start_reads.cast(),
            )
        },
        rusqlite::ffi::SQLITE_OK
    );

    let correction = repository
        .compare_and_set_worklog_times(
            active.id(),
            active.times(),
            WorklogTimes::new(at(540), None),
        )
        .unwrap();

    assert_eq!(correction.task_latest_work_start, Some(at(540)));
    assert_eq!(correction.active_task_latest_work_start, Some(at(540)));
    assert_eq!(
        unsafe { (&*start_reads).load(Ordering::SeqCst) },
        11,
        "the active task reuses the corrected task aggregate"
    );
    unsafe {
        rusqlite::ffi::sqlite3_set_authorizer(
            repository.connection().handle(),
            None,
            std::ptr::null_mut(),
        );
        drop(Box::from_raw(start_reads));
    }
}

#[test]
fn compare_and_set_distinguishes_missing_and_each_stale_expected_end_shape() {
    let repository = repo();
    let task = named_task(1, "editable");
    repository.create_task(task.clone()).unwrap();
    let completed = Worklog::new(worklog_id(1), task.id, at(100), Some(at(200))).unwrap();
    let active = Worklog::begin(worklog_id(2), task.id, at(200));
    repository.insert_worklog(&completed).unwrap();
    repository.insert_worklog(&active).unwrap();

    assert!(matches!(
        repository.compare_and_set_worklog_times(
            worklog_id(99),
            WorklogTimes::new(at(100), Some(at(200))),
            WorklogTimes::new(at(110), Some(at(210))),
        ),
        Err(StorageError::WorklogNotFound { id }) if id == worklog_id(99)
    ));
    for expected in [
        WorklogTimes::new(at(99), Some(at(200))),
        WorklogTimes::new(at(100), Some(at(201))),
        WorklogTimes::new(at(100), None),
    ] {
        assert!(matches!(
            repository.compare_and_set_worklog_times(
                completed.id(),
                expected,
                WorklogTimes::new(at(110), Some(at(210))),
            ),
            Err(StorageError::WorklogChanged { id }) if id == completed.id()
        ));
    }
    assert!(matches!(
        repository.compare_and_set_worklog_times(
            active.id(),
            WorklogTimes::new(at(200), Some(at(250))),
            WorklogTimes::new(at(210), None),
        ),
        Err(StorageError::WorklogChanged { id }) if id == active.id()
    ));
}

#[test]
fn compare_and_set_rejects_shape_changes_and_backwards_replacements() {
    let repository = repo();
    let task = named_task(1, "editable");
    repository.create_task(task.clone()).unwrap();
    let completed = Worklog::new(worklog_id(1), task.id, at(50), Some(at(100))).unwrap();
    let active = Worklog::begin(worklog_id(2), task.id, at(100));
    repository.insert_worklog(&completed).unwrap();
    repository.insert_worklog(&active).unwrap();

    for (worklog, replacement) in [
        (&completed, WorklogTimes::new(at(60), None)),
        (&active, WorklogTimes::new(at(110), Some(at(120)))),
        (&completed, WorklogTimes::new(at(90), Some(at(80)))),
    ] {
        assert!(matches!(
            repository.compare_and_set_worklog_times(worklog.id(), worklog.times(), replacement,),
            Err(StorageError::Constraint(_))
        ));
        assert_eq!(
            repository.find_worklog(worklog.id()).unwrap(),
            Some(worklog.clone())
        );
    }
}

#[test]
fn compare_and_set_enforces_overlap_and_rolls_back_the_failed_update() {
    let repository = repo();
    let task = named_task(1, "editable");
    repository.create_task(task.clone()).unwrap();
    let fixed = Worklog::new(worklog_id(1), task.id, at(100), Some(at(200))).unwrap();
    let editable = Worklog::new(worklog_id(2), task.id, at(300), Some(at(400))).unwrap();
    repository.insert_worklog(&fixed).unwrap();
    repository.insert_worklog(&editable).unwrap();

    let error = repository
        .compare_and_set_worklog_times(
            editable.id(),
            editable.times(),
            WorklogTimes::new(at(150), Some(at(350))),
        )
        .expect_err("the corrected interval overlaps the fixed interval");
    assert!(matches!(
        error,
        StorageError::SameTaskWorklogOverlap { id } if id == editable.id()
    ));
    assert_eq!(
        repository.find_worklog(editable.id()).unwrap(),
        Some(editable.clone())
    );

    let touching = repository
        .compare_and_set_worklog_times(
            editable.id(),
            editable.times(),
            WorklogTimes::new(at(200), Some(at(300))),
        )
        .unwrap()
        .worklog;
    assert_eq!(touching.start(), at(200));
    assert_eq!(touching.end(), Some(at(300)));
}

#[test]
fn compare_and_set_accepts_zero_duration_and_completed_archived_corrections() {
    let repository = repo();
    let task = named_task(1, "archived");
    repository.create_task(task.clone()).unwrap();
    let original = Worklog::new(worklog_id(1), task.id, at(100), Some(at(200))).unwrap();
    repository.insert_worklog(&original).unwrap();
    repository.archive_task(task.id, at(300)).unwrap();

    let corrected = repository
        .compare_and_set_worklog_times(
            original.id(),
            original.times(),
            WorklogTimes::new(at(150), Some(at(150))),
        )
        .unwrap()
        .worklog;
    assert_eq!(corrected.end(), Some(corrected.start()));
    assert!(
        repository
            .find_task(task.id)
            .unwrap()
            .unwrap()
            .is_archived()
    );
}

#[test]
fn a_failed_compare_and_set_transaction_leaves_the_worklog_unchanged() {
    let repository = repo();
    let task = named_task(1, "editable");
    repository.create_task(task.clone()).unwrap();
    let original = Worklog::new(worklog_id(1), task.id, at(100), Some(at(200))).unwrap();
    repository.insert_worklog(&original).unwrap();
    repository
        .connection()
        .execute_batch(
            "CREATE TRIGGER reject_correction
             AFTER UPDATE OF start_us, end_us ON worklogs
             BEGIN SELECT RAISE(ABORT, 'reject correction'); END;",
        )
        .unwrap();

    assert!(matches!(
        repository.compare_and_set_worklog_times(
            original.id(),
            original.times(),
            WorklogTimes::new(at(110), Some(at(210))),
        ),
        Err(StorageError::Sql(_))
    ));
    assert_eq!(
        repository.find_worklog(original.id()).unwrap(),
        Some(original)
    );
}

#[test]
fn two_clients_cannot_apply_corrections_from_the_same_expected_values() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let setup = SqliteRepository::open(&path).unwrap();
    let task = named_task(1, "shared");
    setup.create_task(task.clone()).unwrap();
    let original = Worklog::new(worklog_id(1), task.id, at(100), Some(at(200))).unwrap();
    setup.insert_worklog(&original).unwrap();
    drop(setup);

    let first = SqliteRepository::open(&path).unwrap();
    let second = SqliteRepository::open(&path).unwrap();
    let winner = first
        .compare_and_set_worklog_times(
            original.id(),
            original.times(),
            WorklogTimes::new(at(110), Some(at(210))),
        )
        .unwrap()
        .worklog;
    assert!(matches!(
        second.compare_and_set_worklog_times(
            original.id(),
            original.times(),
            WorklogTimes::new(at(120), Some(at(220))),
        ),
        Err(StorageError::WorklogChanged { id }) if id == original.id()
    ));
    assert_eq!(second.find_worklog(original.id()).unwrap(), Some(winner));
}

#[test]
fn completed_deletion_returns_the_deleted_row_and_latest_task_aggregate() {
    let repository = repo();
    let task = named_task(1, "history");
    repository.create_task(task.clone()).unwrap();
    let earlier = Worklog::new(worklog_id(1), task.id, at(100), Some(at(150))).unwrap();
    let latest = Worklog::new(worklog_id(2), task.id, at(300), Some(at(350))).unwrap();
    repository.insert_worklog(&earlier).unwrap();
    repository.insert_worklog(&latest).unwrap();

    let deletion = repository
        .compare_and_delete_completed_worklog(latest.id(), task.id, latest.times())
        .unwrap();

    assert_eq!(deletion.worklog, latest);
    assert_eq!(deletion.task_latest_work_start, Some(at(100)));
    assert_eq!(repository.list_worklogs(task.id).unwrap(), [earlier]);
    assert_eq!(repository.active_worklog().unwrap(), None);
}

#[test]
fn active_deletion_is_rejected_by_the_repository_and_direct_sql() {
    let repository = repo();
    let task = named_task(1, "running");
    repository.create_task(task.clone()).unwrap();
    let completed = Worklog::new(worklog_id(1), task.id, at(50), Some(at(75))).unwrap();
    let active = Worklog::begin(worklog_id(2), task.id, at(100));
    repository.insert_worklog(&completed).unwrap();
    repository.insert_worklog(&active).unwrap();

    assert!(matches!(
        repository.compare_and_delete_completed_worklog(
            active.id(),
            active.task_id(),
            active.times(),
        ),
        Err(StorageError::WorklogIsActive { id }) if id == active.id()
    ));
    let error = repository
        .connection()
        .execute(
            "DELETE FROM worklogs WHERE id = ?1",
            [active.id().to_string()],
        )
        .expect_err("the schema must preserve the active row");
    assert!(matches!(
        error,
        rusqlite::Error::SqliteFailure(_, Some(message))
            if message == "active worklog cannot be deleted"
    ));

    repository
        .connection()
        .execute(
            "DELETE FROM worklogs WHERE id = ?1",
            [completed.id().to_string()],
        )
        .unwrap();
    assert_eq!(repository.find_worklog(completed.id()).unwrap(), None);
    assert_eq!(repository.find_worklog(active.id()).unwrap(), Some(active));
}

#[test]
fn completed_deletion_distinguishes_missing_stale_and_moved_rows() {
    let repository = repo();
    let alpha = named_task(1, "alpha");
    let beta = named_task(2, "beta");
    repository.create_task(alpha.clone()).unwrap();
    repository.create_task(beta.clone()).unwrap();
    let target = Worklog::new(worklog_id(1), alpha.id, at(100), Some(at(150))).unwrap();
    repository.insert_worklog(&target).unwrap();

    assert!(matches!(
        repository.compare_and_delete_completed_worklog(
            worklog_id(99),
            alpha.id,
            target.times(),
        ),
        Err(StorageError::WorklogNotFound { id }) if id == worklog_id(99)
    ));
    for expected in [
        WorklogTimes::new(at(99), Some(at(150))),
        WorklogTimes::new(at(100), Some(at(151))),
        WorklogTimes::new(at(100), None),
    ] {
        assert!(matches!(
            repository.compare_and_delete_completed_worklog(target.id(), alpha.id, expected),
            Err(StorageError::WorklogChanged { id }) if id == target.id()
        ));
    }

    repository
        .connection()
        .execute(
            "UPDATE worklogs SET task_id = ?1 WHERE id = ?2",
            [beta.id.to_string(), target.id().to_string()],
        )
        .unwrap();
    assert!(matches!(
        repository.compare_and_delete_completed_worklog(
            target.id(),
            alpha.id,
            target.times(),
        ),
        Err(StorageError::WorklogChanged { id }) if id == target.id()
    ));
    assert!(repository.find_worklog(target.id()).unwrap().is_some());
}

#[test]
fn completed_worklogs_on_archived_tasks_can_be_deleted() {
    let repository = repo();
    let task = named_task(1, "archived");
    repository.create_task(task.clone()).unwrap();
    let target = Worklog::new(worklog_id(1), task.id, at(100), Some(at(150))).unwrap();
    repository.insert_worklog(&target).unwrap();
    repository.archive_task(task.id, at(200)).unwrap();

    let deletion = repository
        .compare_and_delete_completed_worklog(target.id(), task.id, target.times())
        .unwrap();

    assert_eq!(deletion.task_latest_work_start, None);
    assert!(
        repository
            .find_task(task.id)
            .unwrap()
            .unwrap()
            .is_archived()
    );
    assert!(repository.list_worklogs(task.id).unwrap().is_empty());
}

#[test]
fn completed_deletion_persists_after_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let task = named_task(1, "persistent");
    let target = Worklog::new(worklog_id(1), task.id, at(100), Some(at(150))).unwrap();
    {
        let repository = SqliteRepository::open(&path).unwrap();
        repository.create_task(task.clone()).unwrap();
        repository.insert_worklog(&target).unwrap();
        repository
            .compare_and_delete_completed_worklog(target.id(), task.id, target.times())
            .unwrap();
    }

    let reopened = SqliteRepository::open(&path).unwrap();
    assert_eq!(reopened.find_worklog(target.id()).unwrap(), None);
    assert!(reopened.list_worklogs(task.id).unwrap().is_empty());
}

#[test]
fn a_version_4_database_gains_the_active_delete_guard() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let task = named_task(1, "migrated");
    let completed = Worklog::new(worklog_id(1), task.id, at(50), Some(at(75))).unwrap();
    let active = Worklog::begin(worklog_id(2), task.id, at(100));
    {
        let repository = SqliteRepository::open(&path).unwrap();
        repository.create_task(task.clone()).unwrap();
        repository.insert_worklog(&completed).unwrap();
        repository.insert_worklog(&active).unwrap();
        repository
            .connection()
            .execute_batch(
                "DROP TRIGGER worklogs_reject_active_delete;
                 PRAGMA user_version = 4;",
            )
            .unwrap();
    }

    let migrated = SqliteRepository::open(&path).unwrap();
    assert_eq!(user_version(&migrated), 5);
    assert!(
        migrated
            .connection()
            .execute(
                "DELETE FROM worklogs WHERE id = ?1",
                [active.id().to_string()],
            )
            .is_err()
    );
    migrated
        .connection()
        .execute(
            "DELETE FROM worklogs WHERE id = ?1",
            [completed.id().to_string()],
        )
        .unwrap();
    assert_eq!(migrated.find_worklog(active.id()).unwrap(), Some(active));
}

#[test]
fn deletion_preserves_existing_history_cursors_and_revisions() {
    let repository = repo();
    let task = named_task(1, "history");
    repository.create_task(task.clone()).unwrap();
    insert_numbered_worklogs(&repository, &task, 55);
    let first = repository.worklog_page(task.id, None).unwrap();
    let cursor = first.next_cursor.unwrap();
    let target = repository.find_worklog(cursor.id).unwrap().unwrap();
    let revision = history_revision(&repository, task.id);

    repository
        .compare_and_delete_completed_worklog(target.id(), task.id, target.times())
        .unwrap();

    assert_eq!(history_revision(&repository, task.id), revision);
    let continuation = repository.worklog_page(task.id, Some(&cursor)).unwrap();
    assert_eq!(
        continuation
            .worklogs
            .iter()
            .map(Worklog::id)
            .collect::<Vec<_>>(),
        (1..=5).rev().map(worklog_id).collect::<Vec<_>>()
    );
}

#[test]
fn a_failed_post_delete_aggregate_read_rolls_back_the_deletion() {
    let repository = repo();
    let task = named_task(1, "rollback");
    repository.create_task(task.clone()).unwrap();
    let target = Worklog::new(worklog_id(1), task.id, at(100), Some(at(150))).unwrap();
    repository.insert_worklog(&target).unwrap();
    repository
        .connection()
        .execute(
            "INSERT INTO worklogs (id, task_id, start_us, end_us)
             VALUES (?1, ?2, ?3, ?3)",
            rusqlite::params![worklog_id(2).to_string(), task.id.to_string(), i64::MAX,],
        )
        .unwrap();

    assert!(matches!(
        repository.compare_and_delete_completed_worklog(target.id(), task.id, target.times()),
        Err(StorageError::CorruptData("timestamp"))
    ));
    let target_count: i64 = repository
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM worklogs WHERE id = ?1",
            [target.id().to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(target_count, 1, "the transaction restored the target row");
}

#[test]
fn a_failed_delete_commit_rolls_back_the_completed_worklog() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let setup = SqliteRepository::open(&path).unwrap();
    let journal_mode: String = setup
        .connection()
        .query_row("PRAGMA journal_mode = DELETE", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal_mode, "delete");
    let task = named_task(1, "rollback");
    let target = Worklog::new(worklog_id(1), task.id, at(100), Some(at(150))).unwrap();
    setup.create_task(task.clone()).unwrap();
    setup.insert_worklog(&target).unwrap();
    drop(setup);

    let deleting_repository = SqliteRepository::open(&path).unwrap();
    deleting_repository
        .connection()
        .busy_timeout(Duration::from_millis(100))
        .unwrap();
    let reading_repository = SqliteRepository::open(&path).unwrap();
    let (delete_reached_tx, delete_reached_rx) = sync_channel(0);
    let (delete_release_tx, delete_release_rx) = sync_channel(0);
    let (aggregate_completed_tx, aggregate_completed_rx) = sync_channel(1);
    let (result_tx, result_rx) = sync_channel(1);
    let delete_target = target.clone();
    let delete = thread::spawn(move || {
        let repository = deleting_repository;
        let write_pause = Box::new(WritePause {
            action: rusqlite::ffi::SQLITE_DELETE,
            paused: AtomicBool::new(false),
            reached: delete_reached_tx,
            release: Mutex::new(delete_release_rx),
        });
        assert_eq!(
            unsafe {
                rusqlite::ffi::sqlite3_set_authorizer(
                    repository.connection().handle(),
                    Some(pause_worklog_write),
                    (&*write_pause as *const WritePause).cast_mut().cast(),
                )
            },
            rusqlite::ffi::SQLITE_OK
        );
        let aggregate_observation = Box::new(AggregateObservation {
            completed: aggregate_completed_tx,
        });
        let aggregate_observation_ptr = (&*aggregate_observation as *const AggregateObservation)
            .cast_mut()
            .cast();
        assert_eq!(
            unsafe {
                rusqlite::ffi::sqlite3_trace_v2(
                    repository.connection().handle(),
                    rusqlite::ffi::SQLITE_TRACE_PROFILE,
                    Some(observe_aggregate),
                    aggregate_observation_ptr,
                )
            },
            rusqlite::ffi::SQLITE_OK
        );
        let result = repository.compare_and_delete_completed_worklog(
            delete_target.id(),
            delete_target.task_id(),
            delete_target.times(),
        );
        unsafe {
            rusqlite::ffi::sqlite3_set_authorizer(
                repository.connection().handle(),
                None,
                std::ptr::null_mut(),
            );
        }
        assert_eq!(
            unsafe {
                rusqlite::ffi::sqlite3_trace_v2(
                    repository.connection().handle(),
                    0,
                    None,
                    std::ptr::null_mut(),
                )
            },
            rusqlite::ffi::SQLITE_OK
        );
        result_tx.send(result).unwrap();
    });
    delete_reached_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap();

    reading_repository
        .connection()
        .execute_batch("BEGIN DEFERRED")
        .unwrap();
    assert_eq!(
        reading_repository.find_worklog(target.id()).unwrap(),
        Some(target.clone()),
        "the reader sees the pre-delete snapshot"
    );
    delete_release_tx.send(()).unwrap();
    aggregate_completed_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("the post-delete aggregate query must complete before commit");

    let error = result_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .expect_err("the held read lock must make commit fail");
    assert!(
        matches!(
            &error,
            StorageError::Sql(error)
                if error.sqlite_error_code() == Some(rusqlite::ErrorCode::DatabaseBusy)
        ),
        "expected a busy commit error, got {error:?}"
    );
    reading_repository
        .connection()
        .execute_batch("ROLLBACK")
        .unwrap();
    delete.join().unwrap();

    let reopened = SqliteRepository::open(&path).unwrap();
    assert_eq!(reopened.find_worklog(target.id()).unwrap(), Some(target));
}

#[test]
fn delete_wins_a_two_connection_race_against_correction() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let setup = SqliteRepository::open(&path).unwrap();
    let task = named_task(1, "shared");
    let target = Worklog::new(worklog_id(1), task.id, at(100), Some(at(150))).unwrap();
    setup.create_task(task.clone()).unwrap();
    setup.insert_worklog(&target).unwrap();
    drop(setup);

    let deleting_repository = SqliteRepository::open(&path).unwrap();
    let correcting_repository = SqliteRepository::open(&path).unwrap();
    let (reached_tx, reached_rx) = sync_channel(0);
    let (release_tx, release_rx) = sync_channel(0);
    let (delete_result_tx, delete_result_rx) = sync_channel(1);
    let delete_target = target.clone();
    let delete = thread::spawn(move || {
        let repository = deleting_repository;
        let pause = Box::new(WritePause {
            action: rusqlite::ffi::SQLITE_DELETE,
            paused: AtomicBool::new(false),
            reached: reached_tx,
            release: Mutex::new(release_rx),
        });
        assert_eq!(
            unsafe {
                rusqlite::ffi::sqlite3_set_authorizer(
                    repository.connection().handle(),
                    Some(pause_worklog_write),
                    (&*pause as *const WritePause).cast_mut().cast(),
                )
            },
            rusqlite::ffi::SQLITE_OK
        );
        let result = repository.compare_and_delete_completed_worklog(
            delete_target.id(),
            delete_target.task_id(),
            delete_target.times(),
        );
        unsafe {
            rusqlite::ffi::sqlite3_set_authorizer(
                repository.connection().handle(),
                None,
                std::ptr::null_mut(),
            );
        }
        delete_result_tx.send(result).unwrap();
    });
    reached_rx.recv_timeout(Duration::from_secs(5)).unwrap();

    let (contended_tx, contended_rx) = sync_channel(1);
    let (correction_result_tx, correction_result_rx) = sync_channel(1);
    let correction_target = target.clone();
    let correction = thread::spawn(move || {
        let repository = correcting_repository;
        let observation = Box::new(BusyObservation {
            observed: AtomicBool::new(false),
            reached: contended_tx,
        });
        let observation_ptr = (&*observation as *const BusyObservation).cast_mut().cast();
        assert_eq!(
            unsafe {
                rusqlite::ffi::sqlite3_busy_handler(
                    repository.connection().handle(),
                    Some(observe_busy),
                    observation_ptr,
                )
            },
            rusqlite::ffi::SQLITE_OK
        );
        let result = repository.compare_and_set_worklog_times(
            correction_target.id(),
            correction_target.times(),
            WorklogTimes::new(at(110), Some(at(160))),
        );
        assert_eq!(
            unsafe {
                rusqlite::ffi::sqlite3_busy_handler(
                    repository.connection().handle(),
                    None,
                    std::ptr::null_mut(),
                )
            },
            rusqlite::ffi::SQLITE_OK
        );
        correction_result_tx.send(result).unwrap();
    });
    contended_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("the correction must encounter the delete transaction's lock");
    assert!(
        matches!(
            correction_result_rx.recv_timeout(Duration::from_millis(100)),
            Err(RecvTimeoutError::Timeout)
        ),
        "the correction completed while the delete transaction was paused"
    );
    release_tx.send(()).unwrap();

    assert_eq!(
        delete_result_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap()
            .worklog,
        target
    );
    assert!(matches!(
        correction_result_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap(),
        Err(StorageError::WorklogNotFound { id }) if id == target.id()
    ));
    delete.join().unwrap();
    correction.join().unwrap();
}

#[test]
fn correction_wins_a_two_connection_race_against_delete() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let setup = SqliteRepository::open(&path).unwrap();
    let task = named_task(1, "shared");
    let target = Worklog::new(worklog_id(1), task.id, at(100), Some(at(150))).unwrap();
    setup.create_task(task.clone()).unwrap();
    setup.insert_worklog(&target).unwrap();
    drop(setup);

    let correcting_repository = SqliteRepository::open(&path).unwrap();
    let deleting_repository = SqliteRepository::open(&path).unwrap();
    let (reached_tx, reached_rx) = sync_channel(0);
    let (release_tx, release_rx) = sync_channel(0);
    let (correction_result_tx, correction_result_rx) = sync_channel(1);
    let correction_target = target.clone();
    let correction = thread::spawn(move || {
        let repository = correcting_repository;
        let pause = Box::new(WritePause {
            action: rusqlite::ffi::SQLITE_UPDATE,
            paused: AtomicBool::new(false),
            reached: reached_tx,
            release: Mutex::new(release_rx),
        });
        assert_eq!(
            unsafe {
                rusqlite::ffi::sqlite3_set_authorizer(
                    repository.connection().handle(),
                    Some(pause_worklog_write),
                    (&*pause as *const WritePause).cast_mut().cast(),
                )
            },
            rusqlite::ffi::SQLITE_OK
        );
        let result = repository.compare_and_set_worklog_times(
            correction_target.id(),
            correction_target.times(),
            WorklogTimes::new(at(110), Some(at(160))),
        );
        unsafe {
            rusqlite::ffi::sqlite3_set_authorizer(
                repository.connection().handle(),
                None,
                std::ptr::null_mut(),
            );
        }
        correction_result_tx.send(result).unwrap();
    });
    reached_rx.recv_timeout(Duration::from_secs(5)).unwrap();

    let (contended_tx, contended_rx) = sync_channel(1);
    let (delete_result_tx, delete_result_rx) = sync_channel(1);
    let delete_target = target.clone();
    let delete = thread::spawn(move || {
        let repository = deleting_repository;
        let observation = Box::new(BusyObservation {
            observed: AtomicBool::new(false),
            reached: contended_tx,
        });
        let observation_ptr = (&*observation as *const BusyObservation).cast_mut().cast();
        assert_eq!(
            unsafe {
                rusqlite::ffi::sqlite3_busy_handler(
                    repository.connection().handle(),
                    Some(observe_busy),
                    observation_ptr,
                )
            },
            rusqlite::ffi::SQLITE_OK
        );
        let result = repository.compare_and_delete_completed_worklog(
            delete_target.id(),
            delete_target.task_id(),
            delete_target.times(),
        );
        assert_eq!(
            unsafe {
                rusqlite::ffi::sqlite3_busy_handler(
                    repository.connection().handle(),
                    None,
                    std::ptr::null_mut(),
                )
            },
            rusqlite::ffi::SQLITE_OK
        );
        delete_result_tx.send(result).unwrap();
    });
    contended_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("the delete must encounter the correction transaction's lock");
    assert!(
        matches!(
            delete_result_rx.recv_timeout(Duration::from_millis(100)),
            Err(RecvTimeoutError::Timeout)
        ),
        "the delete completed while the correction transaction was paused"
    );
    release_tx.send(()).unwrap();

    let corrected = correction_result_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap()
        .worklog;
    assert_eq!(corrected.start(), at(110));
    assert!(matches!(
        delete_result_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap(),
        Err(StorageError::WorklogChanged { id }) if id == target.id()
    ));
    correction.join().unwrap();
    delete.join().unwrap();
    assert_eq!(
        SqliteRepository::open(&path)
            .unwrap()
            .find_worklog(target.id())
            .unwrap(),
        Some(corrected)
    );
}

#[test]
fn an_archived_task_rejects_worklogs_at_the_database_level() {
    let repository = repo();
    let task = named_task(1, "done");
    repository.create_task(task.clone()).unwrap();
    repository.archive_task(task.id, at(300)).unwrap();

    // A stopped worklog is also refused: archived tasks receive nothing.
    let stopped = Worklog::new(worklog_id(42), task.id, at(100), Some(at(150))).unwrap();
    let error = repository
        .insert_worklog(&stopped)
        .expect_err("archived tasks cannot receive worklogs");
    assert!(matches!(
        error,
        StorageError::TaskArchived { id } if id == task.id
    ));

    let active = Worklog::begin(worklog_id(43), task.id, at(200));
    let error = repository
        .insert_worklog(&active)
        .expect_err("archived tasks cannot become active");
    assert!(matches!(
        error,
        StorageError::TaskArchived { id } if id == task.id
    ));

    // Error state: nothing was written.
    assert!(repository.list_worklogs(task.id).unwrap().is_empty());
    assert_eq!(repository.active_worklog().unwrap(), None);
}

#[test]
fn unarchiving_restores_a_task_keeps_its_worklogs_and_persists() {
    let temp = tempfile::tempdir().unwrap();
    let task = stamped_task(1, "done", 100, 100);
    let stopped = Worklog::new(worklog_id(10), task.id, at(100), Some(at(150))).unwrap();

    {
        let repository = file_repo(&temp);
        repository.create_task(task.clone()).unwrap();
        repository.insert_worklog(&stopped).unwrap();
        repository.archive_task(task.id, at(150)).unwrap();
        assert!(
            repository
                .find_task(task.id)
                .unwrap()
                .unwrap()
                .is_archived()
        );

        // A restore through the field-specific port.
        let saved = TaskRepository::unarchive_task(&repository, task.id, at(200)).unwrap();
        assert!(!saved.is_archived());
        assert_eq!(saved.updated_at(), at(200));
        assert_eq!(saved.created_at(), at(100));
        assert_eq!(
            repository.list_worklogs(task.id).unwrap(),
            vec![stopped.clone()]
        );

        // The trigger no longer fires: the restored task accepts worklogs.
        let resumed = Worklog::begin(worklog_id(11), task.id, at(300));
        repository.insert_worklog(&resumed).unwrap();
        // Restoring again is a no-op for the timestamp.
        let current = TaskRepository::unarchive_task(&repository, task.id, at(400)).unwrap();
        assert_eq!(current.updated_at(), at(200));

        repository.create_task(named_task(2, "later")).unwrap();
    }

    let reopened = file_repo(&temp);
    let stored = reopened.find_task(task.id).unwrap().unwrap();
    assert!(!stored.is_archived());
    let worklogs = reopened.list_worklogs(task.id).unwrap();
    assert_eq!(worklogs.len(), 2);
    assert_eq!(worklogs[0], stopped);
    // The list order is untouched by the restore.
    let tasks = reopened.list_tasks().unwrap();
    let names: Vec<&str> = tasks.iter().map(|task| task.name().as_str()).collect();
    assert_eq!(names, ["done", "later"]);
}

#[test]
fn a_task_with_an_active_worklog_cannot_be_archived() {
    let repository = repo();
    let task = named_task(1, "running");
    repository.create_task(task.clone()).unwrap();
    let mut tracker = Tracker::idle();
    let started = tracker.start(&task, at(100)).unwrap();
    repository.insert_worklog(&started).unwrap();

    let error = repository
        .archive_task(task.id, at(300))
        .expect_err("the active task must not archive");
    assert!(matches!(
        error,
        StorageError::TaskIsActive { id } if id == task.id
    ));

    // Error state: the task is still usable and the timer still runs.
    assert!(
        !repository
            .find_task(task.id)
            .unwrap()
            .unwrap()
            .is_archived()
    );
    assert_eq!(repository.active_worklog().unwrap(), Some(started.clone()));

    // After the worklog is stopped, the same call archives the task.
    tracker.stop(at(150)).unwrap();
    repository
        .stop_worklog(started.id(), started.start(), at(150))
        .unwrap();
    let archived = repository.archive_task(task.id, at(300)).unwrap();
    assert!(archived.is_archived());
}

#[test]
fn concurrent_connections_enforce_the_archive_rules() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let process_a = SqliteRepository::open(&path).unwrap();
    let process_b = SqliteRepository::open(&path).unwrap();

    let task = named_task(1, "shared");
    process_a.create_task(task.clone()).unwrap();

    // The other process archives the task; this process's stale view still
    // believes the task is active, but the database refuses the worklog.
    process_b.archive_task(task.id, at(90)).unwrap();
    let stale = Worklog::begin(worklog_id(42), task.id, at(100));
    let error = process_a
        .insert_worklog(&stale)
        .expect_err("the archived task must refuse the worklog");
    assert!(matches!(
        error,
        StorageError::TaskArchived { id } if id == task.id
    ));

    // The reverse: this process tracks a second task, so the other process
    // cannot archive it.
    let other = named_task(2, "other");
    process_a.create_task(other.clone()).unwrap();
    let started = Worklog::begin(worklog_id(43), other.id, at(100));
    process_a.insert_worklog(&started).unwrap();
    let error = process_b
        .archive_task(other.id, at(120))
        .expect_err("the active task must not archive");
    assert!(matches!(
        error,
        StorageError::TaskIsActive { id } if id == other.id
    ));
    assert_eq!(
        process_a.active_worklog().unwrap().unwrap().id(),
        started.id()
    );
    assert!(
        !process_b
            .find_task(other.id)
            .unwrap()
            .unwrap()
            .is_archived()
    );
}

#[test]
fn same_task_toggle_stops_and_later_restarts() {
    let repository = repo();
    let task = named_task(1, "work");
    repository.create_task(task.clone()).unwrap();
    let mut tracker = Tracker::idle();

    let outcome = tracker.toggle(&task, at(100)).unwrap();
    let worklog = match outcome {
        TrackingOutcome::Started { worklog } => worklog,
        other => panic!("expected Started, got {other:?}"),
    };
    repository.insert_worklog(&worklog).unwrap();
    assert_eq!(
        repository.active_worklog().unwrap().unwrap().id(),
        worklog.id()
    );

    let outcome = tracker.toggle(&task, at(150)).unwrap();
    let stopped = match outcome {
        TrackingOutcome::Stopped { worklog } => worklog,
        other => panic!("expected Stopped, got {other:?}"),
    };
    repository
        .stop_worklog(stopped.id(), at(100), at(150))
        .unwrap();
    assert_eq!(repository.active_worklog().unwrap(), None);

    // Toggling again starts a fresh worklog, not a resume.
    let outcome = tracker.toggle(&task, at(200)).unwrap();
    let restarted = match outcome {
        TrackingOutcome::Started { worklog } => worklog,
        other => panic!("expected Started, got {other:?}"),
    };
    repository.insert_worklog(&restarted).unwrap();
    assert_ne!(restarted.id(), worklog.id());
    assert_eq!(
        repository.active_worklog().unwrap().unwrap().id(),
        restarted.id()
    );
    let worklogs = repository.list_worklogs(task_id(1)).unwrap();
    assert_eq!(worklogs.len(), 2);
    assert_eq!(worklogs[0].end(), Some(at(150)));
    assert_eq!(worklogs[1].start(), at(200));
    assert_eq!(worklogs[1].end(), None);
}

#[test]
fn restarts_record_separate_worklogs() {
    let repository = repo();
    let task = named_task(1, "work");
    repository.create_task(task.clone()).unwrap();
    let mut tracker = Tracker::idle();

    let first = tracker.start(&task, at(100)).unwrap();
    repository.insert_worklog(&first).unwrap();
    let first_stopped = tracker.stop(at(150)).unwrap();
    repository
        .stop_worklog(first.id(), first.start(), at(150))
        .unwrap();
    assert_eq!(first_stopped.id(), first.id());

    let second = tracker.start(&task, at(300)).unwrap();
    repository.insert_worklog(&second).unwrap();
    tracker.stop(at(400)).unwrap();
    repository
        .stop_worklog(second.id(), second.start(), at(400))
        .unwrap();

    let worklogs = repository.list_worklogs(task_id(1)).unwrap();
    assert_eq!(worklogs.len(), 2);
    assert_ne!(worklogs[0].id(), worklogs[1].id());
    assert_eq!(worklogs[0].start(), at(100));
    assert_eq!(worklogs[0].end(), Some(at(150)));
    assert_eq!(worklogs[1].start(), at(300));
    assert_eq!(worklogs[1].end(), Some(at(400)));
}

/// Inserts `worklogs` worklogs for the task, one per tag, started at
/// `at(tag)`. The last one is active and the others stop one second later.
fn insert_numbered_worklogs(repository: &SqliteRepository, task: &Task, worklogs: u32) {
    for tag in 1..=worklogs {
        let worklog = if tag == worklogs {
            Worklog::begin(worklog_id(tag), task.id, at(i64::from(tag)))
        } else {
            Worklog::new(
                worklog_id(tag),
                task.id,
                at(i64::from(tag)),
                Some(at(i64::from(tag) + 1)),
            )
            .unwrap()
        };
        repository.insert_worklog(&worklog).unwrap();
    }
}

#[test]
fn worklog_pages_walk_55_records_through_the_next_cursor_without_repeats_or_gaps() {
    let repository = repo();
    let task = named_task(1, "history");
    repository.create_task(task.clone()).unwrap();
    insert_numbered_worklogs(&repository, &task, 55);

    // The first page is bounded by the page size and ordered start
    // descending, newest first.
    let first = repository.worklog_page(task.id, None).unwrap();
    assert_eq!(first.worklogs.len(), WORKLOG_PAGE_SIZE);
    let starts: Vec<i64> = first
        .worklogs
        .iter()
        .map(|worklog| worklog.start().timestamp())
        .collect();
    assert_eq!(starts, (6..=55).rev().collect::<Vec<_>>());
    let cursor = first.next_cursor.expect("more history follows");
    assert_eq!(cursor.start, at(6));
    assert_eq!(cursor.id, worklog_id(6));

    // The second page carries the remaining five records, and the history
    // ends inside it.
    let second = repository.worklog_page(task.id, Some(&cursor)).unwrap();
    assert_eq!(second.worklogs.len(), 5);
    assert_eq!(second.next_cursor, None);
    let rest_starts: Vec<i64> = second
        .worklogs
        .iter()
        .map(|worklog| worklog.start().timestamp())
        .collect();
    assert_eq!(rest_starts, [5, 4, 3, 2, 1]);

    // The two pages cover every record exactly once: none skipped, none
    // repeated. The active worklog is part of the history.
    let mut ids: Vec<u32> = first
        .worklogs
        .iter()
        .chain(&second.worklogs)
        .map(|worklog| worklog.id().as_uuid().as_u128() as u32)
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, (1..=55).collect::<Vec<_>>());
    assert_eq!(
        first.worklogs[0].end(),
        None,
        "the newest worklog is active"
    );
    assert_eq!(
        second.worklogs.last().unwrap().end(),
        Some(at(2)),
        "the oldest worklog is completed"
    );
}

#[test]
fn a_history_of_exactly_one_page_has_no_continuation_cursor() {
    let repository = repo();
    let task = named_task(1, "full page");
    repository.create_task(task.clone()).unwrap();
    insert_numbered_worklogs(&repository, &task, WORKLOG_PAGE_SIZE as u32);

    let page = repository.worklog_page(task.id, None).unwrap();

    assert_eq!(page.worklogs.len(), WORKLOG_PAGE_SIZE);
    assert_eq!(page.next_cursor, None);
}

#[test]
fn a_page_boundary_inside_equal_starts_neither_dups_nor_skips_rows() {
    let repository = repo();
    let task = named_task(1, "simultaneous");
    repository.create_task(task.clone()).unwrap();
    // 55 zero-duration worklogs share one start, so identifier ascending
    // decides the whole order and the page boundary falls between two of
    // them. Zero-duration intervals do not overlap.
    for tag in 1..=55u32 {
        let worklog = Worklog::new(worklog_id(tag), task.id, at(500), Some(at(500))).unwrap();
        repository.insert_worklog(&worklog).unwrap();
    }

    let first = repository.worklog_page(task.id, None).unwrap();
    let ids: Vec<u32> = first
        .worklogs
        .iter()
        .map(|worklog| worklog.id().as_uuid().as_u128() as u32)
        .collect();
    assert_eq!(ids, (1..=50).collect::<Vec<_>>(), "identifier ascending");
    let cursor = first.next_cursor.expect("equal starts continue");
    assert_eq!(cursor.start, at(500));
    assert_eq!(cursor.id, worklog_id(50));

    let second = repository.worklog_page(task.id, Some(&cursor)).unwrap();
    let rest_ids: Vec<u32> = second
        .worklogs
        .iter()
        .map(|worklog| worklog.id().as_uuid().as_u128() as u32)
        .collect();
    assert_eq!(rest_ids, (51..=55).collect::<Vec<_>>());
    assert_eq!(second.next_cursor, None);
}

#[test]
fn an_active_worklog_is_part_of_a_history_page() {
    let repository = repo();
    let task = named_task(1, "running");
    let other = named_task(2, "other");
    repository.create_task(task.clone()).unwrap();
    repository.create_task(other.clone()).unwrap();
    let stopped = Worklog::new(worklog_id(1), task.id, at(100), Some(at(150))).unwrap();
    repository.insert_worklog(&stopped).unwrap();
    let active = Worklog::begin(worklog_id(2), task.id, at(200));
    repository.insert_worklog(&active).unwrap();
    let other_work = Worklog::new(worklog_id(3), other.id, at(300), Some(at(350))).unwrap();
    repository.insert_worklog(&other_work).unwrap();

    let page = repository.worklog_page(task.id, None).unwrap();
    assert_eq!(page.worklogs.len(), 2);
    // History order is start descending, so the active worklog leads.
    assert_eq!(page.worklogs[0].id(), active.id());
    assert_eq!(page.worklogs[0].end(), None);
    assert_eq!(page.worklogs[1].id(), stopped.id());
    assert_eq!(page.next_cursor, None);
    assert_eq!(page.snapshot.active_worklog, Some(active));
    assert_eq!(
        page.snapshot.requested_task_latest_work_start,
        Some(at(200))
    );
    assert_eq!(page.snapshot.active_task_latest_work_start, Some(at(200)));
    assert_eq!(
        repository.list_task_items().unwrap()[1].latest_work_start,
        Some(at(300)),
        "the full task aggregate remains a load and tracking-refresh read"
    );
}

#[test]
fn a_two_connection_continuation_adopts_a_switched_active_worklog_without_cursor_invalidation() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let first = SqliteRepository::open(&path).unwrap();
    let requested = named_task(1, "requested");
    let next_task = named_task(2, "next");
    first.create_task(requested.clone()).unwrap();
    first.create_task(next_task.clone()).unwrap();
    for tag in 1..=51 {
        let worklog = if tag == 51 {
            Worklog::begin(worklog_id(tag), requested.id, at(i64::from(tag) * 10))
        } else {
            Worklog::new(
                worklog_id(tag),
                requested.id,
                at(i64::from(tag) * 10),
                Some(at(i64::from(tag) * 10 + 1)),
            )
            .unwrap()
        };
        first.insert_worklog(&worklog).unwrap();
    }
    let first_page = first.worklog_page(requested.id, None).unwrap();
    let cursor = first_page
        .next_cursor
        .expect("the first page has one older row");
    let loaded_active = first_page
        .worklogs
        .iter()
        .find(|worklog| worklog.is_active())
        .cloned()
        .expect("the active worklog was loaded");

    let second = SqliteRepository::open(&path).unwrap();
    let replacement = Worklog::begin(worklog_id(99), next_task.id, at(520));
    second
        .switch_worklog(
            loaded_active.id(),
            loaded_active.start(),
            at(515),
            &replacement,
        )
        .unwrap();

    let continuation = first.worklog_page(requested.id, Some(&cursor)).unwrap();
    assert_eq!(continuation.worklogs.len(), 1);
    assert_eq!(continuation.snapshot.active_worklog, Some(replacement));
    assert_eq!(
        continuation.snapshot.active_task_latest_work_start,
        Some(at(520))
    );
    assert_eq!(
        continuation.snapshot.requested_task_latest_work_start,
        Some(loaded_active.start())
    );
}

#[test]
fn history_page_reads_use_indexed_bounded_rows_and_two_task_aggregates() {
    let repository = repo();
    let requested = named_task(1, "requested");
    let unrelated = named_task(2, "unrelated");
    repository.create_task(requested.clone()).unwrap();
    repository.create_task(unrelated.clone()).unwrap();
    for tag in 1..=WORKLOG_PAGE_SIZE as u32 + 1 {
        let start = i64::from(tag) * 10;
        repository
            .insert_worklog(
                &Worklog::new(
                    worklog_id(tag),
                    requested.id,
                    at(start),
                    Some(at(start + 1)),
                )
                .unwrap(),
            )
            .unwrap();
    }
    for tag in 1_000..=3_000 {
        let start = i64::from(tag) * 10;
        repository
            .insert_worklog(
                &Worklog::new(
                    worklog_id(tag),
                    unrelated.id,
                    at(start),
                    Some(at(start + 1)),
                )
                .unwrap(),
            )
            .unwrap();
    }
    let active = Worklog::begin(worklog_id(9_999), unrelated.id, at(50_000));
    repository.insert_worklog(&active).unwrap();

    let page = repository.worklog_page(requested.id, None).unwrap();
    assert_eq!(page.worklogs.len(), WORKLOG_PAGE_SIZE);
    assert_eq!(
        page.snapshot.requested_task_latest_work_start,
        Some(at(510))
    );
    assert_eq!(page.snapshot.active_worklog, Some(active));
    assert_eq!(
        page.snapshot.active_task_latest_work_start,
        Some(at(50_000))
    );
    let plan: Vec<String> = repository
        .connection()
        .prepare(
            "EXPLAIN QUERY PLAN SELECT id, task_id, start_us, end_us FROM worklogs
             WHERE task_id = ?1 ORDER BY start_us DESC, id LIMIT ?2",
        )
        .unwrap()
        .query_map(rusqlite::params![requested.id.to_string(), 51_i64], |row| {
            row.get(3)
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        plan.iter()
            .any(|detail| detail.contains("worklogs_task_start"))
    );
    let max_plan: Vec<String> = repository
        .connection()
        .prepare("EXPLAIN QUERY PLAN SELECT MAX(start_us) FROM worklogs WHERE task_id = ?1")
        .unwrap()
        .query_map([requested.id.to_string()], |row| row.get(3))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        max_plan
            .iter()
            .any(|detail| detail.contains("worklogs_task_start"))
    );
}

#[test]
fn a_cursor_stays_stable_when_a_newer_worklog_is_inserted_between_page_reads() {
    let repository = repo();
    let task = named_task(1, "history");
    repository.create_task(task.clone()).unwrap();
    insert_numbered_worklogs(&repository, &task, 55);

    let first = repository.worklog_page(task.id, None).unwrap();
    assert_eq!(first.worklogs.len(), WORKLOG_PAGE_SIZE);
    let cursor = first.next_cursor.expect("more history follows");

    // A newer worklog lands between the two page reads. The keyset cursor
    // still continues after the page's last row, so the second page neither
    // repeats a row of the first page nor skips one.
    let newer = Worklog::new(worklog_id(100), task.id, at(1000), Some(at(1000))).unwrap();
    repository.insert_worklog(&newer).unwrap();

    let second = repository.worklog_page(task.id, Some(&cursor)).unwrap();
    let rest_ids: Vec<u32> = second
        .worklogs
        .iter()
        .map(|worklog| worklog.id().as_uuid().as_u128() as u32)
        .collect();
    assert_eq!(rest_ids, (1..=5).rev().collect::<Vec<_>>());
    assert_eq!(second.next_cursor, None);

    // The newer worklog is not lost; a fresh first page leads with it.
    let fresh = repository.worklog_page(task.id, None).unwrap();
    assert_eq!(fresh.worklogs[0].id(), newer.id());
}

#[test]
fn switch_stops_the_old_worklog_and_starts_the_new_one_atomically() {
    let repository = repo();
    let work = named_task(1, "work");
    let rest = named_task(2, "rest");
    repository.create_task(work.clone()).unwrap();
    repository.create_task(rest.clone()).unwrap();
    let mut tracker = Tracker::idle();

    let started = tracker.start(&work, at(100)).unwrap();
    repository.insert_worklog(&started).unwrap();

    let switched = tracker.switch(&rest, at(150), at(160)).unwrap();
    repository
        .switch_worklog(
            switched.stopped.id(),
            switched.stopped.start(),
            at(150),
            &switched.started,
        )
        .unwrap();

    // Exactly one active worklog, belonging to the new task.
    let active = repository.active_worklog().unwrap().unwrap();
    assert_eq!(active.id(), switched.started.id());
    assert_eq!(active.task_id(), task_id(2));
    assert_eq!(active.start(), at(160));
    assert_eq!(active.end(), None);
    // The old worklog is stopped at the switch instant.
    let old = repository.list_worklogs(task_id(1)).unwrap();
    assert_eq!(old.len(), 1);
    assert_eq!(old[0].id(), started.id());
    assert_eq!(old[0].end(), Some(at(150)));
    // The new task has one active worklog.
    let new_worklogs = repository.list_worklogs(task_id(2)).unwrap();
    assert_eq!(new_worklogs.len(), 1);
    assert_eq!(new_worklogs[0].id(), switched.started.id());
}

#[test]
fn switch_rolls_back_on_a_backwards_stop_timestamp() {
    let repository = repo();
    let work = named_task(1, "work");
    let rest = named_task(2, "rest");
    repository.create_task(work.clone()).unwrap();
    repository.create_task(rest.clone()).unwrap();
    let mut tracker = Tracker::idle();

    let started = tracker.start(&work, at(100)).unwrap();
    repository.insert_worklog(&started).unwrap();

    // Stop before the active worklog's start violates the CHECK constraint.
    let next = Worklog::begin(worklog_id(42), rest.id, at(160));
    let error = repository
        .switch_worklog(started.id(), started.start(), at(99), &next)
        .expect_err("backwards stop must fail");
    assert!(matches!(error, StorageError::Constraint(_)));

    // Rollback: the old worklog is still active and untouched, no new worklog.
    let active = repository.active_worklog().unwrap().unwrap();
    assert_eq!(active.id(), started.id());
    assert_eq!(active.end(), None);
    assert_eq!(repository.list_worklogs(task_id(1)).unwrap().len(), 1);
    assert_eq!(repository.list_worklogs(task_id(2)).unwrap(), Vec::new());
}

#[test]
fn switch_rolls_back_when_the_target_task_is_missing() {
    let repository = repo();
    let work = named_task(1, "work");
    repository.create_task(work.clone()).unwrap();
    let mut tracker = Tracker::idle();

    let started = tracker.start(&work, at(100)).unwrap();
    repository.insert_worklog(&started).unwrap();

    let next = Worklog::begin(worklog_id(42), task_id(99), at(160));
    assert!(matches!(
        repository.switch_worklog(started.id(), started.start(), at(150), &next),
        Err(StorageError::TaskNotFound { id }) if id == task_id(99)
    ));

    // Rollback: the old worklog is still active, no new worklog was inserted.
    let active = repository.active_worklog().unwrap().unwrap();
    assert_eq!(active.id(), started.id());
    assert_eq!(active.end(), None);
    assert_eq!(repository.list_worklogs(task_id(1)).unwrap().len(), 1);
    assert_eq!(repository.list_worklogs(task_id(99)).unwrap(), Vec::new());
}

#[test]
fn switch_reports_a_missing_worklog_and_changes_nothing() {
    let repository = repo();
    let work = named_task(1, "work");
    let rest = named_task(2, "rest");
    repository.create_task(work.clone()).unwrap();
    repository.create_task(rest.clone()).unwrap();
    let mut tracker = Tracker::idle();

    let started = tracker.start(&work, at(100)).unwrap();
    repository.insert_worklog(&started).unwrap();

    let missing = worklog_id(77);
    let next = Worklog::begin(worklog_id(42), rest.id, at(160));
    assert!(matches!(
        repository.switch_worklog(missing, at(100), at(150), &next),
        Err(StorageError::WorklogNotFound { id }) if id == missing
    ));
    assert_eq!(repository.active_worklog().unwrap(), Some(started.clone()));
    assert_eq!(repository.list_worklogs(task_id(1)).unwrap().len(), 1);
    assert_eq!(repository.list_worklogs(task_id(2)).unwrap(), Vec::new());
}

#[test]
fn switch_rejects_an_already_stopped_worklog() {
    let repository = repo();
    let work = named_task(1, "work");
    let rest = named_task(2, "rest");
    repository.create_task(work.clone()).unwrap();
    repository.create_task(rest.clone()).unwrap();
    let mut tracker = Tracker::idle();

    let started = tracker.start(&work, at(100)).unwrap();
    repository.insert_worklog(&started).unwrap();
    repository
        .stop_worklog(started.id(), started.start(), at(150))
        .unwrap();

    let next = Worklog::begin(worklog_id(42), rest.id, at(160));
    assert!(matches!(
        repository.switch_worklog(started.id(), started.start(), at(150), &next),
        Err(StorageError::WorklogAlreadyStopped { id }) if id == started.id()
    ));
    assert_eq!(repository.active_worklog().unwrap(), None);
    assert_eq!(repository.list_worklogs(task_id(2)).unwrap(), Vec::new());
}

#[test]
fn switch_into_an_archived_task_is_rejected_and_rolls_back() {
    let repository = repo();
    let work = named_task(1, "work");
    let rest = named_task(2, "rest");
    repository.create_task(work.clone()).unwrap();
    repository.create_task(rest.clone()).unwrap();
    let mut tracker = Tracker::idle();

    let started = tracker.start(&work, at(100)).unwrap();
    repository.insert_worklog(&started).unwrap();
    // The target is archived after the caller looked at it, which the
    // tracker's in-memory copy cannot see.
    repository.archive_task(rest.id, at(150)).unwrap();

    let next = Worklog::begin(worklog_id(42), rest.id, at(160));
    let error = repository
        .switch_worklog(started.id(), started.start(), at(150), &next)
        .expect_err("the archived target must refuse the switch");
    assert!(matches!(
        error,
        StorageError::TaskArchived { id } if id == rest.id
    ));

    // Rollback: the old worklog is still active, the archived task received
    // nothing.
    assert_eq!(
        repository.active_worklog().unwrap().unwrap().id(),
        started.id()
    );
    assert_eq!(repository.list_worklogs(rest.id).unwrap(), Vec::new());
    let old = repository.list_worklogs(work.id).unwrap();
    assert_eq!(old.len(), 1);
    assert_eq!(old[0].end(), None);
}

#[test]
fn a_switch_with_an_existing_new_worklog_id_rolls_back_and_reports_it() {
    let repository = repo();
    let work = named_task(1, "work");
    let rest = named_task(2, "rest");
    repository.create_task(work.clone()).unwrap();
    repository.create_task(rest.clone()).unwrap();
    let mut tracker = Tracker::idle();

    let started = tracker.start(&work, at(100)).unwrap();
    repository.insert_worklog(&started).unwrap();
    let old_rest = Worklog::new(worklog_id(42), rest.id, at(10), Some(at(20))).unwrap();
    repository.insert_worklog(&old_rest).unwrap();

    // The new worklog reuses the stored id of the old rest worklog.
    let next = Worklog::begin(worklog_id(42), rest.id, at(160));
    let error = repository
        .switch_worklog(started.id(), started.start(), at(150), &next)
        .expect_err("the duplicate id must fail the switch");
    assert!(matches!(
        error,
        StorageError::WorklogAlreadyExists { id } if id == worklog_id(42)
    ));

    // Rollback: the stop half is undone, so work is still active.
    assert_eq!(
        repository.active_worklog().unwrap().unwrap().id(),
        started.id()
    );
    let rest_worklogs = repository.list_worklogs(rest.id).unwrap();
    assert_eq!(rest_worklogs.len(), 1);
    assert_eq!(rest_worklogs[0].end(), Some(at(20)));
}

#[test]
fn archiving_the_active_task_is_rejected() {
    let repository = repo();
    let work = named_task(1, "work");
    let other = named_task(2, "other");
    repository.create_task(work.clone()).unwrap();
    repository.create_task(other.clone()).unwrap();
    let mut tracker = Tracker::idle();

    let started = tracker.start(&work, at(100)).unwrap();
    repository.insert_worklog(&started).unwrap();

    assert_eq!(
        tracker.ensure_archivable(work.id),
        Err(TrackingError::TaskIsActive { id: work.id })
    );
    // The task stays usable and unarchived after the rejection.
    assert!(
        !repository
            .find_task(work.id)
            .unwrap()
            .unwrap()
            .is_archived()
    );
    assert_eq!(repository.active_worklog().unwrap(), Some(started.clone()));

    // A different task archives fine while tracking runs.
    tracker.ensure_archivable(other.id).unwrap();
    repository.archive_task(other.id, at(120)).unwrap();
    assert!(
        repository
            .find_task(other.id)
            .unwrap()
            .unwrap()
            .is_archived()
    );

    // Once the worklog is stopped, the task can be archived.
    tracker.stop(at(150)).unwrap();
    repository
        .stop_worklog(started.id(), started.start(), at(150))
        .unwrap();
    tracker.ensure_archivable(work.id).unwrap();
    repository.archive_task(work.id, at(160)).unwrap();
    assert!(
        repository
            .find_task(work.id)
            .unwrap()
            .unwrap()
            .is_archived()
    );
}

#[test]
fn archived_tasks_cannot_start_or_become_active() {
    let repository = repo();
    let mut tracker = Tracker::idle();
    let mut archived = named_task(1, "old");
    assert!(archived.archive(at(100)));
    repository.create_task(archived.clone()).unwrap();

    assert_eq!(
        tracker.start(&archived, at(100)),
        Err(TrackingError::TaskArchived { id: archived.id })
    );
    assert_eq!(tracker.state(), &TrackingState::Idle);
    assert_eq!(repository.active_worklog().unwrap(), None);
}

#[test]
fn active_worklog_survives_closing_and_reopening_the_database() {
    let temp = tempfile::tempdir().unwrap();
    let task = named_task(1, "long running");
    let worklog_id;

    {
        let repository = file_repo(&temp);
        repository.create_task(task.clone()).unwrap();
        let mut tracker = Tracker::idle();
        let started = tracker.start(&task, at(100)).unwrap();
        worklog_id = started.id();
        // Deliberately no stop: exiting must not implicitly stop tracking.
        repository.insert_worklog(&started).unwrap();
    }

    {
        let repository = SqliteRepository::open(temp.path().join("tracker.db")).unwrap();
        let recovered = repository
            .active_worklog()
            .unwrap()
            .expect("worklog recovered");
        assert_eq!(recovered.id(), worklog_id);
        assert_eq!(recovered.task_id(), task.id);
        assert_eq!(recovered.start(), at(100));
        assert_eq!(recovered.end(), None);

        // The recovered worklog resumes tracking and stops cleanly.
        let mut tracker = Tracker::resume(recovered).unwrap();
        assert_eq!(
            tracker.state(),
            &TrackingState::Running {
                worklog: ActiveWorklog::begin(worklog_id, task.id, at(100))
            }
        );
        let stopped = tracker.stop(at(250)).unwrap();
        repository
            .stop_worklog(stopped.id(), at(100), at(250))
            .unwrap();
    }

    // A third open sees a stopped worklog and no active one.
    let repository = SqliteRepository::open(temp.path().join("tracker.db")).unwrap();
    assert_eq!(repository.active_worklog().unwrap(), None);
    let worklogs = repository.list_worklogs(task.id).unwrap();
    assert_eq!(worklogs.len(), 1);
    assert_eq!(worklogs[0].start(), at(100));
    assert_eq!(worklogs[0].end(), Some(at(250)));
}

#[test]
fn corrupt_task_rows_are_reported_as_corrupt_data() {
    let repository = repo();
    let conn = repository.connection();
    conn.execute(
        "INSERT INTO tasks (id, name, archived, created_at_us, updated_at_us)
         VALUES ('not-a-uuid', 'name', 0, 0, 0)",
        [],
    )
    .unwrap();
    let error = repository.list_tasks().expect_err("bad id is corrupt");
    assert!(matches!(error, StorageError::InvalidId(_)));

    let repository = repo();
    let conn = repository.connection();
    conn.execute(
        "INSERT INTO tasks (id, name, archived, created_at_us, updated_at_us)
         VALUES ('00000000-0000-7000-8000-000000000001', '   ', 0, 0, 0)",
        [],
    )
    .unwrap();
    let error = repository.list_tasks().expect_err("empty name is corrupt");
    assert!(matches!(error, StorageError::CorruptData("task name")));

    // An unrepresentable timestamp cannot be rehydrated.
    let repository = repo();
    let conn = repository.connection();
    conn.execute(
        "INSERT INTO tasks (id, name, archived, created_at_us, updated_at_us)
         VALUES ('00000000-0000-7000-8000-000000000001', 'huge', 0, 9223372036854775807, 9223372036854775807)",
        [],
    )
    .unwrap();
    let error = repository
        .list_tasks()
        .expect_err("huge timestamp is corrupt");
    assert!(matches!(error, StorageError::CorruptData("timestamp")));
}

#[test]
fn corrupt_timestamp_pairs_are_reported_as_corrupt_data() {
    // An updated_at before created_at can only enter the database with
    // CHECKs disabled, which simulates tampering or a broken writer.
    let repository = repo();
    let conn = repository.connection();
    conn.execute_batch("PRAGMA ignore_check_constraints = ON;")
        .unwrap();
    conn.execute(
        "INSERT INTO tasks (id, name, archived, created_at_us, updated_at_us)
         VALUES (?1, 'backwards', 0, 200, 199)",
        [task_id(1).to_string()],
    )
    .unwrap();
    conn.execute_batch("PRAGMA ignore_check_constraints = OFF;")
        .unwrap();
    let error = repository
        .list_tasks()
        .expect_err("updated before created is corrupt");
    assert!(matches!(
        error,
        StorageError::CorruptData("task timestamps")
    ));
    // The read model rejects the same corruption, including its aggregate.
    let error = repository
        .list_task_items()
        .expect_err("the read model rejects the row too");
    assert!(matches!(
        error,
        StorageError::CorruptData("task timestamps")
    ));
    assert!(matches!(
        repository.find_task(task_id(1)),
        Err(StorageError::CorruptData("task timestamps"))
    ));
}

#[test]
fn corrupt_worklog_rows_are_reported_as_corrupt_data() {
    // An out-of-range timestamp cannot be represented in the domain.
    let repository = repo();
    let conn = repository.connection();
    conn.execute(
        "INSERT INTO tasks (id, name, archived, created_at_us, updated_at_us) VALUES ('00000000-0000-0000-0000-000000000001', 'work', 0, 0, 0)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO worklogs (id, task_id, start_us, end_us)
         VALUES ('00000000-0000-7000-8000-000000000010',
                 '00000000-0000-0000-0000-000000000001', 9223372036854775807, NULL)",
        [],
    )
    .unwrap();
    let error = repository
        .list_worklogs(task_id(1))
        .expect_err("huge timestamp is corrupt");
    assert!(matches!(error, StorageError::CorruptData("timestamp")));
    let error = repository
        .worklog_page(task_id(1), None)
        .expect_err("the paged query rejects the huge timestamp too");
    assert!(matches!(error, StorageError::CorruptData("timestamp")));
    // The aggregate rejects the corrupt start as well.
    let error = repository
        .list_task_items()
        .expect_err("the aggregate rejects the corrupt start");
    assert!(matches!(error, StorageError::CorruptData("timestamp")));

    // An end before start can only enter the database with CHECKs disabled,
    // which simulates tampering or a broken writer.
    let repository = repo();
    let conn = repository.connection();
    conn.execute(
        "INSERT INTO tasks (id, name, archived, created_at_us, updated_at_us) VALUES ('00000000-0000-0000-0000-000000000001', 'work', 0, 0, 0)",
        [],
    )
    .unwrap();
    conn.execute_batch(
        "PRAGMA ignore_check_constraints = ON;
         INSERT INTO worklogs (id, task_id, start_us, end_us)
         VALUES ('00000000-0000-7000-8000-000000000010',
                 '00000000-0000-0000-0000-000000000001', 200, 100);
         PRAGMA ignore_check_constraints = OFF;",
    )
    .unwrap();
    let error = repository
        .list_worklogs(task_id(1))
        .expect_err("backwards interval is corrupt");
    assert!(matches!(
        error,
        StorageError::CorruptData("worklog interval")
    ));
    let error = repository
        .worklog_page(task_id(1), None)
        .expect_err("the paged query rejects the backwards interval too");
    assert!(matches!(
        error,
        StorageError::CorruptData("worklog interval")
    ));

    let mut application = TrackerApplication::load(repository).unwrap();
    assert!(matches!(
        application.worklogs_for_task(task_id(1), None),
        Err(ApplicationError::Repository(RepositoryError::CorruptData {
            field: "worklog interval"
        }))
    ));
}

#[test]
fn open_fails_for_an_unusable_path() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp
        .path()
        .join("no")
        .join("such")
        .join("dir")
        .join("db.sqlite");
    let error = SqliteRepository::open(missing).expect_err("missing parent must fail");
    assert!(matches!(error, StorageError::Io(_)));
}

#[test]
fn foreign_keys_are_enforced_per_connection() {
    let repository = repo();
    let conn = repository.connection();
    let value: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .unwrap();
    assert_eq!(value, 1);
}

#[test]
fn the_application_orders_the_sqlite_task_list_per_adr_0002() {
    let repository = repo();
    // gamma worked most recently, then beta; alpha and delta never worked.
    // delta is archived, so the active view hides it.
    let alpha = stamped_task(1, "alpha", 100, 100);
    let beta = stamped_task(2, "beta", 200, 200);
    let gamma = stamped_task(3, "gamma", 300, 300);
    let mut delta = stamped_task(4, "delta", 50, 900);
    assert!(delta.archive(at(900)));
    for task in [&alpha, &beta, &gamma, &delta] {
        repository.create_task(task.clone()).unwrap();
    }
    repository
        .insert_worklog(&Worklog::new(worklog_id(1), gamma.id, at(400), Some(at(450))).unwrap())
        .unwrap();
    repository
        .insert_worklog(&Worklog::new(worklog_id(2), beta.id, at(500), Some(at(550))).unwrap())
        .unwrap();
    repository
        .insert_worklog(&Worklog::new(worklog_id(3), gamma.id, at(600), Some(at(650))).unwrap())
        .unwrap();

    let application = TrackerApplication::load(repository).unwrap();
    let names = |ordering| {
        application
            .tasks(ordering)
            .iter()
            .map(|item| item.task.name().to_string())
            .collect::<Vec<_>>()
    };
    // Recently worked: gamma, beta, then the never-worked tasks newest
    // first; alpha, created later than delta, precedes it.
    assert_eq!(
        names(TaskOrdering::RecentlyWorked),
        [
            "gamma".to_owned(),
            "beta".to_owned(),
            "alpha".to_owned(),
            "delta".to_owned()
        ]
    );
    assert_eq!(
        names(TaskOrdering::RecentlyUpdated),
        [
            "delta".to_owned(),
            "gamma".to_owned(),
            "beta".to_owned(),
            "alpha".to_owned()
        ]
    );
    assert_eq!(
        names(TaskOrdering::RecentlyCreated),
        [
            "gamma".to_owned(),
            "beta".to_owned(),
            "alpha".to_owned(),
            "delta".to_owned()
        ]
    );
}

#[test]
fn concurrent_corrections_use_two_connections_and_one_stale_loser() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let setup = SqliteRepository::open(&path).unwrap();
    let task = named_task(1, "shared");
    let original = Worklog::new(worklog_id(1), task.id, at(100), Some(at(200))).unwrap();
    setup.create_task(task.clone()).unwrap();
    setup.insert_worklog(&original).unwrap();
    drop(setup);

    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = [at(110), at(120)]
        .into_iter()
        .map(|start| {
            let barrier = barrier.clone();
            let path = path.clone();
            let original = original.clone();
            thread::spawn(move || {
                let repository = SqliteRepository::open(path).unwrap();
                barrier.wait();
                repository.compare_and_set_worklog_times(
                    original.id(),
                    original.times(),
                    WorklogTimes::new(start, Some(start + chrono::TimeDelta::seconds(100))),
                )
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(StorageError::WorklogChanged { .. })))
            .count(),
        1
    );
    let stored = SqliteRepository::open(&path)
        .unwrap()
        .find_worklog(original.id())
        .unwrap()
        .unwrap();
    let winner = results.into_iter().find_map(Result::ok).unwrap().worklog;
    assert_eq!(stored, winner);
}

#[test]
fn direct_task_move_cannot_create_a_same_task_overlap() {
    let repository = repo();
    let first = named_task(1, "first");
    let second = named_task(2, "second");
    repository.create_task(first.clone()).unwrap();
    repository.create_task(second.clone()).unwrap();
    let left = Worklog::new(worklog_id(1), first.id, at(100), Some(at(200))).unwrap();
    let right = Worklog::new(worklog_id(2), second.id, at(150), Some(at(250))).unwrap();
    repository.insert_worklog(&left).unwrap();
    repository.insert_worklog(&right).unwrap();

    let error = repository
        .connection()
        .execute(
            "UPDATE worklogs SET task_id = ?1 WHERE id = ?2",
            [first.id.to_string(), right.id().to_string()],
        )
        .expect_err("moving an interval onto an overlap must fail");
    assert!(matches!(
        error,
        rusqlite::Error::SqliteFailure(_, Some(message))
            if message == "same-task worklog overlap"
    ));
    assert_eq!(repository.find_worklog(right.id()).unwrap(), Some(right));
}

#[test]
fn start_corrections_invalidate_continuations_but_inserts_and_end_corrections_do_not() {
    let repository = repo();
    let task = named_task(1, "history");
    repository.create_task(task.clone()).unwrap();
    for tag in 1..=51 {
        let start = i64::from(tag) * 10;
        repository
            .insert_worklog(
                &Worklog::new(worklog_id(tag), task.id, at(start), Some(at(start + 1))).unwrap(),
            )
            .unwrap();
    }

    let first = repository.worklog_page(task.id, None).unwrap();
    let cursor = first.next_cursor.unwrap();
    let unloaded = repository.find_worklog(worklog_id(1)).unwrap().unwrap();
    repository
        .compare_and_set_worklog_times(
            unloaded.id(),
            unloaded.times(),
            WorklogTimes::new(at(1_000), Some(at(1_001))),
        )
        .unwrap();
    assert!(matches!(
        repository.worklog_page(task.id, Some(&cursor)),
        Err(StorageError::WorklogHistoryChanged { task_id }) if task_id == task.id
    ));

    let refreshed = repository.worklog_page(task.id, None).unwrap();
    let cursor = refreshed.next_cursor.unwrap();
    let loaded = refreshed.worklogs[0].clone();
    repository
        .compare_and_set_worklog_times(
            loaded.id(),
            loaded.times(),
            WorklogTimes::new(at(-10), Some(at(-9))),
        )
        .unwrap();
    assert!(matches!(
        repository.worklog_page(task.id, Some(&cursor)),
        Err(StorageError::WorklogHistoryChanged { task_id }) if task_id == task.id
    ));

    let refreshed = repository.worklog_page(task.id, None).unwrap();
    let cursor = refreshed.next_cursor.unwrap();
    let unchanged_order = repository.find_worklog(worklog_id(2)).unwrap().unwrap();
    repository
        .compare_and_set_worklog_times(
            unchanged_order.id(),
            unchanged_order.times(),
            WorklogTimes::new(unchanged_order.start(), Some(at(21))),
        )
        .unwrap();
    assert!(repository.worklog_page(task.id, Some(&cursor)).is_ok());

    repository
        .insert_worklog(&Worklog::new(worklog_id(99), task.id, at(2_000), Some(at(2_001))).unwrap())
        .unwrap();
    assert!(repository.worklog_page(task.id, Some(&cursor)).is_ok());
}

#[test]
fn direct_task_moves_invalidate_both_history_cursors_and_start_changes_bump_once() {
    let repository = repo();
    let alpha = named_task(1, "alpha");
    let beta = named_task(2, "beta");
    repository.create_task(alpha.clone()).unwrap();
    repository.create_task(beta.clone()).unwrap();
    for tag in 1..=51 {
        repository
            .insert_worklog(
                &Worklog::new(
                    worklog_id(tag),
                    alpha.id,
                    at(i64::from(tag) * 10),
                    Some(at(i64::from(tag) * 10)),
                )
                .unwrap(),
            )
            .unwrap();
        repository
            .insert_worklog(
                &Worklog::new(
                    worklog_id(tag + 100),
                    beta.id,
                    at(10_000 + i64::from(tag) * 10),
                    Some(at(10_000 + i64::from(tag) * 10)),
                )
                .unwrap(),
            )
            .unwrap();
    }

    let alpha_cursor = repository
        .worklog_page(alpha.id, None)
        .unwrap()
        .next_cursor
        .unwrap();
    let beta_cursor = repository
        .worklog_page(beta.id, None)
        .unwrap()
        .next_cursor
        .unwrap();
    let moved = worklog_id(1);
    repository
        .connection()
        .execute(
            "UPDATE worklogs SET task_id = ?1 WHERE id = ?2",
            [beta.id.to_string(), moved.to_string()],
        )
        .unwrap();

    assert_eq!(history_revision(&repository, alpha.id), 1);
    assert_eq!(history_revision(&repository, beta.id), 1);
    for (task_id, cursor) in [(alpha.id, alpha_cursor), (beta.id, beta_cursor)] {
        assert!(matches!(
            repository.worklog_page(task_id, Some(&cursor)),
            Err(StorageError::WorklogHistoryChanged { task_id: changed }) if changed == task_id
        ));
    }

    let beta_cursor = repository
        .worklog_page(beta.id, None)
        .unwrap()
        .next_cursor
        .unwrap();
    repository
        .connection()
        .execute(
            "UPDATE worklogs SET start_us = ?1 WHERE id = ?2",
            rusqlite::params![20_000_i64, moved.as_uuid().to_string()],
        )
        .unwrap();
    assert_eq!(history_revision(&repository, alpha.id), 1);
    assert_eq!(history_revision(&repository, beta.id), 2);
    assert!(matches!(
        repository.worklog_page(beta.id, Some(&beta_cursor)),
        Err(StorageError::WorklogHistoryChanged { task_id }) if task_id == beta.id
    ));
}

#[test]
fn overlap_migration_uses_an_ordered_indexed_sweep_for_large_valid_history() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let rows: Vec<_> = (1..=2_000)
        .map(|tag| (tag, i64::from(tag) * 10, Some(i64::from(tag) * 10 + 1)))
        .collect();
    create_v2_database(&path, &rows);

    let repository = SqliteRepository::open(&path).unwrap();
    assert_eq!(user_version(&repository), 5);
    let plan: Vec<String> = repository
        .connection()
        .prepare("EXPLAIN QUERY PLAN SELECT id FROM worklogs WHERE task_id = ?1 ORDER BY start_us DESC, id")
        .unwrap()
        .query_map([task_id(1).to_string()], |row| row.get(3))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        plan.iter()
            .any(|detail| detail.contains("worklogs_task_start"))
    );
}

#[test]
fn version_3_database_gets_indexed_overlap_and_active_delete_guards() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    create_v2_database(&path, &[(1, 100, None)]);
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO tasks (id, name, archived, created_at_us, updated_at_us)
             VALUES (?1, 'archived', 0, 100, 100)",
            [task_id(2).to_string()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO worklogs (id, task_id, start_us, end_us) VALUES (?1, ?2, 10, 20)",
            [worklog_id(2).to_string(), task_id(2).to_string()],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE tasks SET archived = 1 WHERE id = ?1",
            [task_id(2).to_string()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO tasks (id, name, archived, created_at_us, updated_at_us)
             VALUES (?1, 'open', 0, 100, 100)",
            [task_id(3).to_string()],
        )
        .unwrap();
    connection
        .execute_batch(
            "ALTER TABLE tasks ADD COLUMN history_order_revision INTEGER NOT NULL DEFAULT 0;
             CREATE TRIGGER worklogs_reject_same_task_overlap_insert
             BEFORE INSERT ON worklogs BEGIN SELECT 1; END;
             CREATE TRIGGER worklogs_reject_same_task_overlap_update
             BEFORE UPDATE OF task_id, start_us, end_us ON worklogs BEGIN SELECT 1; END;
             CREATE TRIGGER worklogs_bump_history_order_revision
             AFTER UPDATE OF start_us ON worklogs BEGIN SELECT 1; END;",
        )
        .unwrap();
    connection.pragma_update(None, "user_version", 3).unwrap();
    drop(connection);

    let repository = SqliteRepository::open(&path).unwrap();
    assert_eq!(user_version(&repository), 5);
    let active_id = worklog_id(1);
    let error = repository
        .connection()
        .execute(
            "UPDATE worklogs SET task_id = ?1 WHERE id = ?2",
            [task_id(2).to_string(), active_id.to_string()],
        )
        .expect_err("an active worklog cannot move to an archived task");
    assert!(matches!(
        error,
        rusqlite::Error::SqliteFailure(_, Some(message)) if message == "task is archived"
    ));

    // History imported before a task was archived remains editable.
    repository
        .connection()
        .execute(
            "UPDATE worklogs SET start_us = 11, end_us = 21 WHERE id = ?1",
            [worklog_id(2).to_string()],
        )
        .unwrap();

    repository
        .connection()
        .execute(
            "UPDATE worklogs SET task_id = ?1 WHERE id = ?2",
            [task_id(3).to_string(), worklog_id(2).to_string()],
        )
        .unwrap();
    assert_eq!(history_revision(&repository, task_id(2)), 2);
    assert_eq!(history_revision(&repository, task_id(3)), 1);
    repository
        .connection()
        .execute(
            "UPDATE worklogs SET start_us = 12 WHERE id = ?1",
            [worklog_id(2).to_string()],
        )
        .unwrap();
    assert_eq!(history_revision(&repository, task_id(2)), 2);
    assert_eq!(history_revision(&repository, task_id(3)), 2);
}

#[test]
fn a_cursor_for_another_task_is_rejected_before_paging() {
    let repository = repo();
    let alpha = named_task(1, "alpha");
    let beta = named_task(2, "beta");
    repository.create_task(alpha.clone()).unwrap();
    repository.create_task(beta.clone()).unwrap();
    for (id, task, start) in [(1, alpha.id, 100), (2, beta.id, 200)] {
        repository
            .insert_worklog(
                &Worklog::new(worklog_id(id), task, at(start), Some(at(start + 1))).unwrap(),
            )
            .unwrap();
    }
    let cursor = repository.worklog_page(alpha.id, None).unwrap().next_cursor;
    let cursor = cursor.unwrap_or(WorklogCursor {
        task_id: alpha.id,
        start: at(100),
        id: worklog_id(1),
        revision: 0,
    });
    assert!(matches!(
        repository.worklog_page(beta.id, Some(&cursor)),
        Err(StorageError::WorklogHistoryChanged { task_id }) if task_id == beta.id
    ));
}

#[test]
fn predecessor_lookup_uses_the_partial_index_for_large_history_writes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let rows: Vec<_> = (1..=2_000)
        .map(|tag| (tag, i64::from(tag) * 10, Some(i64::from(tag) * 10 + 1)))
        .collect();
    create_v2_database(&path, &rows);
    let repository = SqliteRepository::open(&path).unwrap();
    let plan: Vec<String> = repository
        .connection()
        .prepare(
            "EXPLAIN QUERY PLAN SELECT end_us FROM worklogs
             WHERE task_id = ?1
               AND (end_us IS NULL OR end_us > start_us)
               AND start_us < ?2
             ORDER BY start_us DESC LIMIT 1",
        )
        .unwrap()
        .query_map(
            rusqlite::params![task_id(1).to_string(), 30_000_i64],
            |row| row.get(3),
        )
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(plan.iter().any(|detail| {
        detail.contains("worklogs_nonzero_task_start") && detail.contains("task_id=?")
    }));
    repository
        .insert_worklog(
            &Worklog::new(worklog_id(3_000), task_id(1), at(20_001), Some(at(20_002))).unwrap(),
        )
        .unwrap();
}

#[test]
fn stop_and_switch_report_an_active_start_compare_failure() {
    let repository = repo();
    let alpha = named_task(1, "alpha");
    let beta = named_task(2, "beta");
    repository.create_task(alpha.clone()).unwrap();
    repository.create_task(beta.clone()).unwrap();
    let active = Worklog::begin(worklog_id(1), alpha.id, at(100));
    repository.insert_worklog(&active).unwrap();
    assert!(matches!(
        repository.stop_worklog(active.id(), at(50), at(150)),
        Err(StorageError::WorklogChanged { id }) if id == active.id()
    ));
    let next = Worklog::begin(worklog_id(2), beta.id, at(150));
    assert!(matches!(
        repository.switch_worklog(active.id(), at(50), at(150), &next),
        Err(StorageError::WorklogChanged { id }) if id == active.id()
    ));
    assert_eq!(
        TrackingRepository::active_worklog(&repository).unwrap(),
        Some(active)
    );
}
