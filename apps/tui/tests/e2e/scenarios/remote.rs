//! The public TUI workflows against a separately running HTTP server.

use std::time::Duration;

use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use termlens::Key;

use crate::page::TimeTrackerPage;
use crate::remote::RemoteTestContext;

fn at(hour: u32, minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 8, 1, hour, minute, 0)
        .single()
        .expect("fixture time must be valid")
}

#[test]
fn remote_terminal_redraws_after_resize_and_keeps_selection() {
    let context = RemoteTestContext::new();
    let mut tt = context.launch();
    tt.wait_for_first_frame("the remote task list", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .task_names()
            .len()
            == 3
    });
    tt.press_and_wait(Key::Char('j'), "the second remote task", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .selected_index()
            == Some(1)
    });

    let page = tt.resize_and_wait(110, 34, "the grown remote layout", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.size() == (110, 34)
            && page.task_panel().frame_corners_fit_current_geometry()
            && page.task_panel().selected_index() == Some(1)
            && page.footer().hints_quit()
    });
    assert_eq!(page.task_panel().row(1).name(), "Fix the coffee machine");
    tt.quit().assert_clean_exit();
}

#[test]
fn remote_keypress_renders_worker_frame_without_poll_delay() {
    let context = RemoteTestContext::new();
    let mut tt = context.launch();
    tt.wait_for_first_frame("the remote task list", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .task_names()
            .len()
            == 3
    });

    let mut samples = Vec::new();
    for index in 0..5 {
        let expected_index = if index % 2 == 0 { 1 } else { 0 };
        let key = if expected_index == 1 {
            Key::Char('j')
        } else {
            Key::Char('k')
        };
        let elapsed = tt.press_and_measure(key, "the remote selection response", |screen| {
            TimeTrackerPage::new(screen.clone())
                .task_panel()
                .selected_index()
                == Some(expected_index)
        });
        samples.push(elapsed);
    }
    tt.quit().assert_clean_exit();
    samples.sort_unstable();
    let fourth_fastest = samples[samples.len() - 2];
    assert!(
        fourth_fastest < Duration::from_millis(100),
        "four of five remote key-to-frame responses should be prompt; samples: {samples:?}"
    );
}

#[test]
fn remote_task_create_rename_search_archive_and_restore_use_the_server_database() {
    let context = RemoteTestContext::new();
    let mut tt = context.launch();
    tt.wait_for_first_frame("the server task list", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().is_idle() && page.task_panel().task_names().len() == 3
    });

    tt.press_and_wait(Key::Char('a'), "the task name dialog", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_input_dialog()
            .is_some_and(|dialog| dialog.prompt() == "New task name:")
    });
    tt.type_text("Prepare quarterly review");
    tt.press_and_wait(Key::Enter, "the remote task creation", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_input_dialog().is_none()
            && page.status_bar().text() == "Added \"Prepare quarterly review\""
            && page
                .task_panel()
                .task_names()
                .contains(&"Prepare quarterly review".to_owned())
    });

    tt.press_and_wait(Key::Char('e'), "the rename dialog", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_input_dialog()
            .is_some_and(|dialog| dialog.prompt() == "Rename task:")
    });
    for _ in 0.."Prepare quarterly review".chars().count() {
        tt.press(Key::Backspace);
    }
    tt.type_text("Review quarterly plan");
    tt.press_and_wait(Key::Enter, "the remote rename", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.status_bar().text() == "Renamed to \"Review quarterly plan\""
            && page
                .task_panel()
                .task_names()
                .contains(&"Review quarterly plan".to_owned())
    });

    tt.press_and_wait(Key::Char('/'), "the task search input", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .search_query()
            .as_deref()
            == Some("")
    });
    tt.type_text("RQP");
    tt.wait_for("the remote fuzzy search result", |screen| {
        let panel = TimeTrackerPage::new(screen.clone()).task_panel();
        panel.search_query().as_deref() == Some("RQP")
            && panel.task_names() == ["Review quarterly plan".to_owned()]
    });
    tt.press_and_wait(Key::Esc, "the unfiltered task list", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .search_query()
            .is_none()
    });

    tt.press_and_wait(Key::Char('d'), "the archive confirmation", |screen| {
        TimeTrackerPage::new(screen.clone())
            .archive_dialog()
            .is_some_and(|dialog| dialog.question() == "Archive \"Review quarterly plan\"?")
    });
    tt.press_and_wait(Key::Char('y'), "the remote archived task", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.status_bar().text() == "Archived \"Review quarterly plan\""
            && !page
                .task_panel()
                .task_names()
                .contains(&"Review quarterly plan".to_owned())
    });
    tt.press_and_wait(Key::Tab, "the archived task list", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_archived_tasks()
            && page.task_panel().task_names() == ["Review quarterly plan".to_owned()]
    });
    tt.press_and_wait(Key::Char('u'), "the restored task", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.status_bar().text() == "Restored \"Review quarterly plan\""
            && page.task_panel().empty_hint().as_deref() == Some("No archived tasks.")
    });
    tt.quit().assert_clean_exit();

    let server_db = context.server_database();
    assert!(server_db.task_by_name("Review quarterly plan").is_some());
    assert!(server_db.task_by_name("Prepare quarterly review").is_none());
    assert!(!context.local_database_path().exists());
}

