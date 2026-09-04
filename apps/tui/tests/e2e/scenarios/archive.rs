//! Archive and restore scenarios.

use termlens::Key;

use crate::context::TestContext;
use crate::page::TimeTrackerPage;

#[test]
fn tt_archives_restores_and_persists_the_round_trip() {
    let context = TestContext::new();
    let mut tt = context.launch();
    tt.wait_for_first_frame("the first frame", |screen| {
        screen.contains("Write release notes") && screen.contains("Ready")
    });

    // Archive the selected task through its confirmation dialog.
    let page = tt.press_and_wait(Key::Char('d'), "the archive confirmation", |screen| {
        TimeTrackerPage::new(screen.clone())
            .archive_dialog()
            .is_some_and(|dialog| dialog.question() == "Archive \"Write release notes\"?")
    });
    let dialog = page.archive_dialog().expect("the archive dialog is open");
    assert_eq!(dialog.question(), "Archive \"Write release notes\"?");
    let page = tt.press_and_wait(Key::Char('y'), "the archived status", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.status_bar().text() == "Archived \"Write release notes\""
            && page.task_panel().task_names()
                == [
                    "Fix the coffee machine".to_owned(),
                    "Plan Friday's demo".to_owned(),
                ]
    });
    assert_eq!(page.status_bar().text(), "Archived \"Write release notes\"");
    assert!(
        page.task_panel().task_names().len() == 2,
        "the archived task left the active list:\n{}",
        page.screen()
    );

    // Switch to the archived view and restore the task.
    let page = tt.press_and_wait(Key::Char('l'), "the archived view", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_archived_tasks()
            && page.task_panel().task_names() == ["Write release notes".to_owned()]
            && page.footer().hints_open_history()
            && page.footer().hints_unarchive()
    });
    assert!(
        page.footer().hints_open_history(),
        "the archived footer names the worklog-history shortcut:\n{}",
        page.screen()
    );
    assert!(
        page.footer().hints_unarchive(),
        "the archived footer names the unarchive key:\n{}",
        page.screen()
    );
    let page = tt.press_and_wait(Key::Char('u'), "the restored status", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.status_bar().text() == "Restored \"Write release notes\""
            && page.task_panel().empty_hint().as_deref() == Some("No archived tasks.")
    });
    assert_eq!(
        page.task_panel().empty_hint().as_deref(),
        Some("No archived tasks."),
        "the archived list emptied:\n{}",
        page.screen()
    );
    assert_eq!(page.status_bar().text(), "Restored \"Write release notes\"");

    tt.quit().assert_clean_exit();

    // The archive and the unarchive both went through the real SQLite
    // adapter and ended where they started.
    let database = context.database();
    let tasks = database.tasks();
    assert_eq!(tasks.len(), 3);
    assert!(tasks.iter().all(|task| !task.archived));
    assert_eq!(database.active_worklog_task_name(), None);
}

#[test]
fn tt_persists_an_archive_across_a_quit_and_a_relaunch() {
    let context = TestContext::new();
    let mut tt = context.launch();
    tt.wait_for_first_frame("the first frame", |screen| {
        screen.contains("Write release notes") && screen.contains("Ready")
    });

    tt.press_and_wait(Key::Char('d'), "the archive confirmation", |screen| {
        TimeTrackerPage::new(screen.clone())
            .archive_dialog()
            .is_some_and(|dialog| dialog.question() == "Archive \"Write release notes\"?")
    });
    tt.press_and_wait(Key::Char('y'), "the archived status", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.status_bar().text() == "Archived \"Write release notes\""
            && page.task_panel().task_names()
                == [
                    "Fix the coffee machine".to_owned(),
                    "Plan Friday's demo".to_owned(),
                ]
    });
    tt.exit_with(Key::Char('q')).assert_clean_exit();

    let database = context.database();
    let mut tasks = database.tasks();
    tasks.sort_by(|left, right| left.name.cmp(&right.name));
    assert_eq!(tasks.len(), 3);
    assert_eq!(tasks[2].name, "Write release notes");
    assert!(tasks[2].archived, "the archived task stayed archived");
    assert!(!tasks[0].archived && !tasks[1].archived);

    // A relaunch keeps the archived task out of the active list.
    let mut relaunched = context.launch();
    let page = relaunched.wait_for_first_frame("the first frame", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().task_names()
            == [
                "Fix the coffee machine".to_owned(),
                "Plan Friday's demo".to_owned(),
            ]
    });
    assert_eq!(
        page.task_panel().task_names(),
        [
            "Fix the coffee machine".to_owned(),
            "Plan Friday's demo".to_owned()
        ],
        "the archived task stayed archived across the restart:\n{}",
        page.screen()
    );
    relaunched.quit().assert_clean_exit();
}

