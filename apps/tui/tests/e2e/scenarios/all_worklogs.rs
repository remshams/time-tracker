//! The global worklog tab and its move action.

use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use termlens::Key;

use crate::context::TestContext;
use crate::driver::TuiDriver;
use crate::page::TimeTrackerPage;

fn at(hour: u32, minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 8, 1, hour, minute, 0)
        .single()
        .expect("the fixture timestamp must be valid")
}

fn open_worklogs(tt: &mut TuiDriver) -> TimeTrackerPage {
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
    tt.press_and_wait(Key::Tab, "the reports tab", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .shows_reports()
    });
    tt.press_and_wait(Key::Tab, "the worklogs tab", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.all_worklogs_panel().is_shown() && page.task_panel().selected_tab_is_highlighted()
    })
}

#[test]
fn an_empty_global_worklog_tab_shows_its_hint() {
    let context = TestContext::new();
    let mut tt = context.launch();
    let page = open_worklogs(&mut tt);
    assert_eq!(
        page.all_worklogs_panel().empty_hint().as_deref(),
        Some("No worklogs yet.")
    );
    assert_eq!(page.all_worklogs_panel().row_count(), 0);
    tt.quit().assert_clean_exit();
}

#[test]
fn the_worklogs_tab_lists_active_and_archived_tasks_in_latest_first_order() {
    let context = TestContext::new();
    {
        let database = context.database();
        database.create_task("Early planning");
        database.create_task("Archived review");
        database.create_task("Latest build");
        database.create_worklog("Early planning", at(8, 0), Some(at(8, 30)));
        database.create_worklog("Archived review", at(9, 0), Some(at(9, 45)));
        database.create_worklog("Latest build", at(10, 0), Some(at(10, 15)));
        database.archive_task("Archived review");
    }

    let mut tt = context.launch();
    let page = open_worklogs(&mut tt);
    let panel = page.all_worklogs_panel();
    assert_eq!(panel.row_count(), 3, "{}", page.screen());
    assert_eq!(panel.row(0).task_name(), "Latest build");
    assert_eq!(panel.row(0).start_text(), "2025-08-01 10:00");
    assert_eq!(panel.row(0).duration_text(), "00:15:00");
    assert_eq!(panel.row(1).task_name(), "Archived review");
    assert_eq!(panel.row(1).end_text(), "2025-08-01 09:45");
    assert_eq!(panel.row(2).task_name(), "Early planning");
    assert_eq!(panel.row(2).duration_text(), "00:30:00");

    tt.press_and_wait(Key::Enter, "the first selected worklog", |screen| {
        TimeTrackerPage::new(screen.clone())
            .all_worklogs_panel()
            .selected_index()
            == Some(0)
    });
    tt.press_and_wait(Key::Char('j'), "the archived worklog selection", |screen| {
        TimeTrackerPage::new(screen.clone())
            .all_worklogs_panel()
            .selected_index()
            == Some(1)
    });
    tt.press_and_wait(Key::Esc, "the top worklogs tab focus", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.all_worklogs_panel().is_shown() && page.task_panel().selected_tab_is_highlighted()
    });
    tt.press_and_wait(Key::Tab, "the active tab after wrapping", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .shows_active_tasks()
    });
    tt.quit().assert_clean_exit();
}

#[test]
fn a_long_task_name_keeps_global_worklog_times_visible_at_sixty_columns() {
    let context = TestContext::new();
    let name = "Prepare the complete customer migration plan for the autumn review";
    {
        let database = context.database();
        database.create_task(name);
        database.create_worklog(name, at(8, 0), Some(at(8, 30)));
    }

    let mut tt = context.launch();
    open_worklogs(&mut tt);
    let page = tt.resize_and_wait(60, 20, "the compact worklog tab", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.size() == (60, 20)
            && page.task_panel().frame_corners_fit_current_geometry()
            && page.all_worklogs_panel().row_count() == 1
            && page.all_worklogs_panel().row(0).start_text() == "2025-08-01 08:00"
            && page.all_worklogs_panel().row(0).end_text() == "2025-08-01 08:30"
    });
    assert!(
        page.all_worklogs_panel()
            .row(0)
            .task_name()
            .starts_with("Prepare")
    );
    assert!(page.all_worklogs_panel().row(0).task_name().ends_with('…'));
    assert!(page.footer().text().contains("Enter rows"));
    tt.quit().assert_clean_exit();
}