#[test]
fn remote_tracking_switch_stop_history_and_global_history_are_server_backed() {
    let context = RemoteTestContext::new();
    let mut tt = context.launch();
    tt.wait_for_first_frame("the initial server task list", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().is_idle() && page.task_panel().task_names().len() == 3
    });

    tt.press_and_wait(Key::Char(' '), "the first remote timer", |screen| {
        TimeTrackerPage::new(screen.clone())
            .header()
            .active_task()
            .is_some_and(|task| task.name() == "Write release notes")
    });
    tt.press_and_wait(Key::Char('j'), "the next task selection", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .selected_index()
            == Some(1)
    });
    tt.press_and_wait(Key::Char(' '), "the switched remote timer", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header()
            .active_task()
            .is_some_and(|task| task.name() == "Fix the coffee machine")
            && page.status_bar().text() == "Switched to \"Fix the coffee machine\""
    });
    tt.press_and_wait(Key::Char(' '), "the stopped remote timer", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().is_idle() && page.status_bar().text() == "Stopped \"Fix the coffee machine\""
    });

    tt.press_and_wait(Key::Enter, "the selected task history", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.worklog_history_panel().is_shown()
            && page.worklog_history_panel().task_name() == "Fix the coffee machine"
            && page.worklog_history_panel().row_count() == 1
            && !page.worklog_history_panel().row(0).is_running()
    });
    tt.press_and_wait(Key::Esc, "the task list after history", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .shows_active_tasks()
    });
    tt.press_and_wait(Key::Tab, "the archived view", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .shows_archived_tasks()
    });
    tt.press_and_wait(Key::Tab, "the global worklog view", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.all_worklogs_panel().is_shown() && page.all_worklogs_panel().row_count() == 2
    });
    let page = tt.page();
    assert_eq!(
        page.all_worklogs_panel().row(0).task_name(),
        "Fix the coffee machine"
    );
    assert_eq!(
        page.all_worklogs_panel().row(1).task_name(),
        "Write release notes"
    );
    tt.quit().assert_clean_exit();

    let server_db = context.server_database();
    assert_eq!(server_db.worklogs_for_task("Write release notes").len(), 1);
    assert_eq!(
        server_db.worklogs_for_task("Fix the coffee machine").len(),
        1
    );
    assert!(server_db.active_worklog().is_none());
    assert!(!context.local_database_path().exists());
}

