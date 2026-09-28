// SQLite tasks integration tests.

use super::*;

const FORTNIGHT_SECONDS: i64 = 14 * 24 * 60 * 60;

#[test]
fn inactive_preview_respects_creation_and_worklog_interval_boundaries() {
    let repository = repo();
    let as_of = at(2_000_000);
    let cutoff = 2_000_000 - FORTNIGHT_SECONDS;
    for (id, name, created) in [
        (1, "never worked", cutoff - 1),
        (2, "created at cutoff", cutoff),
        (3, "work ended at cutoff", cutoff - 100),
        (4, "work spans window", cutoff - 100),
        (5, "worked recently", cutoff - 100),
        (6, "future work", cutoff - 100),
        (7, "zero duration work", cutoff - 100),
        (8, "running work", cutoff - 100),
        (9, "renamed old task", cutoff - 100),
        (10, "archived old task", cutoff - 100),
    ] {
        repository
            .create_task(stamped_task(id, name, created, created))
            .unwrap();
    }
    repository
        .insert_worklog(
            &Worklog::new(worklog_id(3), task_id(3), at(cutoff - 20), Some(at(cutoff))).unwrap(),
        )
        .unwrap();
    repository
        .insert_worklog(
            &Worklog::new(
                worklog_id(4),
                task_id(4),
                at(cutoff - 20),
                Some(as_of + chrono::TimeDelta::seconds(1)),
            )
            .unwrap(),
        )
        .unwrap();
    repository
        .insert_worklog(
            &Worklog::new(
                worklog_id(5),
                task_id(5),
                at(as_of.timestamp() - 20),
                Some(at(as_of.timestamp() - 1)),
            )
            .unwrap(),
        )
        .unwrap();
    repository
        .insert_worklog(
            &Worklog::new(
                worklog_id(6),
                task_id(6),
                as_of,
                Some(as_of + chrono::TimeDelta::seconds(10)),
            )
            .unwrap(),
        )
        .unwrap();
    repository
        .insert_worklog(
            &Worklog::new(
                worklog_id(7),
                task_id(7),
                at(cutoff + 10),
                Some(at(cutoff + 10)),
            )
            .unwrap(),
        )
        .unwrap();
    repository
        .insert_worklog(&Worklog::begin(worklog_id(8), task_id(8), at(cutoff - 20)))
        .unwrap();
    repository
        .rename_task(
            task_id(9),
            TaskName::new("renamed recently").unwrap(),
            as_of,
        )
        .unwrap();
    repository.archive_task(task_id(10), as_of).unwrap();

    let ids: Vec<_> = repository
        .preview_inactive_tasks(as_of)
        .unwrap()
        .tasks
        .iter()
        .map(Task::id)
        .collect();
    assert_eq!(
        ids,
        vec![task_id(1), task_id(3), task_id(6), task_id(7), task_id(9)]
    );
}

#[test]
fn inactive_preview_reads_candidates_and_tracking_from_one_current_snapshot() {
    let directory = TempDir::new().unwrap();
    let reader = file_repo(&directory);
    let writer = file_repo(&directory);
    let as_of = at(2_000_000);
    let old = stamped_task(1, "old task", 100, 100);
    writer.create_task(old.clone()).unwrap();
    assert_eq!(reader.tracker_snapshot().unwrap().task_items.len(), 1);

    let recent = stamped_task(2, "recent task", 1_999_000, 1_999_000);
    writer.create_task(recent.clone()).unwrap();
    let running = Worklog::begin(worklog_id(1), old.id(), at(1_999_000));
    writer.insert_worklog(&running).unwrap();

    let preview = reader.preview_inactive_tasks(as_of).unwrap();
    assert!(preview.tasks.is_empty());
    assert_eq!(preview.snapshot.active_worklog, Some(running.clone()));
    assert_eq!(preview.snapshot.task_items.len(), 2);
    assert_eq!(preview.snapshot.task_items[1].task, recent);
    assert_eq!(
        preview.snapshot.task_items[0].latest_work_start,
        Some(running.start())
    );
}