#[test]
fn the_global_move_dialog_uses_the_same_search_and_keeps_the_moved_entry_visible() {
    let context = TestContext::new();
    let original = {
        let database = context.database();
        database.create_task("Archived source");
        database.create_task("Blue sky planning");
        let worklog = database.create_worklog("Archived source", at(9, 0), Some(at(9, 30)));
        database.archive_task("Archived source");
        worklog
    };

    let mut tt = context.launch();
    open_worklogs(&mut tt);
    tt.press_and_wait(Key::Enter, "the selected source worklog", |screen| {
        TimeTrackerPage::new(screen.clone())
            .all_worklogs_panel()
            .selected_index()
            == Some(0)
    });
    let page = tt.press_and_wait(Key::Char('m'), "the move dialog", |screen| {
        TimeTrackerPage::new(screen.clone())
            .move_worklog_dialog()
            .is_some_and(|dialog| {
                dialog.source().starts_with("Source: Archived source")
                    && dialog.query().is_empty()
                    && dialog.search_is_focused()
            })
    });
    assert_eq!(
        page.move_worklog_dialog().unwrap().results(),
        ["Blue sky planning"]
    );
    tt.type_text("bsp");
    tt.wait_for("the fuzzy destination", |screen| {
        TimeTrackerPage::new(screen.clone())
            .move_worklog_dialog()
            .is_some_and(|dialog| {
                dialog.query() == "bsp" && dialog.results() == ["Blue sky planning"]
            })
    });
    tt.press_and_wait(Key::Tab, "the selected destination", |screen| {
        TimeTrackerPage::new(screen.clone())
            .move_worklog_dialog()
            .is_some_and(|dialog| {
                !dialog.search_is_focused()
                    && dialog.selected_result().as_deref() == Some("Blue sky planning")
            })
    });
    let page = tt.press_and_wait(Key::Enter, "the moved global worklog", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.move_worklog_dialog().is_none()
            && page.all_worklogs_panel().is_shown()
            && page.all_worklogs_panel().row_count() == 1
            && page.all_worklogs_panel().row(0).task_name() == "Blue sky planning"
            && page.status_bar().text() == "Moved worklog to \"Blue sky planning\""
    });
    assert_eq!(page.all_worklogs_panel().selected_index(), Some(0));
    assert_eq!(
        page.all_worklogs_panel().row(0).start_text(),
        "2025-08-01 09:00"
    );
    tt.quit().assert_clean_exit();

    let database = context.database();
    assert!(database.worklogs_for_task("Archived source").is_empty());
    let moved = database.worklogs_for_task("Blue sky planning");
    assert_eq!(moved.len(), 1);
    assert_eq!(moved[0].id, original.id);
    assert_eq!(moved[0].start, original.start);
    assert_eq!(moved[0].end, original.end);

    let mut restarted = context.launch();
    let page = open_worklogs(&mut restarted);
    assert_eq!(page.all_worklogs_panel().row_count(), 1);
    assert_eq!(
        page.all_worklogs_panel().row(0).task_name(),
        "Blue sky planning"
    );
    restarted.quit().assert_clean_exit();
}

#[test]
fn an_overlapping_destination_keeps_the_global_move_dialog_open() {
    let context = TestContext::new();
    {
        let database = context.database();
        database.create_task("Overlap source");
        database.create_task("Overlap destination");
        database.create_worklog("Overlap source", at(10, 0), Some(at(11, 0)));
        database.create_worklog("Overlap destination", at(10, 30), Some(at(11, 30)));
    }

    let mut tt = context.launch();
    open_worklogs(&mut tt);
    tt.press_and_wait(Key::Enter, "the latest worklog selection", |screen| {
        TimeTrackerPage::new(screen.clone())
            .all_worklogs_panel()
            .selected_index()
            == Some(0)
    });
    tt.press_and_wait(Key::Char('j'), "the source worklog selection", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.all_worklogs_panel().selected_index() == Some(1)
            && page.all_worklogs_panel().row(1).task_name() == "Overlap source"
    });
    tt.press_and_wait(Key::Char('m'), "the move dialog", |screen| {
        TimeTrackerPage::new(screen.clone())
            .move_worklog_dialog()
            .is_some()
    });
    let page = tt.press_and_wait(Key::Enter, "the overlap error", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.move_worklog_dialog().is_some() && page.status_bar().is_error()
    });
    assert!(page.move_worklog_dialog().is_some());
    assert_eq!(
        context.database().worklogs_for_task("Overlap source").len(),
        1
    );
    assert_eq!(
        context
            .database()
            .worklogs_for_task("Overlap destination")
            .len(),
        1
    );
    tt.press_and_wait(Key::Esc, "the cancelled move", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.move_worklog_dialog().is_none() && page.all_worklogs_panel().is_shown()
    });
    tt.quit().assert_clean_exit();
}