#[test]
fn remote_history_can_move_correct_and_delete_worklogs() {
    let mut context = RemoteTestContext::new_without_server();
    let database = context.server_database();
    database.create_task("Server source");
    database.create_task("Server destination");
    let original = database.create_worklog("Server source", at(10, 0), Some(at(12, 0)));
    database.create_worklog("Server source", at(8, 0), Some(at(8, 30)));
    context.start();

    let mut tt = context.launch();
    tt.wait_for_first_frame("the seeded remote tasks", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .task_names()
            .contains(&"Server source".to_owned())
    });
    let page = tt.page();
    let source_index = page
        .task_panel()
        .task_names()
        .iter()
        .position(|name| name == "Server source")
        .expect("the seeded task is visible");
    let selected = page.task_panel().selected_index().unwrap();
    let movement = if source_index > selected {
        Key::Char('j')
    } else {
        Key::Char('k')
    };
    for _ in 0..source_index.abs_diff(selected) {
        tt.press_and_wait(movement, "the source selection", |screen| {
            TimeTrackerPage::new(screen.clone())
                .task_panel()
                .selected_index()
                == Some(source_index)
        });
    }
    tt.press_and_wait(Key::Enter, "the source history", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.worklog_history_panel().is_shown()
            && page.worklog_history_panel().task_name() == "Server source"
            && page.worklog_history_panel().row_count() == 2
    });
    tt.press_and_wait(Key::Char('e'), "the correction dialog", |screen| {
        TimeTrackerPage::new(screen.clone())
            .correction_dialog()
            .is_some()
    });
    tt.press_and_wait(Key::Char('j'), "the corrected start", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.correction_dialog()
            .is_some_and(|dialog| dialog.start_text() == "2025-08-01 10:05")
    });
    tt.press_and_wait(Key::Tab, "the correction end field", |screen| {
        TimeTrackerPage::new(screen.clone())
            .correction_dialog()
            .is_some_and(|dialog| dialog.focused_field() == "End")
    });
    tt.press_and_wait(Key::Char('K'), "the corrected end", |screen| {
        TimeTrackerPage::new(screen.clone())
            .correction_dialog()
            .is_some_and(|dialog| dialog.end_text().as_deref() == Some("2025-08-01 11:00"))
    });
    tt.press_and_wait(Key::Enter, "the corrected remote history", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.status_bar().text() == "Corrected worklog"
            && page.worklog_history_panel().row(0).start_text() == "2025-08-01 10:05"
    });

    tt.press_and_wait(Key::Char('m'), "the move dialog", |screen| {
        TimeTrackerPage::new(screen.clone())
            .move_worklog_dialog()
            .is_some()
    });
    tt.type_text("Server destination");
    tt.wait_for("the remote move destination", |screen| {
        TimeTrackerPage::new(screen.clone())
            .move_worklog_dialog()
            .is_some_and(|dialog| dialog.results().contains(&"Server destination".to_owned()))
    });
    tt.press(Key::Tab);
    tt.wait_for("the selected move destination", |screen| {
        TimeTrackerPage::new(screen.clone())
            .move_worklog_dialog()
            .is_some_and(|dialog| dialog.selected_result().as_deref() == Some("Server destination"))
    });
    tt.press_and_wait(Key::Enter, "the moved worklog", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.status_bar().text() == "Moved worklog to \"Server destination\""
            && page.worklog_history_panel().row_count() == 1
    });

    tt.press_and_wait(Key::Esc, "the task list", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .shows_active_tasks()
    });
    let page = tt.page();
    let destination_index = page
        .task_panel()
        .task_names()
        .iter()
        .position(|name| name == "Server destination")
        .expect("the destination remains visible");
    let selected = page.task_panel().selected_index().unwrap();
    let movement = if destination_index > selected {
        Key::Char('j')
    } else {
        Key::Char('k')
    };
    for _ in 0..destination_index.abs_diff(selected) {
        tt.press_and_wait(movement, "the destination selection", |screen| {
            TimeTrackerPage::new(screen.clone())
                .task_panel()
                .selected_index()
                == Some(destination_index)
        });
    }
    tt.press_and_wait(Key::Enter, "the destination history", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.worklog_history_panel().is_shown()
            && page.worklog_history_panel().task_name() == "Server destination"
            && page.worklog_history_panel().row_count() == 1
    });
    tt.press_and_wait(Key::Char('d'), "the delete confirmation", |screen| {
        TimeTrackerPage::new(screen.clone())
            .deletion_dialog()
            .is_some()
    });
    tt.press_and_wait(Key::Char('y'), "the deleted remote worklog", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.status_bar().text() == "Deleted worklog"
            && page.worklog_history_panel().row_count() == 0
    });
    tt.quit().assert_clean_exit();

    let server_db = context.server_database();
    assert!(
        server_db
            .worklogs_for_task("Server source")
            .iter()
            .all(|row| row.id != original.id)
    );
    assert!(server_db.worklogs_for_task("Server destination").is_empty());
    assert!(!context.local_database_path().exists());
}

