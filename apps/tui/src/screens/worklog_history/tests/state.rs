//! Tests for stored history-screen state.

use super::*;

#[test]
fn history_owns_the_exact_task_list_state_it_will_restore() {
    let active = task(1, "active");
    let archived = archived_task(2, "archived");
    let mut service = TestService::with_tasks(vec![active, archived.clone()]);
    service.worklog_pages = vec![Ok(page(
        vec![history_worklog(10, archived.id(), 100)],
        None,
    ))];
    let mut app = App::load(service);
    app.handle(Command::ShowArchivedTasks);
    let expected = app.screen.task_list().clone();

    app.handle(Command::OpenHistory);

    let ScreenState::WorklogHistory(history) = &app.screen else {
        panic!("history should be open");
    };
    assert_eq!(history.task_list, expected);
    assert_eq!(history.mode, WorklogHistoryMode::Normal);

    app.handle(Command::BackToTaskList);
    assert_eq!(app.screen, ScreenState::TaskList(expected));
}

#[test]
fn history_keeps_task_navigation_after_its_read_changes_membership() {
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    let gamma = task(3, "gamma");
    let mut service = TestService::with_tasks(vec![alpha, beta.clone(), gamma.clone()]);
    service.tasks_after_next_worklog_read = Some(vec![beta.clone(), gamma]);
    service.worklog_pages = vec![Ok(page(vec![history_worklog(10, beta.id(), 100)], None))];
    let mut app = App::load(service);
    app.handle(Command::MoveDown);
    assert_eq!(app.tasks()[app.selected().unwrap()].id(), beta.id());
    assert_eq!(app.selected(), Some(1));

    app.handle(Command::OpenHistory);

    assert_eq!(app.screen(), Screen::WorklogHistory);
    assert_eq!(app.tasks().len(), 2, "the history read removed alpha");
    assert_eq!(
        app.selected(),
        Some(0),
        "selection followed beta's stable id"
    );
    assert_eq!(app.tasks()[app.selected().unwrap()].id(), beta.id());
    let expected = app.task_list().clone();
    let ScreenState::WorklogHistory(history) = &app.screen else {
        panic!("history should be open");
    };
    assert_eq!(history.task_list(), &expected);

    app.handle(Command::BackToTaskList);

    assert_eq!(app.screen, ScreenState::TaskList(expected));
    assert_eq!(app.tasks()[app.selected().unwrap()].id(), beta.id());
}
