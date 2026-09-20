use super::*;

fn move_app(active: bool) -> (App<TestService>, TestServiceSpy, Task, Task, Task, Worklog) {
    let source = task(1, "alpha");
    let beta = task(2, "Beta reports");
    let gamma = task(3, "Gamma planning");
    let worklog = if active {
        Worklog::begin(worklog_id(10), source.id(), at(100))
    } else {
        history_worklog(10, source.id(), 100)
    };
    let mut service = TestService::with_tasks(vec![source.clone(), beta.clone(), gamma.clone()]);
    service.authoritative_worklogs = vec![worklog.clone()];
    service.worklog_pages = vec![Ok(page(vec![worklog.clone()], None))];
    let spy = service.spy();
    let mut app = app_in_timezone(service, chrono_tz::UTC);
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::OpenMove));
    (app, spy, source, beta, gamma, worklog)
}

fn move_app_with_rows(
    worklogs: Vec<Worklog>,
    move_down: usize,
) -> (App<TestService>, TestServiceSpy, Task, Task) {
    let source = task(1, "alpha");
    let destination = task(2, "Beta reports");
    let mut service = TestService::with_tasks(vec![source.clone(), destination.clone()]);
    service.authoritative_worklogs = worklogs.clone();
    service.worklog_pages = vec![Ok(page(worklogs, None))];
    let spy = service.spy();
    let mut app = app_in_timezone(service, chrono_tz::UTC);
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    for _ in 0..move_down {
        app.handle(Command::WorklogHistory(WorklogHistoryCommand::MoveDown));
    }
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::OpenMove));
    (app, spy, source, destination)
}

#[test]
fn move_opens_for_completed_and_running_worklogs_with_active_destination_search() {
    for active in [false, true] {
        let (app, _, source, beta, gamma, worklog) = move_app(active);
        let draft = app.app_view().move_draft().expect("move dialog is open");
        assert_eq!(draft.worklog(), &worklog);
        assert_eq!(
            draft.focus(),
            crate::screens::worklog_history::MoveFocus::Search
        );
        assert_eq!(draft.query(), "");
        assert_eq!(draft.selected_task_id(), Some(beta.id()));
        let candidates = draft
            .results()
            .map(|candidate| candidate.id())
            .collect::<Vec<_>>();
        assert_eq!(candidates, vec![beta.id(), gamma.id()]);
        assert!(!candidates.contains(&source.id()));
    }
}

#[test]
fn move_search_filters_fuzzily_and_keeps_search_editing_separate_from_results_navigation() {
    let (mut app, _, _, beta, gamma, _) = move_app(false);
    for character in "gp".chars() {
        app.handle(Command::WorklogHistory(
            WorklogHistoryCommand::InsertMoveQuery(character),
        ));
    }
    let draft = app.app_view().move_draft().unwrap();
    assert_eq!(draft.query(), "gp");
    assert_eq!(draft.selected_task_id(), Some(gamma.id()));

    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::ToggleMoveFocus,
    ));
    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::BackspaceMoveQuery,
    ));
    assert_eq!(app.app_view().move_draft().unwrap().query(), "gp");
    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::MoveDestinationUp,
    ));
    assert_eq!(
        app.app_view().move_draft().unwrap().selected_task_id(),
        Some(gamma.id())
    );

    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::ToggleMoveFocus,
    ));
    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::BackspaceMoveQuery,
    ));
    assert_eq!(app.app_view().move_draft().unwrap().query(), "g");
    assert_eq!(
        app.app_view().move_draft().unwrap().selected_task_id(),
        Some(gamma.id())
    );
    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::BackspaceMoveQuery,
    ));
    assert_eq!(
        app.app_view().move_draft().unwrap().selected_task_id(),
        Some(beta.id())
    );
}

#[test]
fn move_with_no_matches_keeps_the_dialog_open_and_does_not_write() {
    let (mut app, spy, _, _, _, _) = move_app(false);
    for character in "zzz".chars() {
        app.handle(Command::WorklogHistory(
            WorklogHistoryCommand::InsertMoveQuery(character),
        ));
    }
    assert_eq!(
        app.app_view().move_draft().unwrap().selected_task_id(),
        None
    );
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));
    assert!(app.app_view().move_draft().is_some());
    assert!(spy.move_calls().is_empty());
}

#[test]
fn successful_move_uses_the_selected_snapshot_and_refreshes_source_history() {
    let (mut app, spy, source, beta, _, worklog) = move_app(true);
    app.application_mut().worklog_pages.push(Ok(WorklogPage {
        worklogs: Vec::new(),
        snapshot: WorklogPageSnapshot {
            requested_task_latest_work_start: None,
            active_worklog: Some(Worklog::begin(worklog.id(), beta.id(), worklog.start())),
            active_task_latest_work_start: Some(worklog.start()),
        },
        next_cursor: None,
    }));

    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));

    assert!(matches!(
        app.app_view().screen_state(),
        ScreenState::WorklogHistory(state) if matches!(state.mode(), WorklogHistoryMode::Normal)
    ));
    assert!(app.app_view().history().unwrap().worklogs().is_empty());
    assert_eq!(
        spy.move_calls(),
        vec![(worklog.id(), source.id(), worklog.times(), beta.id())]
    );
    assert_eq!(app.app_view().active_worklog_id(), Some(worklog.id()));
    assert_eq!(
        text(app.app_view().status()),
        "Moved worklog to \"Beta reports\""
    );
}

