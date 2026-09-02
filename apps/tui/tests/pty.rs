//! End-to-end smoke tests that drive the real `tt` binary through a
//! pseudo-terminal.
//!
//! The tests run on Linux and macOS. The child gets a temporary `HOME`, so
//! `tt` always operates on a temporary database and never touches the user's
//! real data on either platform.

#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::ffi::{CString, c_char};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tracker_storage::SqliteRepository;

/// How long `tt` may take to start up and render its first frame before the
/// test kills it.
const RUN_LIMIT: Duration = Duration::from_secs(15);

/// How long `tt` may take to answer one key press after the first frame, and
/// to exit once quit was sent.
///
/// A healthy prebuilt binary responds in milliseconds, so this stays far
/// below the mutation-test timeout: a program that never receives input —
/// for example when raw mode was not enabled and the tty buffers the keys —
/// produces ordinary failed tests instead of a full run-limit wait in every
/// staged test.
const RESPONSE_LIMIT: Duration = Duration::from_secs(3);

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

struct PtyChild {
    master: Option<libc::c_int>,
    pid: Option<libc::pid_t>,
}

impl PtyChild {
    fn master(&self) -> libc::c_int {
        self.master.expect("pty master is still open")
    }

    fn wait_until(&mut self, deadline: Instant) -> io::Result<libc::c_int> {
        let pid = self.pid.expect("pty child has not been reaped");
        loop {
            let mut status = 0;
            let result = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
            if result == pid {
                self.pid = None;
                return Ok(status);
            }
            if result == -1 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            if result != 0 {
                return Err(io::Error::other(format!(
                    "waitpid returned unexpected pid {result}"
                )));
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the tt process did not exit in time",
                ));
            }
            std::thread::yield_now();
        }
    }

    fn close_master(&mut self) -> io::Result<()> {
        let master = self.master.take().expect("pty master is still open");
        let result = unsafe { libc::close(master) };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    fn cleanup(&mut self) -> io::Result<()> {
        let mut first_error = None;
        if let Some(pid) = self.pid.take() {
            if unsafe { libc::kill(pid, libc::SIGKILL) } == -1 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ESRCH) {
                    first_error = Some(error);
                }
            }

            loop {
                let result = unsafe { libc::waitpid(pid, std::ptr::null_mut(), 0) };
                if result == pid {
                    break;
                }
                if result == -1 {
                    let error = io::Error::last_os_error();
                    if error.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    if error.raw_os_error() != Some(libc::ECHILD) && first_error.is_none() {
                        first_error = Some(error);
                    }
                    break;
                }
                if first_error.is_none() {
                    first_error = Some(io::Error::other(format!(
                        "waitpid returned unexpected pid {result}"
                    )));
                }
                break;
            }
        }

        if let Some(master) = self.master.take()
            && unsafe { libc::close(master) } == -1
            && first_error.is_none()
        {
            first_error = Some(io::Error::last_os_error());
        }

        first_error.map_or(Ok(()), Err)
    }
}

impl Drop for PtyChild {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup()
            && !std::thread::panicking()
        {
            panic!("failed to clean up pty child: {error}");
        }
    }
}

fn spawn_pty(home: &Path) -> PtyChild {
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
        if pid == 0 {
            // The child only executes async-signal-safe calls after fork.
            libc::execve(binary.as_ptr(), argv.as_ptr(), envp.as_ptr());
            libc::_exit(127);
        }
        assert_ne!(pid, -1, "forkpty failed");
        PtyChild {
            master: Some(master),
            pid: Some(pid),
        }
    }
}

fn configure_pty(master: libc::c_int) {
    let size = libc::winsize {
        ws_row: 24,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let result = unsafe { libc::ioctl(master, libc::TIOCSWINSZ, &size) };
    assert_ne!(
        result,
        -1,
        "could not size the pty: {}",
        io::Error::last_os_error()
    );
}

fn write_all_pty(master: libc::c_int, input: &[u8]) {
    let mut written = 0;
    while written < input.len() {
        let result = unsafe {
            libc::write(
                master,
                input[written..].as_ptr().cast(),
                input.len() - written,
            )
        };
        if result > 0 {
            written += result as usize;
            continue;
        }
        if result == -1 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            panic!("could not write to the pty: {error}");
        }
        panic!("writing to the pty made no progress");
    }
}