#[test]
fn remote_reports_include_server_worklogs() {
    let mut context = RemoteTestContext::new_without_server();
    let database = context.server_database();
    let now = Utc::now();
    let start = now - TimeDelta::minutes(30);
    database.create_task("Server report task");
    database.create_worklog("Server report task", start, Some(now));
    context.start();

    let mut tt = context.launch();
    tt.wait_for_first_frame("the remote active task list", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .shows_active_tasks()
    });
    tt.press_and_wait(Key::Tab, "the archived tab", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .shows_archived_tasks()
    });
    tt.press_and_wait(Key::Tab, "the global worklogs tab", |screen| {
        TimeTrackerPage::new(screen.clone())
            .all_worklogs_panel()
            .is_shown()
    });
    tt.press_and_wait(Key::Tab, "the remote reports tab", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .shows_reports()
    });
    tt.wait_for("the report with server data", |screen| {
        let text = TimeTrackerPage::new(screen.clone()).reports_panel().text();
        text.contains("Server report task") && text.contains("Total:")
    });
    assert!(!context.local_database_path().exists());
    tt.quit().assert_clean_exit();
}

#[test]
fn remote_clients_refresh_and_reject_a_stale_worklog_correction() {
    let mut context = RemoteTestContext::new_without_server();
    let database = context.server_database();
    database.create_task("Concurrent review");
    let original = database.create_worklog("Concurrent review", at(10, 0), Some(at(11, 0)));
    context.start();

    let mut stale = context.launch();
    stale.wait_for_first_frame("the stale client's task list", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .task_names()
            .contains(&"Concurrent review".to_owned())
    });
    let stale_page = stale.page();
    let index = stale_page
        .task_panel()
        .task_names()
        .iter()
        .position(|name| name == "Concurrent review")
        .unwrap();
    let selected = stale_page.task_panel().selected_index().unwrap();
    let movement = if index > selected {
        Key::Char('j')
    } else {
        Key::Char('k')
    };
    for _ in 0..index.abs_diff(selected) {
        stale.press_and_wait(movement, "the concurrent task selection", |screen| {
            TimeTrackerPage::new(screen.clone())
                .task_panel()
                .selected_index()
                == Some(index)
        });
    }
    stale.press_and_wait(Key::Enter, "the stale client's history", |screen| {
        TimeTrackerPage::new(screen.clone())
            .worklog_history_panel()
            .row_count()
            == 1
    });
    stale.press_and_wait(Key::Char('e'), "the stale correction form", |screen| {
        TimeTrackerPage::new(screen.clone())
            .correction_dialog()
            .is_some()
    });

    let mut current = context.launch_second_client();
    current.wait_for_first_frame("the second client", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .task_names()
            .contains(&"Concurrent review".to_owned())
    });
    let current_page = current.page();
    let index = current_page
        .task_panel()
        .task_names()
        .iter()
        .position(|name| name == "Concurrent review")
        .unwrap();
    let selected = current_page.task_panel().selected_index().unwrap();
    let movement = if index > selected {
        Key::Char('j')
    } else {
        Key::Char('k')
    };
    for _ in 0..index.abs_diff(selected) {
        current.press_and_wait(movement, "the second client's task selection", |screen| {
            TimeTrackerPage::new(screen.clone())
                .task_panel()
                .selected_index()
                == Some(index)
        });
    }
    current.press_and_wait(Key::Enter, "the second client's history", |screen| {
        TimeTrackerPage::new(screen.clone())
            .worklog_history_panel()
            .row_count()
            == 1
    });
    current.press_and_wait(Key::Char('e'), "the current correction form", |screen| {
        TimeTrackerPage::new(screen.clone())
            .correction_dialog()
            .is_some()
    });
    current.press_and_wait(Key::Char('j'), "the current edit", |screen| {
        TimeTrackerPage::new(screen.clone())
            .correction_dialog()
            .is_some_and(|dialog| dialog.start_text() == "2025-08-01 10:05")
    });
    current.press_and_wait(Key::Enter, "the saved current correction", |screen| {
        TimeTrackerPage::new(screen.clone()).status_bar().text() == "Corrected worklog"
    });

    let stale_page = stale.press_and_wait(Key::Enter, "the stale edit conflict", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.status_bar().is_error() && page.correction_dialog().is_some()
    });
    assert!(
        stale_page
            .status_bar()
            .text()
            .to_lowercase()
            .contains("changed")
    );
    assert!(
        stale_page
            .correction_dialog()
            .unwrap()
            .is_full_draft_visible()
    );
    current.quit().assert_clean_exit();
    stale.quit().assert_clean_exit();

    let updated = context
        .server_database()
        .worklogs_for_task("Concurrent review");
    let row = updated.iter().find(|row| row.id == original.id).unwrap();
    assert_eq!(row.start, at(10, 5));
    assert!(!context.local_database_path().exists());
}

