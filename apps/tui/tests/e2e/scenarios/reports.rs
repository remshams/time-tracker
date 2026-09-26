//! Report period, range, and clipping scenarios.

use chrono::Duration;
use chrono::{DateTime, Utc};
use termlens::Key;

use crate::context::TestContext;
use crate::page::TimeTrackerPage;

const REPORT_NOW: &str = "2025-04-16T15:30:00Z";

fn instant(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .expect("the report fixture timestamp must be valid")
        .to_utc()
}

#[test]
fn reports_choose_presets_clip_day_boundaries_and_validate_custom_dates() {
    let context = TestContext::new();
    {
        let database = context.database();
        database.create_task("Cross midnight");
        database.create_task("Today task");
        database.create_task("Archived task");
        database.create_task("Running task");
        database.create_worklog(
            "Cross midnight",
            instant("2025-04-15T23:30:00Z"),
            Some(instant("2025-04-16T00:30:00Z")),
        );
        database.create_worklog(
            "Today task",
            instant("2025-04-16T14:00:00Z"),
            Some(instant("2025-04-16T15:02:03Z")),
        );
        database.create_worklog(
            "Archived task",
            instant("2025-04-16T15:28:00Z"),
            Some(instant("2025-04-16T15:30:00Z")),
        );
        database.create_worklog("Running task", instant("2025-04-16T14:30:00Z"), None);
        database.archive_task("Archived task");
    }

    let mut tt = context.launch_for_reports(REPORT_NOW);
    tt.wait_for_first_frame("the active tab", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .shows_active_tasks()
    });
    tt.press_and_wait(Key::Tab, "the archived tab", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .shows_archived_tasks()
    });
    tt.press_and_wait(Key::Tab, "the worklogs tab", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .shows_worklogs()
    });
    tt.press_and_wait(Key::Tab, "the reports tab", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .shows_reports()
    });
    tt.wait_for("today's report", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.reports_panel()
            .text()
            .contains("Period: Today · 2025-04-16")
    });
    let today = tt.page().reports_panel().text();
    assert!(
        tt.page()
            .reports_panel()
            .separates_applied_period_from_presets()
    );
    assert!(tt.page().reports_panel().separates_tabs_from_period());
    assert!(tt.page().reports_panel().preset_is_applied("Today"));
    let footer = tt.page().footer().text();
    assert!(
        footer.contains("Enter presets") && footer.contains("Tab/⇧Tab switch tabs"),
        "{footer}"
    );
    assert!(today.contains("Total: 2h 34m 3s"), "{today}");
    for preset in ["Today", "Yesterday", "Week", "Month", "Year", "Custom"] {
        assert!(today.contains(preset), "missing {preset:?} from:\n{today}");
    }
    assert!(
        today.contains("Cross midnight") && today.contains("30m 0s"),
        "{today}"
    );
    assert!(
        today.contains("Archived task") && today.contains("2m 0s"),
        "{today}"
    );
    assert!(
        today.contains("Running task") && today.contains("1h 0m 0s"),
        "{today}"
    );

    tt.press_and_wait(Key::Enter, "the focused preset row", |screen| {
        TimeTrackerPage::new(screen.clone())
            .reports_panel()
            .preset_is_focused("Today")
    });
    assert!(tt.page().reports_panel().preset_is_applied("Today"));
    assert!(tt.page().reports_panel().preset_is_focused("Today"));
    tt.press_and_wait(
        Key::BackTab,
        "the last preset after wrapping backward",
        |screen| {
            TimeTrackerPage::new(screen.clone())
                .reports_panel()
                .preset_is_focused("Custom")
        },
    );
    tt.press_and_wait(Key::Tab, "Today after wrapping forward", |screen| {
        TimeTrackerPage::new(screen.clone())
            .reports_panel()
            .preset_is_focused("Today")
    });
    tt.press_and_wait(Key::Tab, "yesterday's report", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.reports_panel()
            .text()
            .contains("Period: Yesterday · 2025-04-15")
            && page.reports_panel().preset_is_focused("Yesterday")
            && page.reports_panel().preset_is_applied("Yesterday")
    });
    let yesterday = tt.page().reports_panel().text();
    assert!(yesterday.contains("Total: 30m 0s"), "{yesterday}");
    assert!(
        yesterday.contains("Cross midnight") && yesterday.contains("30m 0s"),
        "{yesterday}"
    );

    tt.press_and_wait(Key::Esc, "the Reports top tab focus", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_reports() && page.task_panel().selected_tab_is_highlighted()
    });
    tt.press(Key::Enter);
    tt.press_and_wait(Key::Tab, "the weekly report", |screen| {
        TimeTrackerPage::new(screen.clone())
            .reports_panel()
            .text()
            .contains("Period: Week · 2025-04-14 to 2025-04-20")
    });

    tt.press_and_wait(Key::Tab, "the monthly report", |screen| {
        TimeTrackerPage::new(screen.clone())
            .reports_panel()
            .text()
            .contains("Period: Month · 2025-04-01 to 2025-04-30")
    });

    tt.press_and_wait(Key::Tab, "the yearly report", |screen| {
        TimeTrackerPage::new(screen.clone())
            .reports_panel()
            .text()
            .contains("Period: Year · 2025-01-01 to 2025-12-31")
    });

    tt.press_and_wait(Key::Tab, "Custom focused without opening dates", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.reports_panel().preset_is_focused("Custom")
            && page.reports_panel().text().contains("Period: Year")
            && !page.visible_text().contains("Custom period")
    });
    tt.press_and_wait(Key::Tab, "today's report again", |screen| {
        TimeTrackerPage::new(screen.clone())
            .reports_panel()
            .text()
            .contains("Period: Today · 2025-04-16")
    });
    tt.press_and_wait(Key::BackTab, "Custom focused again", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.reports_panel().preset_is_focused("Custom")
            && page.reports_panel().text().contains("Period: Today")
            && !page.visible_text().contains("Custom period")
    });
    tt.press_and_wait(Key::Enter, "the custom period fields", |screen| {
        TimeTrackerPage::new(screen.clone())
            .visible_text()
            .contains("Custom period")
    });
    let footer = tt.page().footer().text();
    assert!(
        footer.contains("h/l day") && footer.contains("H/L month"),
        "{footer}"
    );
    tt.press_and_wait(
        Key::Esc,
        "the Custom preset after canceling date edit",
        |screen| {
            TimeTrackerPage::new(screen.clone())
                .reports_panel()
                .preset_is_focused("Custom")
        },
    );
    tt.press_and_wait(Key::Enter, "the custom period fields again", |screen| {
        TimeTrackerPage::new(screen.clone())
            .visible_text()
            .contains("Custom period")
    });
    tt.press_and_wait(Key::Char('h'), "the previous From date", |screen| {
        TimeTrackerPage::new(screen.clone())
            .visible_text()
            .contains("From: 2025-04-15")
    });
    tt.press_and_wait(Key::Char('L'), "the next From month", |screen| {
        TimeTrackerPage::new(screen.clone())
            .visible_text()
            .contains("From: 2025-05-15")
    });
    tt.press_and_wait(Key::Char('H'), "the previous From month", |screen| {
        TimeTrackerPage::new(screen.clone())
            .visible_text()
            .contains("From: 2025-04-15")
    });
    tt.press(Key::Tab);
    tt.press_and_wait(Key::Char('l'), "the next To date", |screen| {
        TimeTrackerPage::new(screen.clone())
            .visible_text()
            .contains("To:   2025-04-17")
    });
    tt.press(Key::Tab);

    for _ in 0..10 {
        tt.press(Key::Backspace);
    }
    tt.type_text("2025-04-16");
    tt.press(Key::Tab);
    for _ in 0..10 {
        tt.press(Key::Backspace);
    }
    tt.type_text("2025-04-15");
    tt.press_and_wait(Key::Enter, "custom date order error", |screen| {
        TimeTrackerPage::new(screen.clone())
            .status_bar()
            .text()
            .contains("From must be on or before To")
    });

    for _ in 0..10 {
        tt.press(Key::Backspace);
    }
    tt.type_text("2025-04-16");
    tt.press(Key::Tab);
    for _ in 0..10 {
        tt.press(Key::Backspace);
    }
    tt.type_text("2025-04-15");
    tt.press_and_wait(Key::Enter, "the custom report", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_reports()
            && page
                .reports_panel()
                .text()
                .contains("Period: Custom · 2025-04-15 to 2025-04-16")
    });
    let custom = tt.page().reports_panel().text();
    assert!(custom.contains("Total: 3h 4m 3s"), "{custom}");

    tt.press(Key::Char('j'));
    tt.press_and_wait(Key::Enter, "the selected task's history", |screen| {
        let history = TimeTrackerPage::new(screen.clone()).worklog_history_panel();
        history.is_shown() && history.source_view() == Some("Reports")
    });
    tt.press_and_wait(Key::Esc, "the restored custom report", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_reports()
            && page
                .reports_panel()
                .text()
                .contains("Period: Custom · 2025-04-15 to 2025-04-16")
    });

    tt.resize_and_wait(60, 20, "the compact report", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.size() == (60, 20)
            && page.task_panel().shows_reports()
            && page.task_panel().frame_corners_fit_current_geometry()
            && page.reports_panel().text().contains("Total: 3h 4m 3s")
    });
    tt.quit().assert_clean_exit();
}

