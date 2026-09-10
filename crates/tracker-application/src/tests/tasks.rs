use super::*;

#[test]
fn create_task_stamps_the_client_timestamp_on_both_values() {
    let repository = MemoryRepository::default();
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    let created = application
        .create_task(TaskName::new("alpha").unwrap(), at(500))
        .unwrap();
    assert_eq!(created.created_at(), at(500));
    assert_eq!(created.updated_at(), at(500));
    assert_eq!(application.task(created.id()), Some(&created));
    assert_eq!(
        application
            .tasks(TaskOrdering::RecentlyCreated)
            .into_iter()
            .map(|item| item.task)
            .collect::<Vec<_>>(),
        std::slice::from_ref(&created)
    );
}

#[test]
fn client_timestamps_are_canonicalized_to_microseconds() {
    let repository = MemoryRepository::default();
    let mut application = TrackerApplication::load(repository).unwrap();
    let created = application
        .create_task(
            TaskName::new("precise").unwrap(),
            at_nanos(100, 123_456_789),
        )
        .unwrap();
    assert_eq!(created.created_at(), at_nanos(100, 123_456_000));

    let renamed = application
        .rename_task(
            created.id(),
            TaskName::new("canonical").unwrap(),
            at_nanos(100, 123_456_100),
        )
        .unwrap();
    assert!(renamed.updated_at() == at_nanos(100, 123_456_000));

    let started = application
        .set_active_task(created.id(), at_nanos(200, 987_654_321))
        .unwrap();
    assert!(
        matches!(started, SetActiveTaskOutcome::Started { worklog } if worklog.start() == at_nanos(200, 987_654_000))
    );
}

#[test]
fn task_operations_return_stored_outcomes_and_update_queries() {
    let repository = MemoryRepository::default();
    let mut application = TrackerApplication::load(repository).unwrap();
    let created = application
        .create_task(TaskName::new("alpha").unwrap(), at(100))
        .unwrap();
    assert_eq!(application.task(created.id()), Some(&created));
    let renamed = application
        .rename_task(created.id(), TaskName::new("beta").unwrap(), at(200))
        .unwrap();
    assert!(renamed.name().as_str() == "beta" && renamed.updated_at() == at(200));
    let archived = application.archive_task(created.id(), at(300)).unwrap();
    assert!(archived.is_archived() && archived.updated_at() == at(300));
    assert!(application.task(created.id()).unwrap().is_archived());
}

#[test]
fn renaming_to_the_stored_name_keeps_updated_at() {
    let alpha = stamped_task(1, "alpha", 100, 200);
    let repository = MemoryRepository::with_tasks(vec![alpha]);
    let mut application = TrackerApplication::load(repository).unwrap();

    let renamed = application
        .rename_task(
            TaskId::from_uuid(uuid::Uuid::from_u128(1)),
            TaskName::new("alpha").unwrap(),
            at(900),
        )
        .unwrap();
    assert_eq!(renamed.name().as_str(), "alpha");
    assert_eq!(
        renamed.updated_at(),
        at(200),
        "a no-op rename does not advance"
    );
    assert_eq!(
        application
            .task(TaskId::from_uuid(uuid::Uuid::from_u128(1)))
            .unwrap()
            .updated_at(),
        at(200)
    );
}

#[test]
fn metadata_operations_never_move_updated_at_backward() {
    let alpha = stamped_task(1, "alpha", 100, 500);
    let repository = MemoryRepository::with_tasks(vec![alpha]);
    let mut application = TrackerApplication::load(repository).unwrap();
    let id = TaskId::from_uuid(uuid::Uuid::from_u128(1));

    let renamed = application
        .rename_task(id, TaskName::new("beta").unwrap(), at(200))
        .unwrap();
    assert!(
        renamed.updated_at() == at(500),
        "a late client clock cannot rewind updated_at"
    );
}

