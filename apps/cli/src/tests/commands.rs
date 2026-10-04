use std::path::Path;

use clap::Parser;
use serde_json::Value;

use crate::{Cli, run};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().unwrap()
}

pub(super) fn command(path: &Path, at: &str, arguments: &[&str]) -> Result<Value, crate::CliError> {
    let path = path.to_str().unwrap();
    let cli = Cli::try_parse_from(
        ["tt-cli", "--db", path, "--at", at]
            .into_iter()
            .chain(arguments.iter().copied()),
    )
    .unwrap();
    runtime().block_on(run(cli))
}

const T0: &str = "2026-01-01T00:00:00Z";
const T1: &str = "2026-01-01T01:00:00Z";
const T2: &str = "2026-01-01T02:00:00Z";
const T3: &str = "2026-01-01T03:00:00Z";

#[test]
fn tasks_share_seed_data_and_support_create_rename_archive_restore_and_search() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tt.db");
    let seeded = command(&path, T0, &["tasks", "list"]).unwrap();
    assert_eq!(seeded["tasks"].as_array().unwrap().len(), 3);
    let created = command(&path, T1, &["tasks", "create", "  Build release  "]).unwrap();
    let id = created["id"].as_str().unwrap();
    assert_eq!(created["name"], "Build release");
    assert_eq!(created["archived"], false);
    assert_eq!(command(&path, T1, &["tasks", "get", id]).unwrap(), created);
    assert_eq!(
        command(&path, T1, &["tasks", "list", "--search", "BRe"]).unwrap()["tasks"][0]["task"]["id"],
        id
    );
    assert_eq!(
        command(&path, T1, &["tasks", "list", "--search", "BZe"]).unwrap()["tasks"],
        serde_json::json!([])
    );
    let renamed = command(&path, T2, &["tasks", "rename", id, "Publish release"]).unwrap();
    assert_eq!(renamed["name"], "Publish release");
    assert_eq!(renamed["id"], id);
    assert_eq!(
        command(&path, T2, &["tasks", "archive", id]).unwrap()["archived"],
        true
    );
    assert_eq!(
        command(&path, T2, &["tasks", "list", "--state", "archived"]).unwrap()["tasks"][0]["task"]
            ["id"],
        id
    );
    assert_eq!(
        command(&path, T2, &["tasks", "list", "--state", "all"]).unwrap()["tasks"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        command(&path, T3, &["tasks", "restore", id]).unwrap()["archived"],
        false
    );
    for sort in ["worked", "updated", "created"] {
        assert_eq!(
            command(&path, T3, &["tasks", "list", "--sort", sort]).unwrap()["tasks"][0]["task"]["id"],
            id
        );
    }
    assert_eq!(
        command(
            &path,
            T3,
            &["tasks", "get", "00000000-0000-7000-8000-000000000001"]
        )
        .unwrap_err()
        .exit_code,
        3
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn desired_state_tracking_is_idempotent_and_stale_stop_cannot_stop_a_newer_timer() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tt.db");
    assert_eq!(
        command(&path, T0, &["tracking", "status"]).unwrap(),
        serde_json::json!({"state":"idle","active_worklog":null,"as_of":T0,"elapsed_seconds":0})
    );
    let first = command(&path, T0, &["tasks", "create", "First"]).unwrap();
    let second = command(&path, T0, &["tasks", "create", "Second"]).unwrap();
    let first_id = first["id"].as_str().unwrap();
    let second_id = second["id"].as_str().unwrap();
    let started = command(&path, T1, &["tracking", "start", first_id]).unwrap();
    assert_eq!(started["outcome"], "started");
    let old = started["worklog"]["id"].as_str().unwrap();
    let same = command(&path, T2, &["tracking", "start", first_id]).unwrap();
    assert_eq!(same["outcome"], "already_active");
    assert_eq!(same["worklog"]["id"], old);
    assert_eq!(
        command(&path, T2, &["tasks", "archive", first_id])
            .unwrap_err()
            .code,
        "active_task"
    );
    let switched = command(&path, T2, &["tracking", "start", second_id]).unwrap();
    assert_eq!(switched["outcome"], "switched");
    assert_eq!(switched["stopped"]["id"], old);
    assert_eq!(switched["stopped"]["end"], "2026-01-01T02:00:00Z");
    let new = switched["started"]["id"].as_str().unwrap();
    assert_ne!(new, old);
    assert_eq!(
        command(&path, T3, &["tracking", "stop", "--expected-active", old])
            .unwrap_err()
            .exit_code,
        3
    );
    assert_eq!(
        command(&path, T3, &["tracking", "status"]).unwrap()["active_worklog"]["id"],
        new
    );
    let stopped = command(&path, T3, &["tracking", "stop", "--expected-active", new]).unwrap();
    assert_eq!(stopped["outcome"], "stopped");
    assert_eq!(stopped["worklog"]["end"], "2026-01-01T03:00:00Z");
    assert_eq!(
        command(&path, T3, &["tracking", "stop", "--expected-active", old]).unwrap()["outcome"],
        "already_idle"
    );
    command(&path, T3, &["tasks", "archive", second_id]).unwrap();
    assert!(command(&path, T3, &["tracking", "start", second_id]).is_err());
}

#[test]
fn guarded_correction_move_delete_and_reports_use_committed_values() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tt.db");
    let first = command(&path, T0, &["tasks", "create", "First"]).unwrap();
    let second = command(&path, T0, &["tasks", "create", "Second"]).unwrap();
    let first_id = first["id"].as_str().unwrap();
    let second_id = second["id"].as_str().unwrap();
    let started = command(&path, T1, &["tracking", "start", first_id]).unwrap();
    let id = started["worklog"]["id"].as_str().unwrap();
    command(&path, T2, &["tracking", "stop", "--expected-active", id]).unwrap();
    let args = [
        "worklogs",
        "correct",
        id,
        "--expected-start",
        T1,
        "--expected-end",
        T2,
        "--start",
        "2026-01-01T00:30:00.123456789Z",
    ];
    let corrected = command(&path, T3, &args).unwrap();
    assert_eq!(corrected["start"], "2026-01-01T00:30:00.123456Z");
    assert_eq!(corrected["end"], T2);
    assert_eq!(corrected["id"], id);
    assert_eq!(
        command(&path, T3, &args).unwrap_err().code,
        "worklog_changed"
    );
    let corrected_start = corrected["start"].as_str().unwrap();
    let corrected = command(
        &path,
        T3,
        &[
            "worklogs",
            "correct",
            id,
            "--expected-start",
            corrected_start,
            "--expected-end",
            T2,
            "--end",
            "2026-01-01T02:30:00Z",
        ],
    )
    .unwrap();
    assert_eq!(corrected["start"], corrected_start);
    let moved = command(
        &path,
        T3,
        &[
            "worklogs",
            "move",
            id,
            second_id,
            "--expected-task",
            first_id,
            "--expected-start",
            corrected_start,
            "--expected-end",
            "2026-01-01T02:30:00Z",
        ],
    )
    .unwrap();
    assert_eq!(moved["task_id"], second_id);
    assert_eq!(moved["id"], id);
    assert_eq!(moved["start"], corrected_start);
    let report = command(
        &path,
        T3,
        &["reports", "--start", T0, "--end", T3, "--timezone", "UTC"],
    )
    .unwrap();
    assert_eq!(report["rows"][0]["task"]["id"], second_id);
    assert_eq!(report["total_us"], 7_199_876_544i64);
    assert_eq!(report["start"], T0);
    assert_eq!(report["end"], T3);
    assert_eq!(report["as_of"], T3);
    assert_eq!(report["timezone"], "UTC");
    assert_eq!(
        command(&path, T3, &["worklogs", "list", "--task", second_id]).unwrap()["worklogs"][0],
        moved
    );
    assert_eq!(
        command(&path, T3, &["worklogs", "list"]).unwrap()["worklogs"][0],
        moved
    );
    let deleted = command(
        &path,
        T3,
        &[
            "worklogs",
            "delete",
            id,
            "--expected-task",
            second_id,
            "--expected-start",
            corrected_start,
            "--expected-end",
            "2026-01-01T02:30:00Z",
            "--yes",
        ],
    )
    .unwrap();
    assert_eq!(deleted["deleted"], moved);
    assert_eq!(
        command(&path, T3, &["worklogs", "list"]).unwrap()["worklogs"],
        serde_json::json!([])
    );
}

