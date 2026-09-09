//! Worklog correction scenarios.

use std::collections::HashSet;

use chrono::{DateTime, TimeZone, Utc};
use termlens::Key;

use crate::context::TestContext;
use crate::page::TimeTrackerPage;

fn at(hour: u32, minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 7, 12, hour, minute, 0)
        .single()
        .expect("the fixture timestamp must be valid")
}

fn correction_text(timestamp: DateTime<Utc>) -> String {
    timestamp
        .format("%Y-%m-%dT%H:%M:%S.000000+00:00")
        .to_string()
}

fn open_history(tt: &mut crate::driver::TuiDriver, task_name: &str) -> TimeTrackerPage {
    tt.wait_for_first_frame("the seeded task", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().task_names() == [task_name.to_owned()]
            && page.task_panel().selected_index() == Some(0)
    });
    tt.press_and_wait(Key::Enter, "the task history", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.worklog_history_panel().is_shown()
            && page.worklog_history_panel().task_name() == task_name
    })
}

#[test]
fn completed_worklog_correction_persists_and_refreshes_history() {
    let context = TestContext::new();
    let database = context.database();
    database.create_task("Release review");
    let original = database.create_worklog("Release review", at(10, 0), Some(at(12, 0)));
    database.create_worklog("Release review", at(8, 0), Some(at(8, 30)));

    let mut tt = context.launch();
    open_history(&mut tt, "Release review");
    let page = tt.press_and_wait(Key::Char('e'), "the correction dialog", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.correction_dialog().is_some()
    });
    assert!(page.footer().text().contains("tab/S-tab"));

    let page = tt.press_and_wait(Key::Char('j'), "the minute adjustment", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.status_bar().text() == "Adjusted timestamp" && page.correction_dialog().is_some()
    });
    assert_eq!(
        page.correction_dialog().unwrap().start_text(),
        "2025-07-12T10:05:00.000000+00:00"
    );
    tt.press_and_wait(Key::Tab, "the end correction field", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.correction_dialog()
            .is_some_and(|dialog| dialog.focused_field() == "End")
    });

    tt.press_and_wait(Key::Char('K'), "the hour adjustment", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.correction_dialog().is_some_and(|dialog| {
            dialog.end_text().as_deref() == Some("2025-07-12T11:00:00.000000+00:00")
        })
    });
    let page = tt.press_and_wait(Key::Enter, "the corrected history", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        let panel = page.worklog_history_panel();
        panel.is_shown()
            && page.correction_dialog().is_none()
            && page.status_bar().text() == "Corrected worklog"
            && panel.row_count() == 2
            && panel.row(0).start_text() == "2025-07-12 10:05:00 +00:00"
            && panel.row(0).duration_text() == "00:55:00"
            && panel.row(1).start_text() == "2025-07-12 08:00:00 +00:00"
    });
    assert_eq!(page.worklog_history_panel().selected_index(), Some(0));

    let corrected = context.database().worklogs_for_task("Release review");
    let stored = corrected
        .iter()
        .find(|worklog| worklog.id == original.id)
        .unwrap();
    assert_eq!(stored.id, original.id);
    assert_eq!(stored.task_id, original.task_id);
    assert_eq!(stored.task_name, original.task_name);
    assert_eq!(stored.start, at(10, 5));
    assert_eq!(stored.end, Some(at(11, 0)));
    tt.quit().assert_clean_exit();
}

