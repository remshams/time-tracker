//! Bulk archive workflows through the real terminal and storage adapters.

use chrono::{TimeDelta, Utc};
use termlens::Key;

use crate::context::TestContext;
use crate::controlled_proxy::Route;
use crate::page::TimeTrackerPage;
use crate::remote::RemoteTestContext;

#[test]
fn bulk_archive_reports_when_no_task_qualifies_and_is_inactive_on_other_tabs() {
    let context = TestContext::new();
    let now = Utc::now();
    context.database().create_task_at(
        "New proposal",
        now - TimeDelta::days(2),
        now - TimeDelta::days(2),
    );
    let mut tt = context.launch();
    tt.wait_for_first_frame("the new task", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .task_names()
            == ["New proposal".to_owned()]
    });
    let narrow = tt.resize_and_wait(60, 24, "the compact active footer", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.size() == (60, 24) && page.footer().text().contains("a/e/d/D")
    });
    assert!(narrow.footer().text().contains("a/e/d/D"));
    tt.press_and_wait(Key::Char('D'), "the empty bulk archive result", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.status_bar().text() == "No inactive tasks to archive"
            && !page.visible_text().contains("Archive inactive tasks")
    });
    tt.press_and_wait(Key::Tab, "the archived tab", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .shows_archived_tasks()
    });
    tt.press(Key::Char('D'));
    tt.press_and_wait(Key::BackTab, "the active tab again", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_active_tasks()
            && !page.visible_text().contains("Archive inactive tasks")
    });
    tt.quit().assert_clean_exit();
    assert!(
        !context
            .database()
            .task_by_name("New proposal")
            .unwrap()
            .archived
    );
}

#[test]
fn bulk_archive_uses_all_active_tasks_and_protects_recent_or_running_work() {
    let context = TestContext::new();
    let now = Utc::now();
    let old = now - TimeDelta::days(30);
    {
        let database = context.database();
        database.create_task_at("Dormant planning", old, now);
        database.create_task_at("Dormant review", old, old);
        database.create_task_at("Recently created", now - TimeDelta::days(2), now);
        database.create_task_at("Recently tracked", old, old);
        database.create_task_at("Crosses cutoff", old, old);
        database.create_task_at("Still tracking", old, old);
        database.create_worklog(
            "Recently tracked",
            now - TimeDelta::days(1),
            Some(now - TimeDelta::hours(23)),
        );
        database.create_worklog(
            "Crosses cutoff",
            now - TimeDelta::days(15),
            Some(now - TimeDelta::days(13)),
        );
        database.create_worklog("Still tracking", now - TimeDelta::days(20), None);
    }

    let mut tt = context.launch();
    tt.wait_for_first_frame("the active task list", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .task_names()
            .len()
            == 6
    });
    tt.press_and_wait(Key::Char('/'), "the search field", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .search_query()
            .as_deref()
            == Some("")
    });
    tt.type_text("Dormant planning");
    tt.press_and_wait(Key::Enter, "the filtered task list", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().task_names() == ["Dormant planning".to_owned()]
    });

    let preview = tt.press_and_wait(Key::Char('D'), "the bulk archive preview", |screen| {
        let text = TimeTrackerPage::new(screen.clone()).visible_text();
        text.contains("Archive inactive tasks") && text.contains("2 active tasks inactive")
    });
    assert!(
        preview.visible_text().contains("regardless of search"),
        "the dialog explains its scope:\n{}",
        preview.screen()
    );
    assert!(
        preview.visible_text().contains("Dormant planning")
            || preview.visible_text().contains("Dormant review"),
        "the preview shows an eligible task name:\n{}",
        preview.screen()
    );
    tt.press_and_wait(Key::Esc, "the cancelled archive", |screen| {
        !TimeTrackerPage::new(screen.clone())
            .visible_text()
            .contains("Archive inactive tasks")
    });
    assert!(
        context.database().tasks().iter().all(|task| !task.archived),
        "cancel must not change any task"
    );

    tt.press_and_wait(
        Key::Char('D'),
        "the reopened bulk archive preview",
        |screen| {
            TimeTrackerPage::new(screen.clone())
                .visible_text()
                .contains("2 active tasks inactive")
        },
    );
    tt.press_and_wait(Key::Enter, "the archived tasks", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.status_bar().text().contains("2")
            && !page.visible_text().contains("Archive inactive tasks")
    });
    tt.quit().assert_clean_exit();

    let database = context.database();
    for name in ["Dormant planning", "Dormant review"] {
        assert!(database.task_by_name(name).unwrap().archived, "{name}");
    }
    for name in [
        "Recently created",
        "Recently tracked",
        "Crosses cutoff",
        "Still tracking",
    ] {
        assert!(!database.task_by_name(name).unwrap().archived, "{name}");
    }
    assert_eq!(
        database.active_worklog_task_name().as_deref(),
        Some("Still tracking")
    );
}

