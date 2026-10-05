use super::*;
use tracker_application::TaskQueries;

fn response(pointer: *mut c_char) -> Value {
    assert!(!pointer.is_null());
    // SAFETY: The bridge owns this string until this test releases it once.
    unsafe {
        let value = serde_json::from_str(CStr::from_ptr(pointer).to_str().unwrap()).unwrap();
        tt_bridge_string_free(pointer);
        value
    }
}

fn create(bridge: &mut Bridge, name: &str, at: &str) -> Value {
    let name = CString::new(name).unwrap();
    let at = CString::new(at).unwrap();
    // SAFETY: The bridge is exclusively accessed and both strings remain live.
    response(unsafe { tt_bridge_create_task_at(bridge, name.as_ptr(), at.as_ptr()) })
}

#[test]
fn local_creation_returns_a_permanent_id_and_snapshot_without_changing_tracking() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("creation.db");
    let mut bridge = open_fixture(&path).unwrap();
    let first = bridge.application.tasks(TaskOrdering::default())[0]
        .task
        .id();
    let at: DateTime<Utc> = "2026-10-05T08:00:00.123456Z".parse().unwrap();
    assert!(bridge.application.set_active_task(first, at).is_ok());
    let before = serde_json::to_value(snapshot(&bridge.application)).unwrap();
    let created = create(&mut bridge, "  Plan release 🛠  ", &timestamp(at));
    assert!(created.get("error").is_none(), "{created}");
    let id: TaskId = created["data"]["taskId"].as_str().unwrap().parse().unwrap();
    assert_eq!(id.as_uuid().get_version_num(), 7);
    assert_eq!(created["data"]["snapshot"]["active"], before["active"]);
    let tasks = created["data"]["snapshot"]["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), before["tasks"].as_array().unwrap().len() + 1);
    let task = tasks
        .iter()
        .find(|task| task["id"] == id.to_string())
        .unwrap();
    assert_eq!(task["name"], "Plan release 🛠");
    assert_eq!(task["archived"], false);
    assert!(task["latestStart"].is_null());
    let repeated_name = create(&mut bridge, "Plan release 🛠", &timestamp(at));
    assert_ne!(repeated_name["data"]["taskId"], created["data"]["taskId"]);
    drop(bridge);
    let application = TrackerApplication::load(SqliteRepository::open(path).unwrap()).unwrap();
    let stored = application.task(id).unwrap();
    assert_eq!(stored.name().as_str(), "Plan release 🛠");
    assert_eq!(stored.created_at(), at);
    assert_eq!(stored.updated_at(), at);
}

#[test]
fn task_name_boundaries_are_validated_by_the_domain_without_writes() {
    let directory = tempfile::tempdir().unwrap();
    let mut bridge = open_fixture(&directory.path().join("validation.db")).unwrap();
    let count = bridge.application.tasks(TaskOrdering::default()).len();
    for (name, message) in [
        ("   ".to_owned(), "task name must not be empty"),
        (
            "Task\nname".to_owned(),
            "task name must not contain control characters",
        ),
        ("🛠".repeat(257), "task name must be at most 256 characters"),
    ] {
        let rejected = create(&mut bridge, &name, "2026-10-05T08:00:00Z");
        assert_eq!(rejected["error"], message);
        assert_eq!(rejected["kind"], "general");
        assert_eq!(rejected["uncertain"], false);
    }
    assert_eq!(
        bridge.application.tasks(TaskOrdering::default()).len(),
        count
    );
    let accepted = create(&mut bridge, &"🛠".repeat(256), "2026-10-05T08:00:00Z");
    assert!(accepted.get("error").is_none(), "{accepted}");
}

#[test]
fn creation_rejects_null_and_invalid_c_inputs_without_writes() {
    let directory = tempfile::tempdir().unwrap();
    let mut bridge = open_fixture(&directory.path().join("inputs.db")).unwrap();
    let count = bridge.application.tasks(TaskOrdering::default()).len();
    let name = CString::new("Task").unwrap();
    let invalid = CString::new("invalid").unwrap();
    let non_utf8 = CString::new(vec![0xff]).unwrap();
    let valid_at = CString::new("2026-10-05T08:00:00Z").unwrap();
    // SAFETY: Inputs are null or live C strings, and bridge access is exclusive.
    unsafe {
        assert_eq!(
            response(tt_bridge_create_task_at(
                ptr::null_mut(),
                ptr::null(),
                ptr::null()
            ))["error"],
            "Database is not open"
        );
        for bad in [ptr::null(), non_utf8.as_ptr()] {
            assert_eq!(
                response(tt_bridge_create_task_at(
                    &mut bridge,
                    bad,
                    valid_at.as_ptr()
                ))["error"],
                "Invalid task name"
            );
        }
        for bad in [ptr::null(), invalid.as_ptr(), non_utf8.as_ptr()] {
            assert_eq!(
                response(tt_bridge_create_task_at(&mut bridge, name.as_ptr(), bad))["error"],
                "Invalid creation timestamp"
            );
        }
    }
    assert_eq!(
        bridge.application.tasks(TaskOrdering::default()).len(),
        count
    );
}