#[test]
fn older_global_worklogs_remain_reachable_after_the_first_page() {
    let context = TestContext::new();
    {
        let database = context.database();
        database.create_task("Morning work");
        database.create_task("Afternoon work");
        for minute in 0..51 {
            let name = if minute % 2 == 0 {
                "Morning work"
            } else {
                "Afternoon work"
            };
            let start = at(8, minute);
            database.create_worklog(name, start, Some(start + TimeDelta::seconds(30)));
        }
    }

    let mut tt = context.launch();
    let page = open_worklogs(&mut tt);
    assert_eq!(
        page.all_worklogs_panel().row(0).start_text(),
        "2025-08-01 08:50"
    );
    tt.press_and_wait(Key::Enter, "the first global worklog selection", |screen| {
        TimeTrackerPage::new(screen.clone())
            .all_worklogs_panel()
            .selected_index()
            == Some(0)
    });
    tt.press_and_wait(Key::Char('o'), "the older global page", |screen| {
        TimeTrackerPage::new(screen.clone())
            .status_bar()
            .text()
            .contains("Loaded 1 older")
    });
    let page = tt.press_and_wait(Key::Char('G'), "the oldest global worklog", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        let panel = page.all_worklogs_panel();
        panel
            .selected_index()
            .is_some_and(|index| panel.row(index).start_text() == "2025-08-01 08:00")
    });
    let panel = page.all_worklogs_panel();
    assert_eq!(
        panel.row(panel.selected_index().unwrap()).task_name(),
        "Morning work"
    );
    tt.quit().assert_clean_exit();
}

#[test]
fn moving_a_running_global_worklog_keeps_the_timer_and_identity() {
    let context = TestContext::new();
    let original = {
        let database = context.database();
        database.create_task("Live review");
        database.create_task("Daily planning");
        let start = DateTime::from_timestamp(Utc::now().timestamp() - 8, 0)
            .expect("the active fixture start must be valid");
        database.create_worklog("Live review", start, None)
    };

    let mut tt = context.launch();
    let page = open_worklogs(&mut tt);
    assert_eq!(page.all_worklogs_panel().row_count(), 1);
    assert_eq!(page.all_worklogs_panel().row(0).task_name(), "Live review");
    assert_eq!(page.all_worklogs_panel().row(0).end_text(), "Running");
    assert_eq!(page.header().active_task().unwrap().name(), "Live review");
    tt.press_and_wait(Key::Enter, "the selected running worklog", |screen| {
        TimeTrackerPage::new(screen.clone())
            .all_worklogs_panel()
            .selected_index()
            == Some(0)
    });
    tt.press_and_wait(Key::Char('m'), "the running worklog move", |screen| {
        TimeTrackerPage::new(screen.clone())
            .move_worklog_dialog()
            .is_some_and(|dialog| dialog.source().starts_with("Source: Live review"))
    });
    let page = tt.press_and_wait(Key::Enter, "the moved running worklog", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.move_worklog_dialog().is_none()
            && page.all_worklogs_panel().row(0).task_name() == "Daily planning"
            && page.all_worklogs_panel().row(0).end_text() == "Running"
            && page
                .header()
                .active_task()
                .is_some_and(|task| task.name() == "Daily planning")
    });
    assert_eq!(page.all_worklogs_panel().selected_index(), Some(0));
    tt.quit().assert_clean_exit();

    let active = context
        .database()
        .active_worklog()
        .expect("the moved worklog must remain active");
    assert_eq!(active.id, original.id);
    assert_eq!(active.start, original.start);
    assert_eq!(active.task_name, "Daily planning");
}

#[test]
fn a_stale_global_move_refreshes_without_changing_the_worklog_task() {
    let context = TestContext::new();
    let database = context.database();
    database.create_task("Original task");
    database.create_task("Destination task");
    let original = database.create_worklog("Original task", at(9, 0), Some(at(9, 30)));

    let mut tt = context.launch();
    open_worklogs(&mut tt);
    tt.press_and_wait(Key::Enter, "the selected worklog", |screen| {
        TimeTrackerPage::new(screen.clone())
            .all_worklogs_panel()
            .selected_index()
            == Some(0)
    });
    tt.press_and_wait(Key::Char('m'), "the global move dialog", |screen| {
        TimeTrackerPage::new(screen.clone())
            .move_worklog_dialog()
            .is_some()
    });

    database.correct_worklog_start(&original, at(9, 5));
    let page = tt.press_and_wait(Key::Enter, "the refreshed stale worklog", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.move_worklog_dialog().is_none()
            && page.all_worklogs_panel().row_count() == 1
            && page.all_worklogs_panel().row(0).task_name() == "Original task"
            && page.all_worklogs_panel().row(0).start_text() == "2025-08-01 09:05"
            && page.status_bar().text() == "Worklogs changed and were refreshed"
    });
    assert_eq!(
        page.all_worklogs_panel().row(0).end_text(),
        "2025-08-01 09:35"
    );
    let source = database.worklogs_for_task("Original task");
    assert_eq!(source.len(), 1);
    assert_eq!(source[0].id, original.id);
    assert!(database.worklogs_for_task("Destination task").is_empty());
    tt.quit().assert_clean_exit();
}
