pub(super) fn test_task_values(application: &Backend) -> Value {
    serde_json::to_value(tasks_json(application).map(|resource| resource.value)).unwrap()
}

pub(super) fn test_active_value(application: &Backend) -> Value {
    serde_json::to_value(tracking_json(application).and_then(|resource| resource.value)).unwrap()
}

pub(super) fn test_resource_values(application: &Backend) -> (Value, Value) {
    (
        test_task_values(application),
        test_active_value(application),
    )
}

/// Returns the name the bridge cache holds for `id`, or a placeholder for unknown tasks.
pub(super) fn cached_task_name(application: &Backend, id: TaskId) -> TaskName {
    application
        .tasks(TaskOrdering::default())
        .into_iter()
        .find(|item| item.task.id() == id)
        .map_or_else(
            || TaskName::new("Missing task").unwrap(),
            |item| item.task.name().clone(),
        )
}

/// Archives a fixture task under the name the bridge last read for it.
pub(super) fn archive_cached_task(
    application: &mut Backend,
    id: TaskId,
    at: DateTime<Utc>,
) -> Result<tracker_domain::Task, backend::BridgeError> {
    let name = cached_task_name(application, id);
    application.archive_task_with_expected_name(id, name, at)
}

use super::*;

#[test]
fn opening_without_error_storage_does_not_open_a_database() {
    // SAFETY: Null storage is explicitly accepted and prevents the opener from running.
    let bridge = unsafe {
        open_bridge(
            ptr::null_mut(),
            || panic!("opener must not run"),
            c"Fallback",
        )
    };
    assert!(bridge.is_null());
    // SAFETY: The public opener accepts null error storage.
    assert!(unsafe { tt_bridge_open(ptr::null_mut()) }.is_null());
}

#[test]
fn opening_clears_error_storage_and_returns_an_owned_bridge() {
    let directory = tempfile::tempdir().unwrap();
    let sentinel = c"Old error".as_ptr().cast_mut();
    let mut error = sentinel;
    // SAFETY: Error storage is writable and the returned bridge is released once.
    let bridge = unsafe {
        open_bridge(
            &mut error,
            || open_at(&directory.path().join("tt.db")),
            c"Fallback",
        )
    };
    assert!(!bridge.is_null());
    assert!(error.is_null());
    // SAFETY: The helper returns ownership of a live bridge.
    unsafe { tt_bridge_close(bridge) };
}

#[test]
fn opening_failure_returns_an_owned_error_string() {
    for (message, expected) in [
        ("Database failure", "Database failure"),
        ("Bad\0message", "Fallback"),
    ] {
        let mut error = ptr::null_mut();
        // SAFETY: Error storage is writable and the returned string is released once.
        let bridge = unsafe { open_bridge(&mut error, || Err(message.to_owned()), c"Fallback") };
        assert!(bridge.is_null());
        assert!(!error.is_null());
        // SAFETY: A failed opener returns a live C string.
        assert_eq!(unsafe { CStr::from_ptr(error) }.to_str().unwrap(), expected);
        // SAFETY: The caller owns this string and releases it once.
        unsafe { tt_bridge_string_free(error) };
    }
}

fn response(pointer: *mut c_char) -> Value {
    assert!(!pointer.is_null());
    // SAFETY: Tests pass strings returned by the bridge and release each once.
    unsafe {
        let value = serde_json::from_str(CStr::from_ptr(pointer).to_str().unwrap()).unwrap();
        tt_bridge_string_free(pointer);
        value
    }
}

fn start_tracking(bridge: &mut Bridge, task_id: TaskId) -> Value {
    let task_id = CString::new(task_id.to_string()).unwrap();
    // SAFETY: Both the bridge and task ID are live and uniquely accessed.
    response(unsafe { tt_bridge_start_tracking(bridge, task_id.as_ptr()) })
}

fn stop_tracking(bridge: &mut Bridge, worklog_id: &str) -> Value {
    let worklog_id = CString::new(worklog_id).unwrap();
    // SAFETY: Both the bridge and worklog ID are live and uniquely accessed.
    response(unsafe { tt_bridge_stop_tracking(bridge, worklog_id.as_ptr()) })
}

