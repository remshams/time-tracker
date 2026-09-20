//! Tests for loading and refreshing history pages.

use super::*;

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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::MoveDown));
    assert_eq!(app.app_view().history_selected_index(), Some(1));

    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::LoadOlderWorklogs,
    ));

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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));

    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::LoadOlderWorklogs,
    ));

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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));

    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::LoadOlderWorklogs,
    ));

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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));

    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::LoadOlderWorklogs,
    ));

    let history = app.app_view().history().unwrap();
    assert_eq!(history.worklogs().len(), 52);
    assert_eq!(history.worklogs()[50].id(), worklog_id(51));
    assert_eq!(history.next_cursor(), Some(cursor(400, 52)));

    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::LoadOlderWorklogs,
    ));

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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));

    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::LoadOlderWorklogs,
    ));

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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));

    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::LoadOlderWorklogs,
    ));

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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));

    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::LoadOlderWorklogs,
    ));

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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));

    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::LoadOlderWorklogs,
    ));

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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));

    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::LoadOlderWorklogs,
    ));

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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::MoveDown));

    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::LoadOlderWorklogs,
    ));

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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::MoveDown));

    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::LoadOlderWorklogs,
    ));

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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::MoveDown));

    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::LoadOlderWorklogs,
    ));

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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::MoveDown));

    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::RefreshWorklogs,
    ));

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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::LoadOlderWorklogs,
    ));
    for _ in 0..3 {
        app.handle(Command::WorklogHistory(WorklogHistoryCommand::MoveDown));
    }
    assert_eq!(app.app_view().history_selected_index(), Some(3));

    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::RefreshWorklogs,
    ));

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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));

    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::RefreshWorklogs,
    ));

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
