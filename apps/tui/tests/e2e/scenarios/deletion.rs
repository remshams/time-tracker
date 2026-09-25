//! Completed-worklog deletion scenarios.

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
    tt.wait_for_first_frame("the deletion fixture", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().task_names() == [task_name.to_owned()]
            && page.task_panel().selected_index() == Some(0)
    });
    tt.press_and_wait(Key::Enter, "the worklog history", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.worklog_history_panel().is_shown()
            && page.worklog_history_panel().task_name() == task_name
    })
}

#[test]
fn completed_deletion_removes_the_row_selects_its_replacement_and_persists() {
    let context = TestContext::new();
    let database = context.database();
    database.create_task("Deletion review");
    let surviving = database.create_worklog("Deletion review", at(10, 0), Some(at(10, 30)));
    let target = database.create_worklog("Deletion review", at(12, 0), Some(at(12, 30)));

    let mut tt = context.launch();
    open_history(&mut tt, "Deletion review");
    tt.press_and_wait(Key::Char('d'), "the deletion confirmation", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.deletion_dialog().is_some_and(|dialog| {
            dialog.title() == "Delete worklog"
                && dialog.question() == "Delete this worklog permanently?"
                && dialog.interval() == "2025-08-01 12:00 → 2025-08-01 12:30"
                && dialog.warning() == "This cannot be undone."
        })
    });
    let page = tt.press_and_wait(Key::Char('y'), "the deleted history row", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        let history = page.worklog_history_panel();
        history.row_count() == 1
            && history.row(0).start_text() == "2025-08-01 10:00"
            && history.selected_index() == Some(0)
            && page.deletion_dialog().is_none()
            && page.status_bar().text() == "Deleted worklog"
    });
    assert_eq!(
        page.worklog_history_panel().row(0).start_text(),
        "2025-08-01 10:00"
    );
    assert_eq!(page.worklog_history_panel().selected_index(), Some(0));
    tt.quit().assert_clean_exit();

    let stored = context.database().worklogs_for_task("Deletion review");
    assert_eq!(stored, vec![surviving]);
    assert!(!stored.iter().any(|worklog| worklog.id == target.id));
}

#[test]
fn deletion_escape_and_n_cancel_and_preserve_the_row() {
    let context = TestContext::new();
    let database = context.database();
    database.create_task("Cancel deletion");
    let before = database.create_worklog("Cancel deletion", at(9, 0), Some(at(9, 15)));

    let mut tt = context.launch();
    open_history(&mut tt, "Cancel deletion");
    tt.press_and_wait(
        Key::Char('d'),
        "the first deletion confirmation",
        |screen| {
            TimeTrackerPage::new(screen.clone())
                .deletion_dialog()
                .is_some()
        },
    );
    tt.press_and_wait(Key::Esc, "the escaped deletion", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.deletion_dialog().is_none()
            && page.status_bar().text() == "Deletion cancelled"
            && page.worklog_history_panel().row_count() == 1
    });
    tt.press_and_wait(
        Key::Char('d'),
        "the second deletion confirmation",
        |screen| {
            TimeTrackerPage::new(screen.clone())
                .deletion_dialog()
                .is_some()
        },
    );
    let page = tt.press_and_wait(Key::Char('n'), "the refused deletion", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.deletion_dialog().is_none()
            && page.status_bar().text() == "Deletion cancelled"
            && page.worklog_history_panel().row_count() == 1
    });
    assert_eq!(page.worklog_history_panel().selected_index(), Some(0));
    tt.quit().assert_clean_exit();
    assert_eq!(
        context.database().worklogs_for_task("Cancel deletion"),
        vec![before]
    );
}

#[test]
fn pressing_d_again_confirms_deletion() {
    let context = TestContext::new();
    let database = context.database();
    database.create_task("Double key deletion");
    let target = database.create_worklog("Double key deletion", at(8, 0), Some(at(8, 20)));

    let mut tt = context.launch();
    open_history(&mut tt, "Double key deletion");
    tt.press_and_wait(Key::Char('d'), "the deletion confirmation", |screen| {
        TimeTrackerPage::new(screen.clone())
            .deletion_dialog()
            .is_some()
    });
    tt.press_and_wait(Key::Char('d'), "the second-d deletion", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.deletion_dialog().is_none()
            && page.status_bar().text() == "Deleted worklog"
            && page.worklog_history_panel().row_count() == 0
    });
    tt.quit().assert_clean_exit();
    assert!(
        context
            .database()
            .worklogs_for_task("Double key deletion")
            .iter()
            .all(|worklog| worklog.id != target.id)
    );
}

#[test]
fn running_worklog_rejects_deletion_without_a_dialog() {
    let context = TestContext::new();
    let database = context.database();
    database.create_task("Running deletion");
    let active = database.create_worklog("Running deletion", at(7, 0), None);

    let mut tt = context.launch();
    open_history(&mut tt, "Running deletion");
    let page = tt.press_and_wait(Key::Char('d'), "the running deletion rejection", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.deletion_dialog().is_none()
            && page.status_bar().text() == "Error: Running worklogs cannot be deleted"
            && page.worklog_history_panel().row(0).is_running()
    });
    assert_eq!(
        page.status_bar().text(),
        "Error: Running worklogs cannot be deleted"
    );
    tt.quit().assert_clean_exit();
    let stored = context
        .database()
        .active_worklog()
        .expect("the active row must remain");
    assert_eq!(stored.id, active.id);
    assert_eq!(stored.start, active.start);
    assert!(stored.end.is_none());
}

#[test]
fn archived_history_deletion_stays_deleted_after_reopening_history() {
    let context = TestContext::new();
    let database = context.database();
    database.create_task("Archived deletion");
    let target = database.create_worklog("Archived deletion", at(6, 0), Some(at(6, 10)));
    database.archive_task("Archived deletion");

    let mut tt = context.launch();
    tt.wait_for_first_frame("the empty active view", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .empty_hint()
            .is_some()
    });
    tt.press_and_wait(Key::Tab, "the archived task", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .task_names()
            == ["Archived deletion".to_owned()]
    });
    tt.press_and_wait(Key::Enter, "the archived history", |screen| {
        TimeTrackerPage::new(screen.clone())
            .worklog_history_panel()
            .row_count()
            == 1
    });
    tt.press_and_wait(
        Key::Char('d'),
        "the archived deletion confirmation",
        |screen| {
            TimeTrackerPage::new(screen.clone())
                .deletion_dialog()
                .is_some()
        },
    );
    tt.press_and_wait(Key::Enter, "the archived deletion", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.status_bar().text() == "Deleted worklog"
            && page.worklog_history_panel().row_count() == 0
    });
    tt.press_and_wait(Key::Esc, "the archived task list", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_archived_tasks() && page.task_panel().selected_index() == Some(0)
    });
    let page = tt.press_and_wait(
        Key::Enter,
        "the reopened empty archived history",
        |screen| {
            let page = TimeTrackerPage::new(screen.clone());
            page.worklog_history_panel().row_count() == 0
                && page.worklog_history_panel().empty_hint() == Some("No worklogs yet.")
        },
    );
    assert_eq!(page.worklog_history_panel().row_count(), 0);
    tt.quit().assert_clean_exit();
    assert!(
        context
            .database()
            .worklogs_for_task("Archived deletion")
            .iter()
            .all(|worklog| worklog.id != target.id)
    );
}
