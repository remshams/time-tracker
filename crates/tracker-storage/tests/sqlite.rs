//! Integration tests for the SQLite repository.
//!
//! These tests exercise the schema, the database-level invariants, and the
//! interplay between the domain tracker and persistence, including failure
//! states, rollback behavior, concurrent connections, migrations, and the
//! task-list read model with its latest-work aggregate.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, SyncSender, sync_channel},
};
use std::thread;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tempfile::TempDir;
use tracker_application::{
    ApplicationError, ClearActiveTaskOutcome, RepositoryError, SetActiveTaskOutcome, TaskListItem,
    TaskOperations, TaskOrdering, TaskOutcome, TaskQueries, TaskRepository, TrackerApplication,
    TrackingOperations, TrackingRepository, WORKLOG_PAGE_SIZE, WorklogCursor, WorklogPage,
    WorklogQueries, WorklogRepository,
};
use tracker_domain::{
    ActiveWorklog, Task, TaskId, TaskName, Tracker, TrackingError, TrackingOutcome, TrackingState,
    Worklog, WorklogId,
};
use tracker_storage::{SqliteRepository, StorageError};

fn at(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 0).unwrap()
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
                at(200).timestamp_micros(),
                at(250).timestamp_micros()
            ],
        )
        .unwrap();
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

    fn stop_worklog(&self, id: WorklogId, end: DateTime<Utc>) -> Result<Worklog, RepositoryError> {
        self.repository.stop_worklog(id, end).map_err(Into::into)
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
        stop_at: DateTime<Utc>,
        next: &Worklog,
    ) -> Result<(), RepositoryError> {
        self.repository
            .switch_worklog(id, stop_at, next)
            .map_err(Into::into)
    }
}

fn user_version(repository: &SqliteRepository) -> i64 {
    repository
        .connection()
        .query_row("PRAGMA user_version", [], |row| row.get(0))
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
    assert_eq!(user_version(&reopened), 2);
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
}

