//! Task-list ordering scenarios.

use chrono::{DateTime, Utc};
use termlens::Key;

use crate::context::TestContext;
use crate::page::TimeTrackerPage;

fn at(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 0).expect("the fixture timestamp must be valid")
}

#[test]
fn an_existing_database_starts_with_recently_worked_tasks_first() {
    let context = TestContext::new();
    let (before_launch, recent_worklog, earlier_worklog) = {
        let database = context.database();
        database.create_task_at("Earlier planning", at(100), at(100));
        database.create_task_at("New untouched task", at(1_000), at(1_000));
        database.create_task_at("Recent account work", at(200), at(200));
        let earlier = database.create_worklog("Earlier planning", at(700), Some(at(750)));
        let recent = database.create_worklog("Recent account work", at(900), Some(at(950)));
        (database.tasks(), recent, earlier)
    };

    let mut tt = context.launch();
    let page = tt.wait_for_first_frame("the recently worked task order", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_active_tasks()
            && page.task_panel().shows_ordering("recently worked")
            && page.task_panel().task_names()
                == [
                    "Recent account work".to_owned(),
                    "Earlier planning".to_owned(),
                    "New untouched task".to_owned(),
                ]
            && page.task_panel().selected_index() == Some(0)
            && page.footer().hints_sorting()
    });

    assert_eq!(
        page.task_panel().task_names(),
        [
            "Recent account work".to_owned(),
            "Earlier planning".to_owned(),
            "New untouched task".to_owned(),
        ],
        "worked tasks are ordered by their latest worklog and untouched tasks follow:\n{}",
        page.screen()
    );
    assert!(page.task_panel().shows_ordering("recently worked"));
    assert!(page.footer().hints_sorting());

    tt.quit().assert_clean_exit();

    let database = context.database();
    assert_eq!(
        database.tasks(),
        before_launch,
        "launch changed task metadata"
    );
    let recent = database.worklogs_for_task("Recent account work");
    let earlier = database.worklogs_for_task("Earlier planning");
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].id, recent_worklog.id);
    assert_eq!(earlier.len(), 1);
    assert_eq!(earlier[0].id, earlier_worklog.id);
}

#[test]
fn tracking_and_sorting_reorder_rows_without_losing_selection() {
    let context = TestContext::new();
    let selected_task = {
        let database = context.database();
        let selected = database.create_task_at("Draft proposal", at(100), at(500));
        database.create_task_at("Review metrics", at(300), at(300));
        database.create_task_at("Send invoice", at(200), at(800));
        database.create_worklog("Draft proposal", at(400), Some(at(450)));
        database.create_worklog("Send invoice", at(900), Some(at(950)));
        selected
    };

    let mut tt = context.launch();
    tt.wait_for_first_frame("the default task order", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_ordering("recently worked")
            && page.task_panel().task_names()
                == [
                    "Send invoice".to_owned(),
                    "Draft proposal".to_owned(),
                    "Review metrics".to_owned(),
                ]
            && page.task_panel().selected_index() == Some(0)
    });

    let page = tt.press_and_wait(Key::Char('j'), "the proposal selection", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().selected_index() == Some(1)
            && page.task_panel().row(1).name() == "Draft proposal"
    });
    assert_eq!(page.task_panel().row(1).name(), "Draft proposal");

    let page = tt.press_and_wait(Key::Char(' '), "the tracked task at the top", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header()
            .active_task()
            .is_some_and(|active| active.name() == "Draft proposal")
            && page.task_panel().task_names()
                == [
                    "Draft proposal".to_owned(),
                    "Send invoice".to_owned(),
                    "Review metrics".to_owned(),
                ]
            && page.task_panel().selected_index() == Some(0)
            && page.task_panel().active_marker_index() == Some(0)
            && page.status_bar().text() == "Started \"Draft proposal\""
    });
    assert_eq!(page.task_panel().row(0).name(), "Draft proposal");

    let database = context.database();
    let active = database
        .active_worklog()
        .expect("tracking created one active worklog");
    assert_eq!(active.task_id, selected_task.id);

    let page = tt.press_and_wait(Key::Char('s'), "the recently updated order", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_ordering("recently updated")
            && page.task_panel().task_names()
                == [
                    "Send invoice".to_owned(),
                    "Draft proposal".to_owned(),
                    "Review metrics".to_owned(),
                ]
            && page.task_panel().selected_index() == Some(1)
            && page.task_panel().active_marker_index() == Some(1)
            && page.status_bar().text() == "Sorted by recently updated"
    });
    assert_eq!(page.task_panel().row(1).name(), "Draft proposal");

    let page = tt.press_and_wait(Key::Char('s'), "the recently created order", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_ordering("recently created")
            && page.task_panel().task_names()
                == [
                    "Review metrics".to_owned(),
                    "Send invoice".to_owned(),
                    "Draft proposal".to_owned(),
                ]
            && page.task_panel().selected_index() == Some(2)
            && page.task_panel().active_marker_index() == Some(2)
            && page.status_bar().text() == "Sorted by recently created"
    });
    assert_eq!(page.task_panel().row(2).name(), "Draft proposal");

    let stored = database
        .task_by_name("Draft proposal")
        .expect("the selected task remains stored");
    assert_eq!(stored.id, selected_task.id);
    assert_eq!(stored.created_at, selected_task.created_at);
    assert_eq!(stored.updated_at, selected_task.updated_at);

    tt.quit().assert_clean_exit();
    assert_eq!(
        database.active_worklog().map(|worklog| worklog.id),
        Some(active.id),
        "sorting and quitting leave the same worklog active"
    );
}