#[test]
fn tracking_commands_persist_one_worklog_and_repeated_commands_are_noops() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tt.db");
    let mut bridge = open_fixture(&path).unwrap();
    let task_id = bridge.application.tasks(TaskOrdering::default())[0]
        .task
        .id();

    let started = start_tracking(&mut bridge, task_id);
    assert!(started.get("error").is_none(), "{started}");
    assert_eq!(started["data"]["active"]["taskId"], task_id.to_string());
    assert!(started["data"]["active"]["end"].is_null());
    let worklog_id = started["data"]["active"]["id"].as_str().unwrap();
    let start = started["data"]["active"]["start"].as_str().unwrap();
    let observed_tasks = test_task_values(&bridge.application);
    let task = observed_tasks
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["id"] == task_id.to_string())
        .unwrap();
    assert_eq!(task["latestStart"], start);

    let repeated = start_tracking(&mut bridge, task_id);
    assert_eq!(repeated, started);

    let stopped = stop_tracking(&mut bridge, worklog_id);
    assert!(stopped.get("error").is_none(), "{stopped}");
    assert!(stopped["data"]["active"].is_null());
    let repeated_stop = stop_tracking(&mut bridge, worklog_id);
    assert!(repeated_stop["data"]["active"].is_null());
    assert_eq!(repeated_stop["data"]["didStop"], false);
    assert!(repeated_stop["data"]["stopped"].is_null());
    drop(bridge);

    let mut reopened = open_fixture(&path).unwrap();
    assert!(matches!(
        reopened.application.current_tracking(),
        TrackingState::Idle
    ));
    let page = reopened
        .application
        .worklogs_for_task(task_id, None)
        .unwrap();
    assert_eq!(page.worklogs.len(), 1);
    assert_eq!(page.worklogs[0].id().to_string(), worklog_id);
    assert_eq!(timestamp(page.worklogs[0].start()), start);
    assert!(page.worklogs[0].end().unwrap() >= page.worklogs[0].start());
}

#[test]
fn switching_through_the_bridge_stops_and_starts_at_the_same_instant() {
    let directory = tempfile::tempdir().unwrap();
    let mut bridge = open_fixture(&directory.path().join("tt.db")).unwrap();
    let tasks = bridge.application.tasks(TaskOrdering::default());
    let first = tasks[0].task.id();
    let second = tasks[1].task.id();
    let started = start_tracking(&mut bridge, first);
    let switched = start_tracking(&mut bridge, second);
    assert!(switched.get("error").is_none(), "{switched}");
    assert_eq!(switched["data"]["active"]["taskId"], second.to_string());
    assert_ne!(
        switched["data"]["active"]["id"],
        started["data"]["active"]["id"]
    );

    let old_page = bridge.application.worklogs_for_task(first, None).unwrap();
    assert_eq!(old_page.worklogs.len(), 1);
    assert_eq!(
        timestamp(old_page.worklogs[0].end().unwrap()),
        switched["data"]["active"]["start"].as_str().unwrap()
    );
    let new_page = bridge.application.worklogs_for_task(second, None).unwrap();
    assert_eq!(new_page.worklogs.len(), 1);
    assert!(new_page.worklogs[0].end().is_none());
}

#[test]
fn stale_stop_does_not_stop_another_clients_new_timer() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tt.db");
    let mut first = open_fixture(&path).unwrap();
    let tasks = first.application.tasks(TaskOrdering::default());
    let started = start_tracking(&mut first, tasks[0].task.id());
    let expected_id = started["data"]["active"]["id"].as_str().unwrap();
    let mut second = open_fixture(&path).unwrap();
    let switched = start_tracking(&mut second, tasks[1].task.id());

    let rejected = stop_tracking(&mut first, expected_id);
    assert_eq!(
        rejected["error"],
        "Tracking state changed in another client. Refreshed state."
    );
    assert_eq!(
        test_active_value(&first.application),
        switched["data"]["active"]
    );
    let page = second
        .application
        .worklogs_for_task(tasks[1].task.id(), None)
        .unwrap();
    assert_eq!(page.worklogs.len(), 1);
    assert!(page.worklogs[0].end().is_none());
}

#[test]
fn start_rejects_missing_and_newly_archived_tasks_without_switching() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tt.db");
    let mut bridge = open_fixture(&path).unwrap();
    let tasks = bridge.application.tasks(TaskOrdering::default());
    let started = start_tracking(&mut bridge, tasks[0].task.id());
    let mut other = open_fixture(&path).unwrap();
    archive_cached_task(&mut other.application, tasks[1].task.id(), Utc::now()).unwrap();

    let archived = start_tracking(&mut bridge, tasks[1].task.id());
    assert_eq!(
        archived["error"],
        format!("task {} is archived", tasks[1].task.id())
    );
    assert_eq!(
        start_tracking(&mut bridge, TaskId::generate())["error"],
        "Task not found"
    );
    assert_eq!(
        test_active_value(&bridge.application),
        started["data"]["active"]
    );
}