#[test]
fn inactive_bulk_archive_rechecks_candidates_and_rolls_back_failed_writes() {
    let repository = repo();
    let as_of = at(2_000_000);
    for id in 1..=2 {
        repository
            .create_task(stamped_task(id, "old task", 100, 100))
            .unwrap();
    }
    let expected = [task_id(1), task_id(2)];
    repository
        .connection()
        .execute_batch(&format!(
            "CREATE TRIGGER reject_second_archive BEFORE UPDATE OF archived ON tasks
         WHEN NEW.id = '{}' AND NEW.archived = 1
         BEGIN SELECT RAISE(ABORT, 'injected archive failure'); END;",
            task_id(2)
        ))
        .unwrap();
    assert!(repository.archive_inactive_tasks(&expected, as_of).is_err());
    assert!(
        repository
            .list_tasks()
            .unwrap()
            .iter()
            .all(|task| !task.is_archived())
    );
    repository
        .connection()
        .execute_batch("DROP TRIGGER reject_second_archive")
        .unwrap();

    repository
        .insert_worklog(
            &Worklog::new(
                worklog_id(1),
                task_id(2),
                at(1_999_000),
                Some(at(1_999_100)),
            )
            .unwrap(),
        )
        .unwrap();
    assert!(matches!(
        repository.archive_inactive_tasks(&expected, as_of),
        Err(StorageError::InactiveTaskCandidatesChanged)
    ));
    assert!(
        repository
            .list_tasks()
            .unwrap()
            .iter()
            .all(|task| !task.is_archived())
    );

    let archive = repository
        .archive_inactive_tasks(&[task_id(1)], as_of)
        .unwrap();
    assert_eq!(archive.tasks.len(), 1);
    assert_eq!(archive.tasks[0].updated_at(), as_of);
    assert!(archive.tasks[0].is_archived());
    assert!(archive.snapshot.task_items[0].task.is_archived());
    assert!(!archive.snapshot.task_items[1].task.is_archived());

    repository
        .create_task(stamped_task(3, "newly added old task", 100, 100))
        .unwrap();
    assert!(matches!(
        repository.archive_inactive_tasks(&[], as_of),
        Err(StorageError::InactiveTaskCandidatesChanged)
    ));
    assert!(
        !repository
            .find_task(task_id(3))
            .unwrap()
            .unwrap()
            .is_archived()
    );
}

#[test]
fn sqlite_application_ports_delegate_task_and_worklog_queries() {
    let repository = repo();
    let task = stamped_task(1, "alpha", 100, 100);
    TaskRepository::create_task(&repository, task.clone()).unwrap();
    assert_eq!(repository.find_task(task.id()).unwrap(), Some(task.clone()));
    let items = repository.list_task_items().unwrap();
    assert_eq!(
        items,
        vec![TaskListItem {
            task: task.clone(),
            latest_work_start: None,
        }]
    );

    let worklog = Worklog::begin(worklog_id(1), task.id(), at(100));
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
        WorklogRepository::worklog_page(&repository, task.id(), None)
            .unwrap()
            .worklogs,
        vec![corrected]
    );

    let saved = TaskRepository::rename_task(
        &repository,
        task.id(),
        TaskName::new("renamed").unwrap(),
        at(150),
    )
    .unwrap();
    assert_eq!(saved.name().as_str(), "renamed");
    assert_eq!(saved.updated_at(), at(150));
    assert_eq!(repository.find_task(task.id()).unwrap(), Some(saved));
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
    assert_eq!(items[0].task.id(), task_id(1));
    assert_eq!(items[0].latest_work_start, Some(at(500)));
    assert_eq!(items[1].task.id(), task_id(2));
    assert_eq!(items[1].latest_work_start, None, "no worklogs means none");
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
    let started = Worklog::begin(worklog_id(1), task.id(), at(200));
    repository.insert_worklog(&started).unwrap();

    let error = TaskRepository::archive_task(&repository, task.id(), at(300))
        .expect_err("the active task must not archive");
    assert!(matches!(
        error,
        RepositoryError::TaskIsActive { id } if id == task.id()
    ));
    assert!(
        !repository
            .find_task(task.id())
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
    let archived = TaskRepository::archive_task(&repository, task.id(), at(300)).unwrap();
    assert!(archived.is_archived());
    assert_eq!(archived.updated_at(), at(300));
}

