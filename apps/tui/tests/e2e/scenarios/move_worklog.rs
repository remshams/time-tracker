//! Worklog move scenarios.

use chrono::{DateTime, TimeZone, Utc};
use termlens::Key;

use crate::context::TestContext;
use crate::page::TimeTrackerPage;

fn at(hour: u32, minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 8, 1, hour, minute, 0)
        .single()
        .expect("the fixture timestamp must be valid")
}

fn open_history(tt: &mut crate::driver::TuiDriver, task_name: &str) -> TimeTrackerPage {
    let page = tt.wait_for_first_frame("the move fixture", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel()
            .task_names()
            .contains(&task_name.to_owned())
    });
    let names = page.task_panel().task_names();
    let target = names
        .iter()
        .position(|name| name == task_name)
        .expect("the source task must be visible");
    let selected = page
        .task_panel()
        .selected_index()
        .expect("the task list must have a selection");
    let key = if target >= selected {
        Key::Char('j')
    } else {
        Key::Char('k')
    };
    for _ in 0..target.abs_diff(selected) {
        tt.press_and_wait(key, "the source task selection", |screen| {
            TimeTrackerPage::new(screen.clone())
                .task_panel()
                .selected_index()
                .is_some_and(|index| index == target)
        });
    }
    tt.press_and_wait(Key::Enter, "the source worklog history", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.worklog_history_panel().is_shown()
            && page.worklog_history_panel().task_name() == task_name
    })
}

fn choose_destination(
    tt: &mut crate::driver::TuiDriver,
    query: &str,
    destination: &str,
) -> TimeTrackerPage {
    let page = tt.press_and_wait(Key::Char('m'), "the move dialog", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.move_worklog_dialog().is_some_and(|dialog| {
            dialog.source().starts_with("Source:") && dialog.query().is_empty()
        })
    });
    assert!(page.move_worklog_dialog().is_some());
    tt.type_text(query);
    let page = tt.wait_for("the fuzzy destination result", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.move_worklog_dialog().is_some_and(|dialog| {
            dialog.query() == query && dialog.results().contains(&destination.to_owned())
        })
    });
    assert_eq!(
        page.move_worklog_dialog()
            .expect("the move dialog remains open")
            .results(),
        [destination.to_owned()]
    );
    tt.press(Key::Tab);
    let page = tt.wait_for("the destination result focus", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.move_worklog_dialog()
            .is_some_and(|dialog| dialog.selected_result().as_deref() == Some(destination))
    });
    assert_eq!(
        page.move_worklog_dialog()
            .expect("the move dialog remains open")
            .selected_result()
            .as_deref(),
        Some(destination)
    );
    page
}

#[test]
fn fuzzy_move_results_rank_latest_tracking_or_update_and_exclude_ineligible_tasks() {
    let context = TestContext::new();
    {
        let database = context.database();
        database.create_task("Build task source");
        database.create_task_at("BT", at(6, 0), at(9, 0));
        database.create_task_at("Book travel", at(6, 0), at(10, 0));
        database.create_task_at("Build tools", at(6, 0), at(6, 0));
        database.create_task("Bright team");
        database.create_worklog("Build task source", at(8, 0), Some(at(8, 30)));
        database.create_worklog("Build tools", at(11, 0), Some(at(11, 30)));
        database.create_worklog("Bright team", at(12, 0), Some(at(12, 30)));
        database.archive_task("Bright team");
    }

    let mut tt = context.launch();
    open_history(&mut tt, "Build task source");
    tt.press_and_wait(Key::Char('m'), "the move dialog", |screen| {
        TimeTrackerPage::new(screen.clone())
            .move_worklog_dialog()
            .is_some()
    });
    tt.type_text("bT");
    let expected = ["Build tools", "Book travel", "BT"].map(str::to_owned);
    let page = tt.wait_for("the activity-ranked fuzzy destinations", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.move_worklog_dialog()
            .is_some_and(|dialog| dialog.query() == "bT" && dialog.results() == expected)
    });
    assert_eq!(
        page.move_worklog_dialog()
            .expect("the move dialog remains open")
            .results(),
        expected
    );
    tt.press_and_wait(Key::Tab, "the newest destination selection", |screen| {
        TimeTrackerPage::new(screen.clone())
            .move_worklog_dialog()
            .is_some_and(|dialog| {
                !dialog.search_is_focused()
                    && dialog.selected_result().as_deref() == Some("Build tools")
            })
    });
    tt.press_and_wait(
        Key::Char('j'),
        "the updated destination selection",
        |screen| {
            TimeTrackerPage::new(screen.clone())
                .move_worklog_dialog()
                .is_some_and(|dialog| dialog.selected_result().as_deref() == Some("Book travel"))
        },
    );
    tt.press_and_wait(Key::Esc, "the cancelled move", |screen| {
        TimeTrackerPage::new(screen.clone())
            .move_worklog_dialog()
            .is_none()
    });
    tt.quit().assert_clean_exit();
}

