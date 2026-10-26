use chrono::{TimeZone, Utc};

use super::{T0, command};

#[test]
fn confirmation_keeps_the_preview_window_when_other_tasks_age_into_eligibility() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tt.db");
    let old = command(
        &path,
        "2026-01-01T00:00:00Z",
        &["tasks", "create", "Old task"],
    )
    .unwrap();
    let younger = command(
        &path,
        "2026-01-20T00:00:00Z",
        &["tasks", "create", "Younger task"],
    )
    .unwrap();
    let preview_at = "2026-02-01T00:00:00Z";
    let preview = command(&path, preview_at, &["tasks", "preview-inactive"]).unwrap();
    assert_eq!(preview["count"], 1);
    assert_eq!(preview["tasks"][0]["id"], old["id"]);
    let token = preview["preview"].to_string();
    let confirmation_at = "2026-02-05T00:00:00Z";
    let archived = command(
        &path,
        confirmation_at,
        &["tasks", "archive-inactive", "--preview", &token, "--yes"],
    )
    .unwrap();
    assert_eq!(archived["archived_count"], 1);
    let task = command(
        &path,
        confirmation_at,
        &["tasks", "get", old["id"].as_str().unwrap()],
    )
    .unwrap();
    assert_eq!(task["archived"], true);
    assert_eq!(task["updated_at"], preview_at);
    assert_eq!(
        command(
            &path,
            confirmation_at,
            &["tasks", "get", younger["id"].as_str().unwrap()],
        )
        .unwrap()["archived"],
        false
    );
}

#[test]
fn inactive_archiving_preserves_preview_candidates_and_backend_identity() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tt.db");
    for name in ["Old planning", "Old review", "Old release"] {
        command(&path, T0, &["tasks", "create", name]).unwrap();
    }
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

#[test]
fn remote_preview_tokens_reject_invalid_guards_and_candidates_before_connecting() {
    let backend = "remote:http://127.0.0.1:8765/";
    let id = "00000000-0000-7000-8000-000000000001";
    let valid = serde_json::json!({
        "mode":"remote", "backend":backend,
        "preview":{"as_of":T0,"inactive_days":14,"count":1,"candidate_task_ids":[id]}
    });
    let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let validate = |token: &serde_json::Value, identity: &str| {
        let command = crate::args::Tasks::ArchiveInactive {
            preview: token.to_string(),
            yes: true,
        };
        crate::tasks::validate(command, identity, now)
    };
    validate(&valid, backend).unwrap();
    let mut empty = valid.clone();
    empty["preview"]["count"] = serde_json::json!(0);
    empty["preview"]["candidate_task_ids"] = serde_json::json!([]);
    validate(&empty, backend).unwrap();
    for (field, value) in [
        ("inactive_days", serde_json::json!(0)),
        ("as_of", serde_json::json!("2026-01-01T00:00:00.000000001Z")),
        ("candidate_task_ids", serde_json::json!(["invalid"])),
        ("candidate_task_ids", serde_json::json!([id, id])),
        ("candidate_task_ids", serde_json::json!([])),
        ("count", serde_json::json!(0)),
        ("count", serde_json::json!(2)),
    ] {
        let mut invalid = valid.clone();
        invalid["preview"][field] = value;
        let error = validate(&invalid, backend).unwrap_err();
        assert_eq!((error.code, error.exit_code), ("invalid_input", 2));
        assert_eq!(error.message, "invalid inactive task preview");
    }
    let mut weekly = valid.clone();
    weekly["preview"]["inactive_days"] = serde_json::json!(7);
    let error = validate(&weekly, backend).unwrap_err();
    assert_eq!((error.code, error.exit_code), ("invalid_input", 2));
    assert_eq!(error.message, "remote preview must use the 14-day period");
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
