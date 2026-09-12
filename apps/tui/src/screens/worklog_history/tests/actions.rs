//! Tests for history behavior.

use super::*;

#[test]
fn deletion_opening_requires_normal_history_mode() {
    let alpha = task(1, "alpha");
    let target = history_worklog(10, alpha.id(), 100);
    let mut service = TestService::with_tasks(vec![alpha]);
    service.worklog_pages = vec![Ok(page(vec![target.clone()], None))];
    service.authoritative_worklogs = vec![target];
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);
    app.handle(Command::OpenCorrection);
    let correction = app.app_view().correction().cloned();
    app.handle(Command::OpenDeletion);
    assert_eq!(app.app_view().correction(), correction.as_ref());
    app.handle(Command::Cancel);

    app.handle(Command::BackToTaskList);
    app.handle(Command::OpenArchiveConfirm);
    app.handle(Command::OpenDeletion);
    assert!(
        app.app_view().screen() == Screen::TaskList
            && matches!(
                app.app_view().task_list().mode(),
                TaskListMode::ConfirmArchive { .. }
            )
    );

    app.handle(Command::Cancel);
    app.handle(Command::OpenDeletion);
    assert!(
        app.app_view().screen() == Screen::TaskList
            && matches!(app.app_view().task_list().mode(), TaskListMode::Normal)
    );
}
#[test]
fn completed_deletion_uses_the_snapshot_and_preserves_the_cursor() {
    let alpha = task(1, "alpha");
    let target = history_worklog(12, alpha.id(), 300);
    let following = history_worklog(11, alpha.id(), 200);
    let cursor = cursor(200, 11);
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.latest_work_starts = vec![(alpha.id(), target.start())];
    service.worklog_pages = vec![Ok(page(
        vec![target.clone(), following.clone()],
        Some(cursor),
    ))];
    service.authoritative_worklogs = vec![target.clone(), following.clone()];
    let spy = service.spy();
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);
    app.handle(Command::OpenDeletion);

    assert_eq!(app.app_view().deletion(), Some(&target));
    app.handle(Command::Confirm);

    assert_eq!(
        spy.deletion_calls(),
        vec![(target.id(), alpha.id(), target.times())]
    );
    let history = app.app_view().history().unwrap();
    assert_eq!(history.worklogs(), vec![following.clone()]);
    assert_eq!(history.next_cursor(), Some(cursor));
    assert_eq!(app.app_view().history_selected_index(), Some(0));
    assert!(
        matches!(app.app_view().screen_state(), ScreenState::WorklogHistory(state) if matches!(state.mode(), WorklogHistoryMode::Normal))
    );
    assert_eq!(
        app.app_view().status(),
        &Status::Info("Deleted worklog".to_owned())
    );
    assert_eq!(spy.latest_work_start(alpha.id()), Some(following.start()));
    assert_eq!(spy.worklog_reads(), 1);
}
#[test]
fn deletion_selection_moves_to_the_following_row_or_previous_row() {
    let alpha = task(1, "alpha");
    let rows = vec![
        history_worklog(13, alpha.id(), 300),
        history_worklog(12, alpha.id(), 200),
        history_worklog(11, alpha.id(), 100),
    ];
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.worklog_pages = vec![Ok(page(rows.clone(), None))];
    service.authoritative_worklogs = rows.clone();
    let spy = service.spy();
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);
    app.handle(Command::MoveDown);
    app.handle(Command::OpenDeletion);
    app.handle(Command::Confirm);
    assert_eq!(
        app.app_view()
            .history()
            .unwrap()
            .worklogs()
            .iter()
            .map(Worklog::id)
            .collect::<Vec<_>>(),
        vec![worklog_id(13), worklog_id(11)]
    );
    assert_eq!(app.app_view().history_selected_index(), Some(1));
    assert_eq!(spy.latest_work_start(alpha.id()), Some(at(300)));

    app.handle(Command::OpenDeletion);
    app.handle(Command::Confirm);
    assert_eq!(app.app_view().history_selected_index(), Some(0));
    assert_eq!(spy.latest_work_start(alpha.id()), Some(at(300)));

    app.handle(Command::OpenDeletion);
    app.handle(Command::Confirm);
    assert_eq!(app.app_view().history_selected_index(), None);
    assert!(app.app_view().history().unwrap().worklogs().is_empty());
    assert_eq!(spy.latest_work_start(alpha.id()), None);
}
#[test]
fn repeated_successful_deletions_update_the_latest_work_aggregate() {
    let alpha = task(1, "alpha");
    let rows = vec![
        history_worklog(13, alpha.id(), 300),
        history_worklog(12, alpha.id(), 200),
        history_worklog(11, alpha.id(), 100),
    ];
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.worklog_pages = vec![Ok(page(rows.clone(), None))];
    service.authoritative_worklogs = rows;
    let spy = service.spy();
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);

    for expected in [Some(at(200)), Some(at(100)), None] {
        app.handle(Command::OpenDeletion);
        app.handle(Command::Confirm);
        assert_eq!(spy.latest_work_start(alpha.id()), expected);
    }
}
#[test]
fn test_deletion_uses_authoritative_worklogs_and_validates_the_snapshot() {
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    let target = history_worklog(10, alpha.id(), 300);
    let older = history_worklog(11, alpha.id(), 200);
    let active = Worklog::begin(worklog_id(12), alpha.id(), at(100));
    let mut service = TestService::with_tasks(vec![alpha.clone(), beta.clone()]);
    service.authoritative_worklogs = vec![target.clone(), older.clone(), active.clone()];

    assert_eq!(
        service.delete_completed_worklog(worklog_id(99), alpha.id(), target.times()),
        Err(ApplicationError::worklog_not_found(worklog_id(99)))
    );
    assert_eq!(
        service.delete_completed_worklog(active.id(), alpha.id(), active.times()),
        Err(ApplicationError::active_worklog(active.id()))
    );
    assert_eq!(
        service.delete_completed_worklog(target.id(), beta.id(), target.times()),
        Err(ApplicationError::worklog_changed(target.id()))
    );
    assert_eq!(
        service.delete_completed_worklog(
            target.id(),
            alpha.id(),
            WorklogTimes::new(at(301), Some(at(361)))
        ),
        Err(ApplicationError::worklog_changed(target.id()))
    );

    let outcome = service
        .delete_completed_worklog(target.id(), alpha.id(), target.times())
        .unwrap();
    assert_eq!(outcome, target);
    assert_eq!(service.authoritative_worklogs, vec![older, active]);
    assert_eq!(
        service
            .latest_work_starts
            .iter()
            .find(|(task_id, _)| *task_id == alpha.id())
            .map(|(_, start)| *start),
        Some(at(200))
    );
}
#[test]
fn deleting_the_loaded_newest_row_uses_an_unloaded_older_row_for_latest_work() {
    let alpha = task(1, "alpha");
    let newest = history_worklog(10, alpha.id(), 300);
    let older = history_worklog(11, alpha.id(), 200);
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.latest_work_starts = vec![(alpha.id(), newest.start())];
    service.worklog_pages = vec![Ok(page(vec![newest.clone()], Some(cursor(300, 10))))];
    service.authoritative_worklogs = vec![newest.clone(), older.clone()];
    let spy = service.spy();
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);
    app.handle(Command::OpenDeletion);
    app.handle(Command::Confirm);

    assert!(app.app_view().history().unwrap().worklogs().is_empty());
    assert_eq!(
        app.app_view().history().unwrap().next_cursor(),
        Some(cursor(300, 10))
    );
    assert_eq!(spy.latest_work_start(alpha.id()), Some(older.start()));
}
#[test]
fn active_history_rows_are_not_deletable_and_archived_rows_are() {
    let alpha = task(1, "alpha");
    let active = Worklog::begin(worklog_id(10), alpha.id(), at(300));
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.worklog_pages = vec![Ok(page(vec![active.clone()], None))];
    service.authoritative_worklogs = vec![active.clone()];
    let spy = service.spy();
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);
    app.handle(Command::OpenDeletion);
    assert!(
        matches!(app.app_view().screen_state(), ScreenState::WorklogHistory(state) if matches!(state.mode(), WorklogHistoryMode::Normal))
    );
    assert_eq!(
        app.app_view().status(),
        &Status::Error(ACTIVE_WORKLOG_DELETE_MESSAGE.to_owned())
    );
    assert!(spy.deletion_calls().is_empty());

    let archived = archived_task(2, "archived");
    let completed = history_worklog(11, archived.id(), 100);
    let mut service = TestService::with_tasks(vec![archived.clone()]);
    service.worklog_pages = vec![Ok(page(vec![completed.clone()], None))];
    service.authoritative_worklogs = vec![completed.clone()];
    let mut app = App::load(service);
    app.handle(Command::ShowArchivedTasks);
    app.handle(Command::OpenHistory);
    app.handle(Command::OpenDeletion);
    assert_eq!(app.app_view().deletion(), Some(&completed));
}
#[test]
fn deletion_cancel_and_ordinary_failure_keep_the_snapshot() {
    let alpha = task(1, "alpha");
    let target = history_worklog(10, alpha.id(), 100);
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.worklog_pages = vec![Ok(page(vec![target.clone()], None))];
    service.authoritative_worklogs = vec![target.clone()];
    service.deletion_error = Some(ApplicationError::storage_failure("private backend detail"));
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);
    app.handle(Command::OpenDeletion);
    app.handle(Command::Confirm);
    assert_eq!(app.app_view().deletion(), Some(&target));
    assert_eq!(text(app.app_view().status()), "Storage error");
    assert!(!text(app.app_view().status()).contains("private backend detail"));
    app.handle(Command::Cancel);
    assert!(
        matches!(app.app_view().screen_state(), ScreenState::WorklogHistory(state) if matches!(state.mode(), WorklogHistoryMode::Normal))
    );
    assert_eq!(
        app.app_view().status(),
        &Status::Info("Deletion cancelled".to_owned())
    );
}
#[test]
fn deletion_recovery_failure_keeps_the_snapshot_without_refreshing_history() {
    let alpha = task(1, "alpha");
    let target = history_worklog(10, alpha.id(), 100);
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.worklog_pages = vec![Ok(page(vec![target.clone()], None))];
    service.authoritative_worklogs = vec![target.clone()];
    service.deletion_error = Some(ApplicationError::deletion_changed_with_recovery_failure(
        target.id(),
        "recovery secret",
    ));
    let spy = service.spy();
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);
    app.handle(Command::OpenDeletion);
    app.handle(Command::Confirm);

    assert_eq!(app.app_view().deletion(), Some(&target));
    assert_eq!(app.app_view().history().unwrap().worklogs(), vec![target]);
    assert_eq!(spy.worklog_reads(), 1);
    assert_eq!(
        text(app.app_view().status()),
        "Deletion failed: Worklog changed in another client. State recovery failed: Storage error."
    );
    assert!(!text(app.app_view().status()).contains("recovery secret"));
}
#[test]
fn stale_deletion_refreshes_and_requires_confirmation_again() {
    let alpha = task(1, "alpha");
    let target = history_worklog(10, alpha.id(), 100);
    let newest = history_worklog(10, alpha.id(), 120);
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.worklog_pages = vec![
        Ok(page(vec![target.clone()], Some(cursor(100, 10)))),
        Ok(page(vec![newest.clone()], None)),
    ];
    service.authoritative_worklogs = vec![target.clone()];
    service.deletion_error = Some(ApplicationError::worklog_not_found(target.id()));
    let spy = service.spy();
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);
    app.handle(Command::OpenDeletion);
    app.handle(Command::Confirm);

    assert!(
        matches!(app.app_view().screen_state(), ScreenState::WorklogHistory(state) if matches!(state.mode(), WorklogHistoryMode::Normal))
    );
    assert_eq!(app.app_view().history().unwrap().worklogs(), vec![newest]);
    assert_eq!(app.app_view().history_selected_index(), Some(0));
    assert_eq!(
        text(app.app_view().status()),
        "Worklog was not found. Press d to confirm deletion again."
    );
    assert_eq!(spy.worklog_reads(), 2);
}
#[test]
fn changed_deletion_of_a_missing_row_reports_a_refresh() {
    let alpha = task(1, "alpha");
    let target = history_worklog(10, alpha.id(), 100);
    let replacement = history_worklog(11, alpha.id(), 200);
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.worklog_pages = vec![
        Ok(page(vec![target.clone()], None)),
        Ok(page(vec![replacement], None)),
    ];
    service.authoritative_worklogs = vec![target.clone()];
    service.deletion_error = Some(ApplicationError::worklog_changed(target.id()));
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);
    app.handle(Command::OpenDeletion);
    app.handle(Command::Confirm);

    assert_eq!(
        text(app.app_view().status()),
        "Worklog changed. History was refreshed."
    );
}
#[test]
fn failed_stale_refresh_discards_rows_and_marks_history_unavailable() {
    let alpha = task(1, "alpha");
    let target = history_worklog(10, alpha.id(), 100);
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.worklog_pages = vec![
        Ok(page(vec![target.clone()], None)),
        Err(TestService::failure()),
    ];
    service.authoritative_worklogs = vec![target.clone()];
    service.deletion_error = Some(ApplicationError::worklog_not_found(target.id()));
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);
    app.handle(Command::OpenDeletion);
    app.handle(Command::Confirm);

    let history = app.app_view().history().unwrap();
    assert_eq!(history.availability(), HistoryAvailability::Unavailable);
    assert!(history.worklogs().is_empty());
    assert_eq!(history.next_cursor(), None);
    assert!(
        matches!(app.app_view().screen_state(), ScreenState::WorklogHistory(state) if matches!(state.mode(), WorklogHistoryMode::Normal))
    );
    assert_eq!(
        text(app.app_view().status()),
        "Worklog changed, but history refresh failed: Storage error"
    );
    assert!(!text(app.app_view().status()).contains("private backend detail"));
    app.handle(Command::OpenDeletion);
    assert!(
        matches!(app.app_view().screen_state(), ScreenState::WorklogHistory(state) if matches!(state.mode(), WorklogHistoryMode::Normal))
    );
}
#[test]
fn deleting_loaded_pages_then_loading_older_selects_the_first_appended_row() {
    let alpha = task(1, "alpha");
    let loaded = (1..=50)
        .map(|tag| history_worklog(tag, alpha.id(), 2_000 - tag as i64))
        .collect::<Vec<_>>();
    let older = history_worklog(51, alpha.id(), 1_900);
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.worklog_pages = vec![
        Ok(page(loaded.clone(), Some(cursor(1_950, 50)))),
        Ok(page(vec![older.clone()], None)),
    ];
    service.authoritative_worklogs = loaded.clone();
    service.authoritative_worklogs.push(older.clone());
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);

    for _ in 0..50 {
        app.handle(Command::OpenDeletion);
        app.handle(Command::Confirm);
    }

    let history = app.app_view().history().unwrap();
    assert!(history.worklogs().is_empty());
    assert_eq!(history.next_cursor(), Some(cursor(1_950, 50)));
    assert_eq!(app.app_view().history_selected_index(), None);

    app.handle(Command::LoadOlderWorklogs);

    let history = app.app_view().history().unwrap();
    assert_eq!(history.worklogs(), vec![older]);
    assert_eq!(history.next_cursor(), None);
    assert_eq!(app.app_view().history_selected_index(), Some(0));
}
#[test]
fn active_race_refreshes_history_and_keeps_confirmation_closed() {
    let alpha = task(1, "alpha");
    let completed = history_worklog(10, alpha.id(), 100);
    let active = Worklog::begin(completed.id(), alpha.id(), at(100));
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.worklog_pages = vec![
        Ok(page(vec![completed.clone()], None)),
        Ok(page(vec![active.clone()], None)),
    ];
    service.authoritative_worklogs = vec![completed];
    service.deletion_error = Some(ApplicationError::active_worklog(active.id()));
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);
    app.handle(Command::OpenDeletion);
    app.handle(Command::Confirm);

    assert!(
        matches!(app.app_view().screen_state(), ScreenState::WorklogHistory(state) if matches!(state.mode(), WorklogHistoryMode::Normal))
    );
    assert_eq!(app.app_view().history().unwrap().worklogs(), vec![active]);
    assert_eq!(text(app.app_view().status()), ACTIVE_WORKLOG_DELETE_MESSAGE);
}
#[test]
fn successful_deletion_does_not_reanchor_an_unrelated_timer() {
    let alpha = task(1, "alpha");
    let target = history_worklog(10, alpha.id(), 100);
    let active = Worklog::begin(worklog_id(11), alpha.id(), at(300));
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.tracking = TrackingState::Running {
        worklog: ActiveWorklog::begin(active.id(), alpha.id(), active.start()),
    };
    service.worklog_pages = vec![Ok(page_with_active(
        vec![active.clone(), target.clone()],
        Some(active.clone()),
        None,
    ))];
    service.authoritative_worklogs = vec![active.clone(), target];
    let (mut app, clock) = app_with_test_clock(service, chrono_tz::UTC, active.start());
    app.handle(Command::OpenHistory);
    app.handle(Command::MoveDown);
    clock.advance_monotonic(Duration::from_secs(600));
    let before = app.app_view().elapsed().unwrap();
    app.handle(Command::OpenDeletion);
    app.handle(Command::Confirm);
    assert!(app.app_view().elapsed().unwrap() >= before);
}
#[test]
fn enter_opens_the_selected_tasks_history_and_escape_returns_to_it() {
    let alpha = task(1, "alpha");
    let first = page(
        vec![
            history_worklog(11, alpha.id(), 200),
            history_worklog(10, alpha.id(), 100),
        ],
        Some(cursor(100, 10)),
    );
    let second = page(vec![history_worklog(12, alpha.id(), 300)], None);
    let mut service = TestService::with_tasks(vec![alpha.clone(), task(2, "beta")]);
    service.worklog_pages = vec![Ok(first.clone()), Ok(second.clone())];
    let mut app = App::load(service);

    app.handle(Command::OpenHistory);

    assert_eq!(app.app_view().screen(), Screen::WorklogHistory);
    let history = app.app_view().history().expect("the history is open");
    assert_eq!(history.task_id(), alpha.id());
    assert_eq!(history.worklogs(), first.worklogs);
    assert_eq!(history.next_cursor(), first.next_cursor);
    assert_eq!(
        app.app_view().history_selected_index(),
        Some(0),
        "the newest row leads"
    );
    assert_eq!(
        app.app_view().status(),
        &Status::Info("History of \"alpha\"".to_owned())
    );

    // The task-list selection was never touched, so Escape returns to
    // the same task, and reopening loads the newest page afresh.
    app.handle(Command::MoveDown);
    app.handle(Command::BackToTaskList);
    assert_eq!(app.app_view().screen(), Screen::TaskList);
    assert_eq!(app.app_view().history(), None);
    assert_eq!(app.app_view().selected(), Some(0));
    assert_eq!(app.app_view().tasks()[0].id(), alpha.id());

    app.handle(Command::OpenHistory);
    assert_eq!(
        app.app_view().history().unwrap().worklogs(),
        second.worklogs
    );
}
#[test]
fn enter_opens_the_history_from_the_archived_view() {
    let gone = archived_task(3, "gone");
    let mut service = TestService::with_tasks(vec![task(1, "alpha"), gone.clone()]);
    service.worklog_pages = vec![Ok(page(vec![history_worklog(5, gone.id(), 100)], None))];
    let mut app = App::load(service);
    app.handle(Command::ShowArchivedTasks);

    app.handle(Command::OpenHistory);

    assert_eq!(app.app_view().screen(), Screen::WorklogHistory);
    assert_eq!(app.app_view().history().unwrap().task_id(), gone.id());

    app.handle(Command::BackToTaskList);
    assert_eq!(app.app_view().view(), TaskView::Archived);
    assert_eq!(
        app.app_view().selected(),
        Some(0),
        "the archived selection returned"
    );
}
#[test]
fn a_failed_history_load_keeps_the_task_list_and_reports_the_error() {
    let mut service = TestService::with_tasks(vec![task(1, "alpha")]);
    service.worklog_pages = vec![Err(TestService::failure())];
    let mut app = App::load(service);

    app.handle(Command::OpenHistory);

    assert_eq!(app.app_view().screen(), Screen::TaskList);
    assert_eq!(app.app_view().history(), None);
    assert_eq!(
        app.app_view().status(),
        &Status::Error("Storage error".to_owned())
    );
}
#[test]
fn history_movement_clamps_without_wrapping() {
    let alpha = task(1, "alpha");
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.worklog_pages = vec![Ok(page(
        vec![
            history_worklog(13, alpha.id(), 300),
            history_worklog(12, alpha.id(), 200),
            history_worklog(11, alpha.id(), 100),
        ],
        None,
    ))];
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);

    app.handle(Command::MoveUp);
    assert_eq!(
        app.app_view().history_selected_index(),
        Some(0),
        "no wrap to the end"
    );
    for _ in 0..5 {
        app.handle(Command::MoveDown);
    }
    assert_eq!(
        app.app_view().history_selected_index(),
        Some(2),
        "no wrap to the start"
    );
    app.handle(Command::MoveUp);
    assert_eq!(app.app_view().history_selected_index(), Some(1));
}
#[test]
fn an_empty_history_has_no_selection_and_movement_does_nothing() {
    let mut service = TestService::with_tasks(vec![task(1, "alpha")]);
    service.worklog_pages = vec![Ok(page(Vec::new(), None))];
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);

    assert_eq!(app.app_view().history_selected_index(), None);
    app.handle(Command::MoveDown);
    app.handle(Command::MoveUp);
    assert_eq!(app.app_view().history_selected_index(), None);
}
#[test]
fn loading_older_appends_the_page_and_keeps_the_selection() {
    let alpha = task(1, "alpha");
    let first = page(
        vec![
            history_worklog(20, alpha.id(), 200),
            history_worklog(19, alpha.id(), 100),
        ],
        Some(cursor(100, 19)),
    );
    let older = page(
        vec![
            history_worklog(18, alpha.id(), 50),
            history_worklog(17, alpha.id(), 40),
        ],
        None,
    );
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.worklog_pages = vec![Ok(first), Ok(older)];
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);
    app.handle(Command::MoveDown);
    assert_eq!(app.app_view().history_selected_index(), Some(1));

    app.handle(Command::LoadOlderWorklogs);

    let history = app.app_view().history().expect("the history is still open");
    assert_eq!(history.worklogs().len(), 4, "the older page was appended");
    assert_eq!(
        history
            .worklogs()
            .iter()
            .map(|worklog| worklog.id())
            .collect::<Vec<_>>(),
        vec![
            worklog_id(20),
            worklog_id(19),
            worklog_id(18),
            worklog_id(17)
        ],
        "older worklogs follow the loaded ones"
    );
    assert_eq!(history.next_cursor(), None);
    assert_eq!(
        app.app_view().history_selected_index(),
        Some(1),
        "the row stayed put"
    );
    assert_eq!(
        app.app_view().status(),
        &Status::Info("Loaded 2 older worklogs".to_owned())
    );
}
#[test]
fn loading_older_reloads_newest_when_the_loaded_active_worklog_stopped() {
    let alpha = task(1, "alpha");
    let active = Worklog::begin(worklog_id(20), alpha.id(), at(200));
    let first = page(vec![active], Some(cursor(200, 20)));
    let continuation = page_with_active(vec![history_worklog(19, alpha.id(), 100)], None, None);
    let newest = page(vec![history_worklog(20, alpha.id(), 200)], None);
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.worklog_pages = vec![Ok(first), Ok(continuation), Ok(newest)];
    let spy = service.spy();
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);

    app.handle(Command::LoadOlderWorklogs);

    assert_eq!(
        app.app_view()
            .history()
            .unwrap()
            .worklogs()
            .iter()
            .map(Worklog::id)
            .collect::<Vec<_>>(),
        vec![worklog_id(20)],
        "the continuation was discarded after the active row changed"
    );
    assert_eq!(
        app.app_view().status(),
        &Status::Info("History changed and was refreshed".to_owned())
    );
    assert_eq!(spy.worklog_reads(), 3);
}
#[test]
fn loading_older_appends_when_the_active_worklog_is_unchanged() {
    let alpha = task(1, "alpha");
    let active = Worklog::begin(worklog_id(20), alpha.id(), at(200));
    let first = page(vec![active.clone()], Some(cursor(200, 20)));
    let continuation = page_with_active(
        vec![history_worklog(19, alpha.id(), 100)],
        Some(active),
        None,
    );
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.worklog_pages = vec![Ok(first), Ok(continuation)];
    let spy = service.spy();
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);

    app.handle(Command::LoadOlderWorklogs);

    assert_eq!(
        app.app_view()
            .history()
            .unwrap()
            .worklogs()
            .iter()
            .map(Worklog::id)
            .collect::<Vec<_>>(),
        vec![worklog_id(20), worklog_id(19)]
    );
    assert_eq!(
        app.app_view().status(),
        &Status::Info("Loaded 1 older worklogs".to_owned())
    );
    assert_eq!(spy.worklog_reads(), 2);
}
#[test]
fn loading_older_appends_an_unchanged_active_worklog_after_a_full_newest_page() {
    let alpha = task(1, "alpha");
    let active = Worklog::begin(worklog_id(51), alpha.id(), at(500));
    let newest = (1..=50)
        .map(|tag| Worklog::new(worklog_id(tag), alpha.id(), at(500), Some(at(500))).unwrap())
        .collect();
    let first = page_with_active(newest, Some(active.clone()), Some(cursor(500, 50)));
    let continuation = page_with_active(
        vec![active.clone(), history_worklog(52, alpha.id(), 400)],
        Some(active.clone()),
        Some(cursor(400, 52)),
    );
    let older = page_with_active(
        vec![history_worklog(53, alpha.id(), 300)],
        Some(active),
        None,
    );
    let mut service = TestService::with_tasks(vec![alpha]);
    service.worklog_pages = vec![Ok(first), Ok(continuation), Ok(older)];
    let spy = service.spy();
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);

    app.handle(Command::LoadOlderWorklogs);

    let history = app.app_view().history().unwrap();
    assert_eq!(history.worklogs().len(), 52);
    assert_eq!(history.worklogs()[50].id(), worklog_id(51));
    assert_eq!(history.next_cursor(), Some(cursor(400, 52)));

    app.handle(Command::LoadOlderWorklogs);

    let history = app.app_view().history().unwrap();
    assert_eq!(history.worklogs().len(), 53);
    assert_eq!(history.worklogs()[52].id(), worklog_id(53));
    assert_eq!(history.next_cursor(), None);
    assert_eq!(spy.worklog_reads(), 3);
}
#[test]
fn loading_older_reloads_newest_when_the_active_worklog_start_changes() {
    let alpha = task(1, "alpha");
    let first = page(
        vec![Worklog::begin(worklog_id(20), alpha.id(), at(200))],
        Some(cursor(200, 20)),
    );
    let active = Worklog::begin(worklog_id(20), alpha.id(), at(300));
    let continuation = page_with_active(
        vec![history_worklog(19, alpha.id(), 100)],
        Some(active.clone()),
        None,
    );
    let newest = page_with_active(vec![active.clone()], Some(active), None);
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.worklog_pages = vec![Ok(first), Ok(continuation), Ok(newest)];
    let spy = service.spy();
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);

    app.handle(Command::LoadOlderWorklogs);

    assert_eq!(
        app.app_view()
            .history()
            .unwrap()
            .worklogs()
            .iter()
            .map(Worklog::id)
            .collect::<Vec<_>>(),
        vec![worklog_id(20)]
    );
    assert_eq!(
        app.app_view().status(),
        &Status::Info("History changed and was refreshed".to_owned())
    );
    assert_eq!(spy.worklog_reads(), 3);
}
#[test]
fn loading_older_reloads_newest_when_active_work_starts_for_the_open_task() {
    let alpha = task(1, "alpha");
    let first = page(
        vec![
            history_worklog(20, alpha.id(), 200),
            history_worklog(19, alpha.id(), 100),
        ],
        Some(cursor(100, 19)),
    );
    let active = Worklog::begin(worklog_id(21), alpha.id(), at(300));
    let continuation = page_with_active(
        vec![history_worklog(18, alpha.id(), 50)],
        Some(active.clone()),
        None,
    );
    let newest = page_with_active(
        vec![active, history_worklog(20, alpha.id(), 200)],
        Some(Worklog::begin(worklog_id(21), alpha.id(), at(300))),
        Some(cursor(200, 20)),
    );
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.worklog_pages = vec![Ok(first), Ok(continuation), Ok(newest)];
    let spy = service.spy();
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);

    app.handle(Command::LoadOlderWorklogs);

    assert_eq!(
        app.app_view()
            .history()
            .unwrap()
            .worklogs()
            .iter()
            .map(Worklog::id)
            .collect::<Vec<_>>(),
        vec![worklog_id(21), worklog_id(20)]
    );
    assert_eq!(
        app.app_view().status(),
        &Status::Info("History changed and was refreshed".to_owned())
    );
    assert_eq!(spy.worklog_reads(), 3);
}
#[test]
fn loading_older_ignores_active_work_for_another_task() {
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    let first = page(
        vec![
            history_worklog(20, alpha.id(), 200),
            history_worklog(19, alpha.id(), 100),
        ],
        Some(cursor(100, 19)),
    );
    let continuation = page_with_active(
        vec![history_worklog(18, alpha.id(), 50)],
        Some(Worklog::begin(worklog_id(21), beta.id(), at(300))),
        None,
    );
    let mut service = TestService::with_tasks(vec![alpha.clone(), beta]);
    service.worklog_pages = vec![Ok(first), Ok(continuation)];
    let spy = service.spy();
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);

    app.handle(Command::LoadOlderWorklogs);

    assert_eq!(
        app.app_view()
            .history()
            .unwrap()
            .worklogs()
            .iter()
            .map(Worklog::id)
            .collect::<Vec<_>>(),
        vec![worklog_id(20), worklog_id(19), worklog_id(18)]
    );
    assert_eq!(
        app.app_view().status(),
        &Status::Info("Loaded 1 older worklogs".to_owned())
    );
    assert_eq!(spy.worklog_reads(), 2);
}
#[test]
fn a_failed_active_row_reload_marks_history_unavailable() {
    let alpha = task(1, "alpha");
    let active = Worklog::begin(worklog_id(20), alpha.id(), at(200));
    let first = page(vec![active], Some(cursor(200, 20)));
    let continuation = page_with_active(vec![history_worklog(19, alpha.id(), 100)], None, None);
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.worklog_pages = vec![Ok(first), Ok(continuation), Err(TestService::failure())];
    let spy = service.spy();
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);

    app.handle(Command::LoadOlderWorklogs);

    let history = app.app_view().history().unwrap();
    assert_eq!(history.availability(), HistoryAvailability::Unavailable);
    assert!(history.worklogs().is_empty());
    assert_eq!(spy.worklog_reads(), 3);
}
#[test]
fn loading_older_at_the_end_of_the_history_changes_nothing() {
    let alpha = task(1, "alpha");
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.worklog_pages = vec![Ok(page(vec![history_worklog(20, alpha.id(), 200)], None))];
    let spy = service.spy();
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);

    app.handle(Command::LoadOlderWorklogs);

    assert_eq!(app.app_view().history().unwrap().worklogs().len(), 1);
    assert_eq!(app.app_view().history_selected_index(), Some(0));
    assert_eq!(
        app.app_view().status(),
        &Status::Info("No older worklogs".to_owned())
    );
    assert_eq!(
        spy.worklog_reads(),
        1,
        "the end of the history is not read again"
    );
}
#[test]
fn a_failed_load_older_preserves_the_displayed_history() {
    let alpha = task(1, "alpha");
    let first = page(
        vec![
            history_worklog(20, alpha.id(), 200),
            history_worklog(19, alpha.id(), 100),
        ],
        Some(cursor(100, 19)),
    );
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.worklog_pages = vec![Ok(first), Err(TestService::failure())];
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);
    app.handle(Command::MoveDown);

    app.handle(Command::LoadOlderWorklogs);

    let history = app.app_view().history().expect("the history is still open");
    assert_eq!(history.worklogs().len(), 2, "no row was lost");
    assert_eq!(
        history.next_cursor(),
        Some(cursor(100, 19)),
        "the cursor state survived"
    );
    assert_eq!(
        app.app_view().history_selected_index(),
        Some(1),
        "the row stayed put"
    );
    assert_eq!(
        app.app_view().status(),
        &Status::Error("Storage error".to_owned())
    );
}
#[test]
fn history_change_while_loading_older_resets_to_the_newest_page() {
    let alpha = task(1, "alpha");
    let first = page(
        vec![
            history_worklog(20, alpha.id(), 200),
            history_worklog(19, alpha.id(), 100),
        ],
        Some(cursor(100, 19)),
    );
    let newest = page(
        vec![
            history_worklog(21, alpha.id(), 300),
            history_worklog(19, alpha.id(), 90),
        ],
        Some(cursor(90, 19)),
    );
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.worklog_pages = vec![
        Ok(first),
        Err(ApplicationError::worklog_history_changed(alpha.id())),
        Ok(newest),
    ];
    let spy = service.spy();
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);
    app.handle(Command::MoveDown);

    app.handle(Command::LoadOlderWorklogs);

    let history = app.app_view().history().unwrap();
    assert_eq!(
        history
            .worklogs()
            .iter()
            .map(Worklog::id)
            .collect::<Vec<_>>(),
        vec![worklog_id(21), worklog_id(19)]
    );
    assert_eq!(history.next_cursor(), Some(cursor(90, 19)));
    assert_eq!(app.app_view().history_selected_index(), Some(1));
    assert_eq!(
        app.app_view().status(),
        &Status::Info("History changed and was refreshed".to_owned())
    );
    assert_eq!(spy.worklog_reads(), 3);
}
#[test]
fn failed_reset_after_history_change_marks_history_unavailable() {
    let alpha = task(1, "alpha");
    let first = page(
        vec![
            history_worklog(20, alpha.id(), 200),
            history_worklog(19, alpha.id(), 100),
        ],
        Some(cursor(100, 19)),
    );
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.worklog_pages = vec![
        Ok(first),
        Err(ApplicationError::worklog_history_changed(alpha.id())),
        Err(TestService::failure()),
    ];
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);
    app.handle(Command::MoveDown);

    app.handle(Command::LoadOlderWorklogs);

    let history = app.app_view().history().unwrap();
    assert_eq!(history.availability(), HistoryAvailability::Unavailable);
    assert!(history.worklogs().is_empty());
    assert_eq!(history.next_cursor(), None);
    assert_eq!(history.selected_id(), Some(worklog_id(19)));
    assert_eq!(app.app_view().history_selected_index(), None);
    assert_eq!(
        app.app_view().status(),
        &Status::Error("History changed, but refresh failed: Storage error".to_owned())
    );
}
#[test]
fn refresh_reloads_the_newest_page_and_keeps_the_selection_by_id() {
    let alpha = task(1, "alpha");
    let first = page(
        vec![
            history_worklog(20, alpha.id(), 200),
            history_worklog(19, alpha.id(), 100),
        ],
        Some(cursor(100, 19)),
    );
    let newest = page(
        vec![
            history_worklog(21, alpha.id(), 300),
            history_worklog(19, alpha.id(), 100),
        ],
        None,
    );
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.worklog_pages = vec![Ok(first), Ok(newest)];
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);
    app.handle(Command::MoveDown);

    app.handle(Command::RefreshWorklogs);

    let history = app.app_view().history().expect("the history is still open");
    assert_eq!(
        history
            .worklogs()
            .iter()
            .map(|worklog| worklog.id())
            .collect::<Vec<_>>(),
        vec![worklog_id(21), worklog_id(19)],
        "the newest page replaced the loaded one"
    );
    assert_eq!(history.next_cursor(), None);
    assert_eq!(
        app.app_view().history_selected_index(),
        Some(1),
        "the selection followed its worklog id"
    );
    assert_eq!(
        app.app_view().status(),
        &Status::Info("Refreshed".to_owned())
    );
}
#[test]
fn refresh_falls_back_to_the_newest_row_when_the_selection_left_the_page() {
    let alpha = task(1, "alpha");
    let first = page(
        vec![
            history_worklog(20, alpha.id(), 300),
            history_worklog(19, alpha.id(), 200),
            history_worklog(18, alpha.id(), 100),
        ],
        Some(cursor(100, 18)),
    );
    let older = page(vec![history_worklog(17, alpha.id(), 50)], None);
    // The reload keeps only the newest page; the selected worklog of
    // the longer loaded range is not on it.
    let newest = page(vec![history_worklog(20, alpha.id(), 300)], None);
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.worklog_pages = vec![Ok(first), Ok(older), Ok(newest)];
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);
    app.handle(Command::LoadOlderWorklogs);
    for _ in 0..3 {
        app.handle(Command::MoveDown);
    }
    assert_eq!(app.app_view().history_selected_index(), Some(3));

    app.handle(Command::RefreshWorklogs);

    assert_eq!(app.app_view().history().unwrap().worklogs().len(), 1);
    assert_eq!(app.app_view().history_selected_index(), Some(0));
}
#[test]
fn a_failed_refresh_preserves_the_displayed_history() {
    let alpha = task(1, "alpha");
    let first = page(
        vec![history_worklog(20, alpha.id(), 200)],
        Some(cursor(200, 20)),
    );
    let mut service = TestService::with_tasks(vec![alpha.clone()]);
    service.worklog_pages = vec![Ok(first), Err(TestService::failure())];
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);

    app.handle(Command::RefreshWorklogs);

    let history = app.app_view().history().expect("the history is still open");
    assert_eq!(
        history
            .worklogs()
            .iter()
            .map(|worklog| worklog.id())
            .collect::<Vec<_>>(),
        vec![worklog_id(20)],
        "no row was lost"
    );
    assert_eq!(history.next_cursor(), Some(cursor(200, 20)));
    assert_eq!(app.app_view().history_selected_index(), Some(0));
    assert_eq!(
        app.app_view().status(),
        &Status::Error("Storage error".to_owned())
    );
}
#[test]
fn task_list_commands_do_not_act_on_the_history_screen() {
    let alpha = task(1, "alpha");
    let first = page(
        vec![
            history_worklog(20, alpha.id(), 200),
            history_worklog(19, alpha.id(), 100),
        ],
        None,
    );
    let mut service = TestService::with_tasks(vec![alpha.clone(), task(2, "beta")]);
    service.worklog_pages = vec![Ok(first)];
    let spy = service.spy();
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);
    let before = app
        .app_view()
        .history()
        .expect("the history is open")
        .clone();
    let tasks_before = app.app_view().tasks().to_vec();

    app.handle(Command::ToggleTracking);
    app.handle(Command::CycleOrdering);
    app.handle(Command::ShowArchivedTasks);
    app.handle(Command::ShowActiveTasks);
    app.handle(Command::OpenAdd);
    app.handle(Command::OpenRename);
    app.handle(Command::OpenArchiveConfirm);
    app.handle(Command::UnarchiveSelected);
    app.handle(Command::OpenHistory);
    app.handle(Command::Insert('x'));
    app.handle(Command::Backspace);
    app.handle(Command::Confirm);
    app.handle(Command::Cancel);

    assert_eq!(app.app_view().screen(), Screen::WorklogHistory);
    assert!(
        matches!(app.app_view().screen_state(), ScreenState::WorklogHistory(state) if matches!(state.mode(), WorklogHistoryMode::Normal))
    );
    assert_eq!(app.app_view().history().unwrap(), &before);
    assert_eq!(app.app_view().view(), TaskView::Active);
    assert_eq!(app.app_view().ordering(), TaskOrdering::RecentlyWorked);
    assert_eq!(app.app_view().tasks(), tasks_before.as_slice());
    assert_eq!(
        app.app_view().active_task_id(),
        None,
        "no tracking was started"
    );
    assert_eq!(
        spy.worklog_reads(),
        1,
        "the open history was not read again"
    );
}
#[test]
fn history_commands_do_not_act_on_the_task_list() {
    let service = TestService::with_tasks(vec![task(1, "alpha")]);
    let spy = service.spy();
    let mut app = App::load(service);

    app.handle(Command::LoadOlderWorklogs);
    app.handle(Command::RefreshWorklogs);
    app.handle(Command::BackToTaskList);

    assert_eq!(app.app_view().screen(), Screen::TaskList);
    assert_eq!(app.app_view().history(), None);
    assert_eq!(app.app_view().status(), &Status::Info("Ready".to_owned()));
    assert_eq!(
        spy.worklog_reads(),
        0,
        "no history is open, so nothing was read"
    );
}
#[test]
fn opening_a_history_requires_normal_mode() {
    let service = TestService::with_tasks(vec![task(1, "alpha")]);
    let spy = service.spy();
    let mut app = App::load(service);
    app.handle(Command::OpenAdd);
    app.handle(Command::OpenHistory);
    assert!(
        app.app_view().screen() == Screen::TaskList
            && matches!(
                app.app_view().task_list().mode(),
                TaskListMode::Input { .. }
            )
    );
    assert_eq!(app.app_view().screen(), Screen::TaskList);
    assert_eq!(spy.worklog_reads(), 0);

    app.handle(Command::Cancel);
    app.handle(Command::OpenArchiveConfirm);
    app.handle(Command::OpenHistory);
    assert!(
        app.app_view().screen() == Screen::TaskList
            && matches!(
                app.app_view().task_list().mode(),
                TaskListMode::ConfirmArchive { .. }
            )
    );
    assert_eq!(app.app_view().screen(), Screen::TaskList);
    assert_eq!(spy.worklog_reads(), 0);
}
#[test]
fn quitting_from_the_history_leaves_tracking_active() {
    let task = task(1, "alpha");
    let mut service = TestService::with_tasks(vec![task.clone()]);
    service.tracking = TrackingState::Running {
        worklog: ActiveWorklog::begin(worklog_id(10), task.id(), at(100)),
    };
    service.worklog_pages = vec![Ok(page(
        vec![Worklog::begin(worklog_id(10), task.id(), at(100))],
        None,
    ))];
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);

    app.handle(Command::Quit);

    assert!(!app.is_running());
    assert_eq!(app.app_view().active_task_id(), Some(task.id()));
}
#[test]
fn opening_history_adopts_an_active_worklog_that_another_client_started() {
    let task = task(1, "alpha");
    let active = Worklog::begin(worklog_id(10), task.id(), at(100));
    let mut service = TestService::with_tasks(vec![task.clone()]);
    service.worklog_pages = vec![Ok(page(vec![active.clone()], None))];
    let (mut app, clock) = app_with_test_clock(service, chrono_tz::UTC, active.start());
    assert_eq!(app.app_view().active_task_id(), None);

    app.handle(Command::OpenHistory);

    assert_eq!(app.app_view().active_task_id(), Some(task.id()));
    assert_eq!(app.app_view().active_worklog_id(), Some(active.id()));
    clock.advance_monotonic(Duration::from_secs(125));
    assert_eq!(
        app.app_view().history_row_duration(&active).as_secs(),
        app.app_view()
            .elapsed()
            .expect("the header adopted a monotonic clock")
            .as_secs()
    );
}
#[test]
fn history_row_durations_use_the_monotonic_clock_for_the_running_worklog() {
    let task = task(1, "alpha");
    let active = Worklog::begin(worklog_id(10), task.id(), at(100));
    let mut service = TestService::with_tasks(vec![task.clone()]);
    service.tracking = TrackingState::Running {
        worklog: ActiveWorklog::begin(active.id(), task.id(), at(100)),
    };
    service.worklog_pages = vec![Ok(page(
        vec![active, history_worklog(11, task.id(), 200)],
        None,
    ))];
    let (mut app, clock) = app_with_test_clock(service, chrono_tz::UTC, at(100));
    app.handle(Command::OpenHistory);
    clock.advance_monotonic(Duration::from_secs(125));

    let rows = app
        .app_view()
        .history()
        .expect("the history is open")
        .worklogs()
        .to_vec();
    let running = app.app_view().history_row_duration(&rows[0]);
    assert!(
        running >= Duration::from_secs(125) && running < Duration::from_secs(126),
        "the running row shares the header's clock, got {running:?}"
    );
    assert!(
        app.app_view().elapsed().unwrap() >= Duration::from_secs(125),
        "the header reads the same clock"
    );
    assert_eq!(
        app.app_view().history_row_duration(&rows[1]),
        Duration::from_secs(60),
        "a stopped row derives its duration from its stored times"
    );
    let unmatched = Worklog::begin(worklog_id(12), task.id(), at(300));
    assert_eq!(
        app.app_view().history_row_duration(&unmatched),
        Duration::ZERO,
        "an inconsistent running row never falls back to wall time"
    );
}
#[test]
fn unavailable_history_renders_its_retry_message_with_the_focused_border() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;

    let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
    let worklog = history_worklog(10, task_id, 100);
    let mut service = TestService::with_tasks(vec![task(1, "alpha")]);
    service.worklog_pages = vec![Ok(page(vec![worklog], None)), Err(TestService::failure())];
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);
    app.handle(Command::OpenCorrection);
    app.handle(Command::Confirm);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| crate::ui::render(frame, app.app_view()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .flat_map(|y| (0..buffer.area.width).map(move |x| buffer[(x, y)].symbol()))
        .collect::<String>();

    assert!(text.contains("History unavailable. Press r to retry."));
    assert_eq!(buffer[(0, 1)].fg, Color::Blue);
}