#[test]
fn invalid_inputs_are_rejected_before_a_database_is_created() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("never-created.db");
    let id = "00000000-0000-7000-8000-000000000001";
    for arguments in [
        vec!["tasks", "create", "  "],
        vec!["tasks", "archive-inactive", "--preview", "{}", "--yes"],
        vec!["worklogs", "list", "--cursor", "{}"],
        vec![
            "worklogs",
            "correct",
            id,
            "--expected-start",
            T0,
            "--expected-end",
            T1,
        ],
        vec![
            "worklogs",
            "delete",
            id,
            "--expected-task",
            id,
            "--expected-start",
            T0,
            "--expected-end",
            "running",
            "--yes",
        ],
        vec!["reports", "--from", "2026-01-02", "--to", "2026-01-01"],
    ] {
        assert_eq!(command(&path, T0, &arguments).unwrap_err().exit_code, 2);
        assert!(!path.exists(), "created database for {arguments:?}");
    }
}

#[test]
fn inactive_archiving_preserves_preview_candidates_and_backend_identity() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tt.db");
    command(&path, T0, &["tasks", "list"]).unwrap();
    let preview = command(
        &path,
        "2026-02-01T00:00:00Z",
        &["tasks", "preview-inactive"],
    )
    .unwrap();
    assert_eq!(preview["count"], 3);
    let token = preview["preview"].to_string();
    let other = temp.path().join("other.db");
    assert_eq!(
        command(
            &other,
            T0,
            &["tasks", "archive-inactive", "--preview", &token, "--yes"]
        )
        .unwrap_err()
        .exit_code,
        2
    );
    assert!(!other.exists());
    let mut duplicate = preview["preview"].clone();
    let id = duplicate["task_ids"][0].clone();
    duplicate["task_ids"].as_array_mut().unwrap().push(id);
    assert_eq!(
        command(
            &path,
            "2026-02-01T00:00:00Z",
            &[
                "tasks",
                "archive-inactive",
                "--preview",
                &duplicate.to_string(),
                "--yes"
            ]
        )
        .unwrap_err()
        .exit_code,
        2
    );
    assert_eq!(
        command(
            &path,
            "2026-02-01T00:00:00Z",
            &["tasks", "archive-inactive", "--preview", &token, "--yes"]
        )
        .unwrap()["archived_count"],
        3
    );
    assert_eq!(
        command(
            &path,
            "2026-02-01T00:00:00Z",
            &["tasks", "archive-inactive", "--preview", &token, "--yes"]
        )
        .unwrap_err()
        .code,
        "inactive_candidates_changed"
    );
}
