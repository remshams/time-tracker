use super::*;

#[test]
fn set_active_task_uses_the_explicit_client_timestamp() {
    let alpha = task(1, "alpha");
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    let outcome = application.set_active_task(alpha.id(), at(100)).unwrap();
    assert!(matches!(
        outcome,
        SetActiveTaskOutcome::Started { worklog } if worklog.start() == at(100)
    ));
    assert_eq!(
        repository.active_worklog().unwrap().unwrap().start(),
        at(100)
    );
}

#[test]
fn setting_the_active_task_again_is_a_harmless_noop() {
    let alpha = task(1, "alpha");
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    application.set_active_task(alpha.id(), at(100)).unwrap();
    let first = repository.active_worklog().unwrap().unwrap();
    let outcome = application.set_active_task(alpha.id(), at(999)).unwrap();
    assert!(matches!(
        outcome,
        SetActiveTaskOutcome::AlreadyActive { worklog } if worklog.id() == first.id()
    ));
    assert_eq!(repository.0.borrow().worklogs.len(), 1);
}

#[test]
fn adopting_another_clients_active_work_updates_recent_work_ordering() {
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    let repository = MemoryRepository::with_tasks(vec![alpha, beta.clone()]);
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    repository
        .insert_worklog(&worklog(10, beta.id(), 900))
        .unwrap();

    let outcome = application.set_active_task(beta.id(), at(999)).unwrap();
    assert!(matches!(
        outcome,
        SetActiveTaskOutcome::AlreadyActive { worklog } if worklog.start() == at(900)
    ));
    assert_eq!(
        ordered_names(&application, TaskOrdering::RecentlyWorked),
        ["beta".to_owned(), "alpha".to_owned()]
    );
}

#[test]
fn setting_another_task_switches_at_one_atomic_boundary() {
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone()]);
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    application.set_active_task(alpha.id(), at(100)).unwrap();
    let outcome = application.set_active_task(beta.id(), at(150)).unwrap();
    assert!(matches!(
        outcome,
        SetActiveTaskOutcome::Switched { stopped, started }
            if stopped.end() == Some(at(150)) && started.start() == at(150)
    ));
    let data = repository.0.borrow();
    assert_eq!(data.worklogs.len(), 2);
    assert_eq!(
        data.worklogs
            .iter()
            .filter(|item| item.end().is_none())
            .count(),
        1
    );
    assert_eq!(
        data.worklogs
            .iter()
            .find(|item| item.task_id() == alpha.id())
            .unwrap()
            .end(),
        Some(at(150))
    );
}

#[test]
fn clear_active_task_stops_once_and_is_then_a_harmless_noop() {
    let alpha = task(1, "alpha");
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    application.set_active_task(alpha.id(), at(100)).unwrap();
    let TrackingState::Running { worklog: active } = application.current_tracking() else {
        panic!("the worklog must be active");
    };
    let active = active.id();
    let outcome = application.clear_active_task(active, at(150)).unwrap();
    assert!(matches!(
        outcome,
        ClearActiveTaskOutcome::Stopped { worklog } if worklog.end() == Some(at(150))
    ));
    assert_eq!(
        application.clear_active_task(active, at(200)).unwrap(),
        ClearActiveTaskOutcome::AlreadyIdle
    );
    assert_eq!(repository.0.borrow().worklogs.len(), 1);
}

#[test]
fn stale_clear_refreshes_state_without_stopping_another_clients_worklog() {
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone()]);
    let mut first = TrackerApplication::load(repository.clone()).unwrap();
    first.set_active_task(alpha.id(), at(100)).unwrap();
    let expected = match first.current_tracking() {
        TrackingState::Running { worklog } => worklog.id(),
        TrackingState::Idle => panic!("alpha must be active"),
    };
    let mut second = TrackerApplication::load(repository.clone()).unwrap();
    second.set_active_task(beta.id(), at(150)).unwrap();

    assert_eq!(
        first.clear_active_task(expected, at(200)),
        Err(ApplicationError::TrackingStateChanged)
    );
    assert!(matches!(
        first.current_tracking(),
        TrackingState::Running { worklog } if worklog.task_id() == beta.id()
    ));
    assert_eq!(
        ordered_names(&first, TaskOrdering::RecentlyWorked),
        ["beta".to_owned(), "alpha".to_owned()],
        "the adopted active worklog supplies authoritative recent activity"
    );
    assert!(matches!(
        repository.active_worklog(),
        Ok(Some(worklog)) if worklog.task_id() == beta.id()
    ));
}

