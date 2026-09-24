//! Worklog-history scenarios.

use std::collections::HashSet;

use chrono::{DateTime, TimeZone, Utc};
use termlens::Key;

use crate::context::TestContext;
use crate::page::{TimeTrackerPage, WorklogHistoryPanel};

fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, day, hour, minute, second)
        .single()
        .expect("the fixture timestamp must be valid")
}

fn utc_text(at: DateTime<Utc>) -> String {
    at.format("%Y-%m-%d %H:%M").to_string()
}

fn selected_start(panel: &WorklogHistoryPanel) -> Option<String> {
    panel
        .selected_index()
        .map(|index| panel.row(index).start_text())
}

fn visible_starts(panel: &WorklogHistoryPanel) -> Vec<String> {
    (0..panel.row_count())
        .map(|index| panel.row(index).start_text())
        .collect()
}

#[test]
fn selected_task_history_lists_completed_worklogs_newest_first() {
    let context = TestContext::new();
    {
        let database = context.database();
        database.create_task("Customer migration");
        database.create_worklog(
            "Customer migration",
            at(2025, 1, 15, 8, 0, 0),
            Some(at(2025, 1, 15, 8, 45, 30)),
        );
        database.create_worklog(
            "Customer migration",
            at(2025, 6, 20, 14, 30, 0),
            Some(at(2025, 6, 20, 16, 2, 3)),
        );
    }

    let mut tt = context.launch();
    tt.wait_for_first_frame("the custom task list", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().is_idle()
            && page.task_panel().task_names() == ["Customer migration".to_owned()]
            && page.task_panel().selected_index() == Some(0)
    });

    let page = tt.press_and_wait(Key::Enter, "the completed worklog history", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        let panel = page.worklog_history_panel();
        panel.is_shown()
            && panel.task_name() == "Customer migration"
            && panel.row_count() == 2
            && panel.row(0).start_text() == "2025-06-20 14:30"
            && panel.row(0).end_text() == "2025-06-20 16:02"
            && panel.row(0).duration_text() == "01:32:03"
            && panel.row(1).start_text() == "2025-01-15 08:00"
            && panel.row(1).end_text() == "2025-01-15 08:45"
            && panel.row(1).duration_text() == "00:45:30"
            && panel.selected_index() == Some(0)
            && page.footer().hints_history()
    });
    let panel = page.worklog_history_panel();
    assert_eq!(panel.title(), "Active › Customer migration › Worklogs");
    assert_eq!(panel.task_name(), "Customer migration");
    assert_eq!(panel.row_count(), 2);
    assert_eq!(panel.row(0).start_text(), "2025-06-20 14:30");
    assert_eq!(panel.row(0).end_text(), "2025-06-20 16:02");
    assert_eq!(panel.row(0).duration_text(), "01:32:03");
    assert_eq!(panel.row(1).start_text(), "2025-01-15 08:00");
    assert_eq!(panel.row(1).end_text(), "2025-01-15 08:45");
    assert_eq!(panel.row(1).duration_text(), "00:45:30");

    let page = tt.press_and_wait(Key::Esc, "the same selected task", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_active_tasks()
            && page.task_panel().task_names() == ["Customer migration".to_owned()]
            && page.task_panel().selected_index() == Some(0)
            && page.task_panel().row(0).name() == "Customer migration"
    });
    assert_eq!(page.task_panel().row(0).name(), "Customer migration");
    assert!(page.task_panel().row(0).is_selected());

    tt.quit().assert_clean_exit();
}