#[test]
fn tracking_commands_reject_null_bridges_and_invalid_identifiers() {
    let directory = tempfile::tempdir().unwrap();
    let mut bridge = open_fixture(&directory.path().join("tt.db")).unwrap();
    let malformed = CString::new("not an ID").unwrap();
    let non_utf8 = CString::new(vec![0xff]).unwrap();
    // SAFETY: Null bridges are supported, and no identifiers are dereferenced.
    unsafe {
        assert_eq!(
            response(tt_bridge_start_tracking(ptr::null_mut(), ptr::null()))["error"],
            "Database is not open"
        );
        assert_eq!(
            response(tt_bridge_stop_tracking(ptr::null_mut(), ptr::null()))["error"],
            "Database is not open"
        );
    }
    for identifier in [ptr::null(), malformed.as_ptr(), non_utf8.as_ptr()] {
        // SAFETY: The bridge is uniquely accessed; inputs are null or live C strings.
        unsafe {
            assert_eq!(
                response(tt_bridge_start_tracking(&mut bridge, identifier))["error"],
                "Invalid task ID"
            );
            assert_eq!(
                response(tt_bridge_stop_tracking(&mut bridge, identifier))["error"],
                "Invalid worklog ID"
            );
        }
    }
    assert!(matches!(
        bridge.application.current_tracking(),
        TrackingState::Idle
    ));
}

#[test]
fn history_cursor_rejects_incomplete_input() {
    assert!(parse_cursor("{}").is_err());
    assert!(parse_cursor("not json").is_err());
}

#[test]
fn history_cursor_round_trips_page_position() {
    let id = WorklogId::generate();
    for revision in [0, 7] {
        let text = json!({"id": id.to_string(), "revision": revision}).to_string();
        let cursor = parse_cursor(&text).unwrap();
        assert_eq!(cursor.id, id);
        assert_eq!(cursor.revision, revision);
    }
}

#[test]
fn history_cursor_rejects_invalid_and_legacy_fields() {
    let id = WorklogId::generate();
    for value in [
        json!({"id": id.to_string()}),
        json!({"revision": 0}),
        json!({"id": "invalid", "revision": 0}),
        json!({"id": id.to_string(), "revision": -1}),
        json!({"id": id.to_string(), "revision": 0, "start": "2026-01-01T00:00:00Z"}),
        json!({"id": id.to_string(), "revision": 0, "task_id": TaskId::generate().to_string()}),
    ] {
        assert!(parse_cursor(&value.to_string()).is_err(), "{value}");
    }
}

#[test]
fn task_resources_expose_explicit_tasks_in_the_swift_schema() {
    let directory = tempfile::tempdir().unwrap();
    let bridge = open_fixture(&directory.path().join("tt.db")).unwrap();

    let value = test_resource_values(&bridge.application);
    let tasks = value.0.as_array().unwrap();
    assert_eq!(tasks.len(), 3);
    assert_eq!(value.1, Value::Null);
    assert!(tasks.iter().all(|task| {
        task["id"].is_string()
            && task["name"].is_string()
            && task["archived"] == false
            && task["latestStart"].is_null()
    }));
}

#[test]
fn opening_a_database_starts_empty_and_preserves_existing_tasks() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tt.db");
    let mut first = open_at(&path).unwrap();
    assert!(tasks_json(&first.application).unwrap().value.is_empty());
    let task = first
        .application
        .create_task(TaskName::new("Saved project").unwrap(), Utc::now())
        .unwrap_or_else(|error| panic!("{}", error.message));
    let before = test_resource_values(&first.application);
    drop(first);

    let second = open_at(&path).unwrap();
    assert_eq!(test_resource_values(&second.application), before);
    assert_eq!(
        tasks_json(&second.application).unwrap().value[0].id,
        task.id().to_string()
    );
}