#[test]
fn tracking_never_changes_a_tasks_updated_at() {
    let alpha = stamped_task(1, "alpha", 100, 200);
    let beta = stamped_task(2, "beta", 100, 200);
    let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone()]);
    let mut application = TrackerApplication::load(repository).unwrap();

    application.set_active_task(alpha.id(), at(500)).unwrap();
    application.set_active_task(beta.id(), at(600)).unwrap();
    application
        .clear_active_task(
            match application.current_tracking() {
                TrackingState::Running { worklog } => worklog.id(),
                TrackingState::Idle => panic!("beta must be active"),
            },
            at(700),
        )
        .unwrap();

    assert_eq!(application.task(alpha.id()).unwrap().updated_at(), at(200));
    assert_eq!(application.task(beta.id()).unwrap().updated_at(), at(200));
}

#[test]
fn start_and_switch_reorder_recently_worked_but_stop_does_not() {
    let alpha = stamped_task(1, "alpha", 100, 100);
    let beta = stamped_task(2, "beta", 200, 200);
    let gamma = stamped_task(3, "gamma", 300, 300);
    let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone(), gamma.clone()]);
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    // No work has happened yet, so created_at descending decides:
    // gamma first, alpha last.
    assert_eq!(
        ordered_names(&application, TaskOrdering::RecentlyWorked),
        ["gamma".to_owned(), "beta".to_owned(), "alpha".to_owned()]
    );

    // Starting alpha works it most recently: alpha rises to the top.
    application.set_active_task(alpha.id(), at(500)).unwrap();
    assert_eq!(
        ordered_names(&application, TaskOrdering::RecentlyWorked),
        ["alpha".to_owned(), "gamma".to_owned(), "beta".to_owned()]
    );

    // Switching to beta makes beta the latest work: beta rises above
    // alpha without a backend read.
    let list_reads_before = repository.0.borrow().list_reads;
    application.set_active_task(beta.id(), at(600)).unwrap();
    assert_eq!(
        ordered_names(&application, TaskOrdering::RecentlyWorked),
        ["beta".to_owned(), "alpha".to_owned(), "gamma".to_owned()]
    );
    assert_eq!(
        repository.0.borrow().list_reads,
        list_reads_before,
        "the snapshot reordered without rereading the backend"
    );

    // Stopping changes nothing about the ordering.
    application
        .clear_active_task(
            match application.current_tracking() {
                TrackingState::Running { worklog } => worklog.id(),
                TrackingState::Idle => panic!("beta must be active"),
            },
            at(700),
        )
        .unwrap();
    assert_eq!(
        ordered_names(&application, TaskOrdering::RecentlyWorked),
        ["beta".to_owned(), "alpha".to_owned(), "gamma".to_owned()]
    );
}

#[test]
fn a_committed_write_never_triggers_a_fallible_backend_read() {
    let alpha = stamped_task(1, "alpha", 100, 100);
    let repository = MemoryRepository::with_tasks(vec![alpha]);
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    let list_reads_after_load = repository.0.borrow().list_reads;

    application
        .create_task(TaskName::new("beta").unwrap(), at(200))
        .unwrap();
    application
        .rename_task(
            TaskId::from_uuid(uuid::Uuid::from_u128(1)),
            TaskName::new("renamed").unwrap(),
            at(300),
        )
        .unwrap();
    application
        .set_active_task(TaskId::from_uuid(uuid::Uuid::from_u128(1)), at(400))
        .unwrap();
    assert_eq!(
        repository.0.borrow().list_reads,
        list_reads_after_load,
        "successful writes update the snapshot without another list read"
    );
    // The snapshot still reflects every write.
    let names: Vec<String> = application
        .tasks(TaskOrdering::RecentlyWorked)
        .into_iter()
        .map(|item| item.task.name().to_string())
        .collect();
    assert_eq!(names, ["renamed".to_owned(), "beta".to_owned()]);
}

