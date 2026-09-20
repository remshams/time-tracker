//! A low-level PTY lifecycle check for the one behavior a rendered-screen
//! harness cannot prove: the state of the terminal line discipline after
//! `tt` exits.
//!
//! termlens reads escape sequences, but the termios flags a pty carries
//! after its child dies are invisible to it. This test keeps a raw
//! `forkpty` harness for exactly that check, and nothing more.
//!
//! The test runs on Linux and macOS. The child gets a temporary `HOME`, so
//! `tt` operates on a temporary database and never touches the user's real
//! data on either platform.

#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::ffi::{CString, c_char};
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

/// How long `tt` may take to start up and render its first frame before
/// the test kills it. A healthy debug build renders in well under a
/// second, so a full allowance here still keeps the run inside the
/// mutation test's per-run timeout.
const RUN_LIMIT: Duration = Duration::from_secs(5);

/// How long `tt` may take to answer the quit key and exit.
///
/// Deliberately short: a build that never switches the pty into raw mode
/// leaves the quit byte buffered in the line discipline, and this test
/// must fail quickly instead of timing out.
const RESPONSE_LIMIT: Duration = Duration::from_secs(3);

/// The environment for the child `tt` process: a temporary `HOME` and a
/// minimal `PATH`, so tt operates on the temporary data directory and
/// nothing else. The coverage profile file, when the test runs under
/// cargo-llvm-cov, is passed through so the child's execution counts
/// toward the measured coverage.
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

/// A `tt` child on its own pty, killed and reaped on drop.
///
/// The pid stays owned until the child has actually been reaped; only a
/// successful reaping clears it.
struct PtyChild {
    master: Option<libc::c_int>,
    pid: Option<libc::pid_t>,
}

