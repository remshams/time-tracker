//! Empty startup, existing data, and shutdown scenarios.

use std::io::Read;
use std::process::Stdio;
use std::time::{Duration, Instant};

use termlens::Key;

use crate::context::TestContext;
use crate::database::StoredTask;
use crate::page::TimeTrackerPage;

/// How long the failing `tt` may take to exit before the test kills and
/// reaps it. The failure happens before any terminal setup, so a healthy
/// binary is gone in milliseconds.
const EXIT_LIMIT: Duration = Duration::from_secs(5);

#[test]
fn tt_creates_an_empty_database_and_keeps_it_empty_after_restart() {
    let context = TestContext::new();
    assert!(!context.local_database_path().exists());
    assert!(!context.local_database_path().parent().unwrap().exists());
    let mut tt = context.launch();

    // The predicate covers every region the assertions below rely on:
    // header, task rows, status line, and footer.
    let page = tt.wait_for_first_frame("the first frame", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().title_is_accented()
            && page.header().is_idle()
            && page.task_panel().task_names().is_empty()
            && page.task_panel().empty_hint().is_some()
            && page.task_panel().selected_index().is_none()
            && page.status_bar().text().is_empty()
            && page.footer().hints_open_history()
            && page.footer().hints_quit()
    });

    let header = page.header();
    assert!(
        header.title_is_accented(),
        "the title keeps its blue bold accent:\n{}",
        page.screen()
    );
    assert!(
        header.is_idle(),
        "the fresh app tracks nothing:\n{}",
        page.screen()
    );

    let panel = page.task_panel();
    assert!(panel.task_names().is_empty(), "{}", page.screen());
    assert!(panel.empty_hint().is_some(), "{}", page.screen());
    assert_eq!(panel.selected_index(), None, "{}", page.screen());
    assert!(
        panel.is_focused(),
        "normal mode focuses the panel border:\n{}",
        page.screen()
    );
    assert_eq!(
        page.status_bar().text(),
        "",
        "the fresh app leaves the status line empty:\n{}",
        page.screen()
    );
    // Every active-view action the footer advertises, asserted per action
    // so a dropped hint names itself.
    let footer = page.footer();
    assert!(
        footer.hints_movement(),
        "the footer names the movement keys:\n{}",
        page.screen()
    );
    assert!(
        footer.hints_view_switching(),
        "the footer names the view-switch keys:\n{}",
        page.screen()
    );
    assert!(
        footer.hints_tracking(),
        "the footer names the tracking toggle:\n{}",
        page.screen()
    );
    assert!(
        footer.hints_task_actions(),
        "the footer names add, rename, and archive:\n{}",
        page.screen()
    );
    assert!(
        footer.hints_open_history(),
        "the footer names the worklog-history shortcut:\n{}",
        page.screen()
    );
    assert!(
        footer.hints_quit(),
        "the footer names the quit keys:\n{}",
        page.screen()
    );
    assert!(
        page.screen().alternate_screen(),
        "setup entered the alternate screen:\n{}",
        page.screen()
    );

    let database = context.database();
    assert!(database.tasks().is_empty());
    assert!(database.active_worklogs().is_empty());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = context.local_database_path();
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }

    tt.exit_with(Key::Char('q')).assert_clean_exit();
    assert!(database.tasks().is_empty());
    assert!(database.active_worklogs().is_empty());

    let mut restarted = context.launch();
    restarted.wait_for_first_frame("the empty list after restart", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().is_idle()
            && page.task_panel().empty_hint().is_some()
            && page.task_panel().task_names().is_empty()
            && page.task_panel().selected_index().is_none()
    });
    restarted.quit().assert_clean_exit();
    assert!(database.tasks().is_empty());
    assert!(database.active_worklogs().is_empty());
}