#[test]
fn archiving_an_archived_task_and_restoring_an_active_task_are_noops() {
    let mut archived = stamped_task(1, "archived", 100, 400);
    assert!(archived.archive(at(400)));
    let active = stamped_task(2, "active", 100, 300);
    let repository = MemoryRepository::with_tasks(vec![archived, active]);
    let mut application = TrackerApplication::load(repository).unwrap();

    let again = application
        .archive_task(TaskId::from_uuid(uuid::Uuid::from_u128(1)), at(900))
        .unwrap();
    assert!(again.updated_at() == at(400));
    let restore = application
        .unarchive_task(TaskId::from_uuid(uuid::Uuid::from_u128(2)), at(900))
        .unwrap();
    assert!(!restore.is_archived() && restore.updated_at() == at(300));
    assert_eq!(
        application
            .task(TaskId::from_uuid(uuid::Uuid::from_u128(1)))
            .unwrap()
            .updated_at(),
        at(400)
    );
    assert_eq!(
        application
            .task(TaskId::from_uuid(uuid::Uuid::from_u128(2)))
            .unwrap()
            .updated_at(),
        at(300)
    );
}

#[test]
fn committed_task_writes_report_a_failed_tracking_refresh() {
    let alpha = stamped_task(1, "alpha", 100, 100);
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    repository.fail_refresh_after_next_task_write();

    assert_eq!(
        application.archive_task(alpha.id(), at(200)),
        Err(ApplicationError::TaskRecovery(RepositoryError::Backend {
            message: "read failed".to_owned()
        }))
    );
    assert!(application.task(alpha.id()).unwrap().is_archived());
    assert!(repository.0.borrow().tasks[0].task.is_archived());

    repository.0.borrow_mut().fail_reads = false;
    repository.fail_refresh_after_next_task_write();
    assert_eq!(
        application.unarchive_task(alpha.id(), at(300)),
        Err(ApplicationError::TaskRecovery(RepositoryError::Backend {
            message: "read failed".to_owned()
        }))
    );
    assert!(!application.task(alpha.id()).unwrap().is_archived());
    assert!(!repository.0.borrow().tasks[0].task.is_archived());
}

#[test]
fn unarchive_task_restores_the_task_and_keeps_its_worklogs() {
    let alpha = task(1, "alpha");
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    repository.0.borrow_mut().worklogs.push(
        Worklog::new(
            worklog(10, alpha.id(), 100).id(),
            alpha.id(),
            at(100),
            Some(at(150)),
        )
        .unwrap(),
    );
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    application.archive_task(alpha.id(), at(200)).unwrap();

    let unarchived = application.unarchive_task(alpha.id(), at(300)).unwrap();
    assert!(!unarchived.is_archived() && unarchived.updated_at() == at(300));
    assert!(!application.task(alpha.id()).unwrap().is_archived());
    assert_eq!(
        application
            .worklogs_for_task(alpha.id(), None)
            .unwrap()
            .worklogs
            .len(),
        1
    );
    assert!(!repository.0.borrow().tasks[0].task.is_archived());
}

#[test]
fn unarchiving_a_missing_task_reports_task_not_found() {
    let repository = MemoryRepository::default();
    let mut application = TrackerApplication::load(repository).unwrap();
    let missing = TaskId::from_uuid(uuid::Uuid::from_u128(99));
    assert_eq!(
        application.unarchive_task(missing, at(100)),
        Err(ApplicationError::Repository(
            RepositoryError::TaskNotFound { id: missing }
        ))
    );
}

#[test]
fn unarchive_loads_a_worklog_that_another_client_started_first() {
    let alpha = task(1, "alpha");
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    application.archive_task(alpha.id(), at(150)).unwrap();

    // A second client restores the archived task and starts tracking it
    // before the stale client issues its unarchive.
    repository
        .0
        .borrow_mut()
        .tasks
        .iter_mut()
        .find(|item| item.task.id() == alpha.id())
        .unwrap()
        .task
        .restore(at(160));
    repository
        .insert_worklog(&worklog(10, alpha.id(), 150))
        .unwrap();

    let unarchived = application.unarchive_task(alpha.id(), at(200)).unwrap();
    assert!(!unarchived.is_archived());
    assert_eq!(
        application.current_tracking(),
        &TrackingState::Running {
            worklog: ActiveWorklog::begin(
                WorklogId::from_uuid(uuid::Uuid::from_u128(10)),
                alpha.id(),
                at(150)
            )
        }
    );
    assert!(matches!(
        repository.active_worklog(),
        Ok(Some(worklog)) if worklog.task_id() == alpha.id()
    ));
}