#[test]
fn a_version_1_database_migrates_to_version_2_and_keeps_every_record() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    create_v1_database(&path);

    // The backfill timestamp is captured between these two readings.
    let before = Utc::now();
    let repository = SqliteRepository::open(&path).unwrap();
    let after = Utc::now();
    assert_eq!(user_version(&repository), 2);
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
    assert_eq!(worklogs[0].id, worklog_id(10));
    assert_eq!(worklogs[0].start, at(100));
    assert_eq!(worklogs[0].end, None);
    assert_eq!(worklogs[1].id, worklog_id(11));
    assert_eq!(worklogs[1].start, at(200));
    assert_eq!(worklogs[1].end, Some(at(250)));
    assert_eq!(
        repository.active_worklog().unwrap().unwrap().id,
        worklog_id(10)
    );

    // The read model derives the latest work start from the migrated rows.
    let items = repository.list_task_items().unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].latest_work_start, Some(at(200)));
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
    assert_eq!(user_version(&second), 2);
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
            assert_eq!(latest, 2);
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
    assert_eq!(user_version(&repository), 2, "migrations ran exactly once");
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
        SetActiveTaskOutcome::Started { worklog } if worklog.start == at(100)
    ));
    assert!(matches!(
        application.set_active_task(alpha.id, at(110)).unwrap(),
        SetActiveTaskOutcome::AlreadyActive { .. }
    ));
    assert!(matches!(
        application.set_active_task(beta.id, at(150)).unwrap(),
        SetActiveTaskOutcome::Switched { stopped, started }
            if stopped.end == Some(at(150)) && started.start == at(150)
    ));
    let TrackingState::Running { worklog: active } = application.current_tracking() else {
        panic!("the worklog must be active");
    };
    let active = active.id;
    assert!(matches!(
        application.clear_active_task(active, at(200)).unwrap(),
        ClearActiveTaskOutcome::Stopped { worklog } if worklog.end == Some(at(200))
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
        WorklogRepository::worklog_page(&repository, task.id, None)
            .unwrap()
            .worklogs,
        vec![worklog]
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
        TrackingState::Running { worklog } if worklog.task_id == beta.id
    ));

    assert!(matches!(
        first.set_active_task(alpha.id, at(120)),
        Ok(SetActiveTaskOutcome::Switched { stopped, started })
            if stopped.task_id == beta.id
                && stopped.end == Some(at(120))
                && started.task_id == alpha.id
                && started.start == at(120)
    ));
    let stale_beta = match second.current_tracking() {
        TrackingState::Running { worklog } => worklog.id,
        TrackingState::Idle => panic!("beta must be active in the stale client"),
    };
    assert_eq!(
        second.clear_active_task(stale_beta, at(130)),
        Err(ApplicationError::TrackingStateChanged)
    );
    assert!(matches!(
        second.current_tracking(),
        TrackingState::Running { worklog } if worklog.task_id == alpha.id
    ));

    let other = SqliteRepository::open(&path).unwrap();
    other.archive_task(beta.id, at(130)).unwrap();
    assert!(matches!(
        first.set_active_task(beta.id, at(140)),
        Err(ApplicationError::TrackingWrite(
            tracker_application::RepositoryError::TaskArchived { id }
        )) if id == beta.id
    ));
    assert!(matches!(
        first.current_tracking(),
        TrackingState::Running { worklog } if worklog.task_id == alpha.id
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
            worklog: ActiveWorklog::begin(started.id, task.id, at(200))
        }
    );

    // One active worklog, and the restored task kept its history.
    let stored = SqliteRepository::open(&path).unwrap();
    let active = stored
        .active_worklog()
        .unwrap()
        .expect("one active worklog");
    assert_eq!(active.id, started.id);
    assert_eq!(active.task_id, task.id);
    assert_eq!(active.start, at(200));
    assert_eq!(
        stored.list_worklogs(task.id).unwrap(),
        vec![started.clone()]
    );

    // Client A stops the tracking it adopted, leaving a clean stopped row.
    let cleared = stale.clear_active_task(started.id, at(300)).unwrap();
    assert!(matches!(
        cleared,
        ClearActiveTaskOutcome::Stopped { worklog } if worklog.end == Some(at(300))
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
        TrackingState::Running { worklog: active } if active.id == worklog.id
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
        TrackingState::Running { worklog: active } if active.id == worklog.id
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
        Ok(Some(worklog)) if worklog.task_id == beta.id
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
    assert_eq!(repository.active_worklog().unwrap().unwrap().id, started.id);

    // After the worklog is stopped, the same archive succeeds.
    repository.stop_worklog(started.id, at(400)).unwrap();
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
    let stored = repository.stop_worklog(stopped.id, at(150)).unwrap();
    assert_eq!(stored.id, started.id);
    assert_eq!(stored.start, at(100));
    assert_eq!(stored.end, Some(at(150)));
    assert_eq!(repository.active_worklog().unwrap(), None);
    assert_eq!(repository.list_worklogs(task_id(1)).unwrap(), vec![stored]);

    // Stopping a missing worklog reports which one.
    let missing = worklog_id(77);
    assert!(matches!(
        repository.stop_worklog(missing, at(200)),
        Err(StorageError::WorklogNotFound { id }) if id == missing
    ));
    // Stopping a stopped worklog reports the conflict.
    assert!(matches!(
        repository.stop_worklog(started.id, at(200)),
        Err(StorageError::WorklogAlreadyStopped { id }) if id == started.id
    ));
    // A backwards end time is rejected by the database.
    let mut tracker = Tracker::idle();
    let worklog = tracker.start(&task, at(300)).unwrap();
    repository.insert_worklog(&worklog).unwrap();
    let error = repository
        .stop_worklog(worklog.id, at(299))
        .expect_err("end before start must fail");
    assert!(matches!(error, StorageError::Constraint(_)));
    assert_eq!(repository.active_worklog().unwrap().unwrap().id, worklog.id);
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
    assert!(matches!(error, StorageError::ActiveWorklogExists));
    // The first worklog is still the only active one.
    assert_eq!(repository.active_worklog().unwrap(), Some(first.clone()));

    // An worklog for a missing task is rejected as a missing task. The first
    // worklog is stopped first so the active-worklog rule cannot mask the
    // foreign-key failure.
    tracker.stop(at(200)).unwrap();
    repository.stop_worklog(first.id, at(200)).unwrap();
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
    let duplicate = Worklog::new(first.id, task.id, at(500), Some(at(600))).unwrap();
    let error = repository
        .insert_worklog(&duplicate)
        .expect_err("a stored id must not be inserted twice");
    assert!(matches!(
        error,
        StorageError::WorklogAlreadyExists { id } if id == first.id
    ));

    // A fresh id while another worklog is active hits the single-active rule.
    let second_active = Worklog::begin(worklog_id(42), task.id, at(200));
    let error = repository
        .insert_worklog(&second_active)
        .expect_err("two active worklogs cannot coexist");
    assert!(matches!(error, StorageError::ActiveWorklogExists));
    assert_eq!(repository.active_worklog().unwrap(), Some(first.clone()));
    assert_eq!(repository.list_worklogs(task.id).unwrap().len(), 1);
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
    repository.stop_worklog(started.id, at(150)).unwrap();
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
    assert_eq!(process_a.active_worklog().unwrap().unwrap().id, started.id);
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
    assert_eq!(repository.active_worklog().unwrap().unwrap().id, worklog.id);

    let outcome = tracker.toggle(&task, at(150)).unwrap();
    let stopped = match outcome {
        TrackingOutcome::Stopped { worklog } => worklog,
        other => panic!("expected Stopped, got {other:?}"),
    };
    repository.stop_worklog(stopped.id, at(150)).unwrap();
    assert_eq!(repository.active_worklog().unwrap(), None);

    // Toggling again starts a fresh worklog, not a resume.
    let outcome = tracker.toggle(&task, at(200)).unwrap();
    let restarted = match outcome {
        TrackingOutcome::Started { worklog } => worklog,
        other => panic!("expected Started, got {other:?}"),
    };
    repository.insert_worklog(&restarted).unwrap();
    assert_ne!(restarted.id, worklog.id);
    assert_eq!(
        repository.active_worklog().unwrap().unwrap().id,
        restarted.id
    );
    let worklogs = repository.list_worklogs(task_id(1)).unwrap();
    assert_eq!(worklogs.len(), 2);
    assert_eq!(worklogs[0].end, Some(at(150)));
    assert_eq!(worklogs[1].start, at(200));
    assert_eq!(worklogs[1].end, None);
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
    repository.stop_worklog(first.id, at(150)).unwrap();
    assert_eq!(first_stopped.id, first.id);

    let second = tracker.start(&task, at(300)).unwrap();
    repository.insert_worklog(&second).unwrap();
    tracker.stop(at(400)).unwrap();
    repository.stop_worklog(second.id, at(400)).unwrap();

    let worklogs = repository.list_worklogs(task_id(1)).unwrap();
    assert_eq!(worklogs.len(), 2);
    assert_ne!(worklogs[0].id, worklogs[1].id);
    assert_eq!(worklogs[0].start, at(100));
    assert_eq!(worklogs[0].end, Some(at(150)));
    assert_eq!(worklogs[1].start, at(300));
    assert_eq!(worklogs[1].end, Some(at(400)));
}

/// Inserts `worklogs` worklogs for the task, one per tag, started at
/// `at(tag)`; the first one is active, the others stop one second later.
fn insert_numbered_worklogs(repository: &SqliteRepository, task: &Task, worklogs: u32) {
    for tag in 1..=worklogs {
        let worklog = if tag == 1 {
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
        .map(|worklog| worklog.start.timestamp())
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
        .map(|worklog| worklog.start.timestamp())
        .collect();
    assert_eq!(rest_starts, [5, 4, 3, 2, 1]);

    // The two pages cover every record exactly once: none skipped, none
    // repeated. The active worklog is part of the history.
    let mut ids: Vec<u32> = first
        .worklogs
        .iter()
        .chain(&second.worklogs)
        .map(|worklog| worklog.id.as_uuid().as_u128() as u32)
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, (1..=55).collect::<Vec<_>>());
    assert_eq!(
        second.worklogs.last().unwrap().end,
        None,
        "the active worklog appears in the second page"
    );
}

#[test]
fn a_page_boundary_inside_equal_starts_neither_dups_nor_skips_rows() {
    let repository = repo();
    let task = named_task(1, "simultaneous");
    repository.create_task(task.clone()).unwrap();
    // 55 worklogs share one start, so identifier ascending decides the
    // whole order and the page boundary falls between two of them. A
    // boundary that ignores the identifier would repeat or drop rows 51
    // to 55.
    for tag in 1..=55u32 {
        let worklog = if tag == 1 {
            Worklog::begin(worklog_id(tag), task.id, at(500))
        } else {
            Worklog::new(worklog_id(tag), task.id, at(500), Some(at(501))).unwrap()
        };
        repository.insert_worklog(&worklog).unwrap();
    }

    let first = repository.worklog_page(task.id, None).unwrap();
    let ids: Vec<u32> = first
        .worklogs
        .iter()
        .map(|worklog| worklog.id.as_uuid().as_u128() as u32)
        .collect();
    assert_eq!(ids, (1..=50).collect::<Vec<_>>(), "identifier ascending");
    let cursor = first.next_cursor.expect("equal starts continue");
    assert_eq!(cursor.start, at(500));
    assert_eq!(cursor.id, worklog_id(50));

    let second = repository.worklog_page(task.id, Some(&cursor)).unwrap();
    let rest_ids: Vec<u32> = second
        .worklogs
        .iter()
        .map(|worklog| worklog.id.as_uuid().as_u128() as u32)
        .collect();
    assert_eq!(rest_ids, (51..=55).collect::<Vec<_>>());
    assert_eq!(second.next_cursor, None);
}

#[test]
fn an_active_worklog_is_part_of_a_history_page() {
    let repository = repo();
    let task = named_task(1, "running");
    repository.create_task(task.clone()).unwrap();
    let stopped = Worklog::new(worklog_id(1), task.id, at(100), Some(at(150))).unwrap();
    repository.insert_worklog(&stopped).unwrap();
    let active = Worklog::begin(worklog_id(2), task.id, at(200));
    repository.insert_worklog(&active).unwrap();

    let page = repository.worklog_page(task.id, None).unwrap();
    assert_eq!(page.worklogs.len(), 2);
    // History order is start descending, so the active worklog leads.
    assert_eq!(page.worklogs[0].id, active.id);
    assert_eq!(page.worklogs[0].end, None);
    assert_eq!(page.worklogs[1].id, stopped.id);
    assert_eq!(page.next_cursor, None);
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
    let newer = Worklog::new(worklog_id(100), task.id, at(1000), Some(at(1001))).unwrap();
    repository.insert_worklog(&newer).unwrap();

    let second = repository.worklog_page(task.id, Some(&cursor)).unwrap();
    let rest_ids: Vec<u32> = second
        .worklogs
        .iter()
        .map(|worklog| worklog.id.as_uuid().as_u128() as u32)
        .collect();
    assert_eq!(rest_ids, (1..=5).rev().collect::<Vec<_>>());
    assert_eq!(second.next_cursor, None);

    // The newer worklog is not lost; a fresh first page leads with it.
    let fresh = repository.worklog_page(task.id, None).unwrap();
    assert_eq!(fresh.worklogs[0].id, newer.id);
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
        .switch_worklog(switched.stopped.id, at(150), &switched.started)
        .unwrap();

    // Exactly one active worklog, belonging to the new task.
    let active = repository.active_worklog().unwrap().unwrap();
    assert_eq!(active.id, switched.started.id);
    assert_eq!(active.task_id, task_id(2));
    assert_eq!(active.start, at(160));
    assert_eq!(active.end, None);
    // The old worklog is stopped at the switch instant.
    let old = repository.list_worklogs(task_id(1)).unwrap();
    assert_eq!(old.len(), 1);
    assert_eq!(old[0].id, started.id);
    assert_eq!(old[0].end, Some(at(150)));
    // The new task has one active worklog.
    let new_worklogs = repository.list_worklogs(task_id(2)).unwrap();
    assert_eq!(new_worklogs.len(), 1);
    assert_eq!(new_worklogs[0].id, switched.started.id);
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
        .switch_worklog(started.id, at(99), &next)
        .expect_err("backwards stop must fail");
    assert!(matches!(error, StorageError::Constraint(_)));

    // Rollback: the old worklog is still active and untouched, no new worklog.
    let active = repository.active_worklog().unwrap().unwrap();
    assert_eq!(active.id, started.id);
    assert_eq!(active.end, None);
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
        repository.switch_worklog(started.id, at(150), &next),
        Err(StorageError::TaskNotFound { id }) if id == task_id(99)
    ));

    // Rollback: the old worklog is still active, no new worklog was inserted.
    let active = repository.active_worklog().unwrap().unwrap();
    assert_eq!(active.id, started.id);
    assert_eq!(active.end, None);
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
        repository.switch_worklog(missing, at(150), &next),
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
    repository.stop_worklog(started.id, at(150)).unwrap();

    let next = Worklog::begin(worklog_id(42), rest.id, at(160));
    assert!(matches!(
        repository.switch_worklog(started.id, at(150), &next),
        Err(StorageError::WorklogAlreadyStopped { id }) if id == started.id
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
        .switch_worklog(started.id, at(150), &next)
        .expect_err("the archived target must refuse the switch");
    assert!(matches!(
        error,
        StorageError::TaskArchived { id } if id == rest.id
    ));

    // Rollback: the old worklog is still active, the archived task received
    // nothing.
    assert_eq!(repository.active_worklog().unwrap().unwrap().id, started.id);
    assert_eq!(repository.list_worklogs(rest.id).unwrap(), Vec::new());
    let old = repository.list_worklogs(work.id).unwrap();
    assert_eq!(old.len(), 1);
    assert_eq!(old[0].end, None);
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
        .switch_worklog(started.id, at(150), &next)
        .expect_err("the duplicate id must fail the switch");
    assert!(matches!(
        error,
        StorageError::WorklogAlreadyExists { id } if id == worklog_id(42)
    ));

    // Rollback: the stop half is undone, so work is still active.
    assert_eq!(repository.active_worklog().unwrap().unwrap().id, started.id);
    let rest_worklogs = repository.list_worklogs(rest.id).unwrap();
    assert_eq!(rest_worklogs.len(), 1);
    assert_eq!(rest_worklogs[0].end, Some(at(20)));
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
    repository.stop_worklog(started.id, at(150)).unwrap();
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
        worklog_id = started.id;
        // Deliberately no stop: exiting must not implicitly stop tracking.
        repository.insert_worklog(&started).unwrap();
    }

    {
        let repository = SqliteRepository::open(temp.path().join("tracker.db")).unwrap();
        let recovered = repository
            .active_worklog()
            .unwrap()
            .expect("worklog recovered");
        assert_eq!(recovered.id, worklog_id);
        assert_eq!(recovered.task_id, task.id);
        assert_eq!(recovered.start, at(100));
        assert_eq!(recovered.end, None);

        // The recovered worklog resumes tracking and stops cleanly.
        let mut tracker = Tracker::resume(recovered).unwrap();
        assert_eq!(
            tracker.state(),
            &TrackingState::Running {
                worklog: ActiveWorklog {
                    id: worklog_id,
                    task_id: task.id,
                    start: at(100),
                }
            }
        );
        let stopped = tracker.stop(at(250)).unwrap();
        repository.stop_worklog(stopped.id, at(250)).unwrap();
    }

    // A third open sees a stopped worklog and no active one.
    let repository = SqliteRepository::open(temp.path().join("tracker.db")).unwrap();
    assert_eq!(repository.active_worklog().unwrap(), None);
    let worklogs = repository.list_worklogs(task.id).unwrap();
    assert_eq!(worklogs.len(), 1);
    assert_eq!(worklogs[0].start, at(100));
    assert_eq!(worklogs[0].end, Some(at(250)));
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
