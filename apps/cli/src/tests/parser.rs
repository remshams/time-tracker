use clap::Parser;

use crate::args::{Cli, Command, Tasks, end_timestamp, timestamp};

#[test]
fn commands_parse_with_global_options_before_or_after_subcommands() {
    for arguments in [
        vec!["tt-cli", "--db", "test.db", "tasks", "list"],
        vec!["tt-cli", "tasks", "list", "--db", "test.db"],
        vec![
            "tt-cli",
            "--server",
            "http://127.0.0.1:8765",
            "tracking",
            "status",
        ],
    ] {
        assert!(Cli::try_parse_from(arguments).is_ok());
    }
    assert!(matches!(
        Cli::try_parse_from(["tt-cli", "tasks", "create", "Review build"])
            .unwrap()
            .command,
        Command::Tasks {
            command: Tasks::Create { .. }
        }
    ));
}

#[test]
fn malformed_arguments_and_missing_guards_are_rejected() {
    for arguments in [
        vec!["tt-cli"],
        vec!["tt-cli", "unknown"],
        vec![
            "tt-cli",
            "--db",
            "a.db",
            "--server",
            "http://localhost",
            "tasks",
            "list",
        ],
        vec!["tt-cli", "tasks", "list", "--state", "unknown"],
        vec!["tt-cli", "tasks", "get", "bad-id"],
        vec!["tt-cli", "tracking", "stop"],
        vec!["tt-cli", "tasks", "archive-inactive", "--preview", "{}"],
        vec!["tt-cli", "reports", "--start", "2026-01-01T00:00:00Z"],
        vec![
            "tt-cli",
            "reports",
            "today",
            "--from",
            "2026-01-01",
            "--to",
            "2026-01-02",
        ],
        vec![
            "tt-cli",
            "reports",
            "--from",
            "2026-02-30",
            "--to",
            "2026-03-01",
        ],
        vec!["tt-cli", "reports", "--timezone", "Bad/Zone"],
    ] {
        assert!(
            Cli::try_parse_from(&arguments).is_err(),
            "accepted {arguments:?}"
        );
    }
}

#[test]
fn timestamps_require_offsets_and_preserve_fractional_seconds() {
    let at = timestamp("2026-01-01T03:00:00.123456789+03:00").unwrap();
    assert_eq!(at.to_rfc3339(), "2026-01-01T00:00:00.123456789+00:00");
    assert!(timestamp("2026-01-01T03:00:00").is_err());
    assert!(timestamp("2026-01-01").is_err());
    assert_eq!(end_timestamp("running").unwrap().0, None);
    assert_eq!(
        end_timestamp("2026-01-01T03:00:00.123456789+03:00")
            .unwrap()
            .0,
        Some(at)
    );
    assert!(end_timestamp("null").is_err());
}

#[test]
fn help_is_a_successful_parse_display_with_all_command_groups() {
    let error = Cli::try_parse_from(["tt-cli", "--help"]).unwrap_err();
    assert!(!error.use_stderr());
    for word in [
        "tasks", "tracking", "worklogs", "reports", "--db", "--server",
    ] {
        assert!(error.to_string().contains(word));
    }
}

#[test]
fn json_inputs_are_bounded_and_file_inputs_use_the_same_parser() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("preview.json");
    std::fs::write(&path, r#"{"value": 1}"#).unwrap();
    assert_eq!(
        crate::read_json::<serde_json::Value>(&format!("@{}", path.display())).unwrap()["value"],
        1
    );
    assert!(crate::read_json::<serde_json::Value>("broken").is_err());
    assert!(crate::read_json::<serde_json::Value>("@/missing-preview-file").is_err());
    let oversized = " ".repeat(1024 * 1024 + 1);
    assert!(crate::read_json::<serde_json::Value>(&oversized).is_err());
    std::fs::write(&path, oversized).unwrap();
    assert!(crate::read_json::<serde_json::Value>(&format!("@{}", path.display())).is_err());
}
