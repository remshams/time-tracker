//! Tests for tracking behavior.

use super::*;

#[test]
fn application_and_tui_recover_tracking_after_a_restart() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tracker.db");
    let task = Task::create(TaskId::generate(), TaskName::new("alpha").unwrap(), at(100));
    {
        let repository = SqliteRepository::open(&path).unwrap();
        repository.create_task(task.clone()).unwrap();
        repository
            .insert_worklog(&Worklog::begin(
                WorklogId::generate(),
                task.id(),
                DateTime::from_timestamp(100, 0).unwrap(),
            ))
            .unwrap();
    }
    let app = App::load(TrackerApplication::load(SqliteRepository::open(&path).unwrap()).unwrap());
    assert_eq!(app.app_view().active_task_id(), Some(task.id()));
    assert_eq!(
        app.app_view().status(),
        &Status::Info("Recovered the previous active timer".to_owned())
    );
}
#[test]
fn a_successful_archive_copies_tracking_refreshed_by_the_application() {
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    let mut service = TestService::with_tasks(vec![alpha.clone(), beta.clone()]);
    service.archive_activates = Some(at(200));
    let mut app = App::load(service);

    app.handle(Command::TaskList(TaskListCommand::OpenArchiveConfirm));
    app.handle(Command::TaskList(TaskListCommand::Confirm));

    assert_eq!(app.app_view().active_task_id(), Some(beta.id()));
    assert_eq!(app.app_view().active_task_name(), Some("beta"));
    assert_eq!(
        app.app_view().status(),
        &Status::Info("Archived \"alpha\"".to_owned())
    );
}
#[test]
fn archiving_the_active_task_keeps_the_timer() {
    let mut app = app_with(&["alpha"]);
    app.handle(Command::TaskList(TaskListCommand::ToggleTracking));
    let active = app.app_view().active_task_id();
    app.handle(Command::TaskList(TaskListCommand::OpenArchiveConfirm));
    app.handle(Command::TaskList(TaskListCommand::Confirm));
    assert_eq!(app.app_view().active_task_id(), active);
    assert_eq!(app.app_view().tasks().len(), 1);
    assert_eq!(
        app.app_view().status(),
        &Status::Error("The active task cannot be archived".to_owned())
    );
}
#[test]
fn space_switches_to_another_task() {
    let mut app = app_with(&["alpha", "beta"]);
    app.handle(Command::TaskList(TaskListCommand::ToggleTracking));
    app.handle(Command::TaskList(TaskListCommand::MoveDown));
    let beta = app.app_view().tasks()[1].id();
    app.handle(Command::TaskList(TaskListCommand::ToggleTracking));
    assert_eq!(app.app_view().active_task_id(), Some(beta));
    assert_eq!(
        app.app_view().status(),
        &Status::Info("Switched to \"beta\"".to_owned())
    );
}
#[test]
fn tracking_outcomes_choose_the_right_status_and_clock_anchor() {
    let task = |tag, name| {
        Task::create(
            TaskId::from_uuid(uuid::Uuid::from_u128(tag)),
            TaskName::new(name).unwrap(),
            at(100),
        )
    };
    let old = DateTime::from_timestamp(100, 0).unwrap();

    let mut started_service = TestService::with_tasks(vec![task(1, "alpha")]);
    started_service.set_timestamp = Some(old);
    let mut started = App::load(started_service);
    started.handle(Command::TaskList(TaskListCommand::ToggleTracking));
    assert_eq!(
        started.app_view().status(),
        &Status::Info("Started \"alpha\"".to_owned())
    );
    assert!(started.app_view().elapsed().unwrap() < Duration::from_secs(1));

    let mut existing_service = TestService::with_tasks(vec![task(1, "alpha")]);
    existing_service.set_returns_already_active = true;
    existing_service.set_timestamp = Some(old);
    let mut existing = App::load(existing_service);
    existing.handle(Command::TaskList(TaskListCommand::ToggleTracking));
    assert_eq!(
        existing.app_view().status(),
        &Status::Info("Started \"alpha\"".to_owned())
    );
    assert!(existing.app_view().elapsed().unwrap() > Duration::from_secs(60));

    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    let mut concurrent_service = TestService::with_tasks(vec![alpha.clone(), beta.clone()]);
    concurrent_service.tracking = TrackingState::Running {
        worklog: ActiveWorklog::begin(
            WorklogId::from_uuid(uuid::Uuid::from_u128(10)),
            alpha.id(),
            old,
        ),
    };
    concurrent_service.set_returns_already_active = true;
    concurrent_service.set_timestamp = Some(old + TimeDelta::minutes(1));
    let mut concurrent = App::load(concurrent_service);
    concurrent.handle(Command::TaskList(TaskListCommand::MoveDown));
    concurrent.handle(Command::TaskList(TaskListCommand::ToggleTracking));
    assert_eq!(
        concurrent.app_view().status(),
        &Status::Info("Switched to \"beta\"".to_owned())
    );

    let mut switched_service = TestService::with_tasks(vec![alpha.clone(), beta.clone()]);
    switched_service.tracking = TrackingState::Running {
        worklog: ActiveWorklog::begin(
            WorklogId::from_uuid(uuid::Uuid::from_u128(10)),
            alpha.id(),
            old,
        ),
    };
    switched_service.set_returns_switched = true;
    switched_service.set_timestamp = Some(old + TimeDelta::minutes(1));
    let (mut switched, clock) = app_with_test_clock(switched_service, chrono_tz::UTC, old);
    clock.advance_monotonic(Duration::from_secs(60));
    switched.handle(Command::TaskList(TaskListCommand::MoveDown));
    switched.handle(Command::TaskList(TaskListCommand::ToggleTracking));
    assert_eq!(
        switched.app_view().status(),
        &Status::Info("Switched to \"beta\"".to_owned())
    );
    assert_eq!(switched.app_view().elapsed(), Some(Duration::ZERO));
}
#[test]
fn a_backward_wall_clock_jump_persists_a_nonnegative_duration() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tracker.db");
    let task = Task::create(TaskId::generate(), TaskName::new("alpha").unwrap(), at(100));
    let start = DateTime::from_timestamp_micros(Utc::now().timestamp_micros()).unwrap()
        + TimeDelta::hours(1);
    let repository = SqliteRepository::open(&path).unwrap();
    repository.create_task(task.clone()).unwrap();
    repository
        .insert_worklog(&Worklog::begin(WorklogId::generate(), task.id(), start))
        .unwrap();
    let mut app = App::load(TrackerApplication::load(repository).unwrap());

    app.handle(Command::TaskList(TaskListCommand::ToggleTracking));

    let mut verifier = TrackerApplication::load(SqliteRepository::open(&path).unwrap()).unwrap();
    let stored = verifier
        .worklogs_for_task(task.id(), None)
        .unwrap()
        .worklogs
        .remove(0);
    let duration = (stored.end().unwrap() - stored.start()).to_std().unwrap();
    assert!(duration < Duration::from_secs(60), "got {duration:?}");
    assert!(stored.end().unwrap() >= start);
}
#[test]
fn a_forward_wall_clock_jump_persists_the_displayed_duration() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tracker.db");
    let task = Task::create(TaskId::generate(), TaskName::new("alpha").unwrap(), at(100));
    let start = DateTime::from_timestamp_micros(Utc::now().timestamp_micros()).unwrap()
        - TimeDelta::hours(2);
    let repository = SqliteRepository::open(&path).unwrap();
    repository.create_task(task.clone()).unwrap();
    repository
        .insert_worklog(&Worklog::begin(WorklogId::generate(), task.id(), start))
        .unwrap();
    let (mut app, clock) = app_with_test_clock(
        TrackerApplication::load(repository).unwrap(),
        chrono_tz::UTC,
        start,
    );
    clock.advance_monotonic(Duration::from_secs(5));
    let shown = app.app_view().elapsed().unwrap();

    app.handle(Command::TaskList(TaskListCommand::ToggleTracking));

    let mut verifier = TrackerApplication::load(SqliteRepository::open(&path).unwrap()).unwrap();
    let stored = verifier
        .worklogs_for_task(task.id(), None)
        .unwrap()
        .worklogs
        .remove(0);
    let duration = (stored.end().unwrap() - stored.start()).to_std().unwrap();
    assert!(
        duration >= shown,
        "persisted {duration:?} < shown {shown:?}"
    );
    assert!(
        duration - shown < Duration::from_secs(2),
        "persisted {duration:?} must match shown {shown:?}"
    );
    assert!(
        duration < Duration::from_secs(60),
        "the two-hour wall-clock gap leaked into the worklog: {duration:?}"
    );
}
#[test]
fn quitting_in_a_dialog_does_not_stop_tracking() {
    let mut app = app_with(&["alpha"]);
    app.handle(Command::TaskList(TaskListCommand::ToggleTracking));
    app.handle(Command::TaskList(TaskListCommand::OpenAdd));
    app.handle(Command::Quit);
    assert!(!app.is_running());
    assert!(app.app_view().active_task_id().is_some());
}
#[test]
fn quitting_from_every_mode_leaves_tracking_active() {
    for command in [
        None,
        Some(Command::TaskList(TaskListCommand::OpenAdd)),
        Some(Command::TaskList(TaskListCommand::OpenRename)),
        Some(Command::TaskList(TaskListCommand::OpenArchiveConfirm)),
    ] {
        let task = Task::create(
            TaskId::from_uuid(uuid::Uuid::from_u128(1)),
            TaskName::new("one").unwrap(),
            at(100),
        );
        let mut service = TestService::with_tasks(vec![task.clone()]);
        service.tracking = TrackingState::Running {
            worklog: ActiveWorklog::begin(
                WorklogId::from_uuid(uuid::Uuid::from_u128(10)),
                task.id(),
                DateTime::from_timestamp(100, 0).unwrap(),
            ),
        };
        let mut app = App::load(service);
        if let Some(command) = command {
            app.handle(command);
        }
        app.handle(Command::Quit);
        assert!(!app.is_running());
        assert_eq!(app.app_view().active_task_id(), Some(task.id()));
    }
}
#[test]
fn start_switch_and_stop_keep_selection_on_the_operated_task() {
    let mut app = app_with(&["alpha", "beta"]);
    let alpha = app
        .app_view()
        .tasks()
        .iter()
        .find(|task| task.name().as_str() == "alpha")
        .unwrap()
        .id();
    let beta = app
        .app_view()
        .tasks()
        .iter()
        .find(|task| task.name().as_str() == "beta")
        .unwrap()
        .id();

    app.handle(Command::TaskList(TaskListCommand::MoveDown));
    app.handle(Command::TaskList(TaskListCommand::ToggleTracking));
    assert_eq!(app.app_view().tasks()[0].id(), beta);
    assert_eq!(
        app.app_view().tasks()[app.app_view().selected().unwrap()].id(),
        beta
    );

    app.handle(Command::TaskList(TaskListCommand::MoveDown));
    app.handle(Command::TaskList(TaskListCommand::ToggleTracking));
    assert_eq!(app.app_view().tasks()[0].id(), alpha);
    assert_eq!(
        app.app_view().tasks()[app.app_view().selected().unwrap()].id(),
        alpha
    );

    app.handle(Command::TaskList(TaskListCommand::ToggleTracking));
    assert_eq!(
        app.app_view().tasks()[0].id(),
        alpha,
        "stopping does not reorder"
    );
    assert_eq!(
        app.app_view().tasks()[app.app_view().selected().unwrap()].id(),
        alpha
    );
}
#[test]
fn unarchiving_refreshes_a_timer_another_client_started() {
    let mut service = TestService::with_tasks(vec![task(1, "alpha"), archived_task(3, "gone")]);
    service.unarchive_activates = Some(DateTime::from_timestamp(100, 0).unwrap());
    let mut app = App::load(service);
    app.handle(Command::TaskList(TaskListCommand::ShowArchivedTasks));
    app.handle(Command::TaskList(TaskListCommand::UnarchiveSelected));

    assert_eq!(app.app_view().view(), TaskView::Archived);
    assert_eq!(
        app.app_view().active_task_id(),
        Some(TaskId::from_uuid(uuid::Uuid::from_u128(3))),
        "the concurrent worklog reached the TUI's tracking state"
    );
    assert_eq!(app.app_view().active_task_name(), Some("gone"));
    assert!(
        app.app_view().elapsed().unwrap() > Duration::from_secs(60),
        "the clock anchored to the concurrent worklog's start"
    );
    assert_eq!(
        app.app_view().status(),
        &Status::Info("Restored \"gone\"".to_owned())
    );

    assert_eq!(
        app.app_view().tasks().len(),
        0,
        "the restored task left the list"
    );
    app.handle(Command::TaskList(TaskListCommand::ShowActiveTasks));
    assert_eq!(
        app.app_view().selected(),
        Some(1),
        "the restored task is selected"
    );
    assert_eq!(
        app.app_view().tasks()[1].id(),
        TaskId::from_uuid(uuid::Uuid::from_u128(3))
    );
}
#[test]
fn the_timer_header_resolves_the_active_task_outside_the_visible_list() {
    let alpha = task(1, "alpha");
    let mut service = TestService::with_tasks(vec![archived_task(3, "gone"), alpha.clone()]);
    service.tracking = TrackingState::Running {
        worklog: ActiveWorklog::begin(
            WorklogId::from_uuid(uuid::Uuid::from_u128(10)),
            alpha.id(),
            DateTime::from_timestamp(100, 0).unwrap(),
        ),
    };
    let mut app = App::load(service);
    app.handle(Command::TaskList(TaskListCommand::ShowArchivedTasks));

    assert_eq!(app.app_view().view(), TaskView::Archived);
    assert_eq!(
        app.app_view().tasks().len(),
        1,
        "only archived tasks are listed"
    );
    assert_eq!(app.app_view().active_task_name(), Some("alpha"));
}

#[test]
fn space_starts_stops_and_restarts_with_separate_worklogs() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tracker.db");
    let task = task(1, "alpha");
    let repository = SqliteRepository::open(&path).unwrap();
    repository.create_task(task.clone()).unwrap();
    let mut app = App::load(TrackerApplication::load(repository).unwrap());
    let task_id = task.id();
    app.handle(Command::TaskList(TaskListCommand::ToggleTracking));
    assert_eq!(app.app_view().active_task_id(), Some(task_id));
    assert_eq!(text(app.app_view().status()), "Started \"alpha\"");
    app.handle(Command::TaskList(TaskListCommand::ToggleTracking));
    assert_eq!(app.app_view().active_task_id(), None);
    assert_eq!(app.app_view().elapsed(), None);
    assert_eq!(text(app.app_view().status()), "Stopped \"alpha\"");
    app.handle(Command::TaskList(TaskListCommand::ToggleTracking));
    let mut verifier = TrackerApplication::load(SqliteRepository::open(&path).unwrap()).unwrap();
    let worklogs = verifier.worklogs_for_task(task_id, None).unwrap().worklogs;
    assert_eq!(worklogs.len(), 2);
    assert_ne!(worklogs[0].id(), worklogs[1].id());
}
