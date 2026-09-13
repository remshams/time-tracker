// SQLite tracking integration tests.

use super::*;

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
        application.set_active_task(alpha.id(), at(100)).unwrap(),
        SetActiveTaskOutcome::Started { worklog } if worklog.start() == at(100)
    ));
    assert!(matches!(
        application.set_active_task(alpha.id(), at(110)).unwrap(),
        SetActiveTaskOutcome::AlreadyActive { .. }
    ));
    assert!(matches!(
        application.set_active_task(beta.id(), at(150)).unwrap(),
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
    assert_eq!(stored.list_worklogs(alpha.id()).unwrap().len(), 1);
    assert_eq!(stored.list_worklogs(beta.id()).unwrap().len(), 1);
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

    let second = Worklog::begin(worklog_id(42), task.id(), at(200));
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
    let duplicate = Worklog::new(first.id(), task.id(), at(500), Some(at(600))).unwrap();
    let error = repository
        .insert_worklog(&duplicate)
        .expect_err("a stored id must not be inserted twice");
    assert!(matches!(
        error,
        StorageError::WorklogAlreadyExists { id } if id == first.id()
    ));

    // A fresh id on the same task is reported as an interval overlap.
    let second_active = Worklog::begin(worklog_id(42), task.id(), at(200));
    let error = repository
        .insert_worklog(&second_active)
        .expect_err("two active worklogs cannot coexist");
    assert!(matches!(
        error,
        StorageError::SameTaskWorklogOverlap { id } if id == second_active.id()
    ));
    assert_eq!(repository.active_worklog().unwrap(), Some(first.clone()));
    assert_eq!(repository.list_worklogs(task.id()).unwrap().len(), 1);
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
            &Worklog::new(worklog_id(1), first_task.id(), at(100), Some(at(200))).unwrap(),
        )
        .unwrap();

    let overlap = Worklog::new(worklog_id(2), first_task.id(), at(150), Some(at(250))).unwrap();
    assert!(matches!(
        repository.insert_worklog(&overlap),
        Err(StorageError::SameTaskWorklogOverlap { id }) if id == overlap.id()
    ));

    for worklog in [
        Worklog::new(worklog_id(3), first_task.id(), at(50), Some(at(100))).unwrap(),
        Worklog::new(worklog_id(4), first_task.id(), at(200), Some(at(250))).unwrap(),
        Worklog::new(worklog_id(5), first_task.id(), at(150), Some(at(150))).unwrap(),
        Worklog::new(worklog_id(6), second_task.id(), at(150), Some(at(250))).unwrap(),
    ] {
        repository.insert_worklog(&worklog).unwrap();
    }
    assert_eq!(repository.list_worklogs(first_task.id()).unwrap().len(), 4);
    assert_eq!(repository.list_worklogs(second_task.id()).unwrap().len(), 1);
}

#[test]
fn an_active_interval_overlaps_every_later_interval_on_the_same_task() {
    let repository = repo();
    let task = named_task(1, "running");
    repository.create_task(task.clone()).unwrap();
    let active = Worklog::begin(worklog_id(1), task.id(), at(100));
    repository.insert_worklog(&active).unwrap();

    let later = Worklog::new(worklog_id(2), task.id(), at(200), Some(at(300))).unwrap();
    assert!(matches!(
        repository.insert_worklog(&later),
        Err(StorageError::SameTaskWorklogOverlap { id }) if id == later.id()
    ));
    assert_eq!(repository.active_worklog().unwrap(), Some(active));
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
    let next = Worklog::begin(worklog_id(42), rest.id(), at(160));
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
    let next = Worklog::begin(worklog_id(42), rest.id(), at(160));
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

    let next = Worklog::begin(worklog_id(42), rest.id(), at(160));
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
    repository.archive_task(rest.id(), at(150)).unwrap();

    let next = Worklog::begin(worklog_id(42), rest.id(), at(160));
    let error = repository
        .switch_worklog(started.id(), started.start(), at(150), &next)
        .expect_err("the archived target must refuse the switch");
    assert!(matches!(
        error,
        StorageError::TaskArchived { id } if id == rest.id()
    ));

    // Rollback: the old worklog is still active, the archived task received
    // nothing.
    assert_eq!(
        repository.active_worklog().unwrap().unwrap().id(),
        started.id()
    );
    assert_eq!(repository.list_worklogs(rest.id()).unwrap(), Vec::new());
    let old = repository.list_worklogs(work.id()).unwrap();
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
    let old_rest = Worklog::new(worklog_id(42), rest.id(), at(10), Some(at(20))).unwrap();
    repository.insert_worklog(&old_rest).unwrap();

    // The new worklog reuses the stored id of the old rest worklog.
    let next = Worklog::begin(worklog_id(42), rest.id(), at(160));
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
    let rest_worklogs = repository.list_worklogs(rest.id()).unwrap();
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
        tracker.ensure_archivable(work.id()),
        Err(TrackingError::TaskIsActive { id: work.id() })
    );
    // The task stays usable and unarchived after the rejection.
    assert!(
        !repository
            .find_task(work.id())
            .unwrap()
            .unwrap()
            .is_archived()
    );
    assert_eq!(repository.active_worklog().unwrap(), Some(started.clone()));

    // A different task archives fine while tracking runs.
    tracker.ensure_archivable(other.id()).unwrap();
    repository.archive_task(other.id(), at(120)).unwrap();
    assert!(
        repository
            .find_task(other.id())
            .unwrap()
            .unwrap()
            .is_archived()
    );

    // Once the worklog is stopped, the task can be archived.
    tracker.stop(at(150)).unwrap();
    repository
        .stop_worklog(started.id(), started.start(), at(150))
        .unwrap();
    tracker.ensure_archivable(work.id()).unwrap();
    repository.archive_task(work.id(), at(160)).unwrap();
    assert!(
        repository
            .find_task(work.id())
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
        Err(TrackingError::TaskArchived { id: archived.id() })
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
        assert_eq!(recovered.task_id(), task.id());
        assert_eq!(recovered.start(), at(100));
        assert_eq!(recovered.end(), None);

        // The recovered worklog resumes tracking and stops cleanly.
        let mut tracker = Tracker::resume(recovered).unwrap();
        assert_eq!(
            tracker.state(),
            &TrackingState::Running {
                worklog: ActiveWorklog::begin(worklog_id, task.id(), at(100))
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
    let worklogs = repository.list_worklogs(task.id()).unwrap();
    assert_eq!(worklogs.len(), 1);
    assert_eq!(worklogs[0].start(), at(100));
    assert_eq!(worklogs[0].end(), Some(at(250)));
}

#[test]
fn stop_and_switch_report_an_active_start_compare_failure() {
    let repository = repo();
    let alpha = named_task(1, "alpha");
    let beta = named_task(2, "beta");
    repository.create_task(alpha.clone()).unwrap();
    repository.create_task(beta.clone()).unwrap();
    let active = Worklog::begin(worklog_id(1), alpha.id(), at(100));
    repository.insert_worklog(&active).unwrap();
    assert!(matches!(
        repository.stop_worklog(active.id(), at(50), at(150)),
        Err(StorageError::WorklogChanged { id }) if id == active.id()
    ));
    let next = Worklog::begin(worklog_id(2), beta.id(), at(150));
    assert!(matches!(
        repository.switch_worklog(active.id(), at(50), at(150), &next),
        Err(StorageError::WorklogChanged { id }) if id == active.id()
    ));
    assert_eq!(repository.active_worklog().unwrap(), Some(active));
}
