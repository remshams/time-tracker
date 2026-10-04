use chrono::{Duration, TimeZone, Utc};
use tracker_application::{TaskOperations, TrackerApplication, TrackingOperations};
use tracker_domain::TaskName;
use tracker_storage::SqliteRepository;

use super::commands::command;

const T0: &str = "2026-01-01T00:00:00Z";
const T1: &str = "2026-01-01T01:00:00Z";
const T2: &str = "2026-01-01T02:00:00Z";

#[test]
fn history_pages_keep_exact_scope_revision_and_backend() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tt.db");
    let repository = SqliteRepository::open(&path).unwrap();
    let mut app = TrackerApplication::load(repository).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let task = app
        .create_task(TaskName::new("History").unwrap(), now)
        .unwrap();
    for index in 0..52 {
        app.set_active_task(task.id(), now + Duration::seconds(index * 2))
            .unwrap();
        let tracker_domain::TrackingState::Running { worklog } = app.current_tracking() else {
            panic!("not running")
        };
        app.clear_active_task(worklog.id(), now + Duration::seconds(index * 2 + 1))
            .unwrap();
    }
    drop(app);
    let id = task.id().to_string();
    for task_scoped in [false, true] {
        let mut args = vec!["worklogs", "list"];
        if task_scoped {
            args.extend(["--task", &id]);
        }
        let first = command(&path, T1, &args).unwrap();
        assert_eq!(first["worklogs"].as_array().unwrap().len(), 50);
        let cursor = first["next_cursor"].to_string();
        args.extend(["--cursor", &cursor]);
        let second = command(&path, T1, &args).unwrap();
        assert_eq!(second["worklogs"].as_array().unwrap().len(), 2);
        assert_eq!(second["next_cursor"], serde_json::Value::Null);
        assert_ne!(first["worklogs"][49]["id"], second["worklogs"][0]["id"]);
        let other = temp.path().join("other.db");
        assert_eq!(command(&other, T1, &args).unwrap_err().exit_code, 2);
        assert!(!other.exists());
        let wrong_scope = if task_scoped {
            vec!["worklogs", "list", "--cursor", &cursor]
        } else {
            vec!["worklogs", "list", "--task", &id, "--cursor", &cursor]
        };
        assert_eq!(command(&path, T1, &wrong_scope).unwrap_err().exit_code, 2);
        let row = &first["worklogs"][0];
        let new_start = (now + Duration::seconds(if task_scoped { 102 } else { 101 })).to_rfc3339();
        command(
            &path,
            T1,
            &[
                "worklogs",
                "correct",
                row["id"].as_str().unwrap(),
                "--expected-start",
                row["start"].as_str().unwrap(),
                "--expected-end",
                row["end"].as_str().unwrap(),
                "--start",
                &new_start,
            ],
        )
        .unwrap();
        assert_eq!(
            command(&path, T1, &args).unwrap_err().code,
            "worklog_history_changed"
        );
    }
}

