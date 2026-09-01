//! End-to-end smoke tests that drive the real `tt` binary through a
//! pseudo-terminal.
//!
//! The tests run on Linux only: the platform data directory is overridden
//! with `XDG_DATA_HOME`, so they always operate on a temporary database and
//! never touch the user's real one.

#![cfg(target_os = "linux")]

use std::ffi::CString;
use std::path::Path;
use std::time::{Duration, Instant};

use tracker_core::TrackerRepository;
use tracker_storage::SqliteRepository;

/// How long a `tt` run may take before the test kills it.
const RUN_LIMIT: Duration = Duration::from_secs(15);

/// Runs `tt` in a child pty, feeds `input` to it, and returns the raw wait
/// status plus everything the program printed.
///
/// The child owns the pty as its controlling terminal, so crossterm's raw
/// mode and alternate screen behave exactly as in an interactive session.
fn run_in_pty(data_dir: &Path, input: &[u8]) -> (libc::c_int, String) {
    let binary = CString::new(env!("CARGO_BIN_EXE_tt")).expect("binary path has no NUL");
    let data_dir = data_dir.to_path_buf();

    unsafe {
        let mut master: libc::c_int = -1;
        let pid = libc::forkpty(
            &mut master,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        );
        assert_ne!(pid, -1, "forkpty failed");
        if pid == 0 {
            // Child: point the data directory at the temporary location and
            // exec tt. This branch never returns to the test harness.
            std::env::set_var("XDG_DATA_HOME", &data_dir);
            libc::execl(
                binary.as_ptr(),
                c"tt".as_ptr(),
                std::ptr::null::<libc::c_char>(),
            );
            libc::_exit(127);
        }

        // Give the terminal a sane size before the child queries it.
        let size = libc::winsize {
            ws_row: 24,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        libc::ioctl(master, libc::TIOCSWINSZ, &size);
        if !input.is_empty() {
            libc::write(master, input.as_ptr().cast(), input.len());
        }

        let output = read_until_eof(master, pid);
        libc::close(master);
        let mut status: libc::c_int = 0;
        libc::waitpid(pid, &mut status, 0);
        (status, String::from_utf8_lossy(&output).into_owned())
    }
}

/// Reads from the pty until it closes, with a hard deadline.
fn read_until_eof(master: libc::c_int, pid: libc::pid_t) -> Vec<u8> {
    let deadline = Instant::now() + RUN_LIMIT;
    let mut output = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        if Instant::now() >= deadline {
            unsafe {
                libc::kill(pid, libc::SIGKILL);
                libc::waitpid(pid, std::ptr::null_mut(), 0);
            }
            panic!("the tt process did not close its terminal in time");
        }
        let mut fds = [libc::pollfd {
            fd: master,
            events: libc::POLLIN,
            revents: 0,
        }];
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), 1, 200) };
        if ready <= 0 {
            continue;
        }
        let read = unsafe { libc::read(master, buffer.as_mut_ptr().cast(), buffer.len()) };
        if read <= 0 {
            break;
        }
        output.extend_from_slice(&buffer[..read as usize]);
    }
    output
}

/// The exit code of a wait status, or `None` when the process died abnormally.
fn exit_code(status: libc::c_int) -> Option<i32> {
    if libc::WIFEXITED(status) {
        Some(libc::WEXITSTATUS(status))
    } else {
        None
    }
}

#[test]
fn tt_runs_in_a_pty_seeds_the_database_and_quits_on_q() {
    let temp = tempfile::tempdir().unwrap();
    let data_dir = temp.path().join("data");

    let (status, output) = run_in_pty(&data_dir, b"q");

    assert_eq!(
        exit_code(status),
        Some(0),
        "tt exited abnormally; output:\n{output}"
    );
    // Ratatui's diff stream skips unchanged cells, so task names appear as
    // word fragments; every word of every seeded task must be on screen.
    for word in [
        "Write", "release", "notes", "Fix", "coffee", "machine", "Plan", "Friday's", "demo",
    ] {
        assert!(
            output.contains(word),
            "missing {word:?} in output:\n{output}"
        );
    }
    assert!(output.contains("Time Tracker"), "title missing:\n{output}");
    assert!(output.contains("a add"), "footer missing:\n{output}");
    // The guard restored raw mode and the alternate screen before exiting.
    assert!(
        output.contains("\x1b[?1049l"),
        "the alternate screen was not left:\n{output:?}"
    );

    let database = data_dir.join("Time Tracker").join("tt.db");
    let repository = SqliteRepository::open(&database).unwrap();
    let names: Vec<String> = repository
        .list_tasks()
        .unwrap()
        .into_iter()
        .map(|task| task.name.to_string())
        .collect();
    assert_eq!(
        names,
        [
            "Write release notes".to_owned(),
            "Fix the coffee machine".to_owned(),
            "Plan Friday's demo".to_owned()
        ]
    );
    assert_eq!(
        repository.active_entry().unwrap(),
        None,
        "quitting must not start or stop a timer"
    );
}

#[test]
fn tt_reports_a_startup_failure_and_exits_nonzero() {
    let temp = tempfile::tempdir().unwrap();
    // XDG_DATA_HOME points at a plain file, so creating the application data
    // directory must fail before any terminal setup happens.
    let blocker = temp.path().join("data");
    std::fs::write(&blocker, b"not a directory").unwrap();

    let (status, output) = run_in_pty(&blocker, b"");

    assert_eq!(exit_code(status), Some(1), "output:\n{output}");
    assert!(
        output.contains("tt: "),
        "startup failures print one concise line:\n{output}"
    );
}
