//! Tests for command dispatch and screen boundaries.

use super::*;

#[test]
fn confirm_is_ignored_in_normal_history_mode() {
    let alpha = task(1, "alpha");
    let target = history_worklog(10, alpha.id(), 100);
    let mut service = TestService::with_tasks(vec![alpha]);
    service.worklog_pages = vec![Ok(page(vec![target], None))];
    let mut app = App::load(service);
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    let screen_before = app.app_view().screen_state().clone();
    let status_before = app.app_view().status().clone();

    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));

    assert_eq!(app.app_view().screen_state(), &screen_before);
    assert_eq!(app.app_view().status(), &status_before);
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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    let screen_before = app.app_view().screen_state().clone();
    let status_before = app.app_view().status().clone();
    let tasks_before = app.app_view().tasks().to_vec();

    for command in [
        TaskListCommand::MoveUp,
        TaskListCommand::MoveDown,
        TaskListCommand::ShowActiveTasks,
        TaskListCommand::ShowArchivedTasks,
        TaskListCommand::OpenHistory,
        TaskListCommand::CycleOrdering,
        TaskListCommand::UnarchiveSelected,
        TaskListCommand::ToggleTracking,
        TaskListCommand::OpenAdd,
        TaskListCommand::OpenRename,
        TaskListCommand::OpenArchiveConfirm,
        TaskListCommand::Confirm,
        TaskListCommand::Cancel,
        TaskListCommand::Insert('x'),
        TaskListCommand::Backspace,
    ] {
        app.handle(Command::TaskList(command));
    }

    assert_eq!(app.app_view().screen_state(), &screen_before);
    assert_eq!(app.app_view().status(), &status_before);
    assert_eq!(app.app_view().ordering(), TaskOrdering::RecentlyWorked);
    assert_eq!(app.app_view().tasks(), tasks_before.as_slice());
    assert_eq!(app.app_view().active_task_id(), None);
    assert_eq!(spy.worklog_reads(), 1);
    assert!(spy.correction_calls().is_empty());
    assert!(spy.deletion_calls().is_empty());
    assert!(spy.clear_calls().is_empty());
}
#[test]
fn history_commands_do_not_act_on_the_task_list() {
    let service = TestService::with_tasks(vec![task(1, "alpha")]);
    let spy = service.spy();
    let mut app = App::load(service);
    let screen_before = app.app_view().screen_state().clone();
    let status_before = app.app_view().status().clone();
    let tasks_before = app.app_view().tasks().to_vec();

    for command in [
        WorklogHistoryCommand::MoveUp,
        WorklogHistoryCommand::MoveDown,
        WorklogHistoryCommand::OpenCorrection,
        WorklogHistoryCommand::OpenDeletion,
        WorklogHistoryCommand::SwitchCorrectionField,
        WorklogHistoryCommand::MoveCursorLeft,
        WorklogHistoryCommand::MoveCursorRight,
        WorklogHistoryCommand::Delete,
        WorklogHistoryCommand::AdjustForwardFiveMinutes,
        WorklogHistoryCommand::AdjustBackwardFiveMinutes,
        WorklogHistoryCommand::AdjustForwardOneHour,
        WorklogHistoryCommand::AdjustBackwardOneHour,
        WorklogHistoryCommand::LoadOlderWorklogs,
        WorklogHistoryCommand::RefreshWorklogs,
        WorklogHistoryCommand::BackToTaskList,
        WorklogHistoryCommand::Confirm,
        WorklogHistoryCommand::Cancel,
        WorklogHistoryCommand::Insert('x'),
        WorklogHistoryCommand::Backspace,
    ] {
        app.handle(Command::WorklogHistory(command));
    }

    assert_eq!(app.app_view().screen_state(), &screen_before);
    assert_eq!(app.app_view().status(), &status_before);
    assert_eq!(app.app_view().tasks(), tasks_before.as_slice());
    assert_eq!(app.app_view().active_task_id(), None);
    assert_eq!(spy.worklog_reads(), 0);
    assert!(spy.correction_calls().is_empty());
    assert!(spy.deletion_calls().is_empty());
    assert!(spy.clear_calls().is_empty());
}
#[test]
fn opening_a_history_requires_normal_mode() {
    let service = TestService::with_tasks(vec![task(1, "alpha")]);
    let spy = service.spy();
    let mut app = App::load(service);
    app.handle(Command::TaskList(TaskListCommand::OpenAdd));
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    assert!(
        app.app_view().screen() == Screen::TaskList
            && matches!(
                app.app_view().task_list().mode(),
                TaskListMode::Input { .. }
            )
    );
    assert_eq!(app.app_view().screen(), Screen::TaskList);
    assert_eq!(spy.worklog_reads(), 0);

    app.handle(Command::TaskList(TaskListCommand::Cancel));
    app.handle(Command::TaskList(TaskListCommand::OpenArchiveConfirm));
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));

    app.handle(Command::Quit);

    assert!(!app.is_running());
    assert_eq!(app.app_view().active_task_id(), Some(task.id()));
}