#[test]
fn active_worklog_start_correction_keeps_tracking_and_reanchors_timers() {
    let context = TestContext::new();
    let database = context.database();
    database.create_task("Live review");
    let start = DateTime::from_timestamp(Utc::now().timestamp() - 600, 0)
        .expect("the active fixture timestamp must be valid");
    let original = database.create_worklog("Live review", start, None);

    let mut tt = context.launch();
    open_history(&mut tt, "Live review");
    tt.press_and_wait(Key::Char('e'), "the active correction dialog", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.correction_dialog()
            .is_some_and(|dialog| dialog.end_text().is_none() && dialog.focused_field() == "Start")
    });
    let page = tt.press_and_wait(Key::Char('j'), "the active minute adjustment", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.status_bar().text() == "Adjusted timestamp" && page.correction_dialog().is_some()
    });
    assert_eq!(
        page.correction_dialog().unwrap().start_text(),
        correction_text(start + chrono::TimeDelta::minutes(5))
    );
    assert!(page.correction_dialog().unwrap().end_text().is_none());
    let page = tt.press_and_wait(Key::Enter, "the reanchored active history", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        let header = page.header().active_task();
        let row = page.worklog_history_panel().row(0);
        header.is_some_and(|active| active.name() == "Live review")
            && row.is_running()
            && row.duration_is_hhmmss()
            && page.status_bar().text() == "Corrected worklog"
            && page.correction_dialog().is_none()
    });
    let header_seconds = page
        .header()
        .active_task()
        .unwrap()
        .elapsed_seconds()
        .unwrap();
    let row_seconds = page
        .worklog_history_panel()
        .row(0)
        .duration_seconds()
        .unwrap();
    assert!(header_seconds.abs_diff(row_seconds) <= 1);
    let after = context.database().active_worklog().unwrap();
    assert_eq!(after.id, original.id);
    assert_eq!(after.task_id, original.task_id);
    assert_eq!(after.start, start + chrono::TimeDelta::minutes(5));
    assert!(after.end.is_none());
    tt.quit().assert_clean_exit();
}

#[test]
fn correction_escape_reports_cancelled_and_preserves_storage() {
    let context = TestContext::new();
    let database = context.database();
    database.create_task("Cancel review");
    database.create_worklog("Cancel review", at(10, 0), Some(at(11, 0)));
    let before = database.worklogs_for_task("Cancel review");

    let mut tt = context.launch();
    open_history(&mut tt, "Cancel review");
    tt.press_and_wait(Key::Char('e'), "the correction dialog", |screen| {
        TimeTrackerPage::new(screen.clone())
            .correction_dialog()
            .is_some()
    });

    let page = tt.press_and_wait(Key::Esc, "the cancelled correction", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.correction_dialog().is_none()
            && page.status_bar().text() == "Correction cancelled"
            && page.worklog_history_panel().row(0).start_text() == "2025-07-12 10:00:00 +00:00"
    });
    assert_eq!(page.status_bar().text(), "Correction cancelled");
    assert_eq!(
        context.database().worklogs_for_task("Cancel review"),
        before
    );
    tt.quit().assert_clean_exit();
}

#[test]
fn older_page_load_after_another_client_changes_start_reloads_newest_page() {
    let context = TestContext::new();
    let base = at(9, 0);
    {
        let database = context.database();
        database.create_task("History correction");
        for minute in 0..102 {
            let start = base + chrono::TimeDelta::minutes(minute);
            database.create_worklog(
                "History correction",
                start,
                Some(start + chrono::TimeDelta::seconds(30)),
            );
        }
    }
    let oldest = context
        .database()
        .worklogs_for_task("History correction")
        .into_iter()
        .find(|worklog| worklog.start == base)
        .expect("the oldest worklog must be stored");

    let mut tt = context.launch();
    open_history(&mut tt, "History correction");
    tt.press_and_wait(Key::Char('o'), "the older history page", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.status_bar().text() == "Loaded 50 older worklogs"
    });

    context.database().correct_worklog_start(&oldest, at(12, 0));

    let page = tt.press_and_wait(Key::Char('o'), "the refreshed newest page", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        let panel = page.worklog_history_panel();
        panel.is_shown()
            && page.status_bar().text() == "History changed and was refreshed"
            && panel.row(0).start_text() == "2025-07-12 12:00:00 +00:00"
    });
    let starts: Vec<String> = (0..page.worklog_history_panel().row_count())
        .map(|index| page.worklog_history_panel().row(index).start_text())
        .collect();
    let unique: HashSet<&String> = starts.iter().collect();
    assert_eq!(unique.len(), starts.len());
    assert_eq!(
        starts
            .iter()
            .filter(|start| **start == "2025-07-12 12:00:00 +00:00")
            .count(),
        1
    );
    assert!(!starts.contains(&"2025-07-12 09:51:00 +00:00".to_owned()));

    tt.quit().assert_clean_exit();
}