#[test]
fn an_archived_task_rejects_worklogs_at_the_database_level() {
    let repository = repo();
    let task = named_task(1, "done");
    repository.create_task(task.clone()).unwrap();
    repository.archive_task(task.id(), at(300)).unwrap();

    // A stopped worklog is also refused: archived tasks receive nothing.
    let stopped = Worklog::new(worklog_id(42), task.id(), at(100), Some(at(150))).unwrap();
    let error = repository
        .insert_worklog(&stopped)
        .expect_err("archived tasks cannot receive worklogs");
    assert!(matches!(
        error,
        StorageError::TaskArchived { id } if id == task.id()
    ));

    let active = Worklog::begin(worklog_id(43), task.id(), at(200));
    let error = repository
        .insert_worklog(&active)
        .expect_err("archived tasks cannot become active");
    assert!(matches!(
        error,
        StorageError::TaskArchived { id } if id == task.id()
    ));

    // Error state: nothing was written.
    assert!(repository.list_worklogs(task.id()).unwrap().is_empty());
    assert_eq!(repository.active_worklog().unwrap(), None);
}

#[test]
fn unarchiving_restores_a_task_keeps_its_worklogs_and_persists() {
    let temp = tempfile::tempdir().unwrap();
    let task = stamped_task(1, "done", 100, 100);
    let stopped = Worklog::new(worklog_id(10), task.id(), at(100), Some(at(150))).unwrap();

    {
        let repository = file_repo(&temp);
        repository.create_task(task.clone()).unwrap();
        repository.insert_worklog(&stopped).unwrap();
        repository.archive_task(task.id(), at(150)).unwrap();
        assert!(
            repository
                .find_task(task.id())
                .unwrap()
                .unwrap()
                .is_archived()
        );

        // A restore through the field-specific port.
        let saved = TaskRepository::unarchive_task(&repository, task.id(), at(200)).unwrap();
        assert!(!saved.is_archived());
        assert_eq!(saved.updated_at(), at(200));
        assert_eq!(saved.created_at(), at(100));
        assert_eq!(
            repository.list_worklogs(task.id()).unwrap(),
            vec![stopped.clone()]
        );

        // The trigger no longer fires: the restored task accepts worklogs.
        let resumed = Worklog::begin(worklog_id(11), task.id(), at(300));
        repository.insert_worklog(&resumed).unwrap();
        // Restoring again is a no-op for the timestamp.
        let current = TaskRepository::unarchive_task(&repository, task.id(), at(400)).unwrap();
        assert_eq!(current.updated_at(), at(200));

        repository.create_task(named_task(2, "later")).unwrap();
    }

    let reopened = file_repo(&temp);
    let stored = reopened.find_task(task.id()).unwrap().unwrap();
    assert!(!stored.is_archived());
    let worklogs = reopened.list_worklogs(task.id()).unwrap();
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
        .archive_task(task.id(), at(300))
        .expect_err("the active task must not archive");
    assert!(matches!(
        error,
        StorageError::TaskIsActive { id } if id == task.id()
    ));

    // Error state: the task is still usable and the timer still runs.
    assert!(
        !repository
            .find_task(task.id())
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
    let archived = repository.archive_task(task.id(), at(300)).unwrap();
    assert!(archived.is_archived());
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
        .insert_worklog(&Worklog::new(worklog_id(1), gamma.id(), at(400), Some(at(450))).unwrap())
        .unwrap();
    repository
        .insert_worklog(&Worklog::new(worklog_id(2), beta.id(), at(500), Some(at(550))).unwrap())
        .unwrap();
    repository
        .insert_worklog(&Worklog::new(worklog_id(3), gamma.id(), at(600), Some(at(650))).unwrap())
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
fn direct_task_move_cannot_create_a_same_task_overlap() {
    let repository = repo();
    let first = named_task(1, "first");
    let second = named_task(2, "second");
    repository.create_task(first.clone()).unwrap();
    repository.create_task(second.clone()).unwrap();
    let left = Worklog::new(worklog_id(1), first.id(), at(100), Some(at(200))).unwrap();
    let right = Worklog::new(worklog_id(2), second.id(), at(150), Some(at(250))).unwrap();
    repository.insert_worklog(&left).unwrap();
    repository.insert_worklog(&right).unwrap();

    let error = repository
        .connection()
        .execute(
            "UPDATE worklogs SET task_id = ?1 WHERE id = ?2",
            [first.id().to_string(), right.id().to_string()],
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