#[test]
fn malformed_mutations_do_not_create_storage_and_validate_at_microsecond_precision() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("never.db");
    let id = "00000000-0000-7000-8000-000000000001";
    let other = "00000000-0000-7000-8000-000000000002";
    for arguments in [
        vec![
            "worklogs",
            "correct",
            id,
            "--expected-start",
            T0,
            "--expected-end",
            T1,
            "--end",
            "running",
        ],
        vec![
            "worklogs",
            "correct",
            id,
            "--expected-start",
            T1,
            "--expected-end",
            T0,
            "--start",
            T0,
        ],
        vec![
            "worklogs",
            "correct",
            id,
            "--expected-start",
            T0,
            "--expected-end",
            "running",
            "--end",
            T1,
        ],
        vec![
            "worklogs",
            "correct",
            id,
            "--expected-start",
            T0,
            "--expected-end",
            T1,
            "--start",
            T2,
        ],
        vec![
            "worklogs",
            "correct",
            id,
            "--expected-start",
            T0,
            "--expected-end",
            T1,
            "--end",
            T2,
        ],
        vec![
            "worklogs",
            "move",
            id,
            id,
            "--expected-task",
            id,
            "--expected-start",
            T0,
            "--expected-end",
            T1,
        ],
        vec![
            "worklogs",
            "move",
            id,
            other,
            "--expected-task",
            id,
            "--expected-start",
            T1,
            "--expected-end",
            T0,
        ],
        vec![
            "worklogs",
            "delete",
            id,
            "--expected-task",
            id,
            "--expected-start",
            T1,
            "--expected-end",
            T0,
            "--yes",
        ],
    ] {
        assert_eq!(command(&path, T1, &arguments).unwrap_err().exit_code, 2);
        assert!(!path.exists());
    }
    let created = command(&path, T0, &["tasks", "create", "Tiny interval"]).unwrap();
    let task = created["id"].as_str().unwrap();
    let started = command(
        &path,
        "2026-01-01T00:00:00.000000001Z",
        &["tracking", "start", task],
    )
    .unwrap();
    let id = started["worklog"]["id"].as_str().unwrap();
    let corrected = command(
        &path,
        T0,
        &[
            "worklogs",
            "correct",
            id,
            "--expected-start",
            "2026-01-01T00:00:00.000000001Z",
            "--expected-end",
            "running",
            "--start",
            "2026-01-01T00:00:00.000000999Z",
        ],
    )
    .unwrap();
    assert_eq!(corrected["start"], T0);
    assert_eq!(corrected["end"], serde_json::Value::Null);
    assert_eq!(
        command(&path, "2025-12-31T00:00:00Z", &["tracking", "status"]).unwrap()["elapsed_seconds"],
        0
    );
    assert_eq!(
        command(&path, T1, &["tracking", "status"]).unwrap()["elapsed_seconds"],
        3600
    );
}

#[test]
fn bulk_archive_rechecks_activity_after_preview_and_refuses_time_travel() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tt.db");
    let task = command(&path, T0, &["tasks", "create", "Old task"]).unwrap();
    let as_of = "2026-02-01T00:00:00Z";
    let preview = command(&path, as_of, &["tasks", "preview-inactive"]).unwrap();
    let token = preview["preview"].to_string();
    assert_eq!(
        command(
            &path,
            T0,
            &["tasks", "archive-inactive", "--preview", &token, "--yes"]
        )
        .unwrap_err()
        .exit_code,
        2
    );
    let task_id = task["id"].as_str().unwrap();
    let started = command(
        &path,
        "2026-02-01T00:01:00Z",
        &["tracking", "start", task_id],
    )
    .unwrap();
    command(
        &path,
        "2026-02-01T00:02:00Z",
        &[
            "tracking",
            "stop",
            "--expected-active",
            started["worklog"]["id"].as_str().unwrap(),
        ],
    )
    .unwrap();
    assert_eq!(
        command(
            &path,
            "2026-02-01T00:03:00Z",
            &["tasks", "archive-inactive", "--preview", &token, "--yes"]
        )
        .unwrap_err()
        .code,
        "inactive_candidates_changed"
    );
    assert_eq!(
        command(&path, as_of, &["tasks", "get", task_id]).unwrap()["archived"],
        false
    );
}

#[cfg(unix)]
#[test]
fn backend_paths_reject_non_utf8_and_resolve_missing_descendants_of_symlinks() {
    use std::os::unix::ffi::OsStringExt;
    let temp = tempfile::tempdir().unwrap();
    let invalid = temp.path().join(std::ffi::OsString::from_vec(vec![0xff]));
    assert_eq!(
        crate::backend::identity(Some(&invalid), None)
            .unwrap_err()
            .exit_code,
        2
    );
    assert!(!invalid.exists());
    let real = temp.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let link = temp.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let path = link.join("missing").join("tt.db");
    let before = crate::backend::identity(Some(&path), None).unwrap();
    std::fs::create_dir(real.join("missing")).unwrap();
    let after = crate::backend::identity(Some(&path), None).unwrap();
    assert_eq!(before, after);
    assert_eq!(
        before,
        format!("local:{}", real.join("missing/tt.db").display())
    );
}

