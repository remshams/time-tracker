use std::io::{self, Write};

use serde_json::{Value, json};

use crate::process;

#[test]
fn process_routes_json_results_to_stdout_and_errors_to_stderr() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("tt.db");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let args = [
        "tt-cli",
        "--db",
        database.to_str().unwrap(),
        "--at",
        "2026-01-01T00:00:00Z",
        "tracking",
        "status",
    ];
    assert_eq!(process(args, &mut stdout, &mut stderr), 0);
    assert!(stderr.is_empty());
    let result: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(
        result,
        json!({"ok":true,"data":{"state":"idle","active_worklog":null,"as_of":"2026-01-01T00:00:00Z","elapsed_seconds":0}})
    );
    assert!(stdout.ends_with(b"\n"));
    stdout.clear();
    assert_eq!(
        process(["tt-cli", "bad-command"], &mut stdout, &mut stderr),
        2
    );
    assert!(stdout.is_empty());
    let result: Value = serde_json::from_slice(&stderr).unwrap();
    assert_eq!(result["ok"], false);
    assert_eq!(result["error"]["code"], "invalid_input");
    assert_eq!(result["error"]["recovery_failed"], false);
    assert!(
        result["error"]["message"]
            .as_str()
            .unwrap()
            .contains("bad-command")
    );
    assert!(result["error"].get("exit_code").is_none());
}

#[test]
fn help_and_version_do_not_open_storage() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("never.db");
    for flag in ["--help", "--version"] {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert_eq!(
            process(
                ["tt-cli", "--db", database.to_str().unwrap(), flag],
                &mut stdout,
                &mut stderr
            ),
            0
        );
        assert!(stderr.is_empty());
        assert!(!stdout.is_empty());
        assert!(!database.exists());
    }
}

struct BrokenWriter;
impl Write for BrokenWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("broken output"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn output_failures_return_storage_status_and_attempt_a_structured_error() {
    let mut stderr = Vec::new();
    assert_eq!(
        crate::process::report(Ok(json!({"value":1})), &mut BrokenWriter, &mut stderr),
        4
    );
    let error: Value = serde_json::from_slice(&stderr).unwrap();
    assert_eq!(
        error,
        json!({"ok":false,"error":{"code":"storage","message":"broken output","recovery_failed":false}})
    );
    assert_eq!(
        crate::process::report(
            Err(crate::CliError::input("bad")),
            &mut Vec::new(),
            &mut BrokenWriter
        ),
        4
    );
    stderr.clear();
    assert_eq!(
        process(["tt-cli", "--help"], &mut BrokenWriter, &mut stderr),
        4
    );
    assert!(!stderr.is_empty());
}

struct FlushFailure;
impl Write for FlushFailure {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("flush failed"))
    }
}

#[test]
fn flush_failures_are_reported_for_json_help_and_error_streams() {
    let mut stderr = Vec::new();
    assert_eq!(
        crate::process::report(Ok(json!({})), &mut FlushFailure, &mut stderr),
        4
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&stderr).unwrap()["error"]["message"],
        "flush failed"
    );
    stderr.clear();
    assert_eq!(
        process(["tt-cli", "--help"], &mut FlushFailure, &mut stderr),
        4
    );
    assert!(!stderr.is_empty());
    assert_eq!(
        crate::process::report(
            Err(crate::CliError::input("bad input")),
            &mut Vec::new(),
            &mut FlushFailure
        ),
        4
    );
}