#[test]
fn history_bridge_returns_bounded_pages_without_repeating_rows() {
    let repository = SqliteRepository::open_in_memory().unwrap();
    let mut application = TrackerApplication::load(repository).unwrap();
    let start = DateTime::from_timestamp(1_000, 0).unwrap();
    let task = application
        .create_task(TaskName::new("Review backlog").unwrap(), start)
        .unwrap();
    for index in 0..51 {
        let at = start + chrono::Duration::seconds(index * 10);
        application.set_active_task(task.id(), at).unwrap();
        let active_id = match application.current_tracking() {
            TrackingState::Running { worklog } => worklog.id(),
            TrackingState::Idle => panic!("tracking should be running"),
        };
        application
            .clear_active_task(active_id, at + chrono::Duration::seconds(1))
            .unwrap();
    }

    let bridge = Box::into_raw(Box::new(Bridge::new(Backend::local(application).unwrap())));
    let task_id = CString::new(task.id().to_string()).unwrap();
    // SAFETY: The bridge and input string are live for both calls.
    let first_ptr = unsafe { tt_bridge_history(bridge, task_id.as_ptr(), ptr::null()) };
    // SAFETY: The bridge returns a valid C string.
    let first: Value =
        serde_json::from_str(unsafe { CStr::from_ptr(first_ptr) }.to_str().unwrap()).unwrap();
    let first_rows = first["data"]["worklogs"].as_array().unwrap();
    assert_eq!(first_rows.len(), 50);
    let cursor = CString::new(first["data"]["nextCursor"].as_str().unwrap()).unwrap();
    // SAFETY: The bridge and both input strings are live for this call.
    let second_ptr = unsafe { tt_bridge_history(bridge, task_id.as_ptr(), cursor.as_ptr()) };
    // SAFETY: The bridge returns a valid C string.
    let second: Value =
        serde_json::from_str(unsafe { CStr::from_ptr(second_ptr) }.to_str().unwrap()).unwrap();
    let second_rows = second["data"]["worklogs"].as_array().unwrap();
    assert_eq!(second_rows.len(), 1);
    assert_ne!(first_rows[49]["id"], second_rows[0]["id"]);
    assert!(second["data"]["nextCursor"].is_null());
    assert_eq!(second["data"]["reset"], false);
    // SAFETY: Each pointer is released once after its final use.
    unsafe {
        tt_bridge_string_free(first_ptr);
        tt_bridge_string_free(second_ptr);
        tt_bridge_close(bridge);
    }
}

#[test]
fn null_bridge_returns_an_error_envelope() {
    // SAFETY: The null pointer is explicitly supported by the function.
    let response = unsafe { tt_bridge_tasks(ptr::null_mut()) };
    // SAFETY: The bridge returns a valid C string.
    let text = unsafe { CStr::from_ptr(response) }.to_str().unwrap();
    assert!(text.contains("Database is not open"));
    // SAFETY: The bridge owns the string until it is freed once.
    unsafe { tt_bridge_string_free(response) };
}

#[test]
fn zero_duration_tracking_commands_preserve_bridge_resources_and_history() {
    let directory = tempfile::tempdir().unwrap();
    let mut bridge = open_fixture(&directory.path().join("tt.db")).unwrap();
    let tasks = bridge.application.tasks(TaskOrdering::default());
    let first = CString::new(tasks[0].task.id().to_string()).unwrap();
    let second = CString::new(tasks[1].task.id().to_string()).unwrap();
    let start = c"2026-01-01T00:00:00Z";
    // SAFETY: The bridge and timestamp strings stay live for these calls.
    let started = response(unsafe {
        tt_bridge_start_tracking_at(&mut bridge, first.as_ptr(), start.as_ptr())
    });
    let id = CString::new(started["data"]["active"]["id"].as_str().unwrap()).unwrap();
    let before = test_resource_values(&bridge.application);
    for at in [start, c"2026-01-01T00:00:00.000000999Z"] {
        // SAFETY: Each call has exclusive bridge access and live C strings.
        let stopped =
            response(unsafe { tt_bridge_stop_tracking_at(&mut bridge, id.as_ptr(), at.as_ptr()) });
        assert!(stopped["error"].is_string(), "{stopped}");
        assert_eq!(test_resource_values(&bridge.application), before);
        // SAFETY: The bridge and input strings remain live.
        let switched = response(unsafe {
            tt_bridge_start_tracking_at(&mut bridge, second.as_ptr(), at.as_ptr())
        });
        assert!(switched["error"].is_string(), "{switched}");
        assert_eq!(test_resource_values(&bridge.application), before);
        let first_history = bridge
            .application
            .worklogs_for_task(tasks[0].task.id(), None)
            .unwrap();
        assert_eq!(first_history.worklogs.len(), 1);
        assert!(first_history.worklogs[0].is_active());
        assert!(
            bridge
                .application
                .worklogs_for_task(tasks[1].task.id(), None)
                .unwrap()
                .worklogs
                .is_empty()
        );
    }
    // SAFETY: The bridge and all C strings remain valid.
    let stopped = response(unsafe {
        tt_bridge_stop_tracking_at(
            &mut bridge,
            id.as_ptr(),
            c"2026-01-01T00:00:00.000001Z".as_ptr(),
        )
    });
    assert_eq!(stopped["data"]["didStop"], true);
}
