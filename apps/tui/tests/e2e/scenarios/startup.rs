//! Startup, seeding, and shutdown scenarios.

use std::io::Read;
use std::process::Stdio;
use std::time::{Duration, Instant};

use crate::context::TestContext;
use crate::page::TimeTrackerPage;

/// How long the failing `tt` may take to exit before the test kills and
/// reaps it. The failure happens before any terminal setup, so a healthy
/// binary is gone in milliseconds.
const EXIT_LIMIT: Duration = Duration::from_secs(5);

#[test]
fn tt_seeds_the_database_renders_the_task_list_and_quits_on_q() {
    let context = TestContext::new();
    let mut tt = context.launch();

    // The predicate covers every region the assertions below rely on:
    // header, task rows, status line, and footer.
    let page = tt.wait_for_first_frame("the first frame", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().title_is_accented()
            && page.header().is_idle()
            && page.task_panel().task_names()
                == [
                    "Write release notes".to_owned(),
                    "Fix the coffee machine".to_owned(),
                    "Plan Friday's demo".to_owned(),
                ]
            && page.status_bar().text() == "Ready"
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
    assert_eq!(
        panel.task_names(),
        [
            "Write release notes".to_owned(),
            "Fix the coffee machine".to_owned(),
            "Plan Friday's demo".to_owned()
        ],
        "the seeded task list renders:\n{}",
        page.screen()
    );
    assert!(
        panel.is_focused(),
        "normal mode focuses the panel border:\n{}",
        page.screen()
    );
    assert_eq!(
        page.status_bar().text(),
        "Ready",
        "the fresh app reports readiness:\n{}",
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
        footer.hints_quit(),
        "the footer names the quit keys:\n{}",
        page.screen()
    );
    assert!(
        page.screen().alternate_screen(),
        "setup entered the alternate screen:\n{}",
        page.screen()
    );

    let outcome = tt.exit_with(termlens::Key::Char('q'));
    outcome.assert_clean_exit();

    // Seeding went through the real SQLite adapter and quit touched
    // neither the task list nor the timer.
    let database = context.database();
    assert_eq!(
        database.task_names(),
        [
            "Write release notes".to_owned(),
            "Fix the coffee machine".to_owned(),
            "Plan Friday's demo".to_owned()
        ]
    );
    assert_eq!(
        database.active_worklog_task_name(),
        None,
        "quitting must not start or stop a timer"
    );
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
