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
    assert_eq!(user_version(&repository), 5, "migrations ran exactly once");
    assert_eq!(repository.list_tasks().unwrap().len(), 8);
}

#[test]
fn two_clients_recover_lost_start_switch_and_stale_clear_without_stopping_the_other_timer() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let alpha = named_task(1, "alpha");
    let beta = named_task(2, "beta");
    let alpha_id = alpha.id();
    let beta_id = beta.id();
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
            second.set_active_task(beta_id, at(100)).unwrap();
            started
                .send(())
                .expect("the first client must still await the competing start");
            second
        }
    });
    let first = thread::spawn(move || {
        let mut first = first;
        let outcome = first.set_active_task(alpha_id, at(110));
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
        TrackingState::Running { worklog } if worklog.task_id() == beta_id
    ));

    assert!(matches!(
        first.set_active_task(alpha_id, at(120)),
        Ok(SetActiveTaskOutcome::Switched { stopped, started })
            if stopped.task_id() == beta_id
                && stopped.end() == Some(at(120))
                && started.task_id() == alpha_id
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
        TrackingState::Running { worklog } if worklog.task_id() == alpha_id
    ));

    let other = SqliteRepository::open(&path).unwrap();
    other.archive_task(beta_id, at(130)).unwrap();
    assert!(matches!(
        first.set_active_task(beta_id, at(140)),
        Err(ApplicationError::Domain(TrackingError::TaskArchived { id })) if id == beta_id
    ));
    assert!(matches!(
        first.current_tracking(),
        TrackingState::Running { worklog } if worklog.task_id() == alpha_id
    ));
    assert!(first.task(beta_id).unwrap().is_archived());
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
fn restore_refreshes_tracking_started_between_its_initial_read_and_write() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let task = named_task(1, "restored");
    let task_id = task.id();
    let setup = SqliteRepository::open(&path).unwrap();
    setup.create_task(task.clone()).unwrap();
    setup.archive_task(task_id, at(120)).unwrap();
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
            application.unarchive_task(task_id, at(150)).unwrap();
            let worklog = match application.set_active_task(task_id, at(200)).unwrap() {
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
        let outcome = application.unarchive_task(task_id, at(250));
        (application, outcome)
    })
    .join()
    .unwrap();
    let worklog = competing.join().unwrap();

    assert!(matches!(outcome, Ok(ref restored) if !restored.is_archived()));
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
    let task_id = task.id();
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
            let worklog = match application.set_active_task(task_id, at(200)).unwrap() {
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
        let outcome = application.archive_task(task_id, at(250));
        (application, outcome)
    })
    .join()
    .unwrap();
    let worklog = competing.join().unwrap();

    assert_eq!(
        outcome,
        Err(ApplicationError::Domain(TrackingError::TaskIsActive {
            id: task_id
        }))
    );
    assert!(matches!(
        application.current_tracking(),
        TrackingState::Running { worklog: active } if active.id() == worklog.id()
    ));
    assert!(
        !SqliteRepository::open(&path)
            .unwrap()
            .find_task(task_id)
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
    let active = Worklog::begin(worklog_id(1), alpha.id(), at(100));
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
    let corrected = Worklog::begin(active.id(), alpha.id(), at(120));
    assert_eq!(
        application.current_tracking(),
        &TrackingState::Running {
            worklog: ActiveWorklog::begin(active.id(), alpha.id(), at(120))
        }
    );
    assert_eq!(
        application
            .tasks(TaskOrdering::RecentlyWorked)
            .into_iter()
            .find(|item| item.task.id() == alpha.id())
            .unwrap()
            .latest_work_start,
        Some(at(120))
    );
    let stored = SqliteRepository::open(&path).unwrap();
    assert_eq!(stored.list_worklogs(alpha.id()).unwrap(), vec![corrected]);
    assert_eq!(
        stored.active_worklog().unwrap(),
        Some(Worklog::begin(active.id(), alpha.id(), at(120)))
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
    let active = Worklog::begin(worklog_id(1), alpha.id(), at(100));
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
    let beta_id = beta.id();
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
            worklog: ActiveWorklog::begin(active.id(), alpha.id(), at(120))
        }
    );
    let items = application.tasks(TaskOrdering::RecentlyWorked);
    assert_eq!(
        items
            .iter()
            .find(|item| item.task.id() == alpha.id())
            .unwrap()
            .latest_work_start,
        Some(at(120))
    );
    assert_eq!(
        items
            .iter()
            .find(|item| item.task.id() == beta.id())
            .unwrap()
            .latest_work_start,
        None
    );
    let stored = SqliteRepository::open(&path).unwrap();
    assert_eq!(
        stored.list_worklogs(alpha.id()).unwrap(),
        vec![Worklog::begin(active.id(), alpha.id(), at(120))]
    );
    assert_eq!(stored.list_worklogs(beta.id()).unwrap(), Vec::new());

    assert!(matches!(
        application.set_active_task(beta.id(), at(200)),
        Ok(SetActiveTaskOutcome::Switched { stopped, started })
            if stopped.start() == at(120)
                && stopped.end() == Some(at(200))
                && started.task_id() == beta.id()
                && started.start() == at(200)
    ));
}

#[test]
fn a_real_sqlite_write_conflict_reports_a_failed_recovery_separately() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let alpha = named_task(1, "alpha");
    let beta = named_task(2, "beta");
    let alpha_id = alpha.id();
    let beta_id = beta.id();
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
            competing.set_active_task(beta_id, at(100)).unwrap();
            fail_next_active_read.store(true, Ordering::SeqCst);
            started
                .send(())
                .expect("the first client must still await the competing start");
        }
    });
    let (application, error) = thread::spawn(move || {
        let mut application = application;
        let error = application
            .set_active_task(alpha_id, at(110))
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
        Ok(Some(worklog)) if worklog.task_id() == beta_id
    ));
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
fn concurrent_clients_cannot_insert_overlapping_worklogs_for_one_task() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let setup = SqliteRepository::open(&path).unwrap();
    let task = named_task(1, "shared");
    let task_id = task.id();
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
fn delete_wins_a_two_connection_race_against_correction() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let setup = SqliteRepository::open(&path).unwrap();
    let task = named_task(1, "shared");
    let target = Worklog::new(worklog_id(1), task.id(), at(100), Some(at(150))).unwrap();
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
    let target = Worklog::new(worklog_id(1), task.id(), at(100), Some(at(150))).unwrap();
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
fn concurrent_corrections_use_two_connections_and_one_stale_loser() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let setup = SqliteRepository::open(&path).unwrap();
    let task = named_task(1, "shared");
    let original = Worklog::new(worklog_id(1), task.id(), at(100), Some(at(200))).unwrap();
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