#[test]
fn a_second_remote_client_refreshes_after_a_server_change() {
    let context = RemoteTestContext::new();
    let mut observer = context.launch_second_client();
    observer.wait_for_first_frame("the observer client task list", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .task_names()
            .len()
            == 3
    });

    let mut writer = context.launch();
    writer.wait_for_first_frame("the writer client task list", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .task_names()
            .len()
            == 3
    });
    writer.press_and_wait(Key::Char('a'), "the new task dialog", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_input_dialog()
            .is_some()
    });
    writer.type_text("Added from another client");
    writer.press_and_wait(Key::Enter, "the saved task", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .task_names()
            .contains(&"Added from another client".to_owned())
    });
    observer.wait_for("the observer's refreshed task list", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .task_names()
            .contains(&"Added from another client".to_owned())
    });
    writer.quit().assert_clean_exit();
    observer.quit().assert_clean_exit();
    assert!(
        context
            .server_database()
            .task_by_name("Added from another client")
            .is_some()
    );
}

#[test]
fn a_remote_client_shows_unavailable_at_startup_and_recovers_without_local_fallback() {
    let mut context = RemoteTestContext::new();
    context.stop_server();
    let mut tt = context.launch();
    tt.wait_for_first_frame("the unavailable server state", |screen| {
        TimeTrackerPage::new(screen.clone())
            .status_bar()
            .text()
            .to_lowercase()
            .contains("unavailable")
    });
    assert!(!context.local_database_path().exists());

    context.restart_server();
    let page = tt.wait_for("the startup connection recovery", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().task_names().len() == 3
            && !page
                .status_bar()
                .text()
                .to_lowercase()
                .contains("unavailable")
    });
    assert_eq!(page.task_panel().task_names().len(), 3);
    assert!(!context.local_database_path().exists());
    tt.quit().assert_clean_exit();
}

#[test]
fn remote_disconnect_shows_unavailable_reconnects_and_keeps_active_timer_on_restart() {
    let mut context = RemoteTestContext::new();
    let mut tt = context.launch();
    tt.wait_for_first_frame("the remote task list", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .task_names()
            .len()
            == 3
    });
    tt.press_and_wait(Key::Char(' '), "the running remote timer", |screen| {
        TimeTrackerPage::new(screen.clone())
            .header()
            .active_task()
            .is_some_and(|task| task.name() == "Write release notes")
    });
    context.stop_server();
    tt.wait_for("the unavailable server status", |screen| {
        TimeTrackerPage::new(screen.clone())
            .status_bar()
            .text()
            .to_lowercase()
            .contains("unavailable")
    });
    context.restart_server();
    let page = tt.wait_for("the recovered server state", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header()
            .active_task()
            .is_some_and(|task| task.name() == "Write release notes")
            && !page
                .status_bar()
                .text()
                .to_lowercase()
                .contains("unavailable")
    });
    assert!(page.header().active_task().is_some());
    tt.quit().assert_clean_exit();

    let active = context
        .server_database()
        .active_worklog()
        .expect("server restart must preserve the active worklog");
    assert_eq!(active.task_name, "Write release notes");
    assert!(!context.local_database_path().exists());
}