#[test]
fn remote_bulk_archive_uses_server_storage() {
    let mut context = RemoteTestContext::new_without_server();
    let now = Utc::now();
    {
        let database = context.server_database();
        database.create_task_at("Old remote task", now - TimeDelta::days(30), now);
        database.create_task_at("New remote task", now - TimeDelta::days(1), now);
    }
    context.start();
    let mut tt = context.launch();
    tt.wait_for_first_frame("the server tasks", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .task_names()
            .len()
            == 2
    });
    tt.press_and_wait(Key::Char('D'), "the remote archive preview", |screen| {
        TimeTrackerPage::new(screen.clone())
            .visible_text()
            .contains("1 active task inactive")
    });
    tt.press_and_wait(Key::Char('y'), "the remote bulk archive", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.status_bar().text().contains("1")
            && page.task_panel().task_names() == ["New remote task".to_owned()]
    });
    tt.quit().assert_clean_exit();

    let database = context.server_database();
    assert!(database.task_by_name("Old remote task").unwrap().archived);
    assert!(!database.task_by_name("New remote task").unwrap().archived);
    assert!(!context.local_database_path().exists());
}

#[test]
fn cancelling_a_pending_remote_preview_does_not_reopen_it() {
    let mut context = RemoteTestContext::new_without_server();
    let now = Utc::now();
    context.server_database().create_task_at(
        "Old remote draft",
        now - TimeDelta::days(30),
        now - TimeDelta::days(30),
    );
    context.start();
    let proxy = context.proxy();
    let mut tt = context.launch_through(&proxy);
    tt.wait_for_first_frame("the old task", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .task_names()
            == ["Old remote draft".to_owned()]
    });
    proxy.hold(Route::InactivePreview);
    tt.press(Key::Char('D'));
    proxy.wait_for_request();
    tt.wait_for("the pending preview", |screen| {
        TimeTrackerPage::new(screen.clone())
            .visible_text()
            .contains("Loading preview...")
    });
    tt.press_and_wait(Key::Esc, "the cancelled preview", |screen| {
        !TimeTrackerPage::new(screen.clone())
            .visible_text()
            .contains("Archive inactive tasks")
    });
    proxy.release();
    proxy.wait_for_delivery();
    tt.press_and_wait(
        Key::Tab,
        "the archived tab after the late response",
        |screen| {
            TimeTrackerPage::new(screen.clone())
                .task_panel()
                .shows_archived_tasks()
        },
    );
    tt.press_and_wait(
        Key::BackTab,
        "the task list after the late response",
        |screen| {
            let page = TimeTrackerPage::new(screen.clone());
            page.task_panel().shows_active_tasks()
                && !page.visible_text().contains("Archive inactive tasks")
        },
    );
    tt.quit().assert_clean_exit();
    assert!(
        !context
            .server_database()
            .task_by_name("Old remote draft")
            .unwrap()
            .archived
    );
}