impl PtyChild {
    fn master(&self) -> libc::c_int {
        self.master.expect("pty master is still open")
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

    /// Waits for the child to exit, bounded by `deadline`. On timeout the
    /// child is killed and reaped before the error is returned, so no
    /// call site can leak a live process by giving up on the wait.
    fn wait_for_exit(&mut self, deadline: Instant) -> io::Result<libc::c_int> {
        let pid = self.pid.expect("pty child has not been reaped");
        loop {
            let mut status = 0;
            let reaped = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
            if reaped == pid {
                self.pid = None;
                return Ok(status);
            }
            if reaped == -1 {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::Interrupted {
                    return Err(error);
                }
                continue;
            }
            if Instant::now() >= deadline {
                kill_and_reap(pid)?;
                self.pid = None;
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the child missed the deadline",
                ));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn cleanup(&mut self) -> io::Result<()> {
        let mut first_error = None;
        if let Some(pid) = self.pid {
            match kill_and_reap(pid) {
                Ok(()) => self.pid = None,
                Err(error) => first_error = Some(error),
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

/// Kills `pid` with SIGKILL and reaps it, retrying both calls through
/// EINTR. An absent process is still passed to `waitpid`, because an exited
/// child can remain available for reaping after `kill` reports ESRCH.
fn kill_and_reap(pid: libc::pid_t) -> io::Result<()> {
    loop {
        if unsafe { libc::kill(pid, libc::SIGKILL) } == 0 {
            break;
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            break;
        }
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
    loop {
        let mut status = 0;
        let reaped = unsafe { libc::waitpid(pid, &mut status, 0) };
        if reaped == pid {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ECHILD) {
            return Ok(());
        }
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

/// Captures the termios a fresh pty slave starts with, so the child is
/// forked into a known baseline and the cooked-mode check compares
/// against that baseline instead of assumed defaults.
fn baseline_termios() -> libc::termios {
    let mut master: libc::c_int = -1;
    let mut slave: libc::c_int = -1;
    let opened = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(
        opened,
        0,
        "could not open a reference pty: {}",
        io::Error::last_os_error()
    );
    let mut termios: libc::termios = unsafe { std::mem::zeroed() };
    let read = unsafe { libc::tcgetattr(slave, &mut termios) };
    unsafe {
        libc::close(master);
        libc::close(slave);
    }
    assert_eq!(
        read,
        0,
        "could not read the reference pty's termios: {}",
        io::Error::last_os_error()
    );
    termios
}

/// Forks a child running `tt` on a fresh pty whose slave starts with
/// `baseline` as its line discipline.
///
/// `argv` and `envp` are built before the fork. After the fork the child
/// performs only async-signal-safe calls: `execve`, and `_exit` if the
/// exec fails. It never touches the Rust runtime of the parent process.
fn spawn_pty(home: &Path, baseline: &libc::termios) -> PtyChild {
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
        // Apple's declarations take mutable pointers while Linux declares
        // these inputs const. A private copy satisfies both signatures and
        // keeps the baseline used by the restoration assertion unchanged.
        let mut child_termios = *baseline;
        let pid = libc::forkpty(
            &mut master,
            std::ptr::null_mut(),
            &mut child_termios,
            std::ptr::null_mut(),
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

/// Reads from the pty until it closes or `deadline` passes. Panics past
/// the deadline, so a build that stops reading input fails fast instead
/// of hanging.
fn drain_pty(master: libc::c_int, deadline: Instant, output: &mut Vec<u8>) {
    while read_some_pty(master, deadline, output) {}
}

/// Reads from the pty until `needle` arrives or the deadline passes.
/// Panics when the pty closes first or the deadline expires, always with
/// everything tt printed so far.
fn read_until_contains(
    master: libc::c_int,
    deadline: Instant,
    needle: &[u8],
    output: &mut Vec<u8>,
) {
    while !output.windows(needle.len()).any(|window| window == needle) {
        if !read_some_pty(master, deadline, output) {
            panic!(
                "the pty closed before tt rendered the expected frame; output:\n{}",
                String::from_utf8_lossy(output)
            );
        }
    }
}

/// Reads one chunk from the pty, blocking at most until `deadline`.
/// Returns false at end of file, panics past the deadline.
fn read_some_pty(master: libc::c_int, deadline: Instant, output: &mut Vec<u8>) -> bool {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .unwrap_or_else(|| {
            panic!(
                "timed out waiting for tt; output:\n{}",
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
        return read_some_pty(master, deadline, output);
    }
    if ready == -1 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            return read_some_pty(master, deadline, output);
        }
        panic!("could not poll the pty: {error}");
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
        return read_some_pty(master, deadline, output);
    }
    // Linux reports EIO when the last slave descriptor closes. macOS may
    // report a zero-length read instead.
    if error.raw_os_error() == Some(libc::EIO) {
        return false;
    }
    panic!("could not read the pty: {error}");
}

/// The exit code of a wait status, or `None` when the process died
/// abnormally.
fn exit_code(status: libc::c_int) -> Option<i32> {
    if libc::WIFEXITED(status) {
        Some(libc::WEXITSTATUS(status))
    } else {
        None
    }
}

/// Names every portable difference between two termios snapshots: the
/// four flag words, every control character, the line discipline byte,
/// and both line speeds. Field by field, because the struct carries
/// padding that a byte comparison would read as a difference.
fn termios_differences(left: &libc::termios, right: &libc::termios) -> Vec<String> {
    let mut differences = Vec::new();
    let flags = [
        ("c_iflag", left.c_iflag, right.c_iflag),
        ("c_oflag", left.c_oflag, right.c_oflag),
        ("c_cflag", left.c_cflag, right.c_cflag),
        ("c_lflag", left.c_lflag, right.c_lflag),
    ];
    for (name, left, right) in flags {
        if left != right {
            differences.push(format!("{name}: {left:#x} != {right:#x}"));
        }
    }
    for (index, (left, right)) in left.c_cc.iter().zip(right.c_cc.iter()).enumerate() {
        if left != right {
            differences.push(format!("c_cc[{index}]: {left:#x} != {right:#x}"));
        }
    }
    #[cfg(target_os = "linux")]
    if left.c_line != right.c_line {
        differences.push(format!("c_line: {:#x} != {:#x}", left.c_line, right.c_line));
    }
    let left_ispeed = unsafe { libc::cfgetispeed(left) };
    let right_ispeed = unsafe { libc::cfgetispeed(right) };
    if left_ispeed != right_ispeed {
        differences.push(format!("input speed: {left_ispeed} != {right_ispeed}"));
    }
    let left_ospeed = unsafe { libc::cfgetospeed(left) };
    let right_ospeed = unsafe { libc::cfgetospeed(right) };
    if left_ospeed != right_ospeed {
        differences.push(format!("output speed: {left_ospeed} != {right_ospeed}"));
    }
    differences
}

#[test]
fn tt_quits_cleanly_and_leaves_the_terminal_in_cooked_mode() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    std::fs::create_dir(&home).unwrap();

    // The child starts from a captured known baseline, so a complete
    // restoration brings the pty back to exactly that baseline.
    let baseline = baseline_termios();
    let mut pty = spawn_pty(&home, &baseline);
    configure_pty(pty.master());

    // Startup keeps the generous run limit; the first frame proves the
    // program has rendered, so the quit response and the exit fall under
    // the short response allowance.
    let mut output = Vec::new();
    let startup_deadline = Instant::now() + RUN_LIMIT;
    read_until_contains(pty.master(), startup_deadline, b"Time Tracker", &mut output);

    write_all_pty(pty.master(), b"q");
    let response_deadline = Instant::now() + RESPONSE_LIMIT;
    drain_pty(pty.master(), response_deadline, &mut output);

    let status = pty
        .wait_for_exit(Instant::now() + RESPONSE_LIMIT)
        .unwrap_or_else(|error| {
            panic!(
                "tt did not exit on its own: {error}; output:\n{}",
                String::from_utf8_lossy(&output)
            )
        });
    let code = exit_code(status);

    // Raw mode was restored: the pty's line discipline matches the
    // baseline field for field, before the master closes.
    let mut termios: libc::termios = unsafe { std::mem::zeroed() };
    let read = unsafe { libc::tcgetattr(pty.master(), &mut termios) };
    let differences = if read == 0 {
        termios_differences(&termios, &baseline)
    } else {
        vec![format!(
            "could not read the pty's termios: {}",
            io::Error::last_os_error()
        )]
    };
    pty.close_master()
        .unwrap_or_else(|error| panic!("could not close the pty master: {error}"));

    let output = String::from_utf8_lossy(&output).into_owned();
    assert_eq!(code, Some(0), "tt exited abnormally; output:\n{output}");
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
    // A skipped restore leaves at least one altered flag, control
    // character, or speed behind; the diff names exactly which.
    assert!(
        differences.is_empty(),
        "the pty must be left in cooked mode; {}",
        differences.join(", ")
    );
}