#[test]
fn a_failing_tracking_refresh_prevents_the_unarchive_write() {
    let alpha = task(1, "alpha");
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    application.archive_task(alpha.id(), at(100)).unwrap();
    repository.0.borrow_mut().fail_reads = true;

    assert_eq!(
        application.unarchive_task(alpha.id(), at(150)),
        Err(ApplicationError::Repository(RepositoryError::Backend {
            message: "read failed".to_owned()
        }))
    );
    assert!(repository.0.borrow().tasks[0].task.is_archived());
    assert_eq!(application.current_tracking(), &TrackingState::Idle);
}

#[test]
fn unarchiving_an_unarchived_task_is_a_harmless_noop() {
    let alpha = task(1, "alpha");
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    let first = application.unarchive_task(alpha.id(), at(100)).unwrap();
    assert!(!first.is_archived());
    assert!(!application.task(alpha.id()).unwrap().is_archived());
    let second = application.unarchive_task(alpha.id(), at(200)).unwrap();
    assert!(!second.is_archived());
    assert_eq!(repository.0.borrow().tasks.len(), 1);
}

#[test]
fn archiving_the_active_task_is_rejected_before_persistence() {
    let alpha = task(1, "alpha");
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    let mut application = TrackerApplication::load(repository).unwrap();
    application.set_active_task(alpha.id(), at(100)).unwrap();
    assert_eq!(
        application.archive_task(alpha.id(), at(150)),
        Err(ApplicationError::Domain(TrackingError::TaskIsActive {
            id: alpha.id()
        }))
    );
    assert!(!application.task(alpha.id()).unwrap().is_archived());
}

#[test]
fn rename_task_never_moves_stored_updated_at_backward() {
    let alpha = stamped_task(1, "alpha", 100, 500);
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    let saved = repository
        .rename_task(alpha.id(), TaskName::new("older clock").unwrap(), at(200))
        .unwrap();
    assert_eq!(
        saved.updated_at(),
        at(500),
        "the backend keeps the newer value"
    );
    assert_eq!(saved.name().as_str(), "older clock");
    assert_eq!(saved.created_at(), at(100), "creation is never rewritten");
}

#[test]
fn renaming_a_missing_task_reports_task_not_found() {
    let repository = MemoryRepository::default();
    let missing = TaskId::from_uuid(uuid::Uuid::from_u128(99));
    assert_eq!(
        repository.rename_task(missing, TaskName::new("ghost").unwrap(), at(100)),
        Err(RepositoryError::TaskNotFound { id: missing })
    );
}

#[test]
fn renaming_after_a_concurrent_archive_keeps_the_archived_state() {
    let alpha = stamped_task(1, "alpha", 100, 100);
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    // Another client archives the task after this client last looked.
    repository.archive_task(alpha.id(), at(150)).unwrap();

    // The stale client renames from its unarchived view: the rename
    // lands and the archive state survives it.
    let renamed = repository
        .rename_task(alpha.id(), TaskName::new("beta").unwrap(), at(200))
        .unwrap();
    assert_eq!(renamed.name().as_str(), "beta");
    assert!(
        renamed.is_archived(),
        "the rename did not resurrect the task"
    );
}

#[test]
fn archiving_after_a_concurrent_rename_keeps_the_name() {
    let alpha = stamped_task(1, "alpha", 100, 100);
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    repository
        .rename_task(alpha.id(), TaskName::new("beta").unwrap(), at(150))
        .unwrap();

    let archived = repository.archive_task(alpha.id(), at(200)).unwrap();
    assert!(archived.is_archived());
    assert_eq!(
        archived.name().as_str(),
        "beta",
        "the archive kept the rename"
    );
    assert_eq!(archived.updated_at(), at(200));
}
