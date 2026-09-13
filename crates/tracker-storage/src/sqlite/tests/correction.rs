// SQLite correction integration tests.

use super::*;

#[test]
fn find_and_compare_and_set_correct_a_completed_worklog_without_changing_identity() {
    let repository = repo();
    let task = named_task(1, "editable");
    repository.create_task(task.clone()).unwrap();
    let original = Worklog::new(worklog_id(1), task.id(), at(100), Some(at(200))).unwrap();
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
    assert_eq!(corrected.task_id(), task.id());
    assert_eq!(corrected.start(), at(120));
    assert_eq!(corrected.end(), Some(at(220)));
    assert_eq!(
        repository.find_worklog(original.id()).unwrap(),
        Some(corrected)
    );

    let active = Worklog::begin(worklog_id(2), task.id(), at(300));
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
    let active = Worklog::begin(worklog_id(1), active_task.id(), at(600));
    let corrected =
        Worklog::new(worklog_id(2), corrected_task.id(), at(400), Some(at(410))).unwrap();
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
    let active = Worklog::begin(worklog_id(1), task.id(), at(600));
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
    let completed = Worklog::new(worklog_id(1), task.id(), at(100), Some(at(200))).unwrap();
    let active = Worklog::begin(worklog_id(2), task.id(), at(200));
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
    let completed = Worklog::new(worklog_id(1), task.id(), at(50), Some(at(100))).unwrap();
    let active = Worklog::begin(worklog_id(2), task.id(), at(100));
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
    let fixed = Worklog::new(worklog_id(1), task.id(), at(100), Some(at(200))).unwrap();
    let editable = Worklog::new(worklog_id(2), task.id(), at(300), Some(at(400))).unwrap();
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
    let original = Worklog::new(worklog_id(1), task.id(), at(100), Some(at(200))).unwrap();
    repository.insert_worklog(&original).unwrap();
    repository.archive_task(task.id(), at(300)).unwrap();

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
            .find_task(task.id())
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
    let original = Worklog::new(worklog_id(1), task.id(), at(100), Some(at(200))).unwrap();
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
