use super::{T0, T1, T2, T3, command};

#[test]
fn tasks_start_empty_and_support_create_rename_archive_restore_and_search() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tt.db");
    for _ in 0..2 {
        let empty = command(&path, T0, &["tasks", "list"]).unwrap();
        assert_eq!(empty["tasks"], serde_json::json!([]));
    }
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
        1
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
fn task_reads_include_the_latest_start_for_running_and_completed_work() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tt.db");
    let tracked = command(&path, T0, &["tasks", "create", "Tracked"]).unwrap();
    let untouched = command(&path, T0, &["tasks", "create", "Untouched"]).unwrap();
    let tracked_id = tracked["id"].as_str().unwrap();
    let untouched_id = untouched["id"].as_str().unwrap();
    let started = command(&path, T1, &["tracking", "start", tracked_id]).unwrap();
    assert_eq!(
        command(&path, T2, &["tasks", "get", tracked_id]).unwrap()["latest_work_start"],
        T1
    );
    assert!(
        command(&path, T2, &["tasks", "get", untouched_id]).unwrap()["latest_work_start"].is_null()
    );
    let worklog_id = started["worklog"]["id"].as_str().unwrap();
    command(
        &path,
        T2,
        &["tracking", "stop", "--expected-active", worklog_id],
    )
    .unwrap();
    assert_eq!(
        command(&path, T3, &["tasks", "get", tracked_id]).unwrap()["latest_work_start"],
        T1
    );
    command(&path, T3, &["tracking", "start", tracked_id]).unwrap();
    assert_eq!(
        command(&path, T3, &["tasks", "get", tracked_id]).unwrap()["latest_work_start"],
        T3
    );
}