#[test]
fn moving_a_middle_row_selects_its_former_successor_after_refresh() {
    let source = task(1, "alpha");
    let newest = history_worklog(12, source.id(), 300);
    let middle = history_worklog(11, source.id(), 200);
    let oldest = history_worklog(10, source.id(), 100);
    let (mut app, _, _, _) = move_app_with_rows(vec![newest.clone(), middle, oldest.clone()], 1);
    app.application_mut()
        .worklog_pages
        .push(Ok(page(vec![newest, oldest.clone()], None)));

    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));

    assert!(app.app_view().move_draft().is_none());
    assert_eq!(
        app.app_view().history().unwrap().selected_id(),
        Some(oldest.id())
    );
}

#[test]
fn moving_the_last_row_selects_its_former_predecessor_after_refresh() {
    let source = task(1, "alpha");
    let newest = history_worklog(12, source.id(), 300);
    let middle = history_worklog(11, source.id(), 200);
    let oldest = history_worklog(10, source.id(), 100);
    let (mut app, _, _, _) = move_app_with_rows(vec![newest.clone(), middle.clone(), oldest], 2);
    app.application_mut()
        .worklog_pages
        .push(Ok(page(vec![newest, middle.clone()], None)));

    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));

    assert!(app.app_view().move_draft().is_none());
    assert_eq!(
        app.app_view().history().unwrap().selected_id(),
        Some(middle.id())
    );
}

#[test]
fn ordinary_move_failures_keep_the_search_selection_and_dialog_open() {
    let (mut app, spy, _, _, gamma, _) = move_app(false);
    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::InsertMoveQuery('g'),
    ));
    let before = app.app_view().move_draft().unwrap().clone();
    spy.set_move_error(ApplicationError::worklog_overlap(before.worklog().id()));

    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));

    assert_eq!(app.app_view().move_draft(), Some(&before));
    assert_eq!(
        spy.move_calls()[0].3,
        gamma.id(),
        "the selected destination reached the application"
    );
    assert_eq!(
        text(app.app_view().status()),
        "The worklog overlaps another worklog"
    );
}

#[test]
fn failed_move_recovery_keeps_the_draft_for_an_explicit_retry_or_cancel() {
    let (mut app, spy, _, _, _, worklog) = move_app(false);
    let before = app.app_view().move_draft().unwrap().clone();
    spy.set_move_error(ApplicationError::WorklogMoveRecovery {
        write: RepositoryError::WorklogChanged { id: worklog.id() },
        recovery: RepositoryError::Backend {
            message: "reload failed".to_owned(),
        },
    });

    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));

    assert_eq!(app.app_view().move_draft(), Some(&before));
    assert!(text(app.app_view().status()).contains("State recovery failed"));
}

#[test]
fn failed_post_move_refresh_preserves_the_active_timers_monotonic_anchor() {
    let source = task(1, "alpha");
    let destination = task(2, "Beta reports");
    let worklog = Worklog::begin(worklog_id(10), source.id(), at(100));
    let mut service = TestService::with_tasks(vec![source, destination]);
    service.authoritative_worklogs = vec![worklog.clone()];
    service.worklog_pages = vec![
        Ok(page(vec![worklog], None)),
        Err(ApplicationError::storage_failure("refresh failed")),
    ];
    let (mut app, clock) = app_with_test_clock(service, chrono_tz::UTC, at(100));
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::OpenMove));
    clock.advance_monotonic(Duration::from_secs(600));
    clock.set_wall_clock(at(10_000));

    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));

    assert_eq!(app.app_view().elapsed(), Some(Duration::from_secs(600)));
    assert_eq!(app.app_view().active_task_name(), Some("Beta reports"));
    assert_eq!(
        app.app_view().history().unwrap().availability(),
        HistoryAvailability::Unavailable
    );
    assert_eq!(
        text(app.app_view().status()),
        "Move saved, but history refresh failed"
    );
}

#[test]
fn stale_and_archived_destination_failures_close_and_refresh_source_history() {
    let (mut app, spy, source, _beta, _, worklog) = move_app(false);
    app.application_mut()
        .worklog_pages
        .push(Ok(page(vec![history_worklog(11, source.id(), 200)], None)));
    spy.set_move_error(ApplicationError::worklog_changed(worklog.id()));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));
    assert!(app.app_view().move_draft().is_none());
    assert_eq!(
        app.app_view().history().unwrap().worklogs()[0].id(),
        worklog_id(11)
    );

    let (mut app, _, source, beta, _, _) = move_app(false);
    app.application_mut().tasks[1].archive(at(200));
    app.application_mut()
        .worklog_pages
        .push(Ok(page(Vec::new(), None)));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));
    assert!(app.app_view().move_draft().is_none());
    assert_eq!(app.app_view().history().unwrap().task_id(), source.id());
    assert!(
        app.app_view()
            .tasks()
            .iter()
            .all(|task| task.id() != beta.id())
    );
}