#[test]
fn active_worklog_appears_running_and_its_duration_advances() {
    let context = TestContext::new();
    let before = {
        let database = context.database();
        database.create_task("Live incident review");
        let start = DateTime::from_timestamp(Utc::now().timestamp() - 3, 0)
            .expect("the active fixture start must be valid");
        database.create_worklog("Live incident review", start, None);
        database
            .active_worklog()
            .expect("the active fixture must be stored")
    };

    let mut tt = context.launch();
    tt.wait_for_first_frame("the recovered active task", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().active_task().is_some_and(|active| {
            active.name() == "Live incident review" && active.elapsed_is_hhmmss()
        }) && page.task_panel().task_names() == ["Live incident review".to_owned()]
            && page.task_panel().selected_index() == Some(0)
    });

    let page = tt.press_and_wait(Key::Enter, "the running worklog", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        let panel = page.worklog_history_panel();
        panel.is_shown()
            && panel.task_name() == "Live incident review"
            && panel.row_count() == 1
            && panel.row(0).is_running()
            && panel.row(0).duration_is_hhmmss()
            && page
                .header()
                .active_task()
                .is_some_and(|active| active.elapsed_is_hhmmss())
    });
    let initial_header_seconds = page
        .header()
        .active_task()
        .and_then(|active| active.elapsed_seconds())
        .expect("the header timer must be valid");
    let initial_row_seconds = page
        .worklog_history_panel()
        .row(0)
        .duration_seconds()
        .expect("the running row timer must be valid");

    let page = tt.wait_for("the running header and row timers to advance", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        let panel = page.worklog_history_panel();
        panel.is_shown()
            && panel.row_count() == 1
            && panel.row(0).is_running()
            && panel
                .row(0)
                .duration_seconds()
                .is_some_and(|seconds| seconds > initial_row_seconds)
            && page.header().active_task().is_some_and(|active| {
                active
                    .elapsed_seconds()
                    .is_some_and(|seconds| seconds > initial_header_seconds)
            })
    });
    let advanced_header_seconds = page
        .header()
        .active_task()
        .and_then(|active| active.elapsed_seconds())
        .expect("the advanced header timer must be valid");
    let advanced_row_seconds = page
        .worklog_history_panel()
        .row(0)
        .duration_seconds()
        .expect("the advanced row timer must be valid");
    assert!(advanced_header_seconds > initial_header_seconds);
    assert!(advanced_row_seconds > initial_row_seconds);
    assert!(page.worklog_history_panel().row(0).is_running());

    tt.exit_with(Key::Char('q')).assert_clean_exit();

    let after = context
        .database()
        .active_worklog()
        .expect("viewing and quitting must leave the worklog active");
    assert_eq!(after.id, before.id, "the worklog identity changed");
    assert_eq!(after.task_id, before.task_id, "the tracked task changed");
    assert_eq!(after.start, before.start, "the start time changed");
    assert_eq!(after.end, before.end, "the active end time changed");
}

#[test]
fn history_loads_older_worklogs_without_duplicates() {
    let context = TestContext::new();
    let base = at(2025, 3, 1, 9, 0, 0);
    {
        let database = context.database();
        database.create_task("Long-running migration");
        for minute in 0..51 {
            let start = base + chrono::TimeDelta::minutes(minute);
            database.create_worklog(
                "Long-running migration",
                start,
                Some(start + chrono::TimeDelta::seconds(30)),
            );
        }
    }
    let oldest = utc_text(base);
    let second_oldest = utc_text(base + chrono::TimeDelta::minutes(1));

    let mut tt = context.launch();
    tt.wait_for_first_frame("the custom task list", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().task_names() == ["Long-running migration".to_owned()]
            && page.task_panel().selected_index() == Some(0)
    });
    tt.press_and_wait(Key::Enter, "the newest history page", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        let panel = page.worklog_history_panel();
        panel.is_shown()
            && panel.task_name() == "Long-running migration"
            && panel.selected_index() == Some(0)
    });

    tt.type_text(&"j".repeat(60));
    let page = tt.wait_for("the oldest row of the initial page", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        selected_start(&page.worklog_history_panel()).as_deref() == Some(second_oldest.as_str())
    });
    let initial_panel = page.worklog_history_panel();
    assert_eq!(
        selected_start(&initial_panel).as_deref(),
        Some(second_oldest.as_str())
    );
    assert!(
        !visible_starts(&initial_panel).contains(&oldest),
        "the fifty-row initial page exposed the older worklog before o:\n{}",
        page.screen()
    );

    tt.press_and_wait(Key::Char('o'), "the older page load", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.worklog_history_panel().is_shown()
            && page.status_bar().text() == "Loaded 1 older worklogs"
            && selected_start(&page.worklog_history_panel()).as_deref()
                == Some(second_oldest.as_str())
    });
    let page = tt.press_and_wait(
        Key::Char('j'),
        "the newly loaded oldest worklog",
        |screen| {
            let page = TimeTrackerPage::new(screen.clone());
            selected_start(&page.worklog_history_panel()).as_deref() == Some(oldest.as_str())
        },
    );
    let loaded_panel = page.worklog_history_panel();
    assert_eq!(
        selected_start(&loaded_panel).as_deref(),
        Some(oldest.as_str())
    );
    let starts = visible_starts(&loaded_panel);
    let unique: HashSet<&String> = starts.iter().collect();
    assert_eq!(
        unique.len(),
        starts.len(),
        "the visible history repeated a worklog after loading older rows: {starts:?}"
    );
    assert_eq!(starts.iter().filter(|start| **start == oldest).count(), 1);

    tt.quit().assert_clean_exit();
}