#[test]
fn tracking_write_conflicts_reload_authoritative_state() {
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone()]);
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    let foreign = worklog(99, beta.id(), 90);
    repository.fail_next_write(RepositoryError::ActiveWorklogExists, Some(foreign.clone()));

    let error = application
        .set_active_task(alpha.id(), at(100))
        .expect_err("the simulated conflict must fail");
    assert_eq!(
        error,
        ApplicationError::TrackingWrite(RepositoryError::ActiveWorklogExists)
    );
    assert_eq!(
        application.current_tracking(),
        &TrackingState::Running {
            worklog: ActiveWorklog::begin(foreign.id(), foreign.task_id(), foreign.start())
        }
    );
    // Recovery refreshes both tracking and recently worked ordering.
    assert_eq!(
        ordered_names(&application, TaskOrdering::RecentlyWorked),
        ["beta".to_owned(), "alpha".to_owned()]
    );
}

#[test]
fn a_failed_authoritative_reload_is_reported_separately() {
    let alpha = task(1, "alpha");
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    repository.fail_next_write(RepositoryError::ActiveWorklogExists, None);
    repository.fail_recovery_after_next_write();
    let error = application
        .set_active_task(alpha.id(), at(100))
        .expect_err("write and recovery must fail");
    assert_eq!(
        error,
        ApplicationError::TrackingRecovery(RepositoryError::Backend {
            message: "read failed".to_owned()
        })
    );
    assert_eq!(application.current_tracking(), &TrackingState::Idle);
}

#[test]
fn active_start_changed_before_refresh_requires_a_clear_retry() {
    let alpha = task(1, "alpha");
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    let active = match application.set_active_task(alpha.id(), at(100)).unwrap() {
        SetActiveTaskOutcome::Started { worklog } => worklog,
        other => panic!("expected start, got {other:?}"),
    };
    repository.0.borrow_mut().worklogs[0] =
        worklog(active.id().as_uuid().as_u128(), alpha.id(), 50);

    assert_eq!(
        application.clear_active_task(active.id(), at(150)),
        Err(ApplicationError::TrackingStateChanged)
    );
    assert!(matches!(
        application.current_tracking(),
        TrackingState::Running { worklog } if worklog.id() == active.id() && worklog.start() == at(50)
    ));
}

#[test]
fn active_start_changed_between_refresh_and_write_requires_stop_and_switch_retries() {
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone()]);
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    let active = match application.set_active_task(alpha.id(), at(100)).unwrap() {
        SetActiveTaskOutcome::Started { worklog } => worklog,
        other => panic!("expected start, got {other:?}"),
    };
    let corrected = worklog(active.id().as_uuid().as_u128(), alpha.id(), 50);
    repository.fail_next_write(
        RepositoryError::WorklogChanged { id: active.id() },
        Some(corrected.clone()),
    );
    assert_eq!(
        application.clear_active_task(active.id(), at(150)),
        Err(ApplicationError::TrackingStateChanged)
    );
    assert!(matches!(
        application.current_tracking(),
        TrackingState::Running { worklog } if worklog.start() == corrected.start()
    ));

    repository.fail_next_write(
        RepositoryError::WorklogChanged { id: active.id() },
        Some(corrected.clone()),
    );
    assert_eq!(
        application.set_active_task(beta.id(), at(160)),
        Err(ApplicationError::TrackingStateChanged)
    );
    assert!(matches!(
        application.current_tracking(),
        TrackingState::Running { worklog } if worklog.id() == active.id() && worklog.start() == corrected.start()
    ));
    assert!(
        repository
            .0
            .borrow()
            .worklogs
            .iter()
            .all(|worklog| worklog.end().is_none())
    );
}

#[test]
fn active_start_changed_before_refresh_requires_a_set_active_retry() {
    let alpha = task(1, "alpha");
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    let active = match application.set_active_task(alpha.id(), at(100)).unwrap() {
        SetActiveTaskOutcome::Started { worklog } => worklog,
        other => panic!("expected start, got {other:?}"),
    };
    repository.0.borrow_mut().worklogs[0] =
        worklog(active.id().as_uuid().as_u128(), alpha.id(), 50);
    assert_eq!(
        application.set_active_task(alpha.id(), at(150)),
        Err(ApplicationError::TrackingStateChanged)
    );
}
