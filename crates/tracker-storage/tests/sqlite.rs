//! Integration tests for the SQLite repository.
//!
//! These tests exercise the schema, the database-level invariants, and the
//! interplay between the domain tracker and persistence, including failure
//! states and rollback behavior.

use chrono::{DateTime, Utc};
use tempfile::TempDir;
use tracker_core::{
    ActiveEntry, EntryId, Task, TaskId, TaskName, TimeEntry, Tracker, TrackerRepository,
    TrackingError, TrackingOutcome, TrackingState,
};
use tracker_storage::{SqliteRepository, StorageError};

fn at(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 0).unwrap()
}

fn task_id(tag: u32) -> TaskId {
    // Fixed, ordered identifiers keep the ordering tests deterministic.
    TaskId::from_uuid(uuid::Uuid::from_u128(u128::from(tag)))
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

#[test]
fn migrations_create_the_schema_and_are_idempotent() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("nested").join("tracker.db");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();

    {
        let repository = SqliteRepository::open(&path).unwrap();
        repository.create_task(named_task(1, "first")).unwrap();
    }
    // Reopening applies no migration again and keeps the data.
    let reopened = SqliteRepository::open(&path).unwrap();
    let conn = reopened.connection();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);
    let tasks = reopened.list_tasks().unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].name.as_str(), "first");
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(tables.contains(&"tasks".to_owned()));
    assert!(tables.contains(&"time_entries".to_owned()));
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
    repository.insert_entry(&started).unwrap();
    assert_eq!(repository.active_entry().unwrap(), Some(started.clone()));

    let stopped = tracker.stop(at(150)).unwrap();
    let stored = repository.stop_entry(stopped.id, at(150)).unwrap();
    assert_eq!(stored.id, started.id);
    assert_eq!(stored.start, at(100));
    assert_eq!(stored.end, Some(at(150)));
    assert_eq!(repository.active_entry().unwrap(), None);
    assert_eq!(repository.list_entries(task_id(1)).unwrap(), vec![stored]);

    // Stopping a missing entry reports which one.
    let missing = EntryId::from_uuid(uuid::Uuid::from_u128(77));
    assert!(matches!(
        repository.stop_entry(missing, at(200)),
        Err(StorageError::EntryNotFound { id }) if id == missing
    ));
    // Stopping a stopped entry reports the conflict.
    assert!(matches!(
        repository.stop_entry(started.id, at(200)),
        Err(StorageError::EntryAlreadyStopped { id }) if id == started.id
    ));
    // A backwards end time is rejected by the database.
    let mut tracker = Tracker::idle();
    let entry = tracker.start(&task, at(300)).unwrap();
    repository.insert_entry(&entry).unwrap();
    let error = repository
        .stop_entry(entry.id, at(299))
        .expect_err("end before start must fail");
    assert!(matches!(error, StorageError::Constraint(_)));
    assert_eq!(repository.active_entry().unwrap().unwrap().id, entry.id);
}

#[test]
fn inserting_a_second_active_entry_is_rejected_by_the_database() {
    let repository = repo();
    let task = named_task(1, "work");
    repository.create_task(task.clone()).unwrap();
    let mut tracker = Tracker::idle();
    let first = tracker.start(&task, at(100)).unwrap();
    repository.insert_entry(&first).unwrap();

    let second = TimeEntry::begin(
        EntryId::from_uuid(uuid::Uuid::from_u128(42)),
        task.id,
        at(200),
    );
    let error = repository
        .insert_entry(&second)
        .expect_err("second active entry must fail");
    assert!(matches!(error, StorageError::ActiveEntryExists));
    // The first entry is still the only active one.
    assert_eq!(repository.active_entry().unwrap(), Some(first.clone()));

    // An entry for a missing task is rejected as a missing task. The first
    // entry is stopped first so the active-entry rule cannot mask the
    // foreign-key failure.
    tracker.stop(at(200)).unwrap();
    repository.stop_entry(first.id, at(200)).unwrap();
    let orphan = TimeEntry::begin(
        EntryId::from_uuid(uuid::Uuid::from_u128(43)),
        task_id(99),
        at(300),
    );
    assert!(matches!(
        repository.insert_entry(&orphan),
        Err(StorageError::TaskNotFound { id }) if id == task_id(99)
    ));
}