#[test]
fn archived_history_allows_worklog_correction() {
    let context = TestContext::new();
    let database = context.database();
    database.create_task("Archived review");
    database.create_worklog("Archived review", at(9, 0), Some(at(9, 30)));
    database.archive_task("Archived review");

    let mut tt = context.launch();
    tt.wait_for_first_frame("the empty active view", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .empty_hint()
            .is_some()
    });
    tt.press_and_wait(Key::Char('l'), "the archived task", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().task_names() == ["Archived review".to_owned()]
    });
    tt.press_and_wait(Key::Enter, "the archived history", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.worklog_history_panel().row_count() == 1
    });
    tt.press_and_wait(Key::Char('e'), "the archived correction dialog", |screen| {
        TimeTrackerPage::new(screen.clone())
            .correction_dialog()
            .is_some()
    });
    tt.press_and_wait(Key::Esc, "the unchanged archived history", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.correction_dialog().is_none()
            && page.worklog_history_panel().row(0).start_text() == "2025-07-12 09:00:00 +00:00"
    });
    tt.quit().assert_clean_exit();
}

#[test]
fn overlapping_correction_keeps_the_full_draft_and_storage() {
    let context = TestContext::new();
    let database = context.database();
    database.create_task("Overlap review");
    database.create_worklog("Overlap review", at(10, 0), Some(at(11, 0)));
    database.create_worklog("Overlap review", at(12, 0), Some(at(13, 0)));
    let before = database.worklogs_for_task("Overlap review");

    let mut tt = context.launch();
    open_history(&mut tt, "Overlap review");
    tt.press_and_wait(Key::Char('e'), "the overlap correction dialog", |screen| {
        TimeTrackerPage::new(screen.clone())
            .correction_dialog()
            .is_some()
    });
    tt.type_text(&"k".repeat(13));
    let page = tt.press_and_wait(Key::Enter, "the overlap error", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.status_bar().text() == "Error: The corrected time overlaps another worklog"
            && page
                .correction_dialog()
                .is_some_and(|dialog| dialog.is_full_draft_visible())
    });
    assert!(page.status_bar().is_error());
    assert_eq!(
        context.database().worklogs_for_task("Overlap review"),
        before
    );
    tt.quit().assert_clean_exit();
}

#[test]
fn stale_correction_keeps_draft_and_requests_cancel_and_refresh() {
    let context = TestContext::new();
    let database = context.database();
    database.create_task("Stale review");
    database.create_worklog("Stale review", at(10, 0), Some(at(11, 0)));

    let mut stale = context.launch();
    open_history(&mut stale, "Stale review");
    stale.press_and_wait(Key::Char('e'), "the stale correction dialog", |screen| {
        TimeTrackerPage::new(screen.clone())
            .correction_dialog()
            .is_some()
    });

    let mut current = context.launch();
    open_history(&mut current, "Stale review");
    current.press_and_wait(Key::Char('e'), "the current correction dialog", |screen| {
        TimeTrackerPage::new(screen.clone())
            .correction_dialog()
            .is_some()
    });
    current.press_and_wait(Key::Char('j'), "the current adjustment", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.status_bar().text() == "Adjusted timestamp" && page.correction_dialog().is_some()
    });
    current.press_and_wait(Key::Enter, "the current correction", |screen| {
        TimeTrackerPage::new(screen.clone()).status_bar().text() == "Corrected worklog"
    });

    let page = stale.press_and_wait(Key::Enter, "the stale correction error", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.status_bar().text() == "Error: Worklog changed. Cancel and press r to refresh."
            && page.correction_dialog().is_some()
            && page.correction_dialog().unwrap().is_full_draft_visible()
    });
    assert!(page.status_bar().is_error());
    assert_eq!(
        context.database().worklogs_for_task("Stale review")[0].start,
        at(10, 5)
    );
    stale.quit().assert_clean_exit();
    current.quit().assert_clean_exit();
}