#[test]
fn reports_copy_the_selected_name_exact_duration_and_rounded_duration() {
    for (key, expected) in [
        (Key::Char('c'), "Copy target"),
        (Key::Char('t'), "1h 2m 3s"),
        (Key::Char('s'), "1h 0m"),
    ] {
        let context = TestContext::new();
        {
            let database = context.database();
            database.create_task("Copy target");
            database.create_worklog(
                "Copy target",
                instant("2025-04-16T14:00:00Z"),
                Some(instant("2025-04-16T15:02:03Z")),
            );
        }

        let mut tt = context.launch_for_reports(REPORT_NOW);
        tt.wait_for_first_frame("the active tab", |screen| {
            TimeTrackerPage::new(screen.clone())
                .task_panel()
                .shows_active_tasks()
        });
        tt.press_and_wait(Key::Tab, "the archived tab", |screen| {
            TimeTrackerPage::new(screen.clone())
                .task_panel()
                .shows_archived_tasks()
        });
        tt.press_and_wait(Key::Tab, "the worklogs tab", |screen| {
            TimeTrackerPage::new(screen.clone())
                .task_panel()
                .shows_worklogs()
        });
        tt.press_and_wait(Key::Tab, "the reports tab", |screen| {
            TimeTrackerPage::new(screen.clone())
                .task_panel()
                .shows_reports()
        });
        tt.press(Key::Enter);
        tt.press(Key::Char('j'));

        tt.press_and_wait(key, "the copied report value", |screen| {
            TimeTrackerPage::new(screen.clone())
                .status_bar()
                .text()
                .contains("Copied to clipboard")
        });
        assert_eq!(context.clipboard_text(), expected);
        tt.quit().assert_clean_exit();
    }
}