fn finish_pty(
    mut pty: PtyChild,
    output: Vec<u8>,
    deadline: Instant,
) -> (libc::c_int, String, bool) {
    let status = pty
        .wait_until(deadline)
        .unwrap_or_else(|error| panic!("could not reap the tt process: {error}"));
    // A skipped raw-mode restore leaves at least one of these local flags
    // disabled. Check the full cooked-mode set before closing the master.
    let mut termios: libc::termios = unsafe { std::mem::zeroed() };
    let cooked_flags = libc::ECHO | libc::ICANON | libc::ISIG;
    let cooked = unsafe { libc::tcgetattr(pty.master(), &mut termios) } == 0
        && termios.c_lflag & cooked_flags == cooked_flags;
    pty.close_master()
        .unwrap_or_else(|error| panic!("could not close the pty master: {error}"));
    (
        status,
        String::from_utf8_lossy(&output).into_owned(),
        cooked,
    )
}

fn run_in_pty(home: &Path, input: &[u8]) -> (libc::c_int, String, bool) {
    let pty = spawn_pty(home);
    configure_pty(pty.master());
    write_all_pty(pty.master(), input);
    // Startup keeps the generous run limit; after the first output arrives
    // the program has rendered, so the quit response and the exit fall
    // under the short response allowance.
    let mut output = Vec::new();
    read_from_pty(
        pty.master(),
        Instant::now() + RUN_LIMIT,
        "the first output",
        &mut output,
    );
    let response_deadline = Instant::now() + RESPONSE_LIMIT;
    while read_from_pty(
        pty.master(),
        response_deadline,
        "end of output",
        &mut output,
    ) {}
    finish_pty(pty, output, response_deadline)
}

struct PtyStages {
    status: libc::c_int,
    initial: String,
    active: String,
    modal: String,
    error: String,
    exit: String,
    cooked: bool,
}

fn run_stages_in_pty(home: &Path) -> PtyStages {
    let pty = spawn_pty(home);
    configure_pty(pty.master());
    // Every wait after the first frame is a response to one key press and
    // gets a fresh short allowance.
    let respond = || Instant::now() + RESPONSE_LIMIT;

    let initial = read_until_expected(
        pty.master(),
        Instant::now() + RUN_LIMIT,
        "initial frame",
        |output| {
            contains_bytes(output, b"Time Tracker")
                && contains_bytes(output, b"quit")
                && contains_sgr_parameters(output, &[38, 5, 4])
                && contains_sgr_parameters(output, &[7])
        },
    );

    write_all_pty(pty.master(), b" ");
    let active = read_until_expected(pty.master(), respond(), "active marker", |output| {
        contains_sgr_parameters(output, &[38, 5, 2]) && contains_bytes(output, "▶".as_bytes())
    });

    write_all_pty(pty.master(), b"a");
    let modal = read_until_expected(pty.master(), respond(), "input modal", |output| {
        [b"New".as_slice(), b"task", b"name:"]
            .into_iter()
            .all(|fragment| contains_bytes(output, fragment))
    });

    write_all_pty(pty.master(), b"\r");
    let error = read_until_expected(pty.master(), respond(), "error label", |output| {
        contains_sgr_parameters(output, &[38, 5, 1]) && contains_bytes(output, b"Error:")
    });

    write_all_pty(pty.master(), b"\x03");
    let exit_deadline = respond();
    let exit = read_until_eof(pty.master(), exit_deadline);
    let (status, exit, cooked) = finish_pty(pty, exit, exit_deadline);
    PtyStages {
        status,
        initial: String::from_utf8_lossy(&initial).into_owned(),
        active: String::from_utf8_lossy(&active).into_owned(),
        modal: String::from_utf8_lossy(&modal).into_owned(),
        error: String::from_utf8_lossy(&error).into_owned(),
        exit,
        cooked,
    }
}

