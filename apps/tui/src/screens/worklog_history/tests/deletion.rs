//! Tests for deleting worklogs from history.

use super::*;

#[test]
fn deletion_opening_requires_normal_history_mode() {
    let alpha = task(1, "alpha");
    let target = history_worklog(10, alpha.id(), 100);
    let mut service = TestService::with_tasks(vec![alpha]);
    service.worklog_pages = vec![Ok(page(vec![target.clone()], None))];
    service.authoritative_worklogs = vec![target];
    let mut app = App::load(service);
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::OpenCorrection,
    ));
    let correction = app.app_view().correction().cloned();
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::OpenDeletion));
    assert_eq!(app.app_view().correction(), correction.as_ref());
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Cancel));

    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::BackToTaskList,
    ));
    app.handle(Command::TaskList(TaskListCommand::OpenArchiveConfirm));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::OpenDeletion));
    assert!(
        app.app_view().screen() == Screen::TaskList
            && matches!(
                app.app_view().task_list().mode(),
                TaskListMode::ConfirmArchive { .. }
            )
    );

    app.handle(Command::TaskList(TaskListCommand::Cancel));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::OpenDeletion));
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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::OpenDeletion));

    assert_eq!(app.app_view().deletion(), Some(&target));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));

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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::MoveDown));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::OpenDeletion));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));
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

    app.handle(Command::WorklogHistory(WorklogHistoryCommand::OpenDeletion));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));
    assert_eq!(app.app_view().history_selected_index(), Some(0));
    assert_eq!(spy.latest_work_start(alpha.id()), Some(at(300)));

    app.handle(Command::WorklogHistory(WorklogHistoryCommand::OpenDeletion));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));
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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));

    for expected in [Some(at(200)), Some(at(100)), None] {
        app.handle(Command::WorklogHistory(WorklogHistoryCommand::OpenDeletion));
        app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));
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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::OpenDeletion));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));

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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::OpenDeletion));
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
    app.handle(Command::TaskList(TaskListCommand::ShowArchivedTasks));
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::OpenDeletion));
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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::OpenDeletion));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));
    assert_eq!(app.app_view().deletion(), Some(&target));
    assert_eq!(text(app.app_view().status()), "Storage error");
    assert!(!text(app.app_view().status()).contains("private backend detail"));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Cancel));
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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::OpenDeletion));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));

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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::OpenDeletion));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));

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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::OpenDeletion));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));

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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::OpenDeletion));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));

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
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::OpenDeletion));
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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));

    for _ in 0..50 {
        app.handle(Command::WorklogHistory(WorklogHistoryCommand::OpenDeletion));
        app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));
    }

    let history = app.app_view().history().unwrap();
    assert!(history.worklogs().is_empty());
    assert_eq!(history.next_cursor(), Some(cursor(1_950, 50)));
    assert_eq!(app.app_view().history_selected_index(), None);

    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::LoadOlderWorklogs,
    ));

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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::OpenDeletion));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));

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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::MoveDown));
    clock.advance_monotonic(Duration::from_secs(600));
    let before = app.app_view().elapsed().unwrap();
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::OpenDeletion));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));
    assert!(app.app_view().elapsed().unwrap() >= before);
}