#[test]
fn active_task_list_copies_the_selected_name_to_the_private_clipboard() {
    let context = TestContext::new();
    context.database().create_task("Copy from active list");

    let mut tt = context.launch_for_reports(REPORT_NOW);
    tt.wait_for_first_frame("the selected active task", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_active_tasks() && page.task_panel().selected_index() == Some(0)
    });
    tt.press_and_wait(Key::Char('c'), "the copied active task name", |screen| {
        TimeTrackerPage::new(screen.clone())
            .status_bar()
            .text()
            .contains("Copied to clipboard")
    });
    assert_eq!(context.clipboard_text(), "Copy from active list");
    tt.wait_for("the copy confirmation to expire", |screen| {
        TimeTrackerPage::new(screen.clone())
            .status_bar()
            .text()
            .is_empty()
    });
    tt.quit().assert_clean_exit();
}

#[test]
fn reports_vim_and_page_motions_move_and_copy_the_selected_row() {
    let context = TestContext::new();
    {
        let database = context.database();
        for index in 0..13 {
            let name = format!("Report task {index:02}");
            database.create_task(&name);
            let start = instant("2025-04-16T10:00:00Z") + Duration::seconds(index * 60);
            let end = start + Duration::seconds(60 + index * 60);
            database.create_worklog(&name, start, Some(end));
        }
    }

    let mut tt = context.launch_for_reports(REPORT_NOW);
    tt.wait_for_first_frame("the active tab", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .shows_active_tasks()
    });
    tt.press_and_wait(Key::Tab, "the archived tab", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .shows_archived_tasks()
    });
    tt.press_and_wait(Key::Tab, "the worklogs tab", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .shows_worklogs()
    });
    tt.press_and_wait(Key::Tab, "the reports tab", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .shows_reports()
    });
    tt.press(Key::Enter);
    tt.press_and_wait(Key::Char('j'), "the selected first report row", |screen| {
        TimeTrackerPage::new(screen.clone())
            .reports_panel()
            .selected_row_text()
            .is_some()
    });

    let first = tt
        .page()
        .reports_panel()
        .selected_row_text()
        .expect("the report must select a task row");
    tt.press_and_wait(Key::Char('j'), "the next report row", |screen| {
        TimeTrackerPage::new(screen.clone())
            .reports_panel()
            .selected_row_text()
            .is_some_and(|row| row != first)
    });
    tt.press_and_wait(Key::Char('k'), "the first report row", |screen| {
        TimeTrackerPage::new(screen.clone())
            .reports_panel()
            .selected_row_text()
            .is_some_and(|row| row == first)
    });

    tt.press_and_wait(Key::Char('G'), "the last report row", |screen| {
        TimeTrackerPage::new(screen.clone())
            .reports_panel()
            .selected_row_text()
            .is_some_and(|row| row != first)
    });
    tt.press(Key::Char('g'));
    tt.press_and_wait(Key::Char('g'), "the first row after gg", |screen| {
        TimeTrackerPage::new(screen.clone())
            .reports_panel()
            .selected_row_text()
            .is_some_and(|row| row == first)
    });

    tt.press_and_wait(Key::Ctrl('d'), "ten rows down", |screen| {
        TimeTrackerPage::new(screen.clone())
            .reports_panel()
            .selected_row_text()
            .is_some_and(|row| row != first)
    });
    let row_ten = tt
        .page()
        .reports_panel()
        .selected_row_text()
        .expect("page down must keep a task selected");
    tt.press_and_wait(Key::Ctrl('d'), "the last row after page down", |screen| {
        TimeTrackerPage::new(screen.clone())
            .reports_panel()
            .selected_row_text()
            .is_some_and(|row| row != row_ten)
    });
    let last = tt
        .page()
        .reports_panel()
        .selected_row_text()
        .expect("the last report row must be selected");
    tt.press_and_wait(Key::Ctrl('u'), "two rows after page up", |screen| {
        TimeTrackerPage::new(screen.clone())
            .reports_panel()
            .selected_row_text()
            .is_some_and(|row| row != last && row != first)
    });
    tt.press_and_wait(Key::Ctrl('u'), "the first row after page up", |screen| {
        TimeTrackerPage::new(screen.clone())
            .reports_panel()
            .selected_row_text()
            .is_some_and(|row| row == first)
    });

    let expected_name = first
        .rsplit_once("  ")
        .expect("the report row must separate its duration from its task name")
        .0
        .trim();
    tt.press_and_wait(Key::Char('c'), "the copied first task name", |screen| {
        TimeTrackerPage::new(screen.clone())
            .status_bar()
            .text()
            .contains("Copied to clipboard")
    });
    assert_eq!(context.clipboard_text(), expected_name);
    tt.quit().assert_clean_exit();
}

