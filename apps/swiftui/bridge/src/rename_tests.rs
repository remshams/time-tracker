use super::*;
use tracker_application::TaskQueries;

fn response(pointer: *mut c_char) -> Value {
    assert!(!pointer.is_null());
    // SAFETY: This test owns the returned bridge string and releases it once.
    unsafe {
        let value = serde_json::from_str(CStr::from_ptr(pointer).to_str().unwrap()).unwrap();
        tt_bridge_string_free(pointer);
        value
    }
}

fn rename(bridge: &mut Bridge, id: TaskId, name: &str, at: DateTime<Utc>) -> Value {
    let id = CString::new(id.to_string()).unwrap();
    let name = CString::new(name).unwrap();
    let at = CString::new(timestamp(at)).unwrap();
    // SAFETY: All inputs remain live and this test owns exclusive bridge access.
    response(unsafe { tt_bridge_rename_task_at(bridge, id.as_ptr(), name.as_ptr(), at.as_ptr()) })
}

#[test]
fn renaming_active_and_archived_tasks_preserves_ids_tracking_and_worklog_history() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("rename.db");
    let mut bridge = open_at(&path).unwrap();
    let tasks = bridge.application.tasks(TaskOrdering::default());
    let active = tasks[0].task.id();
    let archived = tasks[1].task.id();
    let at = Utc::now();
    assert!(bridge.application.set_active_task(archived, at).is_ok());
    let TrackingState::Running { worklog } = bridge.application.current_tracking() else {
        panic!("tracking should be running")
    };
    let old_worklog = worklog.id();
    assert!(
        bridge
            .application
            .clear_active_task(old_worklog, at + chrono::Duration::seconds(1))
            .is_ok()
    );
    bridge
        .application
        .archive_task(archived, at + chrono::Duration::seconds(2))
        .unwrap();
    assert!(
        bridge
            .application
            .set_active_task(active, at + chrono::Duration::seconds(3))
            .is_ok()
    );
    let before = serde_json::to_value(snapshot(&bridge.application)).unwrap();
    let active_history = bridge
        .application
        .worklogs_for_task(active, None)
        .unwrap()
        .worklogs;
    let archived_history = bridge
        .application
        .worklogs_for_task(archived, None)
        .unwrap()
        .worklogs;
    let renamed_at = at + chrono::Duration::seconds(4);
    for (id, expected_archived) in [(active, false), (archived, true)] {
        let renamed = rename(&mut bridge, id, "  Shared name 🛠  ", renamed_at);
        assert!(renamed.get("error").is_none(), "{renamed}");
        assert_eq!(renamed["data"]["active"], before["active"]);
        assert_eq!(
            renamed["data"]["tasks"].as_array().unwrap().len(),
            tasks.len()
        );
        let item = renamed["data"]["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["id"] == id.to_string())
            .unwrap();
        assert_eq!(item["name"], "Shared name 🛠");
        assert_eq!(item["archived"], expected_archived);
        let old = before["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["id"] == id.to_string())
            .unwrap();
        assert_eq!(item["latestStart"], old["latestStart"]);
    }
    assert_eq!(
        bridge
            .application
            .worklogs_for_task(active, None)
            .unwrap()
            .worklogs,
        active_history
    );
    assert_eq!(
        bridge
            .application
            .worklogs_for_task(archived, None)
            .unwrap()
            .worklogs,
        archived_history
    );
    drop(bridge);
    let stored = TrackerApplication::load(SqliteRepository::open(path).unwrap()).unwrap();
    for id in [active, archived] {
        let task = stored.task(id).unwrap();
        assert_eq!(task.name().as_str(), "Shared name 🛠");
        assert_eq!(
            task.created_at(),
            tasks
                .iter()
                .find(|item| item.task.id() == id)
                .unwrap()
                .task
                .created_at()
        );
        assert_eq!(
            task.updated_at(),
            DateTime::from_timestamp_micros(renamed_at.timestamp_micros()).unwrap()
        );
    }
}

#[test]
fn rename_validates_task_names_and_missing_ids_without_changes() {
    let directory = tempfile::tempdir().unwrap();
    let mut bridge = open_at(&directory.path().join("validation.db")).unwrap();
    let before = serde_json::to_value(snapshot(&bridge.application)).unwrap();
    let id = bridge.application.tasks(TaskOrdering::default())[0]
        .task
        .id();
    for (name, expected) in [
        (" ".to_owned(), "task name must not be empty"),
        (
            "Task\nname".to_owned(),
            "task name must not contain control characters",
        ),
        ("🛠".repeat(257), "task name must be at most 256 characters"),
    ] {
        let failure = rename(&mut bridge, id, &name, Utc::now());
        assert_eq!(failure["error"], expected);
        assert_eq!(failure["kind"], "general");
        assert_eq!(failure["uncertain"], false);
    }
    assert_eq!(
        rename(&mut bridge, TaskId::generate(), "Missing", Utc::now())["error"],
        "Task not found"
    );
    assert_eq!(
        serde_json::to_value(snapshot(&bridge.application)).unwrap(),
        before
    );
    assert!(
        rename(&mut bridge, id, &"🛠".repeat(256), Utc::now())
            .get("error")
            .is_none()
    );
}

#[test]
fn rename_rejects_null_and_invalid_c_inputs() {
    let directory = tempfile::tempdir().unwrap();
    let mut bridge = open_at(&directory.path().join("inputs.db")).unwrap();
    let before = serde_json::to_value(snapshot(&bridge.application)).unwrap();
    let id = CString::new(
        bridge.application.tasks(TaskOrdering::default())[0]
            .task
            .id()
            .to_string(),
    )
    .unwrap();
    let name = CString::new("Task").unwrap();
    let at = CString::new(timestamp(Utc::now())).unwrap();
    let invalid = CString::new("invalid").unwrap();
    let non_utf8 = CString::new(vec![0xff]).unwrap();
    // SAFETY: Inputs are live C strings or null, and bridge access is exclusive.
    unsafe {
        assert_eq!(
            response(tt_bridge_rename_task_at(
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                ptr::null()
            ))["error"],
            "Database is not open"
        );
        for bad in [ptr::null(), invalid.as_ptr(), non_utf8.as_ptr()] {
            assert_eq!(
                response(tt_bridge_rename_task_at(
                    &mut bridge,
                    bad,
                    name.as_ptr(),
                    at.as_ptr()
                ))["error"],
                "Invalid task ID"
            );
            assert_eq!(
                response(tt_bridge_rename_task_at(
                    &mut bridge,
                    id.as_ptr(),
                    name.as_ptr(),
                    bad
                ))["error"],
                "Invalid rename timestamp"
            );
        }
        for bad in [ptr::null(), non_utf8.as_ptr()] {
            assert_eq!(
                response(tt_bridge_rename_task_at(
                    &mut bridge,
                    id.as_ptr(),
                    bad,
                    at.as_ptr()
                ))["error"],
                "Invalid task name"
            );
        }
    }
    assert_eq!(
        serde_json::to_value(snapshot(&bridge.application)).unwrap(),
        before
    );
}
