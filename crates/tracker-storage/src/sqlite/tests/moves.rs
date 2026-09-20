// SQLite worklog move integration tests.

use super::*;

#[test]
fn moving_a_completed_worklog_preserves_it_and_returns_every_affected_snapshot_value() {
    let repository = repo();
    let source = named_task(1, "source");
    let destination = named_task(2, "destination");
    let active_task = named_task(3, "active");
    for task in [&source, &destination, &active_task] {
        repository.create_task(task.clone()).unwrap();
    }
    let earlier_source = Worklog::new(worklog_id(1), source.id(), at(100), Some(at(110))).unwrap();
    let moved = Worklog::new(worklog_id(2), source.id(), at(300), Some(at(310))).unwrap();
    let destination_work =
        Worklog::new(worklog_id(3), destination.id(), at(200), Some(at(210))).unwrap();
    let active = Worklog::begin(worklog_id(4), active_task.id(), at(500));
    for worklog in [&earlier_source, &moved, &destination_work, &active] {
        repository.insert_worklog(worklog).unwrap();
    }
    let source_updated_before: i64 = repository
        .connection()
        .query_row(
            "SELECT updated_at_us FROM tasks WHERE id = ?1",
            [source.id().to_string()],
            |row| row.get(0),
        )
        .unwrap();
    let destination_updated_before: i64 = repository
        .connection()
        .query_row(
            "SELECT updated_at_us FROM tasks WHERE id = ?1",
            [destination.id().to_string()],
            |row| row.get(0),
        )
        .unwrap();

    let movement = repository
        .compare_and_move_worklog(moved.id(), source.id(), moved.times(), destination.id())
        .unwrap();

    assert_eq!(movement.worklog.id(), moved.id());
    assert_eq!(movement.worklog.task_id(), destination.id());
    assert_eq!(movement.worklog.times(), moved.times());
    assert_eq!(movement.source_task_latest_work_start, Some(at(100)));
    assert_eq!(movement.destination_task_latest_work_start, Some(at(300)));
    assert_eq!(movement.active_worklog, Some(active));
    assert_eq!(movement.active_task_latest_work_start, Some(at(500)));
    assert_eq!(history_revision(&repository, source.id()), 1);
    assert_eq!(history_revision(&repository, destination.id()), 1);
    for (task_id, updated_at) in [
        (source.id(), source_updated_before),
        (destination.id(), destination_updated_before),
    ] {
        let stored: i64 = repository
            .connection()
            .query_row(
                "SELECT updated_at_us FROM tasks WHERE id = ?1",
                [task_id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, updated_at);
    }
}

#[test]
fn moving_an_active_worklog_keeps_it_active_on_the_destination() {
    let repository = repo();
    let source = named_task(1, "source");
    let destination = named_task(2, "destination");
    repository.create_task(source.clone()).unwrap();
    repository.create_task(destination.clone()).unwrap();
    let earlier_source = Worklog::new(worklog_id(1), source.id(), at(20), Some(at(30))).unwrap();
    let destination_work =
        Worklog::new(worklog_id(2), destination.id(), at(50), Some(at(60))).unwrap();
    let active = Worklog::begin(worklog_id(3), source.id(), at(100));
    for worklog in [&earlier_source, &destination_work, &active] {
        repository.insert_worklog(worklog).unwrap();
    }

    let movement = repository
        .compare_and_move_worklog(active.id(), source.id(), active.times(), destination.id())
        .unwrap();

    assert_eq!(movement.worklog.id(), active.id());
    assert_eq!(movement.worklog.task_id(), destination.id());
    assert_eq!(movement.worklog.times(), active.times());
    assert!(movement.worklog.is_active());
    assert_eq!(movement.source_task_latest_work_start, Some(at(20)));
    assert_eq!(movement.destination_task_latest_work_start, Some(at(100)));
    assert_eq!(movement.active_worklog, Some(movement.worklog.clone()));
    assert_eq!(movement.active_task_latest_work_start, Some(at(100)));
}

#[test]
fn move_distinguishes_missing_stale_and_unchanged_destinations() {
    let repository = repo();
    let source = named_task(1, "source");
    let destination = named_task(2, "destination");
    repository.create_task(source.clone()).unwrap();
    repository.create_task(destination.clone()).unwrap();
    let worklog = Worklog::new(worklog_id(1), source.id(), at(100), Some(at(200))).unwrap();
    repository.insert_worklog(&worklog).unwrap();

    assert!(matches!(
        repository.compare_and_move_worklog(
            worklog_id(99),
            source.id(),
            worklog.times(),
            destination.id(),
        ),
        Err(StorageError::WorklogNotFound { id }) if id == worklog_id(99)
    ));
    assert!(matches!(
        repository.compare_and_move_worklog(
            worklog.id(),
            source.id(),
            WorklogTimes::new(at(101), Some(at(200))),
            destination.id(),
        ),
        Err(StorageError::WorklogChanged { id }) if id == worklog.id()
    ));
    assert!(matches!(
        repository.compare_and_move_worklog(
            worklog.id(),
            source.id(),
            WorklogTimes::new(at(100), Some(at(201))),
            destination.id(),
        ),
        Err(StorageError::WorklogChanged { id }) if id == worklog.id()
    ));
    assert!(matches!(
        repository.compare_and_move_worklog(
            worklog.id(),
            destination.id(),
            worklog.times(),
            source.id(),
        ),
        Err(StorageError::WorklogChanged { id }) if id == worklog.id()
    ));
    assert!(matches!(
        repository.compare_and_move_worklog(
            worklog.id(),
            source.id(),
            worklog.times(),
            source.id(),
        ),
        Err(StorageError::Constraint(_))
    ));
    assert_eq!(
        repository.find_worklog(worklog.id()).unwrap(),
        Some(worklog)
    );
}

#[test]
fn move_rejects_missing_archived_and_overlapping_destinations_without_changing_the_row() {
    let repository = repo();
    let source = named_task(1, "source");
    let destination = named_task(2, "destination");
    let archived = named_task(3, "archived");
    repository.create_task(source.clone()).unwrap();
    repository.create_task(destination.clone()).unwrap();
    repository.create_task(archived.clone()).unwrap();
    let worklog = Worklog::new(worklog_id(1), source.id(), at(100), Some(at(200))).unwrap();
    let overlap = Worklog::new(worklog_id(2), destination.id(), at(150), Some(at(250))).unwrap();
    repository.insert_worklog(&worklog).unwrap();
    repository.insert_worklog(&overlap).unwrap();
    repository.archive_task(archived.id(), at(300)).unwrap();
    let missing = TaskId::from_uuid(uuid::Uuid::from_u128(99));

    assert!(matches!(
        repository.compare_and_move_worklog(worklog.id(), source.id(), worklog.times(), missing),
        Err(StorageError::TaskNotFound { id }) if id == missing
    ));
    assert!(matches!(
        repository.compare_and_move_worklog(
            worklog.id(),
            source.id(),
            worklog.times(),
            archived.id(),
        ),
        Err(StorageError::TaskArchived { id }) if id == archived.id()
    ));
    assert!(matches!(
        repository.compare_and_move_worklog(
            worklog.id(),
            source.id(),
            worklog.times(),
            destination.id(),
        ),
        Err(StorageError::SameTaskWorklogOverlap { id }) if id == worklog.id()
    ));
    assert_eq!(
        repository.find_worklog(worklog.id()).unwrap(),
        Some(worklog)
    );
}