#[test]
fn archived_and_empty_tasks_expose_their_history() {
    let context = TestContext::new();
    let stored_before = {
        let database = context.database();
        database.create_task("Archived audit");
        database.create_worklog(
            "Archived audit",
            at(2025, 4, 8, 10, 0, 0),
            Some(at(2025, 4, 8, 10, 20, 15)),
        );
        database.archive_task("Archived audit");
        database.create_task("Archived backlog");
        database.archive_task("Archived backlog");
        database.tasks()
    };

    let mut tt = context.launch();
    tt.wait_for_first_frame("the empty active view", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_active_tasks()
            && page.task_panel().empty_hint().as_deref()
                == Some("No active tasks. Press a to add one.")
    });
    tt.press_and_wait(Key::Char('l'), "the explicit archived tasks", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_archived_tasks()
            && page.task_panel().task_names()
                == ["Archived audit".to_owned(), "Archived backlog".to_owned()]
            && page.task_panel().selected_index() == Some(0)
    });

    let page = tt.press_and_wait(
        Key::Enter,
        "the archived task's completed history",
        |screen| {
            let page = TimeTrackerPage::new(screen.clone());
            let panel = page.worklog_history_panel();
            panel.is_shown()
                && panel.task_name() == "Archived audit"
                && panel.row_count() == 1
                && panel.row(0).start_text() == "2025-04-08 10:00"
                && panel.row(0).end_text() == "2025-04-08 10:20"
                && panel.row(0).duration_text() == "00:20:15"
        },
    );
    assert_eq!(page.worklog_history_panel().row_count(), 1);

    let page = tt.press_and_wait(Key::Esc, "the same archived task", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_archived_tasks()
            && page.task_panel().selected_index() == Some(0)
            && page.task_panel().row(0).name() == "Archived audit"
    });
    assert_eq!(page.task_panel().row(0).name(), "Archived audit");

    tt.press_and_wait(
        Key::Char('j'),
        "the archived task without worklogs",
        |screen| {
            let page = TimeTrackerPage::new(screen.clone());
            page.task_panel().selected_index() == Some(1)
                && page.task_panel().row(1).name() == "Archived backlog"
        },
    );
    let page = tt.press_and_wait(Key::Enter, "the archived empty history", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        let panel = page.worklog_history_panel();
        panel.is_shown()
            && panel.task_name() == "Archived backlog"
            && panel.row_count() == 0
            && panel.empty_hint() == Some("No worklogs yet.")
    });
    assert_eq!(
        page.worklog_history_panel().empty_hint(),
        Some("No worklogs yet.")
    );

    let page = tt.press_and_wait(Key::Esc, "the same empty archived task", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_archived_tasks()
            && page.task_panel().selected_index() == Some(1)
            && page.task_panel().row(1).name() == "Archived backlog"
    });
    assert_eq!(page.task_panel().row(1).name(), "Archived backlog");

    tt.quit().assert_clean_exit();
    assert_eq!(
        context.database().tasks(),
        stored_before,
        "viewing archived histories changed task storage"
    );
}