#[test]
fn remote_bulk_archive_cannot_be_cancelled_after_confirmation() {
    let mut context = RemoteTestContext::new_without_server();
    let now = Utc::now();
    context.server_database().create_task_at(
        "Old remote note",
        now - TimeDelta::days(30),
        now - TimeDelta::days(30),
    );
    context.start();
    let proxy = context.proxy();
    let mut tt = context.launch_through(&proxy);
    tt.wait_for_first_frame("the old server task", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .task_names()
            == ["Old remote note".to_owned()]
    });
    tt.press_and_wait(Key::Char('D'), "the archive preview", |screen| {
        TimeTrackerPage::new(screen.clone())
            .visible_text()
            .contains("1 active task inactive")
    });
    proxy.hold(Route::ArchiveInactive);
    tt.press(Key::Enter);
    proxy.wait_for_request();
    tt.wait_for("the archive in progress", |screen| {
        TimeTrackerPage::new(screen.clone())
            .visible_text()
            .contains("Archiving inactive tasks")
    });
    tt.press(Key::Esc);
    tt.press(Key::Enter);
    tt.resize_and_wait(
        100,
        35,
        "the archive still in progress after keys",
        |screen| {
            let page = TimeTrackerPage::new(screen.clone());
            page.size() == (100, 35) && page.visible_text().contains("Archiving inactive tasks")
        },
    );
    assert!(
        tt.page()
            .visible_text()
            .contains("Archiving inactive tasks"),
        "the accepted write cannot be canceled"
    );
    proxy.release();
    proxy.wait_for_delivery();
    tt.wait_for("the completed archive", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.status_bar().text() == "Archived 1 inactive task"
            && !page.visible_text().contains("Archiving inactive tasks")
    });
    assert_eq!(proxy.request_count(), 1, "confirmation sends one write");
    tt.quit().assert_clean_exit();
    assert!(
        context
            .server_database()
            .task_by_name("Old remote note")
            .unwrap()
            .archived
    );
}

#[test]
fn remote_bulk_archive_rejects_a_stale_preview() {
    let mut context = RemoteTestContext::new_without_server();
    let now = Utc::now();
    context.server_database().create_task_at(
        "Old shared task",
        now - TimeDelta::days(30),
        now - TimeDelta::days(30),
    );
    context.start();
    let mut stale = context.launch();
    stale.wait_for_first_frame("the old server task", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .task_names()
            == ["Old shared task".to_owned()]
    });
    stale.press_and_wait(Key::Char('D'), "the stale client's preview", |screen| {
        TimeTrackerPage::new(screen.clone())
            .visible_text()
            .contains("1 active task inactive")
    });

    let mut writer = context.launch_second_client();
    writer.wait_for_first_frame("the writer's task list", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .task_names()
            .len()
            == 1
    });
    writer.press_and_wait(Key::Char('a'), "the new task dialog", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_input_dialog()
            .is_some()
    });
    writer.type_text("Another client wrote");
    writer.press_and_wait(Key::Enter, "the other client's write", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .task_names()
            .contains(&"Another client wrote".to_owned())
    });

    let rejected = stale.press_and_wait(Key::Enter, "the stale archive rejection", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.status_bar().is_error() && !page.visible_text().contains("Archive inactive tasks")
    });
    assert!(
        rejected
            .status_bar()
            .text()
            .contains("press D to preview again"),
        "the status asks for a fresh preview:\n{}",
        rejected.screen()
    );
    assert!(
        !context
            .server_database()
            .task_by_name("Old shared task")
            .unwrap()
            .archived
    );

    stale.press_and_wait(Key::Char('D'), "the fresh preview", |screen| {
        TimeTrackerPage::new(screen.clone())
            .visible_text()
            .contains("1 active task inactive")
    });
    stale.press_and_wait(Key::Char('y'), "the successful retry", |screen| {
        TimeTrackerPage::new(screen.clone())
            .status_bar()
            .text()
            .contains("1")
    });
    writer.quit().assert_clean_exit();
    stale.quit().assert_clean_exit();
    assert!(
        context
            .server_database()
            .task_by_name("Old shared task")
            .unwrap()
            .archived
    );
}
