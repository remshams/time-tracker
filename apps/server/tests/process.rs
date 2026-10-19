//! Exercise the installed command forms against a real server process.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn check_server(compatibility: bool, explicit_database: bool) {
    let directory = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address: SocketAddr = listener.local_addr().unwrap();
    drop(listener);
    let database = directory.path().join("explicit.db");
    let mut command = Command::new(env!("CARGO_BIN_EXE_tt-server"));
    if compatibility {
        command.arg("serve");
    }
    command.args(["--bind", &address.to_string()]);
    if explicit_database {
        command.arg("--db").arg(&database);
    }
    command
        .env("HOME", directory.path())
        .env("XDG_DATA_HOME", directory.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    let mut server = Server(command.spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(
            server.0.try_wait().unwrap().is_none(),
            "server exited before readiness"
        );
        if let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(100)) {
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            write!(
                stream,
                "GET /v1/health HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            assert!(response.starts_with("HTTP/1.1 200"), "{response}");
            assert!(response.contains("\"status\":\"ok\""), "{response}");
            break;
        }
        assert!(Instant::now() < deadline, "server did not become healthy");
        std::thread::sleep(Duration::from_millis(20));
    }
    if explicit_database {
        assert!(database.is_file());
    } else {
        #[cfg(target_os = "macos")]
        let database = directory
            .path()
            .join("Library/Application Support/Time Tracker/tt-server.db");
        #[cfg(not(target_os = "macos"))]
        let database = directory.path().join("Time Tracker/tt-server.db");
        assert!(database.is_file(), "missing {}", database.display());
        assert!(!database.with_file_name("tt.db").exists());
    }
}

#[test]
fn canonical_and_compatibility_commands_serve_explicit_and_default_databases() {
    for compatibility in [false, true] {
        for explicit_database in [false, true] {
            check_server(compatibility, explicit_database);
        }
    }
}

#[test]
fn help_and_validation_report_the_standalone_command() {
    let help = Command::new(env!("CARGO_BIN_EXE_tt-server"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(text.contains("tt-server --bind ADDRESS:PORT [--db PATH]"));
    assert!(text.contains("Compatibility form"));
    for prefix in [vec![], vec!["serve"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_tt-server"))
            .args(prefix)
            .args(["--bind", "0.0.0.0:8765"])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "tt-server: --bind must use a loopback or Tailscale address; run `tt-server --help` for usage\n"
        );
    }
}
