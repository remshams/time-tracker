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
