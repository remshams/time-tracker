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
    app.handle(Command::TaskList(TaskListCommand::ShowArchivedTasks));
    let expected = app.app_view().task_list().clone();

    app.handle(Command::TaskList(TaskListCommand::OpenHistory));

    let ScreenState::WorklogHistory(history) = app.app_view().screen_state() else {
        panic!("history should be open");
    };
    assert_eq!(history.mode(), &WorklogHistoryMode::Normal);
    assert_eq!(app.app_view().task_list(), &expected);

    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::BackToTaskList,
    ));
    assert!(matches!(
        app.app_view().screen_state(),
        ScreenState::TaskList(_)
    ));
    assert_eq!(app.app_view().task_list(), &expected);
}

#[test]
fn history_read_keeps_unrelated_tasks_until_returning_to_the_task_list() {
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    let gamma = task(3, "gamma");
    let mut service = TestService::with_tasks(vec![alpha, beta.clone(), gamma.clone()]);
    service.tasks_after_next_worklog_read = Some(vec![beta.clone(), gamma]);
    service.worklog_pages = vec![Ok(page(vec![history_worklog(10, beta.id(), 100)], None))];
    let mut app = App::load(service);
    app.handle(Command::TaskList(TaskListCommand::MoveDown));
    assert_eq!(
        app.app_view().tasks()[app.app_view().selected().unwrap()].id(),
        beta.id()
    );
    assert_eq!(app.app_view().selected(), Some(1));

    app.handle(Command::TaskList(TaskListCommand::OpenHistory));

    assert_eq!(app.app_view().screen(), Screen::WorklogHistory);
    assert_eq!(
        app.app_view().tasks().len(),
        3,
        "the history read leaves unrelated task metadata unchanged"
    );
    assert_eq!(
        app.app_view().selected(),
        Some(1),
        "selection remains on beta's stable id"
    );
    assert_eq!(
        app.app_view().tasks()[app.app_view().selected().unwrap()].id(),
        beta.id()
    );
    let expected = app.app_view().task_list().clone();
    assert!(matches!(
        app.app_view().screen_state(),
        ScreenState::WorklogHistory(_)
    ));

    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::BackToTaskList,
    ));

    assert!(matches!(
        app.app_view().screen_state(),
        ScreenState::TaskList(_)
    ));
    assert_eq!(app.app_view().task_list(), &expected);
    assert_eq!(
        app.app_view().tasks()[app.app_view().selected().unwrap()].id(),
        beta.id()
    );
}
