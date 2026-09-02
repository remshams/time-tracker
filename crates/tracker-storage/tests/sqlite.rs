//! Integration tests for the SQLite repository.
//!
//! These tests exercise the schema, the database-level invariants, and the
//! interplay between the domain tracker and persistence, including failure
//! states, rollback behavior, and concurrent connections.

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
    ApplicationError, ClearActiveTaskOutcome, RepositoryError, SetActiveTaskOutcome, TaskQueries,
    TaskRepository, TrackerApplication, TrackingOperations, TrackingRepository, WorklogRepository,
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
    Task::new(task_id(tag), TaskName::new(name).unwrap())
}

fn repo() -> SqliteRepository {
    SqliteRepository::open_in_memory().unwrap()
}

fn file_repo(dir: &TempDir) -> SqliteRepository {
    SqliteRepository::open(dir.path().join("tracker.db")).unwrap()
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

    fn list_tasks(&self) -> Result<Vec<Task>, RepositoryError> {
        self.repository.list_tasks().map_err(Into::into)
    }

    fn rename_task(&self, id: TaskId, name: TaskName) -> Result<Task, RepositoryError> {
        self.repository.rename_task(id, name).map_err(Into::into)
    }

    fn archive_task(&self, id: TaskId) -> Result<Task, RepositoryError> {
        self.repository.archive_task(id).map_err(Into::into)
    }
}

