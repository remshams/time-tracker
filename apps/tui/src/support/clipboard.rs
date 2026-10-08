use std::ffi::OsStr;
use std::io::{self, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const COPY_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) fn copy(value: &str) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    let program = "pbcopy";
    #[cfg(not(target_os = "macos"))]
    let program = "wl-copy";
    copy_with_program(OsStr::new(program), value, COPY_TIMEOUT)
}

fn copy_with_program(program: &OsStr, value: &str, timeout: Duration) -> io::Result<()> {
    let mut child = Command::new(program)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdin = child.stdin.take().expect("clipboard stdin is piped");
    let bytes = value.as_bytes().to_vec();
    let (sender, receiver) = mpsc::channel();
    let _writer = thread::spawn(move || {
        let result = stdin.write_all(&bytes);
        drop(stdin);
        let _ = sender.send(result);
    });
    let deadline = Instant::now() + timeout;
    wait_for_write(&mut child, receiver, deadline)?;
    wait_for_exit(&mut child, deadline)
}

fn wait_for_write(
    child: &mut Child,
    receiver: mpsc::Receiver<io::Result<()>>,
    deadline: Instant,
) -> io::Result<()> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    match receiver.recv_timeout(remaining) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => {
            terminate_and_reap(child);
            Err(error)
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            terminate_and_reap(child);
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "clipboard write timed out",
            ))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            terminate_and_reap(child);
            Err(io::Error::other("clipboard write failed"))
        }
    }
}

fn wait_for_exit(child: &mut Child, deadline: Instant) -> io::Result<()> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(io::Error::other(format!("clipboard exited with {status}")));
            }
            Ok(None) => {
                if Instant::now().checked_duration_since(deadline).is_none() {
                    thread::sleep(Duration::from_millis(10));
                } else {
                    terminate_and_reap(child);
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "clipboard timed out",
                    ));
                }
            }
            Err(error) => {
                terminate_and_reap(child);
                return Err(error);
            }
        }
    }
}

fn terminate_and_reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn a_failed_writer_terminates_and_reaps_the_helper() {
        let mut child = Command::new("sleep").arg("10").spawn().unwrap();
        let (sender, receiver) = mpsc::channel();
        sender
            .send(Err(io::Error::from(io::ErrorKind::BrokenPipe)))
            .unwrap();
        let error = wait_for_write(
            &mut child,
            receiver,
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        assert!(child.try_wait().unwrap().is_some());
    }

    #[test]
    fn a_disconnected_writer_terminates_and_reaps_the_helper() {
        let mut child = Command::new("sleep").arg("10").spawn().unwrap();
        let (sender, receiver) = mpsc::channel();
        drop(sender);
        let error = wait_for_write(
            &mut child,
            receiver,
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "clipboard write failed");
        assert!(child.try_wait().unwrap().is_some());
    }

    #[test]
    fn a_hanging_clipboard_helper_is_terminated() {
        let directory = tempfile::tempdir().unwrap();
        let helper = directory.path().join("clipboard-helper");
        fs::write(&helper, "#!/bin/sh\nread input\nexec sleep 10\n").unwrap();
        let mut permissions = fs::metadata(&helper).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&helper, permissions).unwrap();
        let started = Instant::now();
        let error = copy_with_program(helper.as_os_str(), "hello\n", Duration::from_millis(80))
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn a_helper_that_never_reads_input_cannot_block_the_write() {
        let directory = tempfile::tempdir().unwrap();
        let helper = directory.path().join("clipboard-helper");
        fs::write(&helper, "#!/bin/sh\nexec sleep 10\n").unwrap();
        let mut permissions = fs::metadata(&helper).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&helper, permissions).unwrap();
        let started = Instant::now();
        let error = copy_with_program(
            helper.as_os_str(),
            &"x".repeat(1_000_000),
            Duration::from_millis(80),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(error.to_string(), "clipboard write timed out");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn a_helper_that_exits_unsuccessfully_reports_failure() {
        let directory = tempfile::tempdir().unwrap();
        let helper = directory.path().join("clipboard-helper");
        fs::write(&helper, "#!/bin/sh\ncat >/dev/null\nexit 7\n").unwrap();
        let mut permissions = fs::metadata(&helper).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&helper, permissions).unwrap();
        let error =
            copy_with_program(helper.as_os_str(), "hello", Duration::from_secs(2)).unwrap_err();
        assert!(error.to_string().contains("exit status: 7"));
    }

    #[test]
    fn a_timed_out_helper_cannot_continue_after_copy_returns() {
        let directory = tempfile::tempdir().unwrap();
        let helper = directory.path().join("clipboard-helper");
        let marker = directory.path().join("late-write");
        fs::write(
            &helper,
            format!(
                "#!/bin/sh\ncat >/dev/null\nsleep 0.25\nprintf done > '{}'\n",
                marker.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&helper).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&helper, permissions).unwrap();
        let error =
            copy_with_program(helper.as_os_str(), "hello", Duration::from_millis(50)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        thread::sleep(Duration::from_millis(350));
        assert!(!marker.exists());
    }
}