#[test]
fn same_task_toggle_stops_and_later_restarts() {
    let repository = repo();
    let task = named_task(1, "work");
    repository.create_task(task.clone()).unwrap();
    let mut tracker = Tracker::idle();

    let outcome = tracker.toggle(&task, at(100)).unwrap();
    let entry = match outcome {
        TrackingOutcome::Started { entry } => entry,
        other => panic!("expected Started, got {other:?}"),
    };
    repository.insert_entry(&entry).unwrap();
    assert_eq!(repository.active_entry().unwrap().unwrap().id, entry.id);

    let outcome = tracker.toggle(&task, at(150)).unwrap();
    let stopped = match outcome {
        TrackingOutcome::Stopped { entry } => entry,
        other => panic!("expected Stopped, got {other:?}"),
    };
    repository.stop_entry(stopped.id, at(150)).unwrap();
    assert_eq!(repository.active_entry().unwrap(), None);

    // Toggling again starts a fresh entry, not a resume.
    let outcome = tracker.toggle(&task, at(200)).unwrap();
    let restarted = match outcome {
        TrackingOutcome::Started { entry } => entry,
        other => panic!("expected Started, got {other:?}"),
    };
    repository.insert_entry(&restarted).unwrap();
    assert_ne!(restarted.id, entry.id);
    assert_eq!(repository.active_entry().unwrap().unwrap().id, restarted.id);
    let entries = repository.list_entries(task_id(1)).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].end, Some(at(150)));
    assert_eq!(entries[1].start, at(200));
    assert_eq!(entries[1].end, None);
}

#[test]
fn restarts_record_separate_entries() {
    let repository = repo();
    let task = named_task(1, "work");
    repository.create_task(task.clone()).unwrap();
    let mut tracker = Tracker::idle();

    let first = tracker.start(&task, at(100)).unwrap();
    repository.insert_entry(&first).unwrap();
    let first_stopped = tracker.stop(at(150)).unwrap();
    repository.stop_entry(first.id, at(150)).unwrap();
    assert_eq!(first_stopped.id, first.id);

    let second = tracker.start(&task, at(300)).unwrap();
    repository.insert_entry(&second).unwrap();
    tracker.stop(at(400)).unwrap();
    repository.stop_entry(second.id, at(400)).unwrap();

    let entries = repository.list_entries(task_id(1)).unwrap();
    assert_eq!(entries.len(), 2);
    assert_ne!(entries[0].id, entries[1].id);
    assert_eq!(entries[0].start, at(100));
    assert_eq!(entries[0].end, Some(at(150)));
    assert_eq!(entries[1].start, at(300));
    assert_eq!(entries[1].end, Some(at(400)));
}

