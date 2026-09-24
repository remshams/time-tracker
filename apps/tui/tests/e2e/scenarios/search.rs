//! Task-list search scenarios.

use chrono::{DateTime, Utc};
use termlens::Key;

use crate::context::TestContext;
use crate::page::TimeTrackerPage;

fn at(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 0).expect("the fixture timestamp must be valid")
}

#[test]
fn active_search_filters_fuzzily_by_latest_work_or_update_and_preserves_sort() {
    let context = TestContext::new();
    {
        let database = context.database();
        database.create_task_at("Book travel", at(100), at(800));
        database.create_task_at("Build testing", at(100), at(200));
        database.create_task_at("Backlog triage", at(100), at(300));
        database.create_task_at("Update docs", at(100), at(950));
        database.create_worklog("Build testing", at(900), Some(at(910)));
        database.create_worklog("Backlog triage", at(700), Some(at(710)));
    }

    let mut tt = context.launch();
    tt.wait_for_first_frame("the active task list", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .shows_ordering("recently worked")
    });
    tt.press_and_wait(Key::Char('s'), "the recently updated sort", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_ordering("recently updated")
            && page.task_panel().task_names().first().map(String::as_str) == Some("Update docs")
    });

    tt.press_and_wait(Key::Char('/'), "the empty search field", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .search_query()
            .as_deref()
            == Some("")
    });
    tt.type_text("bT");
    let expected = ["Build testing", "Book travel", "Backlog triage"].map(str::to_owned);
    let page = tt.wait_for("the fuzzy search results", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().search_query().as_deref() == Some("bT")
            && page.task_panel().shows_ordering("latest activity")
            && page.task_panel().task_names() == expected
            && page.task_panel().selected_index() == Some(0)
    });
    assert_eq!(page.task_panel().task_names(), expected);

    tt.press_and_wait(Key::Enter, "the committed search", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().search_query().as_deref() == Some("bT")
            && page.footer().hints_open_history()
    });
    tt.press_and_wait(Key::Char('j'), "the updated task selection", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().selected_index() == Some(1)
            && page.task_panel().row(1).name() == "Book travel"
    });
    tt.press_and_wait(Key::Enter, "the selected search result history", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.worklog_history_panel().is_shown()
            && page.worklog_history_panel().task_name() == "Book travel"
    });
    tt.press_and_wait(Key::Esc, "the filtered task list", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().task_names() == expected && page.task_panel().selected_index() == Some(1)
    });
    let page = tt.press_and_wait(Key::Esc, "the restored sort", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().search_query().is_none()
            && page.task_panel().shows_ordering("recently updated")
            && page.task_panel().task_names()
                == [
                    "Update docs".to_owned(),
                    "Book travel".to_owned(),
                    "Backlog triage".to_owned(),
                    "Build testing".to_owned(),
                ]
    });
    assert!(page.task_panel().search_query().is_none());
    tt.quit().assert_clean_exit();
}

#[test]
fn search_no_match_and_escape_restore_the_full_task_list() {
    let context = TestContext::new();
    {
        let database = context.database();
        database.create_task("Plan release");
        database.create_task("Review patch");
    }
    let mut tt = context.launch();
    let initial = tt.wait_for_first_frame("the task list", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .task_names()
            .len()
            == 2
    });
    let all_tasks = initial.task_panel().task_names();

    tt.press_and_wait(Key::Char('/'), "the search field", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .search_query()
            .is_some()
    });
    tt.type_text("zz");
    tt.wait_for("the no-match hint", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().search_query().as_deref() == Some("zz")
            && page.task_panel().search_has_no_matches()
            && page.task_panel().task_names().is_empty()
            && page.task_panel().empty_hint().is_none()
    });
    tt.press_and_wait(Key::Esc, "the cancelled search", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().search_query().is_none() && page.task_panel().task_names() == all_tasks
    });

    tt.press_and_wait(Key::Char('/'), "the second search field", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .search_query()
            .is_some()
    });
    tt.type_text("pr");
    tt.wait_for("the matching search result", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().search_query().as_deref() == Some("pr")
            && page.task_panel().task_names() == ["Plan release".to_owned()]
    });
    tt.press_and_wait(Key::Enter, "the committed filter", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().search_query().as_deref() == Some("pr")
            && page.footer().hints_open_history()
    });
    tt.press_and_wait(Key::Esc, "the cleared filter", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().search_query().is_none() && page.task_panel().task_names() == all_tasks
    });
    tt.quit().assert_clean_exit();
}

