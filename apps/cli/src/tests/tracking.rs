use super::{T0, T1, T2, T3, command};

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
fn zero_duration_stop_and_switch_preserve_tracking_and_history() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tt.db");
    let first = command(&path, T0, &["tasks", "create", "First task"]).unwrap();
    let second = command(&path, T0, &["tasks", "create", "Second task"]).unwrap();
    let first_id = first["id"].as_str().unwrap();
    let second_id = second["id"].as_str().unwrap();
    let started = command(&path, T0, &["tracking", "start", first_id]).unwrap();
    let id = started["worklog"]["id"].as_str().unwrap();
    let before = command(&path, T0, &["tracking", "status"]).unwrap();
    let history = command(&path, T0, &["worklogs", "list"]).unwrap();
    for at in [T0, "2026-01-01T00:00:00.000000999Z"] {
        for arguments in [
            vec!["tracking", "stop", "--expected-active", id],
            vec!["tracking", "start", second_id],
        ] {
            assert!(command(&path, at, &arguments).is_err());
            assert_eq!(command(&path, T0, &["tracking", "status"]).unwrap(), before);
            assert_eq!(command(&path, T0, &["worklogs", "list"]).unwrap(), history);
        }
    }
    command(
        &path,
        "2026-01-01T00:00:00.000001Z",
        &["tracking", "stop", "--expected-active", id],
    )
    .unwrap();
    assert_eq!(
        command(&path, T1, &["worklogs", "list"]).unwrap()["worklogs"][0]["end"],
        "2026-01-01T00:00:00.000001Z"
    );
}
