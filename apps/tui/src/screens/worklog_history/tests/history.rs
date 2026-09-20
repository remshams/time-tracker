//! Tests for opening, navigating, and rendering history.

use super::*;

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

    app.handle(Command::TaskList(TaskListCommand::OpenHistory));

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
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::MoveDown));
    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::BackToTaskList,
    ));
    assert_eq!(app.app_view().screen(), Screen::TaskList);
    assert_eq!(app.app_view().history(), None);
    assert_eq!(app.app_view().selected(), Some(0));
    assert_eq!(app.app_view().tasks()[0].id(), alpha.id());

    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
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
    app.handle(Command::TaskList(TaskListCommand::ShowArchivedTasks));

    app.handle(Command::TaskList(TaskListCommand::OpenHistory));

    assert_eq!(app.app_view().screen(), Screen::WorklogHistory);
    assert_eq!(app.app_view().history().unwrap().task_id(), gone.id());

    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::BackToTaskList,
    ));
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

    app.handle(Command::TaskList(TaskListCommand::OpenHistory));

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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));

    app.handle(Command::WorklogHistory(WorklogHistoryCommand::MoveUp));
    assert_eq!(
        app.app_view().history_selected_index(),
        Some(0),
        "no wrap to the end"
    );
    for _ in 0..5 {
        app.handle(Command::WorklogHistory(WorklogHistoryCommand::MoveDown));
    }
    assert_eq!(
        app.app_view().history_selected_index(),
        Some(2),
        "no wrap to the start"
    );
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::MoveUp));
    assert_eq!(app.app_view().history_selected_index(), Some(1));
}
#[test]
fn an_empty_history_has_no_selection_and_movement_does_nothing() {
    let mut service = TestService::with_tasks(vec![task(1, "alpha")]);
    service.worklog_pages = vec![Ok(page(Vec::new(), None))];
    let mut app = App::load(service);
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));

    assert_eq!(app.app_view().history_selected_index(), None);
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::MoveDown));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::MoveUp));
    assert_eq!(app.app_view().history_selected_index(), None);
}
#[test]
fn opening_history_adopts_an_active_worklog_that_another_client_started() {
    let task = task(1, "alpha");
    let active = Worklog::begin(worklog_id(10), task.id(), at(100));
    let mut service = TestService::with_tasks(vec![task.clone()]);
    service.worklog_pages = vec![Ok(page(vec![active.clone()], None))];
    let (mut app, clock) = app_with_test_clock(service, chrono_tz::UTC, active.start());
    assert_eq!(app.app_view().active_task_id(), None);

    app.handle(Command::TaskList(TaskListCommand::OpenHistory));

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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
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
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::OpenCorrection,
    ));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));
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