fn contains_bytes(output: &[u8], expected: &[u8]) -> bool {
    output
        .windows(expected.len())
        .any(|window| window == expected)
}

/// Matches complete numeric SGR parameters rather than decimal prefixes.
fn contains_sgr_parameters(output: &[u8], expected: &[u16]) -> bool {
    let mut offset = 0;
    while let Some(start) = output[offset..]
        .windows(2)
        .position(|bytes| bytes == b"\x1b[")
    {
        let parameters_start = offset + start + 2;
        let Some(final_offset) = output[parameters_start..]
            .iter()
            .position(|byte| (0x40..=0x7e).contains(byte))
        else {
            return false;
        };
        let final_index = parameters_start + final_offset;
        if output[final_index] == b'm' {
            let parameters = output[parameters_start..final_index]
                .split(|byte| *byte == b';')
                .map(|parameter| {
                    if parameter.is_empty() {
                        Some(0)
                    } else {
                        std::str::from_utf8(parameter).ok()?.parse::<u16>().ok()
                    }
                })
                .collect::<Option<Vec<_>>>();
            if parameters.is_some_and(|parameters| {
                parameters
                    .windows(expected.len())
                    .any(|window| window == expected)
            }) {
                return true;
            }
        }
        offset = final_index + 1;
    }
    false
}

fn read_until_expected(
    master: libc::c_int,
    deadline: Instant,
    expected: &str,
    predicate: impl Fn(&[u8]) -> bool,
) -> Vec<u8> {
    let mut output = Vec::new();
    while !predicate(&output) {
        assert!(
            read_from_pty(master, deadline, expected, &mut output),
            "the pty closed before tt rendered {expected}; output:\n{}",
            String::from_utf8_lossy(&output)
        );
    }
    output
}

/// Reads from the pty until it closes or the deadline passes.
fn read_until_eof(master: libc::c_int, deadline: Instant) -> Vec<u8> {
    let mut output = Vec::new();
    while read_from_pty(master, deadline, "end of output", &mut output) {}
    output
}

fn read_from_pty(
    master: libc::c_int,
    deadline: Instant,
    expected: &str,
    output: &mut Vec<u8>,
) -> bool {
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .unwrap_or_else(|| {
                panic!(
                    "timed out waiting for {expected}; output:\n{}",
                    String::from_utf8_lossy(output)
                )
            });
        let timeout = remaining.as_millis().clamp(1, libc::c_int::MAX as u128) as libc::c_int;
        let mut fd = libc::pollfd {
            fd: master,
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut fd, 1, timeout) };
        if ready == 0 {
            continue;
        }
        if ready == -1 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            panic!("could not poll the pty while waiting for {expected}: {error}");
        }

        let mut buffer = [0u8; 4096];
        let read = unsafe { libc::read(master, buffer.as_mut_ptr().cast(), buffer.len()) };
        if read > 0 {
            output.extend_from_slice(&buffer[..read as usize]);
            return true;
        }
        if read == 0 {
            return false;
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            continue;
        }
        // Linux reports EIO when the last slave descriptor closes. macOS may
        // report a zero-length read instead.
        if error.raw_os_error() == Some(libc::EIO) {
            return false;
        }
        panic!("could not read the pty while waiting for {expected}: {error}");
    }
}

/// The exit code of a wait status, or `None` when the process died abnormally.
fn exit_code(status: libc::c_int) -> Option<i32> {
    if libc::WIFEXITED(status) {
        Some(libc::WEXITSTATUS(status))
    } else {
        None
    }
}

/// Drives one `tt` run through staged key presses, waiting for a marker of
/// each expected frame before continuing.
struct StagedRun {
    pty: PtyChild,
    /// The startup allowance, consumed by the first wait.
    startup: Option<Instant>,
}