impl WorklogRepository for SynchronizingRepository {
    fn list_worklogs(&self, task_id: TaskId) -> Result<Vec<Worklog>, RepositoryError> {
        self.repository.list_worklogs(task_id).map_err(Into::into)
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
        repository.create_task(named_task(1, "first")).unwrap();
    }
    // Reopening applies no migration again and keeps the data.
    let reopened = SqliteRepository::open(&path).unwrap();
    assert_eq!(user_version(&reopened), 1);
    let tasks = reopened.list_tasks().unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].name.as_str(), "first");
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
    conn.execute(
        "INSERT INTO tasks (id, name, archived) VALUES ('00000000-0000-7000-8000-000000000001', 'name', 0)",
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
            assert_eq!(latest, 1);
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
                repository.create_task(Task::new(TaskId::generate(), name))?;
                Ok(())
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap().expect("every first open completes");
    }
    let repository = SqliteRepository::open(&path).unwrap();
    assert_eq!(user_version(&repository), 1, "migrations ran exactly once");
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
    let task = named_task(1, "alpha");
    TaskRepository::create_task(&repository, task.clone()).unwrap();
    assert_eq!(
        TaskRepository::find_task(&repository, task.id).unwrap(),
        Some(task.clone())
    );

    let worklog = Worklog::begin(worklog_id(1), task.id, at(100));
    TrackingRepository::insert_worklog(&repository, &worklog).unwrap();
    assert_eq!(
        WorklogRepository::list_worklogs(&repository, task.id).unwrap(),
        vec![worklog]
    );
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
    other.archive_task(beta.id).unwrap();
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
    assert!(first.task(beta.id).unwrap().archived);
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
fn task_create_find_list_rename_archive_and_missing_errors() {
    let repository = repo();
    // Insert out of id order; listing orders by id, not insertion order.
    repository.create_task(named_task(3, "third")).unwrap();
    repository.create_task(named_task(1, "first")).unwrap();
    repository.create_task(named_task(2, "second")).unwrap();

    let tasks = repository.list_tasks().unwrap();
    let names: Vec<&str> = tasks.iter().map(|task| task.name.as_str()).collect();
    assert_eq!(names, ["first", "second", "third"]);

    let found = repository.find_task(task_id(2)).unwrap().unwrap();
    assert_eq!(found.name.as_str(), "second");
    assert!(!found.archived);
    assert!(repository.find_task(task_id(9)).unwrap().is_none());

    let renamed = repository
        .rename_task(task_id(2), TaskName::new("renamed").unwrap())
        .unwrap();
    assert_eq!(renamed.name.as_str(), "renamed");
    assert_eq!(
        repository.find_task(task_id(2)).unwrap().unwrap().name,
        renamed.name
    );
    // Renaming does not change the list position.
    let tasks = repository.list_tasks().unwrap();
    let names: Vec<&str> = tasks.iter().map(|task| task.name.as_str()).collect();
    assert_eq!(names, ["first", "renamed", "third"]);

    let archived = repository.archive_task(task_id(3)).unwrap();
    assert!(archived.archived);
    assert!(repository.find_task(task_id(3)).unwrap().unwrap().archived);
    assert!(!repository.find_task(task_id(1)).unwrap().unwrap().archived);

    assert!(matches!(
        repository.rename_task(task_id(9), TaskName::new("x").unwrap()),
        Err(StorageError::TaskNotFound { id }) if id == task_id(9)
    ));
    assert!(matches!(
        repository.archive_task(task_id(9)),
        Err(StorageError::TaskNotFound { id }) if id == task_id(9)
    ));
    assert!(matches!(
        repository.create_task(named_task(1, "duplicate")),
        Err(StorageError::TaskAlreadyExists { id }) if id == task_id(1)
    ));
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
    repository.archive_task(task.id).unwrap();

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
fn a_task_with_an_active_worklog_cannot_be_archived() {
    let repository = repo();
    let task = named_task(1, "running");
    repository.create_task(task.clone()).unwrap();
    let mut tracker = Tracker::idle();
    let started = tracker.start(&task, at(100)).unwrap();
    repository.insert_worklog(&started).unwrap();

    let error = repository
        .archive_task(task.id)
        .expect_err("the active task must not archive");
    assert!(matches!(
        error,
        StorageError::TaskIsActive { id } if id == task.id
    ));

    // Error state: the task is still usable and the timer still runs.
    assert!(!repository.find_task(task.id).unwrap().unwrap().archived);
    assert_eq!(repository.active_worklog().unwrap(), Some(started.clone()));

    // After the worklog is stopped, the same call archives the task.
    tracker.stop(at(150)).unwrap();
    repository.stop_worklog(started.id, at(150)).unwrap();
    let archived = repository.archive_task(task.id).unwrap();
    assert!(archived.archived);
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
    process_b.archive_task(task.id).unwrap();
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
        .archive_task(other.id)
        .expect_err("the active task must not archive");
    assert!(matches!(
        error,
        StorageError::TaskIsActive { id } if id == other.id
    ));
    assert_eq!(process_a.active_worklog().unwrap().unwrap().id, started.id);
    assert!(!process_b.find_task(other.id).unwrap().unwrap().archived);
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
    repository.archive_task(rest.id).unwrap();

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
    assert!(!repository.find_task(work.id).unwrap().unwrap().archived);
    assert_eq!(repository.active_worklog().unwrap(), Some(started.clone()));

    // A different task archives fine while tracking runs.
    tracker.ensure_archivable(other.id).unwrap();
    repository.archive_task(other.id).unwrap();
    assert!(repository.find_task(other.id).unwrap().unwrap().archived);

    // Once the worklog is stopped, the task can be archived.
    tracker.stop(at(150)).unwrap();
    repository.stop_worklog(started.id, at(150)).unwrap();
    tracker.ensure_archivable(work.id).unwrap();
    repository.archive_task(work.id).unwrap();
    assert!(repository.find_task(work.id).unwrap().unwrap().archived);
}

#[test]
fn archived_tasks_cannot_start_or_become_active() {
    let repository = repo();
    let mut tracker = Tracker::idle();
    let archived = Task {
        archived: true,
        ..named_task(1, "old")
    };
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
        "INSERT INTO tasks (id, name, archived) VALUES ('not-a-uuid', 'name', 0)",
        [],
    )
    .unwrap();
    let error = repository.list_tasks().expect_err("bad id is corrupt");
    assert!(matches!(error, StorageError::InvalidId(_)));

    let repository = repo();
    let conn = repository.connection();
    conn.execute(
        "INSERT INTO tasks (id, name, archived) VALUES ('00000000-0000-7000-8000-000000000001', '   ', 0)",
        [],
    )
    .unwrap();
    let error = repository.list_tasks().expect_err("empty name is corrupt");
    assert!(matches!(error, StorageError::CorruptData("task name")));
}

#[test]
fn corrupt_worklog_rows_are_reported_as_corrupt_data() {
    // An out-of-range timestamp cannot be represented in the domain.
    let repository = repo();
    let conn = repository.connection();
    conn.execute(
        "INSERT INTO tasks (id, name, archived) VALUES ('00000000-0000-0000-0000-000000000001', 'work', 0)",
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

    // An end before start can only enter the database with CHECKs disabled,
    // which simulates tampering or a broken writer.
    let repository = repo();
    let conn = repository.connection();
    conn.execute(
        "INSERT INTO tasks (id, name, archived) VALUES ('00000000-0000-0000-0000-000000000001', 'work', 0)",
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
