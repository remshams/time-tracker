//! End-to-end smoke tests that drive the real `tt` binary through a
//! pseudo-terminal.
//!
//! The tests run on Linux and macOS. The child gets a temporary `HOME`, so
//! `tt` always operates on a temporary database and never touches the user's
//! real data on either platform.

#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::ffi::{CString, c_char};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tracker_core::TrackerRepository;
use tracker_storage::SqliteRepository;

/// How long a `tt` run may take before the test kills it.
const RUN_LIMIT: Duration = Duration::from_secs(15);

/// The database path inside a temporary home directory.
///
/// On Linux the platform data directory is `$HOME/.local/share`; on macOS it
/// is `$HOME/Library/Application Support`.
fn database_in(home: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home.join("Library")
            .join("Application Support")
            .join("Time Tracker")
            .join("tt.db")
    }
    #[cfg(target_os = "linux")]
    {
        home.join(".local")
            .join("share")
            .join("Time Tracker")
            .join("tt.db")
    }
}

/// Runs `tt` in a child pty with the given temporary `HOME`, feeds `input` to
/// it, and returns the raw wait status, everything the program printed, and
/// whether the terminal was left in cooked mode (echo on) after the child
/// exited.
///
/// `argv` and `envp` are built before the fork. After the fork the child
/// performs only async-signal-safe calls: `execve`, and `_exit` if the exec
/// fails. It never touches the Rust runtime of the parent process.
/// The environment for the child `tt` process: a temporary `HOME` and a
/// minimal `PATH`, so tt operates on the temporary data directory and
/// nothing else. The coverage profile file, when the test runs under
/// cargo-llvm-cov, is passed through so the child's execution counts toward
/// the measured coverage.
fn child_env(home: &Path) -> Vec<CString> {
    let home_entry =
        CString::new(format!("HOME={}", home.display())).expect("temporary home has no NUL");
    let path_entry = CString::new("PATH=/usr/bin:/bin").unwrap();
    let mut entries = vec![home_entry, path_entry];
    if let Ok(profile) = std::env::var("LLVM_PROFILE_FILE")
        && let Ok(entry) = CString::new(format!("LLVM_PROFILE_FILE={profile}"))
    {
        entries.push(entry);
    }
    entries
}

fn run_in_pty(home: &Path, input: &[u8]) -> (libc::c_int, String, bool) {
    let binary = CString::new(env!("CARGO_BIN_EXE_tt")).expect("binary path has no NUL");
    let argv = [binary.as_ptr(), c"tt".as_ptr(), std::ptr::null::<c_char>()];
    let env_entries = child_env(home);
    let envp: Vec<*const c_char> = env_entries
        .iter()
        .map(|entry| entry.as_ptr())
        .chain(std::iter::once(std::ptr::null()))
        .collect();

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
            // Child: exec tt with the temporary HOME. This branch never
            // returns to the test harness.
            libc::execve(binary.as_ptr(), argv.as_ptr(), envp.as_ptr());
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
        // A zero-length write is harmless, so the input is written as is.
        libc::write(master, input.as_ptr().cast(), input.len());

        let output = read_until_eof(master, pid);
        let mut status: libc::c_int = 0;
        libc::waitpid(pid, &mut status, 0);
        // After tt restored the terminal and exited, the pty line discipline
        // is back in cooked mode: echo is on again. A skipped raw-mode
        // restore would leave the pty in raw mode here.
        let mut termios: libc::termios = std::mem::zeroed();
        let cooked =
            libc::tcgetattr(master, &mut termios) == 0 && termios.c_lflag & libc::ECHO != 0;
        libc::close(master);
        (
            status,
            String::from_utf8_lossy(&output).into_owned(),
            cooked,
        )
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
    let home = temp.path().join("home");

    let (status, output, cooked) = run_in_pty(&home, b"q");

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
    // Default-styled spaces may be emitted as cursor movements, so check the
    // footer's meaningful fragments rather than one contiguous byte string.
    for hint in [
        "j/k",
        "start/stop",
        "add",
        "rename",
        "archive",
        "q/esc/ctrl+c",
        "quit",
    ] {
        assert!(
            output.contains(hint),
            "footer fragment {hint:?} missing:\n{output}"
        );
    }
    // Setup entered the alternate screen, and the guard left it and showed
    // the cursor again before exiting.
    assert!(
        output.contains("\x1b[?1049h"),
        "the alternate screen was not entered:\n{output:?}"
    );
    assert!(
        output.contains("\x1b[?1049l"),
        "the alternate screen was not left:\n{output:?}"
    );
    assert!(
        output.contains("\x1b[?25h"),
        "the cursor was not shown again:\n{output:?}"
    );
    // Raw mode was restored: the terminal is left usable for the shell.
    assert!(cooked, "the pty must be left in cooked mode");

    let database = database_in(&home);
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
    // HOME points at a plain file, so the platform data directory below it
    // cannot be created and startup must fail before any terminal setup
    // happens.
    let blocker = temp.path().join("home");
    std::fs::write(&blocker, b"not a directory").unwrap();

    let (status, output, _cooked) = run_in_pty(&blocker, b"");

    assert_eq!(exit_code(status), Some(1), "output:\n{output}");
    assert!(
        output.contains("tt: "),
        "startup failures print one concise line:\n{output}"
    );
    // Nothing entered the alternate screen: no terminal setup ran.
    assert!(
        !output.contains("\x1b[?1049h"),
        "setup must not touch the terminal on a startup failure:\n{output:?}"
    );
}
