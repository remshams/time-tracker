//! Tests for tasks behavior.

use super::*;

#[test]
fn fresh_and_empty_apps_have_safe_selection() {
    let app = app_with(&["one", "two"]);
    assert_eq!(app.selected(), Some(0));
    assert_eq!(app.status(), &Status::Info("Ready".to_owned()));
    let mut empty = app_with(&[]);
    empty.handle(Command::MoveDown);
    empty.handle(Command::MoveUp);
    assert_eq!(empty.selected(), None);
}
#[test]
fn selection_movement_stays_inside_the_task_list() {
    let mut app = app_with(&["one", "two", "three"]);
    app.handle(Command::MoveUp);
    assert_eq!(app.selected(), Some(0));
    for _ in 0..5 {
        app.handle(Command::MoveDown);
    }
    assert_eq!(app.selected(), Some(2));
    app.handle(Command::MoveUp);
    assert_eq!(app.selected(), Some(1));
    app.screen.task_list_mut().active_selection = None;
    app.handle(Command::MoveUp);
    assert_eq!(app.selected(), Some(2));
}
#[test]
fn adding_a_task_updates_the_list_and_selection() {
    let mut app = app_with(&["one"]);
    app.handle(Command::OpenAdd);
    for character in "new task".chars() {
        app.handle(Command::Insert(character));
    }
    app.handle(Command::Confirm);
    assert_eq!(app.mode(), Mode::Normal);
    assert_eq!(app.selected(), Some(0));
    assert_eq!(
        app.tasks()[app.selected().unwrap()].name().as_str(),
        "new task"
    );
    assert_eq!(app.status(), &Status::Info("Added \"new task\"".to_owned()));
}
#[test]
fn adding_in_the_middle_selects_the_new_task() {
    let task = |tag, name| {
        Task::create(
            TaskId::from_uuid(uuid::Uuid::from_u128(tag)),
            TaskName::new(name).unwrap(),
            at(100),
        )
    };
    let mut app = App::load(TestService::with_tasks(vec![
        task(1, "one"),
        task(3, "three"),
    ]));
    app.handle(Command::MoveDown);
    app.handle(Command::OpenAdd);
    app.handle(Command::Insert('t'));
    app.handle(Command::Confirm);
    assert_eq!(
        app.tasks().iter().map(|task| task.id()).collect::<Vec<_>>(),
        vec![
            TaskId::from_uuid(uuid::Uuid::from_u128(2)),
            TaskId::from_uuid(uuid::Uuid::from_u128(1)),
            TaskId::from_uuid(uuid::Uuid::from_u128(3)),
        ]
    );
    assert_eq!(app.selected(), Some(0));
}
#[test]
fn refreshing_tasks_preserves_the_selected_task_after_reordering() {
    let task = |tag, name| {
        Task::create(
            TaskId::from_uuid(uuid::Uuid::from_u128(tag)),
            TaskName::new(name).unwrap(),
            at(100),
        )
    };
    let selected = task(3, "three");
    let mut app = App::load(TestService::with_tasks(vec![
        task(1, "one"),
        selected.clone(),
    ]));
    app.handle(Command::MoveDown);
    app.application.tasks.insert(1, task(2, "two"));
    app.sync_tasks_from_application();
    assert_eq!(app.selected(), Some(2));
    assert_eq!(app.tasks()[2].id(), selected.id());
}
#[test]
fn failed_writes_keep_the_active_modal_and_input_buffer() {
    let task = Task::create(
        TaskId::from_uuid(uuid::Uuid::from_u128(1)),
        TaskName::new("one").unwrap(),
        at(100),
    );
    let mut service = TestService::with_tasks(vec![task]);
    service.fail_create = true;
    service.fail_rename = true;
    service.fail_archive = true;
    let mut app = App::load(service);

    app.handle(Command::OpenAdd);
    for character in "blocked".chars() {
        app.handle(Command::Insert(character));
    }
    app.handle(Command::Confirm);
    assert!(matches!(
        app.mode(),
        Mode::Input {
            purpose: InputPurpose::Add,
            buffer,
        } if buffer == "blocked"
    ));
    assert_eq!(app.status(), &Status::Error("Storage error".to_owned()));
    app.handle(Command::Cancel);

    app.handle(Command::OpenRename);
    app.handle(Command::Confirm);
    assert!(matches!(
        app.mode(),
        Mode::Input {
            purpose: InputPurpose::Rename { .. },
            buffer,
        } if buffer == "one"
    ));
    assert_eq!(app.status(), &Status::Error("Storage error".to_owned()));
    app.handle(Command::Cancel);

    app.handle(Command::OpenArchiveConfirm);
    app.handle(Command::Confirm);
    assert!(matches!(app.mode(), Mode::ConfirmArchive { .. }));
    assert_eq!(app.status(), &Status::Error("Storage error".to_owned()));
}
#[test]
fn invalid_input_keeps_the_dialog_and_reports_the_same_text() {
    let mut app = app_with(&["one"]);
    app.handle(Command::OpenAdd);
    app.handle(Command::Confirm);
    assert!(matches!(app.mode(), Mode::Input { .. }));
    assert_eq!(
        app.status(),
        &Status::Error("The task name must not be empty".to_owned())
    );
}
#[test]
fn renaming_a_task_keeps_its_row_selected() {
    let mut app = app_with(&["old"]);
    app.handle(Command::OpenRename);
    for _ in 0..3 {
        app.handle(Command::Backspace);
    }
    for character in "new".chars() {
        app.handle(Command::Insert(character));
    }
    app.handle(Command::Confirm);
    assert_eq!(app.tasks()[0].name().as_str(), "new");
    assert_eq!(app.selected(), Some(0));
    assert_eq!(app.status(), &Status::Info("Renamed to \"new\"".to_owned()));
}
#[test]
fn archiving_hides_the_task_and_clamps_selection() {
    let mut app = app_with(&["alpha", "beta"]);
    app.handle(Command::MoveDown);
    app.handle(Command::OpenArchiveConfirm);
    app.handle(Command::Confirm);
    assert_eq!(app.tasks().len(), 1);
    assert_eq!(app.tasks()[0].name().as_str(), "alpha");
    assert_eq!(app.selected(), Some(0));
    assert_eq!(app.status(), &Status::Info("Archived \"beta\"".to_owned()));
}
#[test]
fn input_is_bounded_to_the_domain_limit() {
    let mut app = app_with(&[]);
    app.handle(Command::OpenAdd);
    for character in "x".repeat(TaskName::MAX_LEN + 10).chars() {
        app.handle(Command::Insert(character));
    }
    app.handle(Command::Confirm);
    assert_eq!(
        app.tasks()[0].name().as_str().chars().count(),
        TaskName::MAX_LEN
    );
}
#[test]
fn ordering_defaults_cycles_and_is_shared_by_both_views() {
    let tasks = vec![
        stamped_task(1, "older active", false, 100, 900),
        stamped_task(2, "newer active", false, 800, 800),
        stamped_task(3, "older archived", true, 200, 700),
        stamped_task(4, "newer archived", true, 600, 600),
    ];
    let mut service = TestService::with_tasks(tasks);
    service.latest_work_starts = vec![(TaskId::from_uuid(uuid::Uuid::from_u128(3)), at(1_000))];
    let mut app = App::load(service);

    assert_eq!(app.ordering(), TaskOrdering::RecentlyWorked);
    assert_eq!(app.tasks()[0].name().as_str(), "newer active");
    app.handle(Command::ShowArchivedTasks);
    assert_eq!(app.tasks()[0].name().as_str(), "older archived");
    app.handle(Command::ShowActiveTasks);
    app.handle(Command::CycleOrdering);
    assert_eq!(app.ordering(), TaskOrdering::RecentlyUpdated);
    assert_eq!(app.tasks()[0].name().as_str(), "older active");

    app.handle(Command::ShowArchivedTasks);
    assert_eq!(app.ordering(), TaskOrdering::RecentlyUpdated);
    assert_eq!(app.tasks()[0].name().as_str(), "older archived");
    app.handle(Command::CycleOrdering);
    assert_eq!(app.ordering(), TaskOrdering::RecentlyCreated);
    assert_eq!(app.tasks()[0].name().as_str(), "newer archived");
    app.handle(Command::CycleOrdering);
    assert_eq!(app.ordering(), TaskOrdering::RecentlyWorked);
}
#[test]
fn sorting_preserves_each_views_selected_task_id() {
    let active = stamped_task(1, "active selection", false, 100, 900);
    let archived = stamped_task(3, "archived selection", true, 100, 900);
    let mut app = App::load(TestService::with_tasks(vec![
        active.clone(),
        stamped_task(2, "new active", false, 800, 800),
        archived.clone(),
        stamped_task(4, "new archived", true, 800, 800),
    ]));

    app.handle(Command::MoveDown);
    assert_eq!(app.tasks()[app.selected().unwrap()].id(), active.id());
    app.handle(Command::ShowArchivedTasks);
    app.handle(Command::MoveDown);
    assert_eq!(app.tasks()[app.selected().unwrap()].id(), archived.id());

    app.handle(Command::CycleOrdering);
    assert_eq!(app.tasks()[app.selected().unwrap()].id(), archived.id());
    app.handle(Command::ShowActiveTasks);
    assert_eq!(app.tasks()[app.selected().unwrap()].id(), active.id());
}
#[test]
fn input_and_confirmation_modes_block_sorting() {
    let mut app = App::load(TestService::with_tasks(vec![task(1, "alpha")]));
    app.handle(Command::OpenAdd);
    app.handle(Command::CycleOrdering);
    assert_eq!(app.ordering(), TaskOrdering::RecentlyWorked);
    assert!(matches!(app.mode(), Mode::Input { .. }));

    app.handle(Command::Cancel);
    app.handle(Command::OpenArchiveConfirm);
    app.handle(Command::CycleOrdering);
    assert_eq!(app.ordering(), TaskOrdering::RecentlyWorked);
    assert!(matches!(app.mode(), Mode::ConfirmArchive { .. }));
}
#[test]
fn rename_reorders_recently_updated_and_keeps_the_task_selected() {
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    let mut app = App::load(TestService::with_tasks(vec![alpha, beta.clone()]));
    app.handle(Command::CycleOrdering);
    app.handle(Command::MoveDown);
    app.handle(Command::OpenRename);
    app.handle(Command::Backspace);
    app.handle(Command::Insert('x'));
    app.handle(Command::Confirm);

    assert_eq!(app.ordering(), TaskOrdering::RecentlyUpdated);
    assert_eq!(app.tasks()[0].id(), beta.id());
    assert_eq!(app.tasks()[app.selected().unwrap()].id(), beta.id());
}
#[test]
fn the_app_starts_in_the_active_view_and_switches_directionally() {
    let mut app = App::load(TestService::with_tasks(vec![
        task(1, "alpha"),
        archived_task(3, "gone"),
    ]));
    assert_eq!(app.view(), TaskView::Active);
    assert_eq!(app.tasks().len(), 1, "the archived task is not visible");

    app.handle(Command::ShowArchivedTasks);
    assert_eq!(app.view(), TaskView::Archived);
    assert_eq!(app.tasks().len(), 1);

    // l on the archived view is idempotent.
    app.handle(Command::ShowArchivedTasks);
    assert_eq!(app.view(), TaskView::Archived);

    app.handle(Command::ShowActiveTasks);
    assert_eq!(app.view(), TaskView::Active);

    // h on the active view is idempotent.
    app.handle(Command::ShowActiveTasks);
    assert_eq!(app.view(), TaskView::Active);
}
#[test]
fn a_same_view_switch_does_not_select_for_a_view_without_a_memory() {
    let mut app = App::load(TestService::with_tasks(vec![
        task(1, "alpha"),
        task(2, "beta"),
        archived_task(3, "gone"),
    ]));
    // No remembered selection: the view's own command is a no-op and
    // must not invent one from the first row.
    app.screen.task_list_mut().active_selection = None;
    app.handle(Command::ShowActiveTasks);
    assert_eq!(app.selected(), None);

    app.handle(Command::ShowArchivedTasks);
    assert_eq!(app.selected(), Some(0), "a fresh view starts on row one");
    app.screen.task_list_mut().archived_selection = None;
    app.handle(Command::ShowArchivedTasks);
    assert_eq!(app.selected(), None);
}
#[test]
fn each_view_remembers_its_selection_across_switches() {
    let mut app = App::load(TestService::with_tasks(vec![
        task(1, "alpha"),
        task(2, "beta"),
        archived_task(3, "gone"),
    ]));
    app.handle(Command::MoveDown);
    app.handle(Command::ShowArchivedTasks);
    assert_eq!(
        app.selected(),
        Some(0),
        "a fresh view starts on its first row"
    );

    app.handle(Command::ShowActiveTasks);
    assert_eq!(app.selected(), Some(1), "the active selection came back");
    assert_eq!(
        app.tasks()[1].id(),
        TaskId::from_uuid(uuid::Uuid::from_u128(2))
    );

    app.handle(Command::ShowArchivedTasks);
    assert_eq!(app.selected(), Some(0), "the archived selection came back");
}
#[test]
fn archived_movement_stays_inside_the_archived_list() {
    let mut app = App::load(TestService::with_tasks(vec![
        task(1, "alpha"),
        archived_task(3, "gone"),
        archived_task(4, "also gone"),
    ]));
    app.handle(Command::ShowArchivedTasks);
    assert_eq!(app.selected(), Some(0));
    app.handle(Command::MoveDown);
    assert_eq!(app.selected(), Some(1));
    app.handle(Command::MoveDown);
    assert_eq!(app.selected(), Some(1));
    app.handle(Command::MoveUp);
    assert_eq!(app.selected(), Some(0));
}
#[test]
fn an_empty_view_has_no_selection() {
    let mut app = App::load(TestService::with_tasks(vec![task(1, "alpha")]));
    app.handle(Command::ShowArchivedTasks);
    assert_eq!(app.selected(), None);
    app.handle(Command::MoveDown);
    app.handle(Command::MoveUp);
    assert_eq!(app.selected(), None);
}
#[test]
fn the_archived_view_refuses_active_actions_in_command_handling() {
    let mut app = App::load(TestService::with_tasks(vec![
        task(1, "alpha"),
        archived_task(3, "gone"),
    ]));
    app.handle(Command::ShowArchivedTasks);

    app.handle(Command::ToggleTracking);
    app.handle(Command::OpenAdd);
    app.handle(Command::OpenRename);
    app.handle(Command::OpenArchiveConfirm);

    assert_eq!(app.mode(), Mode::Normal);
    assert_eq!(app.view(), TaskView::Archived);
    assert_eq!(app.active_task_id(), None, "no tracking was started");

    // Movement still works after the refused actions.
    app.handle(Command::ShowActiveTasks);
    app.handle(Command::ShowArchivedTasks);
    app.handle(Command::MoveDown);
    app.handle(Command::MoveUp);
    assert_eq!(app.selected(), Some(0));
}
#[test]
fn the_active_view_refuses_unarchiving() {
    let mut app = App::load(TestService::with_tasks(vec![
        task(1, "alpha"),
        archived_task(3, "gone"),
    ]));
    app.handle(Command::UnarchiveSelected);

    assert_eq!(app.view(), TaskView::Active);
    assert_eq!(app.status(), &Status::Info("Ready".to_owned()));
    assert_eq!(app.tasks().len(), 1, "nothing was unarchived");
}
#[test]
fn a_modal_in_the_archived_view_refuses_unarchiving() {
    let mut app = App::load(TestService::with_tasks(vec![
        task(1, "alpha"),
        archived_task(3, "gone"),
    ]));
    app.handle(Command::ShowArchivedTasks);
    // No command path reaches a modal on the archived view; the guard
    // must still refuse one for whenever the key map changes.
    app.screen.task_list_mut().mode = TaskListMode::Input {
        purpose: InputPurpose::Add,
        buffer: String::new(),
    };
    app.handle(Command::UnarchiveSelected);

    assert!(matches!(app.mode(), Mode::Input { .. }));
    assert_eq!(app.view(), TaskView::Archived);
    assert_eq!(app.tasks().len(), 1, "nothing was unarchived");
    assert_eq!(app.status(), &Status::Info("Ready".to_owned()));
}
#[test]
fn modals_block_view_switching_and_unarchiving() {
    let mut app = App::load(TestService::with_tasks(vec![
        task(1, "alpha"),
        archived_task(3, "gone"),
    ]));
    app.handle(Command::OpenAdd);
    app.handle(Command::Insert('x'));

    // Each switch is asserted immediately, so a guard that only blocks
    // one of the two commands cannot cancel the other out.
    app.handle(Command::ShowArchivedTasks);
    assert_eq!(app.view(), TaskView::Active, "the modal blocks the switch");

    app.handle(Command::ShowActiveTasks);
    assert_eq!(app.view(), TaskView::Active);

    app.handle(Command::UnarchiveSelected);
    assert!(matches!(app.mode(), Mode::Input { .. }));
    assert_eq!(app.view(), TaskView::Active);
    assert!(matches!(app.mode(), Mode::Input { buffer, .. } if buffer == "x"));

    app.handle(Command::Cancel);
    app.handle(Command::OpenArchiveConfirm);
    app.handle(Command::UnarchiveSelected);
    assert!(matches!(app.mode(), Mode::ConfirmArchive { .. }));
    assert_eq!(app.view(), TaskView::Active);
}
#[test]
fn archiving_remembers_the_task_in_the_archived_view_and_clamps() {
    let mut app = App::load(TestService::with_tasks(vec![
        task(1, "alpha"),
        task(2, "beta"),
        task(3, "gamma"),
    ]));
    app.handle(Command::MoveDown);
    app.handle(Command::OpenArchiveConfirm);
    app.handle(Command::Confirm);

    assert_eq!(app.view(), TaskView::Active);
    assert_eq!(app.selected(), Some(1), "the selection clamped to gamma");

    app.handle(Command::ShowArchivedTasks);
    assert_eq!(app.selected(), Some(0));
    assert_eq!(
        app.tasks()[0].id(),
        TaskId::from_uuid(uuid::Uuid::from_u128(2))
    );
}
#[test]
fn archiving_the_last_row_clamps_to_the_previous_row() {
    let mut app = App::load(TestService::with_tasks(vec![
        task(1, "alpha"),
        task(2, "beta"),
    ]));
    app.handle(Command::MoveDown);
    app.handle(Command::OpenArchiveConfirm);
    app.handle(Command::Confirm);
    assert_eq!(app.tasks().len(), 1);
    assert_eq!(app.selected(), Some(0));
    assert_eq!(
        app.tasks()[0].id(),
        TaskId::from_uuid(uuid::Uuid::from_u128(1))
    );
}
#[test]
fn unarchiving_stays_in_the_archived_view_and_reports_restored() {
    let mut app = App::load(TestService::with_tasks(vec![
        task(1, "alpha"),
        task(2, "beta"),
        archived_task(3, "gone"),
    ]));
    app.handle(Command::ShowArchivedTasks);
    app.handle(Command::UnarchiveSelected);

    assert_eq!(app.view(), TaskView::Archived);
    assert_eq!(app.status(), &Status::Info("Restored \"gone\"".to_owned()));
    assert_eq!(app.tasks().len(), 0, "the restored task left the list");
    assert_eq!(app.selected(), None, "the archived list is empty now");

    app.handle(Command::ShowActiveTasks);
    assert_eq!(app.tasks().len(), 3);
    assert_eq!(app.selected(), Some(2), "the restored task is selected");
    assert_eq!(
        app.tasks()[2].id(),
        TaskId::from_uuid(uuid::Uuid::from_u128(3))
    );
}
#[test]
fn unarchiving_clamps_the_archived_selection_to_the_nearest_row() {
    let mut app = App::load(TestService::with_tasks(vec![
        task(1, "alpha"),
        archived_task(3, "gone"),
        archived_task(4, "also gone"),
    ]));
    app.handle(Command::ShowArchivedTasks);
    app.handle(Command::MoveDown);
    app.handle(Command::UnarchiveSelected);

    assert_eq!(app.view(), TaskView::Archived);
    assert_eq!(app.tasks().len(), 1);
    assert_eq!(app.selected(), Some(0));
    assert_eq!(
        app.tasks()[0].id(),
        TaskId::from_uuid(uuid::Uuid::from_u128(3))
    );
}
#[test]
fn a_failed_unarchive_keeps_the_view_selection_and_reports_the_error() {
    let mut service = TestService::with_tasks(vec![task(1, "alpha"), archived_task(3, "gone")]);
    service.fail_unarchive = true;
    let mut app = App::load(service);
    // Another client's activity reaches the service after the app loaded;
    // the failed write must still resynchronize the TUI's tracking state.
    app.application.tracking = TrackingState::Running {
        worklog: ActiveWorklog::begin(
            WorklogId::from_uuid(uuid::Uuid::from_u128(20)),
            TaskId::from_uuid(uuid::Uuid::from_u128(3)),
            DateTime::from_timestamp(100, 0).unwrap(),
        ),
    };
    app.handle(Command::ShowArchivedTasks);
    app.handle(Command::UnarchiveSelected);

    assert_eq!(app.view(), TaskView::Archived);
    assert_eq!(app.selected(), Some(0));
    assert_eq!(
        app.tasks()[0].id(),
        TaskId::from_uuid(uuid::Uuid::from_u128(3))
    );
    assert_eq!(
        app.active_task_id(),
        Some(TaskId::from_uuid(uuid::Uuid::from_u128(3))),
        "the failed unarchive still refreshes tracking state"
    );
    assert_eq!(app.status(), &Status::Error("Storage error".to_owned()));
}
#[test]
fn enter_on_an_empty_task_list_is_a_no_op() {
    let mut app = App::load(TestService::with_tasks(vec![]));

    app.handle(Command::OpenHistory);

    assert_eq!(app.screen(), Screen::TaskList);
    assert_eq!(app.history(), None);
    assert_eq!(app.status(), &Status::Info("Ready".to_owned()));
    assert_eq!(
        app.application.worklog_reads.get(),
        0,
        "no task was selected, so nothing was read"
    );
}
