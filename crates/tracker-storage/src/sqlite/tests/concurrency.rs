// SQLite concurrency integration tests.

use super::*;

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
    assert_eq!(user_version(&repository), 7, "migrations ran exactly once");
    assert_eq!(repository.list_tasks().unwrap().len(), 8);
}

#[test]
fn a_stale_client_unarchive_adopts_the_tracking_another_client_started() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let task = named_task(1, "restored");
    let setup = SqliteRepository::open(&path).unwrap();
    setup.create_task(task.clone()).unwrap();
    setup.archive_task(task.id(), at(120)).unwrap();
    drop(setup);

    // Client A loads while the task is archived and nothing is tracked.
    let mut stale = TrackerApplication::load(SqliteRepository::open(&path).unwrap()).unwrap();
    // Client B loads on the same file and restores the task.
    let mut restoring = TrackerApplication::load(SqliteRepository::open(&path).unwrap()).unwrap();
    assert!(matches!(
        restoring.unarchive_task(task.id(), at(150)).unwrap(),
        restored if !restored.is_archived()
    ));
    let started = match restoring.set_active_task(task.id(), at(200)).unwrap() {
        SetActiveTaskOutcome::Started { worklog } => worklog,
        other => panic!("expected Started, got {other:?}"),
    };

    // Client A unarchives from its stale archived row. The refresh before
    // the write must pick up the tracking B started, not report Idle over
    // an active worklog that survived the call.
    assert!(matches!(
        stale.unarchive_task(task.id(), at(250)).unwrap(),
        restored if !restored.is_archived()
    ));
    assert_eq!(
        stale.current_tracking(),
        &TrackingState::Running {
            worklog: ActiveWorklog::begin(started.id(), task.id(), at(200))
        }
    );

    // One active worklog, and the restored task kept its history.
    let stored = SqliteRepository::open(&path).unwrap();
    let active = stored
        .active_worklog()
        .unwrap()
        .expect("one active worklog");
    assert_eq!(active.id(), started.id());
    assert_eq!(active.task_id(), task.id());
    assert_eq!(active.start(), at(200));
    assert_eq!(
        stored.list_worklogs(task.id()).unwrap(),
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
fn a_stale_rename_cannot_overwrite_a_concurrent_archive_state() {
    let repository = repo();
    repository
        .create_task(stamped_task(1, "alpha", 100, 100))
        .unwrap();
    // A second client archives the task after this client last read it.
    let archived = TrackerRepository::archive_task(&repository, task_id(1), at(150)).unwrap();
    assert!(archived.is_archived());

    // The stale client renames from its unarchived view: the rename lands,
    // and the concurrent archive state survives it.
    let renamed = TrackerRepository::rename_task(
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
    let renamed = TrackerRepository::rename_task(
        &repository,
        task_id(1),
        TaskName::new("beta").unwrap(),
        at(150),
    )
    .unwrap();
    assert_eq!(renamed.name().as_str(), "beta");

    // The stale client archives: the archive lands and keeps the rename.
    let archived = TrackerRepository::archive_task(&repository, task_id(1), at(200)).unwrap();
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
fn concurrent_clients_cannot_insert_overlapping_worklogs_for_one_task() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let setup = SqliteRepository::open(&path).unwrap();
    let task = named_task(1, "shared");
    let task_id = task.id();
    setup.create_task(task.clone()).unwrap();
    drop(setup);

    let repositories = [
        SqliteRepository::open(&path).unwrap(),
        SqliteRepository::open(&path).unwrap(),
    ];
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = repositories
        .into_iter()
        .zip([(1, 100, 200), (2, 150, 250)])
        .map(|(repository, (tag, start, end))| {
            let barrier = barrier.clone();
            thread::spawn(move || {
                let worklog =
                    Worklog::new(worklog_id(tag), task_id, at(start), Some(at(end))).unwrap();
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
            .list_worklogs(task_id)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn two_clients_cannot_apply_corrections_from_the_same_expected_values() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let setup = SqliteRepository::open(&path).unwrap();
    let task = named_task(1, "shared");
    setup.create_task(task.clone()).unwrap();
    let original = Worklog::new(worklog_id(1), task.id(), at(100), Some(at(200))).unwrap();
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
fn concurrent_connections_enforce_the_archive_rules() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let process_a = SqliteRepository::open(&path).unwrap();
    let process_b = SqliteRepository::open(&path).unwrap();

    let task = named_task(1, "shared");
    process_a.create_task(task.clone()).unwrap();

    // The other process archives the task; this process's stale view still
    // believes the task is active, but the database refuses the worklog.
    process_b.archive_task(task.id(), at(90)).unwrap();
    let stale = Worklog::begin(worklog_id(42), task.id(), at(100));
    let error = process_a
        .insert_worklog(&stale)
        .expect_err("the archived task must refuse the worklog");
    assert!(matches!(
        error,
        StorageError::TaskArchived { id } if id == task.id()
    ));

    // The reverse: this process tracks a second task, so the other process
    // cannot archive it.
    let other = named_task(2, "other");
    process_a.create_task(other.clone()).unwrap();
    let started = Worklog::begin(worklog_id(43), other.id(), at(100));
    process_a.insert_worklog(&started).unwrap();
    let error = process_b
        .archive_task(other.id(), at(120))
        .expect_err("the active task must not archive");
    assert!(matches!(
        error,
        StorageError::TaskIsActive { id } if id == other.id()
    ));
    assert_eq!(
        process_a.active_worklog().unwrap().unwrap().id(),
        started.id()
    );
    assert!(
        !process_b
            .find_task(other.id())
            .unwrap()
            .unwrap()
            .is_archived()
    );
}

#[test]
fn concurrent_moves_commit_once_and_leave_one_destination_with_revisions() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let setup = SqliteRepository::open(&path).unwrap();
    let source = named_task(1, "source");
    let first_destination = named_task(2, "first destination");
    let second_destination = named_task(3, "second destination");
    for task in [&source, &first_destination, &second_destination] {
        setup.create_task(task.clone()).unwrap();
    }
    let original = Worklog::new(worklog_id(1), source.id(), at(100), Some(at(200))).unwrap();
    setup.insert_worklog(&original).unwrap();
    drop(setup);

    let repositories = [
        SqliteRepository::open(&path).unwrap(),
        SqliteRepository::open(&path).unwrap(),
    ];
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = repositories
        .into_iter()
        .zip([first_destination.id(), second_destination.id()])
        .map(|(repository, destination_task_id)| {
            let barrier = barrier.clone();
            let original = original.clone();
            thread::spawn(move || {
                barrier.wait();
                repository.compare_and_move_worklog(
                    original.id(),
                    original.task_id(),
                    original.times(),
                    destination_task_id,
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
    let winner = results.into_iter().find_map(Result::ok).unwrap().worklog;
    let stored = SqliteRepository::open(&path)
        .unwrap()
        .find_worklog(original.id())
        .unwrap()
        .unwrap();
    assert_eq!(stored, winner);
    assert_eq!(
        history_revision(&SqliteRepository::open(&path).unwrap(), source.id()),
        1
    );
    let repository = SqliteRepository::open(&path).unwrap();
    for task_id in [first_destination.id(), second_destination.id()] {
        let expected_revision = if task_id == winner.task_id() { 1 } else { 0 };
        assert_eq!(history_revision(&repository, task_id), expected_revision);
    }
}

/// Demonstrates that competing connections cannot both overwrite a name they reviewed before either write.
#[test]
fn concurrent_guarded_renames_cannot_both_replace_the_same_reviewed_name() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let task = named_task(1, "Reviewed name");
    SqliteRepository::open(&path)
        .unwrap()
        .create_task(task.clone())
        .unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = ["First draft", "Second draft"]
        .into_iter()
        .map(|name| {
            let path = path.clone();
            let task = task.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                let repository = SqliteRepository::open(path).unwrap();
                barrier.wait();
                repository.compare_and_rename_task(
                    task.id(),
                    task.name(),
                    TaskName::new(name).unwrap(),
                    at(200),
                )
            })
        })
        .collect();
    let outcomes: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
    assert_eq!(outcomes.iter().filter(|outcome| matches!(outcome, Err(StorageError::TaskChanged { id }) if *id == task.id())).count(), 1);
    let successful = outcomes.into_iter().find_map(Result::ok).unwrap();
    assert_eq!(
        SqliteRepository::open(path)
            .unwrap()
            .find_task(task.id())
            .unwrap(),
        Some(successful)
    );
}