impl StagedRun {
    fn start(home: &Path) -> Self {
        let pty = spawn_pty(home);
        configure_pty(pty.master());
        Self {
            pty,
            startup: Some(Instant::now() + RUN_LIMIT),
        }
    }

    fn press(&mut self, input: &[u8], expected: &str, predicate: impl Fn(&[u8]) -> bool) -> String {
        write_all_pty(self.pty.master(), input);
        // The first wait covers startup and keeps the generous run limit;
        // every later wait is a response to one key press with a fresh
        // short allowance.
        let deadline = self
            .startup
            .take()
            .unwrap_or_else(|| Instant::now() + RESPONSE_LIMIT);
        String::from_utf8_lossy(&read_until_expected(
            self.pty.master(),
            deadline,
            expected,
            predicate,
        ))
        .into_owned()
    }

    fn quit(self) -> (libc::c_int, String, bool) {
        let deadline = Instant::now() + RESPONSE_LIMIT;
        write_all_pty(self.pty.master(), b"\x03");
        let output = read_until_eof(self.pty.master(), deadline);
        finish_pty(self.pty, output, deadline)
    }
}

#[test]
fn tt_browses_the_archived_view_restores_a_task_and_persists_the_round_trip() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let mut run = StagedRun::start(&home);

    run.press(b"", "initial frame", |output| {
        contains_bytes(output, b"Time Tracker") && contains_bytes(output, b"quit")
    });

    // Archive the selected task through its confirmation dialog. Ratatui
    // skips unchanged cells, so match whole words, never phrases with
    // spaces that the diff may elide.
    run.press(b"d", "archive confirmation", |output| {
        contains_bytes(output, b"Confirm archive")
    });
    run.press(b"y", "archived status", |output| {
        contains_bytes(output, b"Archived")
    });

    // Switch to the archived view and restore the task.
    let archived = run.press(b"l", "archived view", |output| {
        contains_bytes(output, b"u unarchive")
    });
    assert!(archived.contains("unarchive"), "got {archived:?}");
    // Unarchiving empties the archived list and reports the restoration.
    // The status suffix matches the archived text, so the diff skips those
    // cells; the emptied list is the reliable frame marker.
    let restored = run.press(b"u", "restored status", |output| {
        contains_bytes(output, b"No archived tasks")
    });
    assert!(restored.contains("Restor"), "got {restored:?}");

    let (status, exit, cooked) = run.quit();
    assert_eq!(
        exit_code(status),
        Some(0),
        "tt exited abnormally; output:\n{exit}"
    );
    assert!(
        exit.contains("\x1b[?1049l"),
        "alternate screen was not left"
    );
    assert!(exit.contains("\x1b[?25h"), "cursor was not shown again");
    assert!(cooked, "the pty must be left in cooked mode");

    // The archive and the unarchive both went through the real SQLite
    // adapter and ended where they started.
    let repository = SqliteRepository::open(database_in(&home)).unwrap();
    let tasks = repository.list_tasks().unwrap();
    assert_eq!(tasks.len(), 3);
    assert!(tasks.iter().all(|task| !task.archived));
    assert_eq!(repository.active_worklog().unwrap(), None);
}

#[test]
fn tt_persists_an_archive_across_a_quit() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let mut run = StagedRun::start(&home);

    run.press(b"", "initial frame", |output| {
        contains_bytes(output, b"Time Tracker") && contains_bytes(output, b"quit")
    });
    run.press(b"d", "archive confirmation", |output| {
        contains_bytes(output, b"Confirm archive")
    });
    run.press(b"y", "archived status", |output| {
        contains_bytes(output, b"Archived")
    });

    let (status, exit, cooked) = run.quit();
    assert_eq!(exit_code(status), Some(0), "output:\n{exit}");
    assert!(cooked, "the pty must be left in cooked mode");

    let repository = SqliteRepository::open(database_in(&home)).unwrap();
    let mut tasks = repository.list_tasks().unwrap();
    tasks.sort_by_key(|task| task.name.to_string());
    assert_eq!(tasks.len(), 3);
    assert_eq!(tasks[2].name.as_str(), "Write release notes");
    assert!(tasks[2].archived, "the archived task stayed archived");
    assert!(!tasks[0].archived && !tasks[1].archived);
}

