use chrono::{Duration, TimeZone, Utc};
use tracker_application::TrackerApplication;
use tracker_domain::TaskName;
use tracker_storage::SqliteRepository;

use super::{T0, T1, T2, T3, command};

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
        assert_eq!(first["next_cursor"]["cursor"].as_object().unwrap().len(), 2);
        assert_eq!(
            first["next_cursor"]["cursor"]["id"],
            first["worklogs"][49]["id"]
        );
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
fn invalid_inputs_are_rejected_before_a_database_is_created() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("never-created.db");
    let id = "00000000-0000-7000-8000-000000000001";
    let other = "00000000-0000-7000-8000-000000000002";
    for (arguments, message) in [
        (vec!["tasks", "create", "  "], "task name must not be empty"),
        (
            vec!["tasks", "archive-inactive", "--preview", "{}", "--yes"],
            "missing field `mode` at line 1 column 2",
        ),
        (
            vec!["worklogs", "list", "--cursor", "{}"],
            "missing field `scope` at line 1 column 2",
        ),
        (
            vec![
                "worklogs",
                "correct",
                id,
                "--expected-start",
                T0,
                "--expected-end",
                T1,
            ],
            "correction requires --start or --end",
        ),
        (
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
            "only completed worklogs can be deleted",
        ),
        (
            vec!["reports", "--from", "2026-01-02", "--to", "2026-01-01"],
            "--from must be on or before --to",
        ),
        (
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
            "a correction must preserve whether the worklog is active or completed",
        ),
        (
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
            "worklog end must not precede its start",
        ),
        (
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
            "a correction must preserve whether the worklog is active or completed",
        ),
        (
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
            "corrected worklog end must not precede its start",
        ),
        (
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
            "corrected worklog end must not be later than occurred_at",
        ),
        (
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
            "a worklog must move to a different task",
        ),
        (
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
            "worklog end must not precede its start",
        ),
        (
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
            "worklog end must not precede its start",
        ),
    ] {
        let error = command(&path, T1, &arguments).unwrap_err();
        assert_eq!(error.exit_code, 2, "{arguments:?}");
        assert_eq!(error.message, message, "{arguments:?}");
        assert!(!path.exists(), "created database for {arguments:?}");
    }
}

#[test]
fn corrections_and_elapsed_time_use_microsecond_precision() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tt.db");
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