#[test]
fn reports_adopt_external_tracking_and_return_to_current_active_tasks() {
    let context = TestContext::new();
    let database = context.database();
    let now = Utc::now();
    database.create_task("Existing task");
    database.create_worklog(
        "Existing task",
        now - Duration::minutes(30),
        Some(now - Duration::minutes(20)),
    );

    let mut tt = context.launch();
    tt.wait_for_first_frame("the active tab", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .shows_active_tasks()
    });
    tt.press_and_wait(Key::Tab, "the archived tab", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .shows_archived_tasks()
    });
    tt.press_and_wait(Key::Tab, "the worklogs tab", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .shows_worklogs()
    });
    tt.press_and_wait(Key::Tab, "today's report", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_reports() && page.reports_panel().text().contains("Existing task")
    });
    tt.press(Key::Enter);
    tt.press_and_wait(
        Key::Char('j'),
        "the selected existing report row",
        |screen| {
            TimeTrackerPage::new(screen.clone())
                .reports_panel()
                .selected_row_text()
                .is_some()
        },
    );
    let selected_report_task = tt
        .page()
        .reports_panel()
        .selected_row_text()
        .expect("the report must select the existing task")
        .split_once("  ")
        .expect("the report row must separate its duration from its task name")
        .0
        .trim()
        .to_owned();

    database.create_task("External task");
    database.create_worklog("External task", Utc::now() - Duration::seconds(30), None);

    tt.wait_for("the external worklog in the report and header", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header()
            .active_task()
            .is_some_and(|task| task.name() == "External task")
            && page.reports_panel().text().contains("External task")
            && page
                .reports_panel()
                .selected_row_text()
                .is_some_and(|row| row.starts_with(&selected_report_task))
    });
    tt.press_and_wait(Key::Esc, "top-level report tabs", |screen| {
        TimeTrackerPage::new(screen.clone())
            .footer()
            .text()
            .contains("switch tabs")
    });
    tt.press_and_wait(
        Key::BackTab,
        "the external worklog in the global tab",
        |screen| {
            let page = TimeTrackerPage::new(screen.clone());
            page.all_worklogs_panel().is_shown()
                && page.all_worklogs_panel().row_count() > 0
                && page.all_worklogs_panel().row(0).task_name() == "External task"
        },
    );
    tt.press_and_wait(Key::Tab, "the restored reports tab", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .shows_reports()
    });
    tt.press_and_wait(Key::Tab, "the refreshed active task list", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        let panel = page.task_panel();
        if panel.active_marker_index().is_none() {
            return false;
        }
        let Some(selected) = panel.selected_index() else {
            return false;
        };
        panel.shows_active_tasks()
            && panel.task_names().contains(&"External task".to_owned())
            && panel.row(selected).name() == selected_report_task
    });
    let active = tt.page().task_panel();
    let marker = active
        .active_marker_index()
        .expect("the active list must show the external tracking marker");
    assert_eq!(active.row(marker).name(), "External task");
    let selected = active
        .selected_index()
        .expect("the active list must keep a selected task");
    assert_eq!(active.row(selected).name(), selected_report_task);
    tt.quit().assert_clean_exit();
}