#[test]
fn completed_worklog_moves_to_a_fuzzy_active_destination_and_survives_restart() {
    let context = TestContext::new();
    let original = {
        let database = context.database();
        database.create_task("Build release");
        database.create_task("Blue sky planning");
        database.create_worklog("Build release", at(9, 0), Some(at(9, 30)))
    };

    let mut tt = context.launch();
    open_history(&mut tt, "Build release");
    choose_destination(&mut tt, "bsp", "Blue sky planning");
    let page = tt.press_and_wait(Key::Enter, "the moved completed history", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.worklog_history_panel().is_shown()
            && page.worklog_history_panel().task_name() == "Build release"
            && page.worklog_history_panel().row_count() == 0
            && page.status_bar().text() == "Moved worklog to \"Blue sky planning\""
    });
    assert_eq!(page.worklog_history_panel().row_count(), 0);
    tt.quit().assert_clean_exit();

    let database = context.database();
    assert!(database.worklogs_for_task("Build release").is_empty());
    let moved = database.worklogs_for_task("Blue sky planning");
    assert_eq!(moved.len(), 1);
    assert_eq!(moved[0].id, original.id);
    assert_eq!(moved[0].start, original.start);
    assert_eq!(moved[0].end, original.end);

    let mut restarted = context.launch();
    open_history(&mut restarted, "Blue sky planning");
    let page = restarted.page();
    assert_eq!(page.worklog_history_panel().row_count(), 1);
    assert_eq!(
        page.worklog_history_panel().row(0).start_text(),
        "2025-08-01 09:00"
    );
    restarted.quit().assert_clean_exit();
}

#[test]
fn active_worklog_move_preserves_identity_start_and_elapsed_timer() {
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
    let before = tt.wait_for_first_frame("the active source", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header()
            .active_task()
            .is_some_and(|active| active.name() == "Live review")
    });
    let before_elapsed = before
        .header()
        .active_task()
        .expect("the source timer is visible")
        .elapsed_seconds()
        .expect("the source timer has a valid duration");
    open_history(&mut tt, "Live review");
    choose_destination(&mut tt, "dp", "Daily planning");
    let page = tt.press_and_wait(Key::Enter, "the moved active history", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.worklog_history_panel().is_shown()
            && page.worklog_history_panel().task_name() == "Live review"
            && page.worklog_history_panel().row_count() == 0
            && page.header().active_task().is_some_and(|active| {
                active.name() == "Daily planning"
                    && active
                        .elapsed_seconds()
                        .is_some_and(|elapsed| elapsed >= before_elapsed)
            })
    });
    assert!(
        page.header().active_task().is_some_and(|active| {
            active.name() == "Daily planning" && active.elapsed_is_hhmmss()
        })
    );
    tt.quit().assert_clean_exit();

    let moved = context
        .database()
        .active_worklog()
        .expect("the active worklog must remain active");
    assert_eq!(moved.id, original.id);
    assert_eq!(moved.start, original.start);
    assert_eq!(moved.task_name, "Daily planning");
}

#[test]
fn destination_overlap_keeps_move_dialog_and_storage_unchanged() {
    let context = TestContext::new();
    let (source, before_source, before_destination) = {
        let database = context.database();
        database.create_task("Overlap source");
        database.create_task("Overlap destination");
        let source = database.create_worklog("Overlap source", at(10, 0), Some(at(11, 0)));
        database.create_worklog("Overlap destination", at(10, 30), Some(at(11, 30)));
        (
            source,
            database.worklogs_for_task("Overlap source"),
            database.worklogs_for_task("Overlap destination"),
        )
    };

    let mut tt = context.launch();
    open_history(&mut tt, "Overlap source");
    choose_destination(&mut tt, "od", "Overlap destination");
    let page = tt.press_and_wait(Key::Enter, "the overlap rejection", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.move_worklog_dialog().is_some_and(|dialog| {
            dialog.query() == "od"
                && dialog.selected_result().as_deref() == Some("Overlap destination")
        }) && page.status_bar().is_error()
    });
    assert!(page.move_worklog_dialog().is_some());
    assert_eq!(page.worklog_history_panel().task_name(), "Overlap source");
    assert_eq!(
        context.database().worklogs_for_task("Overlap source"),
        before_source
    );
    assert_eq!(
        context.database().worklogs_for_task("Overlap destination"),
        before_destination
    );
    assert_eq!(source.task_name, "Overlap source");
    tt.press_and_wait(Key::Esc, "the cancelled overlap move", |screen| {
        TimeTrackerPage::new(screen.clone())
            .move_worklog_dialog()
            .is_none()
    });
    tt.quit().assert_clean_exit();
}

#[test]
fn archived_source_history_moves_a_completed_worklog_to_an_active_task() {
    let context = TestContext::new();
    let original = {
        let database = context.database();
        database.create_task("Archived source");
        database.create_task("Active destination");
        let worklog = database.create_worklog("Archived source", at(8, 0), Some(at(8, 20)));
        database.archive_task("Archived source");
        worklog
    };

    let mut tt = context.launch();
    tt.press_and_wait(Key::Tab, "the archived task view", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_archived_tasks()
            && page.task_panel().task_names() == ["Archived source".to_owned()]
    });
    tt.press_and_wait(Key::Enter, "the archived source history", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.worklog_history_panel().is_shown()
            && page.worklog_history_panel().task_name() == "Archived source"
    });
    choose_destination(&mut tt, "ad", "Active destination");
    let page = tt.press_and_wait(Key::Enter, "the moved archived history", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.worklog_history_panel().task_name() == "Archived source"
            && page.worklog_history_panel().row_count() == 0
            && page.status_bar().text() == "Moved worklog to \"Active destination\""
    });
    assert_eq!(page.worklog_history_panel().row_count(), 0);
    tt.quit().assert_clean_exit();

    let database = context.database();
    assert!(database.worklogs_for_task("Archived source").is_empty());
    let moved = database.worklogs_for_task("Active destination");
    assert_eq!(moved.len(), 1);
    assert_eq!(moved[0].id, original.id);
    assert_eq!(moved[0].start, original.start);
    assert_eq!(moved[0].end, original.end);
}
