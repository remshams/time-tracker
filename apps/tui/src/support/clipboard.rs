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
    let remaining = deadline.saturating_duration_since(Instant::now());
    match receiver.recv_timeout(remaining) {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            terminate_and_reap(&mut child);
            return Err(error);
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            terminate_and_reap(&mut child);
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "clipboard write timed out",
            ));
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            terminate_and_reap(&mut child);
            return Err(io::Error::other("clipboard write failed"));
        }
    }
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(io::Error::other(format!("clipboard exited with {status}")));
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                terminate_and_reap(&mut child);
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "clipboard timed out",
                ));
            }
            Err(error) => {
                terminate_and_reap(&mut child);
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
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