#[test]
fn switch_stops_the_old_entry_and_starts_the_new_one_atomically() {
    let repository = repo();
    let work = named_task(1, "work");
    let rest = named_task(2, "rest");
    repository.create_task(work.clone()).unwrap();
    repository.create_task(rest.clone()).unwrap();
    let mut tracker = Tracker::idle();

    let started = tracker.start(&work, at(100)).unwrap();
    repository.insert_entry(&started).unwrap();

    let switched = tracker.switch(&rest, at(150), at(160)).unwrap();
    repository
        .switch_entry(switched.stopped.id, at(150), &switched.started)
        .unwrap();

    // Exactly one active entry, belonging to the new task.
    let active = repository.active_entry().unwrap().unwrap();
    assert_eq!(active.id, switched.started.id);
    assert_eq!(active.task_id, task_id(2));
    assert_eq!(active.start, at(160));
    assert_eq!(active.end, None);
    // The old entry is stopped at the switch instant.
    let old = repository.list_entries(task_id(1)).unwrap();
    assert_eq!(old.len(), 1);
    assert_eq!(old[0].id, started.id);
    assert_eq!(old[0].end, Some(at(150)));
    // The new task has one active entry.
    let new_entries = repository.list_entries(task_id(2)).unwrap();
    assert_eq!(new_entries.len(), 1);
    assert_eq!(new_entries[0].id, switched.started.id);
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
    repository.insert_entry(&started).unwrap();

    // Stop before the active entry's start violates the CHECK constraint.
    let next = TimeEntry::begin(
        EntryId::from_uuid(uuid::Uuid::from_u128(42)),
        rest.id,
        at(160),
    );
    let error = repository
        .switch_entry(started.id, at(99), &next)
        .expect_err("backwards stop must fail");
    assert!(matches!(error, StorageError::Constraint(_)));

    // Rollback: the old entry is still active and untouched, no new entry.
    let active = repository.active_entry().unwrap().unwrap();
    assert_eq!(active.id, started.id);
    assert_eq!(active.end, None);
    assert_eq!(repository.list_entries(task_id(1)).unwrap().len(), 1);
    assert_eq!(repository.list_entries(task_id(2)).unwrap(), Vec::new());
}

#[test]
fn switch_rolls_back_when_the_target_task_is_missing() {
    let repository = repo();
    let work = named_task(1, "work");
    repository.create_task(work.clone()).unwrap();
    let mut tracker = Tracker::idle();

    let started = tracker.start(&work, at(100)).unwrap();
    repository.insert_entry(&started).unwrap();

    let next = TimeEntry::begin(
        EntryId::from_uuid(uuid::Uuid::from_u128(42)),
        task_id(99),
        at(160),
    );
    assert!(matches!(
        repository.switch_entry(started.id, at(150), &next),
        Err(StorageError::TaskNotFound { id }) if id == task_id(99)
    ));

    // Rollback: the old entry is still active, no new entry was inserted.
    let active = repository.active_entry().unwrap().unwrap();
    assert_eq!(active.id, started.id);
    assert_eq!(active.end, None);
    assert_eq!(repository.list_entries(task_id(1)).unwrap().len(), 1);
    assert_eq!(repository.list_entries(task_id(99)).unwrap(), Vec::new());
}

#[test]
fn switch_reports_a_missing_entry_and_changes_nothing() {
    let repository = repo();
    let work = named_task(1, "work");
    let rest = named_task(2, "rest");
    repository.create_task(work.clone()).unwrap();
    repository.create_task(rest.clone()).unwrap();
    let mut tracker = Tracker::idle();

    let started = tracker.start(&work, at(100)).unwrap();
    repository.insert_entry(&started).unwrap();

    let missing = EntryId::from_uuid(uuid::Uuid::from_u128(77));
    let next = TimeEntry::begin(
        EntryId::from_uuid(uuid::Uuid::from_u128(42)),
        rest.id,
        at(160),
    );
    assert!(matches!(
        repository.switch_entry(missing, at(150), &next),
        Err(StorageError::EntryNotFound { id }) if id == missing
    ));
    assert_eq!(repository.active_entry().unwrap(), Some(started.clone()));
    assert_eq!(repository.list_entries(task_id(1)).unwrap().len(), 1);
    assert_eq!(repository.list_entries(task_id(2)).unwrap(), Vec::new());
}