#[test]
fn tt_reports_a_startup_failure_and_exits_nonzero() {
    let temp = tempfile::tempdir().unwrap();
    // HOME points at a plain file, so the platform data directory below it
    // cannot be created and startup fails before any terminal setup runs.
    // A pty is not needed: nothing ever reaches the terminal.
    let blocker = temp.path().join("home");
    std::fs::write(&blocker, b"not a directory").unwrap();

    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_tt"));
    command
        .env_clear()
        .env("HOME", &blocker)
        .env("TERM", "xterm-256color")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Ok(profile) = std::env::var("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile);
    }
    let mut child = command.spawn().expect("tt must start");

    // Bounded wait: a hung child is killed and reaped instead of hanging
    // the suite.
    let deadline = Instant::now() + EXIT_LIMIT;
    let status = loop {
        match child.try_wait().expect("tt must be pollable") {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("tt did not exit within the startup allowance");
            }
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    child
        .stdout
        .take()
        .expect("stdout is piped")
        .read_to_end(&mut stdout)
        .expect("stdout must be readable");
    child
        .stderr
        .take()
        .expect("stderr is piped")
        .read_to_end(&mut stderr)
        .expect("stderr must be readable");

    assert_eq!(status.code(), Some(1), "status: {status}");
    let stderr = String::from_utf8_lossy(&stderr);
    assert_eq!(
        stderr.lines().count(),
        1,
        "startup failures print one concise line:\n{stderr}"
    );
    assert!(
        stderr.starts_with("tt: "),
        "startup failures print one concise line:\n{stderr}"
    );
    // No escape sequence anywhere: no terminal setup ran.
    let stdout = String::from_utf8_lossy(&stdout);
    assert!(
        !stdout.contains('\x1b') && !stderr.contains('\x1b'),
        "setup must not touch the terminal on a startup failure:\n{stdout:?}\n{stderr:?}"
    );
}

#[test]
fn an_existing_custom_database_is_shown_without_changes() {
    let context = TestContext::new();
    // Seed through the real adapter before tt starts, and keep the exact
    // stored records the launch must leave untouched.
    let before_launch: Vec<StoredTask> = {
        let database = context.database();
        database.create_task("Ship the beta");
        database.create_task("Water the plants");
        database.tasks()
    };
    let mut tt = context.launch();

    let page = tt.wait_for_first_frame("the first frame", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().is_idle()
            && page.task_panel().task_names()
                == ["Ship the beta".to_owned(), "Water the plants".to_owned()]
            && page.status_bar().text().is_empty()
            && page.footer().hints_quit()
    });
    // The rendered view is compared against fixed names, not against
    // whatever the fixture happened to store.
    assert_eq!(
        page.task_panel().task_names(),
        ["Ship the beta".to_owned(), "Water the plants".to_owned()],
        "the existing tasks render in storage order:\n{}",
        page.screen()
    );
    assert!(
        page.header().is_idle(),
        "an existing database starts idle:\n{}",
        page.screen()
    );

    tt.quit().assert_clean_exit();

    // Startup keeps each stored record unchanged.
    let database = context.database();
    assert_eq!(
        database.tasks(),
        before_launch,
        "launching an existing database changed no stored record"
    );
    assert_eq!(
        database.active_worklog_task_name(),
        None,
        "quitting must not start a timer"
    );
}

#[test]
fn a_mixed_database_shows_active_and_archived_tasks_in_their_own_views() {
    let context = TestContext::new();
    // Seed through the real adapter before tt starts, and keep the exact
    // stored records the launch must leave untouched.
    let before_launch: Vec<StoredTask> = {
        let database = context.database();
        database.create_task("Plan the sprint");
        database.create_task("Review the budget");
        database.create_task("Retire the old importer");
        let target = database
            .task_by_name("Retire the old importer")
            .expect("the task to archive is stored");
        let archived = database.archive_task("Retire the old importer");
        assert_eq!(archived.id, target.id, "archiving kept the stored identity");
        assert!(archived.archived, "the fixture archived the task");
        database.tasks()
    };
    let mut tt = context.launch();

    let page = tt.wait_for_first_frame("the first frame", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().is_idle()
            && page.task_panel().shows_active_tasks()
            && page.task_panel().task_names()
                == ["Plan the sprint".to_owned(), "Review the budget".to_owned()]
            && page.task_panel().selected_index() == Some(0)
    });
    assert_eq!(
        page.task_panel().task_names(),
        ["Plan the sprint".to_owned(), "Review the budget".to_owned()],
        "the active view shows exactly the active tasks:\n{}",
        page.screen()
    );
    assert_eq!(
        page.task_panel().selected_index(),
        Some(0),
        "the active view selects its first row:\n{}",
        page.screen()
    );

    // Move to the second active row first, so the view switch happens
    // from a selection past the archived view's single row.
    let page = tt.press_and_wait(Key::Char('j'), "the second active row", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().selected_index() == Some(1)
    });
    assert_eq!(page.task_panel().row(1).name(), "Review the budget");

    let page = tt.press_and_wait(Key::Tab, "the archived view", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_archived_tasks()
            && page.task_panel().task_names() == ["Retire the old importer".to_owned()]
            && page.task_panel().selected_index() == Some(0)
            && page.footer().hints_unarchive()
    });
    assert_eq!(
        page.task_panel().task_names(),
        ["Retire the old importer".to_owned()],
        "the archived view shows exactly the archived tasks:\n{}",
        page.screen()
    );
    assert_eq!(
        page.task_panel().selected_index(),
        Some(0),
        "the one-row archived view selects safely:\n{}",
        page.screen()
    );
    assert!(
        page.footer().hints_unarchive(),
        "the archived footer names the unarchive key:\n{}",
        page.screen()
    );

    tt.quit().assert_clean_exit();

    // Every stored record is exactly what setup put there, so no default
    // task was inserted and the archive flag survived the launch.
    let database = context.database();
    assert_eq!(
        database.tasks(),
        before_launch,
        "launching a mixed database changed no stored record"
    );
    assert_eq!(
        database.active_worklog_task_name(),
        None,
        "launching a mixed database starts nothing"
    );
}

#[test]
fn tasks_from_earlier_sample_data_remain_unchanged() {
    let context = TestContext::new_with_tasks();
    let before = context.database().tasks();
    let mut tt = context.launch();
    tt.wait_for_first_frame("the existing sample tasks", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .task_names()
            == [
                "Write release notes",
                "Fix the coffee machine",
                "Plan Friday's demo",
            ]
    });
    tt.quit().assert_clean_exit();
    assert_eq!(context.database().tasks(), before);
}