#[test]
fn tt_ignores_active_view_keys_in_the_archived_view() {
    let context = TestContext::new();
    let mut tt = context.launch();
    tt.wait_for_first_frame("the first frame", |screen| {
        screen.contains("Write release notes") && screen.contains("Ready")
    });

    let page = tt.press_and_wait(Key::Char('l'), "the archived view", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_archived_tasks()
            && page.task_panel().empty_hint().as_deref() == Some("No archived tasks.")
            && page.footer().hints_unarchive()
    });
    assert!(
        page.footer().hints_unarchive(),
        "the archived footer names the unarchive key:\n{}",
        page.screen()
    );

    // Every active-view action key is dead here, but the keys are still
    // processed one per tick. An allowed key proves the queue drained:
    // if any prohibited key had opened a modal, this view switch would
    // land in that modal instead, the archived view would never change,
    // and the wait below would fail.
    tt.type_text("a ed \"");
    let page = tt.press_and_wait(Key::Char('h'), "the active view again", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_active_tasks() && page.footer().hints_task_actions()
    });

    // The queued keys left no trace on the live page: no dialog open, no
    // timer running.
    assert!(
        page.task_input_dialog().is_none(),
        "the archived view must not open the task input:\n{}",
        page.screen()
    );
    assert!(
        page.archive_dialog().is_none(),
        "the archived view must not open the archive dialog:\n{}",
        page.screen()
    );
    assert!(
        page.header().is_idle(),
        "the archived view must not start the timer:\n{}",
        page.screen()
    );

    tt.quit().assert_clean_exit();

    let database = context.database();
    let tasks = database.tasks();
    assert_eq!(tasks.len(), 3);
    assert!(tasks.iter().all(|task| !task.archived));
    assert_eq!(database.active_worklog_task_name(), None);
}

#[test]
fn archiving_the_active_task_is_rejected_and_keeps_tracking() {
    let context = TestContext::new();
    let mut tt = context.launch();
    tt.wait_for_first_frame("the first frame", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().is_idle() && page.task_panel().task_names().len() == 3
    });
    let database = context.database();
    let original = database
        .task_by_name("Write release notes")
        .expect("the tracked task is stored");

    tt.press_and_wait(Key::Char(' '), "the running header", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header()
            .active_task()
            .is_some_and(|active| active.name() == "Write release notes")
    });
    let before = database
        .active_worklog()
        .expect("the active worklog is stored");
    assert_eq!(before.task_name, "Write release notes");

    let page = tt.press_and_wait(Key::Char('d'), "the archive confirmation", |screen| {
        TimeTrackerPage::new(screen.clone())
            .archive_dialog()
            .is_some_and(|dialog| dialog.question() == "Archive \"Write release notes\"?")
    });
    assert_eq!(
        page.archive_dialog()
            .expect("the archive dialog is open")
            .question(),
        "Archive \"Write release notes\"?"
    );

    let page = tt.press_and_wait(Key::Char('y'), "the rejection", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.status_bar().text() == "Error: The active task cannot be archived"
            && page.status_bar().is_error()
            && page.status_bar().label_is_accented()
            && page.archive_dialog().is_none()
            && page
                .header()
                .active_task()
                .is_some_and(|active| active.name() == "Write release notes")
            && page.task_panel().active_marker_index() == Some(0)
    });
    assert_eq!(
        page.status_bar().text(),
        "Error: The active task cannot be archived"
    );
    assert!(
        page.status_bar().label_is_accented(),
        "the error label keeps its red bold accent:\n{}",
        page.screen()
    );
    assert!(
        page.task_panel()
            .task_names()
            .contains(&"Write release notes".to_owned()),
        "the active task stayed in the active list:\n{}",
        page.screen()
    );

    tt.quit().assert_clean_exit();

    let stored = database
        .task_by_name("Write release notes")
        .expect("the task is stored");
    assert_eq!(
        stored.id, original.id,
        "the rejected archive kept the task's identity"
    );
    assert!(!stored.archived, "the active task was not archived");
    let after = database
        .active_worklog()
        .expect("the active worklog survived the rejected archive");
    assert_eq!(after.id, before.id, "the same worklog stayed active");
    assert_eq!(after.start, before.start, "the worklog was not rewritten");
    assert_eq!(
        after.task_name, before.task_name,
        "the worklog still tracks the same task"
    );
    assert!(after.end.is_none(), "the worklog is still active");
}