#[test]
fn archived_search_only_shows_archived_matches_in_activity_order() {
    let context = TestContext::new();
    {
        let database = context.database();
        database.create_task_at("Build proposal", at(0), at(800));
        database.create_task_at("Brief planning", at(0), at(100));
        database.create_task("Bright project");
        database.create_worklog("Brief planning", at(900), Some(at(910)));
        database.create_worklog("Bright project", at(1_000), Some(at(1_010)));
        database.archive_task("Build proposal");
        database.archive_task("Brief planning");
    }

    let mut tt = context.launch();
    tt.wait_for_first_frame("the active task", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .task_names()
            == ["Bright project".to_owned()]
    });
    tt.press_and_wait(Key::Char('l'), "the archived task list", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_archived_tasks() && page.task_panel().task_names().len() == 2
    });
    tt.press_and_wait(Key::Char('/'), "the archived search field", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .search_query()
            .is_some()
    });
    tt.type_text("bP");
    let page = tt.wait_for("the archived fuzzy matches", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_archived_tasks()
            && page.task_panel().search_query().as_deref() == Some("bP")
            && page.task_panel().task_names()
                == ["Brief planning".to_owned(), "Build proposal".to_owned()]
    });
    assert_eq!(
        page.task_panel().task_names(),
        ["Brief planning".to_owned(), "Build proposal".to_owned()]
    );
    tt.press_and_wait(Key::Enter, "the committed archived search", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().search_query().as_deref() == Some("bP")
            && page.footer().hints_open_history()
    });
    tt.quit().assert_clean_exit();
}

#[test]
fn tracking_and_renaming_a_filtered_task_refreshes_results_and_selection() {
    let context = TestContext::new();
    let book_id = {
        let database = context.database();
        database.create_task_at("Build docs", at(100), at(800));
        let book = database.create_task_at("Book design", at(100), at(700));
        database.create_task_at("Other work", at(100), at(900));
        book.id
    };

    let mut tt = context.launch();
    tt.wait_for_first_frame("the task list", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .task_names()
            .len()
            == 3
    });
    tt.press_and_wait(Key::Char('/'), "the task search", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .search_query()
            .is_some()
    });
    tt.type_text("b");
    tt.wait_for("the matching tasks", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .task_names()
            == ["Build docs".to_owned(), "Book design".to_owned()]
    });
    tt.press_and_wait(Key::Enter, "the committed filter", |screen| {
        TimeTrackerPage::new(screen.clone())
            .footer()
            .hints_open_history()
    });
    tt.press_and_wait(Key::Char('j'), "the second result", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .selected_index()
            == Some(1)
    });

    tt.press_and_wait(Key::Char(' '), "the tracked result", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().task_names().first().map(String::as_str) == Some("Book design")
            && page.task_panel().selected_index() == Some(0)
            && page
                .header()
                .active_task()
                .is_some_and(|active| active.name() == "Book design")
    });
    tt.press_and_wait(Key::Char('e'), "the rename dialog", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_input_dialog()
            .is_some_and(|dialog| dialog.text() == "Book design")
    });
    for _ in 0.."Book design".chars().count() {
        tt.press(Key::Backspace);
    }
    tt.type_text("Travel design");
    tt.wait_for("the replacement name", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_input_dialog()
            .is_some_and(|dialog| dialog.text() == "Travel design")
    });
    tt.press_and_wait(Key::Enter, "the remaining result", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().task_names() == ["Build docs".to_owned()]
            && page.task_panel().selected_index() == Some(0)
            && page
                .header()
                .active_task()
                .is_some_and(|active| active.name() == "Travel design")
    });
    tt.quit().assert_clean_exit();

    let database = context.database();
    assert_eq!(database.task_by_name("Travel design").unwrap().id, book_id);
    assert_eq!(database.worklogs_for_task("Travel design").len(), 1);
}