#[test]
fn tt_ignores_active_view_keys_in_the_archived_view() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let mut run = StagedRun::start(&home);

    run.press(b"", "initial frame", |output| {
        contains_bytes(output, b"Time Tracker") && contains_bytes(output, b"quit")
    });
    let archived = run.press(b"l", "archived view", |output| {
        contains_bytes(output, b"No archived tasks")
    });
    assert!(archived.contains("u unarchive"), "got {archived:?}");

    // Every active-view action key is dead in the archived view.
    write_all_pty(run.pty.master(), b"a ed \"");
    let (status, exit, cooked) = run.quit();

    assert_eq!(exit_code(status), Some(0), "output:\n{exit}");
    for fragment in ["New task name:", "Rename task:", "Archive \"", "Started"] {
        assert!(
            !exit.contains(fragment),
            "archived view must not open {fragment:?}:\n{exit}"
        );
    }
    assert!(
        exit.contains("\x1b[?1049l"),
        "alternate screen was not left"
    );
    assert!(exit.contains("\x1b[?25h"), "cursor was not shown again");
    assert!(cooked, "the pty must be left in cooked mode");

    let repository = SqliteRepository::open(database_in(&home)).unwrap();
    let tasks = repository.list_tasks().unwrap();
    assert_eq!(tasks.len(), 3);
    assert!(tasks.iter().all(|task| !task.archived));
    assert_eq!(repository.active_worklog().unwrap(), None);
}

#[test]
fn sgr_matching_uses_complete_numeric_parameters() {
    assert!(contains_sgr_parameters(b"\x1b[38;5;4;49m", &[38, 5, 4]));
    assert!(!contains_sgr_parameters(b"\x1b[38;5;40m", &[38, 5, 4]));
    assert!(contains_sgr_parameters(b"\x1b[1;7m", &[7]));
    assert!(!contains_sgr_parameters(b"\x1b[70m", &[7]));
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
        "h/l",
        "track",
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
        repository.active_worklog().unwrap(),
        None,
        "quitting must not start or stop a timer"
    );
}

#[test]
fn tt_frames_show_tracking_modal_error_and_restore_the_terminal() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");

    let stages = run_stages_in_pty(&home);

    assert_eq!(
        exit_code(stages.status),
        Some(0),
        "exit output:\n{}",
        stages.exit
    );
    assert!(
        contains_sgr_parameters(stages.initial.as_bytes(), &[38, 5, 4]),
        "blue SGR missing from initial frame:\n{:?}",
        stages.initial
    );
    assert!(
        contains_sgr_parameters(stages.initial.as_bytes(), &[7]),
        "reverse SGR missing from initial frame:\n{:?}",
        stages.initial
    );
    assert!(
        contains_sgr_parameters(stages.active.as_bytes(), &[38, 5, 2]),
        "green SGR missing from active frame:\n{:?}",
        stages.active
    );
    assert!(
        stages.active.contains("▶"),
        "active marker missing from active frame:\n{:?}",
        stages.active
    );
    for fragment in ["New", "task", "name:"] {
        assert!(
            stages.modal.contains(fragment),
            "input modal fragment {fragment:?} missing from modal frame:\n{:?}",
            stages.modal
        );
    }
    assert!(
        contains_sgr_parameters(stages.error.as_bytes(), &[38, 5, 1]),
        "red SGR missing from error frame:\n{:?}",
        stages.error
    );
    assert!(
        stages.error.contains("Error:"),
        "error label missing from error frame:\n{:?}",
        stages.error
    );
    assert!(
        stages.exit.contains("\x1b[?1049l"),
        "alternate screen was not left"
    );
    assert!(
        stages.exit.contains("\x1b[?25h"),
        "cursor was not shown again"
    );
    assert!(stages.cooked, "the pty must be left in cooked mode");
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
