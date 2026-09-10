// SQLite deletion integration tests.

use super::*;

#[test]
fn completed_deletion_returns_the_deleted_row_and_latest_task_aggregate() {
    let repository = repo();
    let task = named_task(1, "history");
    repository.create_task(task.clone()).unwrap();
    let earlier = Worklog::new(worklog_id(1), task.id(), at(100), Some(at(150))).unwrap();
    let latest = Worklog::new(worklog_id(2), task.id(), at(300), Some(at(350))).unwrap();
    repository.insert_worklog(&earlier).unwrap();
    repository.insert_worklog(&latest).unwrap();

    let deletion = repository
        .compare_and_delete_completed_worklog(latest.id(), task.id(), latest.times())
        .unwrap();

    assert_eq!(deletion.worklog, latest);
    assert_eq!(deletion.task_latest_work_start, Some(at(100)));
    assert_eq!(repository.list_worklogs(task.id()).unwrap(), [earlier]);
    assert_eq!(repository.active_worklog().unwrap(), None);
}

#[test]
fn active_deletion_is_rejected_by_the_repository_and_direct_sql() {
    let repository = repo();
    let task = named_task(1, "running");
    repository.create_task(task.clone()).unwrap();
    let completed = Worklog::new(worklog_id(1), task.id(), at(50), Some(at(75))).unwrap();
    let active = Worklog::begin(worklog_id(2), task.id(), at(100));
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
    let target = Worklog::new(worklog_id(1), alpha.id(), at(100), Some(at(150))).unwrap();
    repository.insert_worklog(&target).unwrap();

    assert!(matches!(
        repository.compare_and_delete_completed_worklog(
            worklog_id(99),
            alpha.id(),
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
            repository.compare_and_delete_completed_worklog(target.id(), alpha.id(), expected),
            Err(StorageError::WorklogChanged { id }) if id == target.id()
        ));
    }

    repository
        .connection()
        .execute(
            "UPDATE worklogs SET task_id = ?1 WHERE id = ?2",
            [beta.id().to_string(), target.id().to_string()],
        )
        .unwrap();
    assert!(matches!(
        repository.compare_and_delete_completed_worklog(
            target.id(),
            alpha.id(),
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
    let target = Worklog::new(worklog_id(1), task.id(), at(100), Some(at(150))).unwrap();
    repository.insert_worklog(&target).unwrap();
    repository.archive_task(task.id(), at(200)).unwrap();

    let deletion = repository
        .compare_and_delete_completed_worklog(target.id(), task.id(), target.times())
        .unwrap();

    assert_eq!(deletion.task_latest_work_start, None);
    assert!(
        repository
            .find_task(task.id())
            .unwrap()
            .unwrap()
            .is_archived()
    );
    assert!(repository.list_worklogs(task.id()).unwrap().is_empty());
}

#[test]
fn completed_deletion_persists_after_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let task = named_task(1, "persistent");
    let target = Worklog::new(worklog_id(1), task.id(), at(100), Some(at(150))).unwrap();
    {
        let repository = SqliteRepository::open(&path).unwrap();
        repository.create_task(task.clone()).unwrap();
        repository.insert_worklog(&target).unwrap();
        repository
            .compare_and_delete_completed_worklog(target.id(), task.id(), target.times())
            .unwrap();
    }

    let reopened = SqliteRepository::open(&path).unwrap();
    assert_eq!(reopened.find_worklog(target.id()).unwrap(), None);
    assert!(reopened.list_worklogs(task.id()).unwrap().is_empty());
}

#[test]
fn deletion_preserves_existing_history_cursors_and_revisions() {
    let repository = repo();
    let task = named_task(1, "history");
    repository.create_task(task.clone()).unwrap();
    insert_numbered_worklogs(&repository, &task, 55);
    let first = repository.worklog_page(task.id(), None).unwrap();
    let cursor = first.next_cursor.unwrap();
    let target = repository.find_worklog(cursor.id).unwrap().unwrap();
    let revision = history_revision(&repository, task.id());

    repository
        .compare_and_delete_completed_worklog(target.id(), task.id(), target.times())
        .unwrap();

    assert_eq!(history_revision(&repository, task.id()), revision);
    let continuation = repository.worklog_page(task.id(), Some(&cursor)).unwrap();
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
    let target = Worklog::new(worklog_id(1), task.id(), at(100), Some(at(150))).unwrap();
    repository.insert_worklog(&target).unwrap();
    repository
        .connection()
        .execute(
            "INSERT INTO worklogs (id, task_id, start_us, end_us)
             VALUES (?1, ?2, ?3, ?3)",
            rusqlite::params![worklog_id(2).to_string(), task.id().to_string(), i64::MAX,],
        )
        .unwrap();

    assert!(matches!(
        repository.compare_and_delete_completed_worklog(target.id(), task.id(), target.times()),
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
    let target = Worklog::new(worklog_id(1), task.id(), at(100), Some(at(150))).unwrap();
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