#[cfg(unix)]
#[test]
fn json_file_inputs_reject_fifos_and_directories_without_waiting_for_writers() {
    use std::os::unix::ffi::OsStrExt;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("fifo");
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: c_path is a valid terminated path and the mode is a file permission mask.
    assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
    assert_eq!(
        crate::read_json::<serde_json::Value>(&format!("@{}", path.display()))
            .unwrap_err()
            .exit_code,
        2
    );
    assert_eq!(
        crate::read_json::<serde_json::Value>(&format!("@{}", temp.path().display()))
            .unwrap_err()
            .exit_code,
        2
    );
}

#[test]
fn remote_preview_tokens_reject_invalid_guards_and_candidates_before_connecting() {
    let backend = "remote:http://127.0.0.1:8765/";
    let valid = serde_json::json!({
        "mode":"remote", "backend":backend,
        "preview":{"as_of":T0,"count":1,"sample_names":["Old task"],"revision":"revision-1","candidate_fingerprint":"a".repeat(64)}
    });
    let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let validate = |token: &serde_json::Value, identity: &str| {
        let mut command = crate::args::Tasks::ArchiveInactive {
            preview: token.to_string(),
            yes: true,
        };
        crate::tasks::validate(&mut command, identity, now)
    };
    validate(&valid, backend).unwrap();
    let mut exactly_five = valid.clone();
    exactly_five["preview"]["count"] = serde_json::json!(5);
    exactly_five["preview"]["sample_names"] =
        serde_json::json!(["One", "Two", "Three", "Four", "Five"]);
    validate(&exactly_five, backend).unwrap();
    let mut fewer_than_count = valid.clone();
    fewer_than_count["preview"]["count"] = serde_json::json!(5);
    validate(&fewer_than_count, backend).unwrap();
    let mut too_many = exactly_five.clone();
    too_many["preview"]["count"] = serde_json::json!(6);
    too_many["preview"]["sample_names"] =
        serde_json::json!(["One", "Two", "Three", "Four", "Five", "Six"]);
    assert_eq!(validate(&too_many, backend).unwrap_err().exit_code, 2);
    for (field, value) in [
        ("revision", serde_json::json!("")),
        ("candidate_fingerprint", serde_json::json!("A".repeat(64))),
        ("candidate_fingerprint", serde_json::json!("g".repeat(64))),
        ("candidate_fingerprint", serde_json::json!("a".repeat(63))),
        ("candidate_fingerprint", serde_json::json!("a".repeat(65))),
        ("sample_names", serde_json::json!([""])),
        (
            "sample_names",
            serde_json::json!(["One", "Two", "Three", "Four", "Five", "Six"]),
        ),
        ("count", serde_json::json!(0)),
    ] {
        let mut invalid = valid.clone();
        invalid["preview"][field] = value;
        assert_eq!(validate(&invalid, backend).unwrap_err().exit_code, 2);
    }
    assert_eq!(
        validate(&valid, "local:/tmp/tt.db").unwrap_err().exit_code,
        2
    );
    assert_eq!(
        validate(&valid, "remote:http://127.0.0.1:9000/")
            .unwrap_err()
            .exit_code,
        2
    );
}

#[test]
fn correction_requires_a_replacement_even_when_expected_times_are_valid() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("never-created.db");
    let id = "00000000-0000-7000-8000-000000000001";
    let error = command(
        &path,
        T2,
        &[
            "worklogs",
            "correct",
            id,
            "--expected-start",
            T0,
            "--expected-end",
            T1,
        ],
    )
    .unwrap_err();
    assert_eq!(error.exit_code, 2);
    assert_eq!(error.message, "correction requires --start or --end");
    assert!(!path.exists());
}