#[test]
fn switch_rejects_an_already_stopped_entry() {
    let repository = repo();
    let work = named_task(1, "work");
    let rest = named_task(2, "rest");
    repository.create_task(work.clone()).unwrap();
    repository.create_task(rest.clone()).unwrap();
    let mut tracker = Tracker::idle();

    let started = tracker.start(&work, at(100)).unwrap();
    repository.insert_entry(&started).unwrap();
    repository.stop_entry(started.id, at(150)).unwrap();

    let next = TimeEntry::begin(
        EntryId::from_uuid(uuid::Uuid::from_u128(42)),
        rest.id,
        at(160),
    );
    assert!(matches!(
        repository.switch_entry(started.id, at(150), &next),
        Err(StorageError::EntryAlreadyStopped { id }) if id == started.id
    ));
    assert_eq!(repository.active_entry().unwrap(), None);
    assert_eq!(repository.list_entries(task_id(2)).unwrap(), Vec::new());
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
    repository.insert_entry(&started).unwrap();

    assert_eq!(
        tracker.ensure_archivable(work.id),
        Err(TrackingError::TaskIsActive { id: work.id })
    );
    // The task stays usable and unarchived after the rejection.
    assert!(!repository.find_task(work.id).unwrap().unwrap().archived);
    assert_eq!(repository.active_entry().unwrap(), Some(started.clone()));

    // A different task archives fine while tracking runs.
    tracker.ensure_archivable(other.id).unwrap();
    repository.archive_task(other.id).unwrap();
    assert!(repository.find_task(other.id).unwrap().unwrap().archived);

    // Once the entry is stopped, the task can be archived.
    tracker.stop(at(150)).unwrap();
    repository.stop_entry(started.id, at(150)).unwrap();
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
    assert_eq!(repository.active_entry().unwrap(), None);
}

#[test]
fn active_entry_survives_closing_and_reopening_the_database() {
    let temp = tempfile::tempdir().unwrap();
    let task = named_task(1, "long running");
    let entry_id;

    {
        let repository = file_repo(&temp);
        repository.create_task(task.clone()).unwrap();
        let mut tracker = Tracker::idle();
        let started = tracker.start(&task, at(100)).unwrap();
        entry_id = started.id;
        // Deliberately no stop: exiting must not implicitly stop tracking.
        repository.insert_entry(&started).unwrap();
    }

    {
        let repository = SqliteRepository::open(temp.path().join("tracker.db")).unwrap();
        let recovered = repository.active_entry().unwrap().expect("entry recovered");
        assert_eq!(recovered.id, entry_id);
        assert_eq!(recovered.task_id, task.id);
        assert_eq!(recovered.start, at(100));
        assert_eq!(recovered.end, None);

        // The recovered entry resumes tracking and stops cleanly.
        let mut tracker = Tracker::resume(recovered).unwrap();
        assert_eq!(
            tracker.state(),
            &TrackingState::Running {
                entry: ActiveEntry {
                    id: entry_id,
                    task_id: task.id,
                    start: at(100),
                }
            }
        );
        let stopped = tracker.stop(at(250)).unwrap();
        repository.stop_entry(stopped.id, at(250)).unwrap();
    }

    // A third open sees a stopped entry and no active one.
    let repository = SqliteRepository::open(temp.path().join("tracker.db")).unwrap();
    assert_eq!(repository.active_entry().unwrap(), None);
    let entries = repository.list_entries(task.id).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].start, at(100));
    assert_eq!(entries[0].end, Some(at(250)));
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
fn corrupt_entry_rows_are_reported_as_corrupt_data() {
    // An out-of-range timestamp cannot be represented in the domain.
    let repository = repo();
    let conn = repository.connection();
    conn.execute(
        "INSERT INTO tasks (id, name, archived) VALUES ('00000000-0000-0000-0000-000000000001', 'work', 0)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO time_entries (id, task_id, start_us, end_us)
         VALUES ('00000000-0000-7000-8000-000000000010',
                 '00000000-0000-0000-0000-000000000001', 9223372036854775807, NULL)",
        [],
    )
    .unwrap();
    let error = repository
        .list_entries(task_id(1))
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
         INSERT INTO time_entries (id, task_id, start_us, end_us)
         VALUES ('00000000-0000-7000-8000-000000000010',
                 '00000000-0000-0000-0000-000000000001', 200, 100);
         PRAGMA ignore_check_constraints = OFF;",
    )
    .unwrap();
    let error = repository
        .list_entries(task_id(1))
        .expect_err("backwards interval is corrupt");
    assert!(matches!(error, StorageError::CorruptData("entry interval")));
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
    assert!(matches!(error, StorageError::Sql(_)));
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
