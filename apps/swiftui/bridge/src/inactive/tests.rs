use super::*;
use chrono::Duration;
use tracker_application::{TaskOperations, TrackingOperations};

fn response(pointer: *mut c_char) -> Value {
    assert!(!pointer.is_null());
    // SAFETY: The test owns this bridge string and releases it once.
    unsafe {
        let value = serde_json::from_str(CStr::from_ptr(pointer).to_str().unwrap()).unwrap();
        tt_bridge_string_free(pointer);
        value
    }
}

fn preview(bridge: &mut Bridge, days: u32, as_of: DateTime<Utc>) -> Value {
    let instant = CString::new(timestamp(as_of)).unwrap();
    // SAFETY: The bridge and string remain live with exclusive access.
    response(unsafe { tt_bridge_preview_inactive_tasks_at(bridge, days, instant.as_ptr()) })
}

fn archive(bridge: &mut Bridge, days: u32, as_of: DateTime<Utc>) -> Value {
    let instant = CString::new(timestamp(as_of)).unwrap();
    // SAFETY: The bridge and string remain live with exclusive access.
    response(unsafe { tt_bridge_archive_inactive_tasks_at(bridge, days, instant.as_ptr()) })
}

#[test]
fn local_preview_uses_chosen_period_and_archive_preserves_running_work() {
    let repository = SqliteRepository::open_in_memory().unwrap();
    let mut application = TrackerApplication::load(repository).unwrap();
    let as_of = Utc::now();
    let old = application
        .create_task(
            TaskName::new("Old task").unwrap(),
            as_of - Duration::days(40),
        )
        .unwrap();
    let medium = application
        .create_task(
            TaskName::new("Medium task").unwrap(),
            as_of - Duration::days(10),
        )
        .unwrap();
    let recent = application
        .create_task(
            TaskName::new("Recent task").unwrap(),
            as_of - Duration::days(1),
        )
        .unwrap();
    let running = application
        .create_task(
            TaskName::new("Running task").unwrap(),
            as_of - Duration::days(40),
        )
        .unwrap();
    application
        .set_active_task(running.id(), as_of - Duration::hours(1))
        .unwrap();
    let mut bridge = Bridge::new(Backend::Local(application));
    let week = preview(&mut bridge, 7, as_of);
    let tasks = week["data"]["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 2);
    assert!(
        tasks
            .iter()
            .any(|task| task["id"] == medium.id().to_string())
    );
    assert!(
        tasks
            .iter()
            .all(|task| task["archived"] == false && task["latestStart"].is_null())
    );
    let fortnight = preview(&mut bridge, 14, as_of);
    assert_eq!(fortnight["data"]["asOf"], timestamp(canonical(as_of)));
    assert_eq!(fortnight["data"]["inactiveDays"], 14);
    assert_eq!(fortnight["data"]["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(fortnight["data"]["tasks"][0]["id"], old.id().to_string());
    let result = archive(&mut bridge, 14, as_of);
    assert_eq!(result["data"]["archivedCount"], 1);
    let observations = crate::tests::test_resource_values(&bridge.application);
    assert_eq!(observations.1["taskId"], running.id().to_string());
    for (id, expected) in [
        (old.id(), true),
        (medium.id(), false),
        (recent.id(), false),
        (running.id(), false),
    ] {
        assert_eq!(
            observations
                .0
                .as_array()
                .unwrap()
                .iter()
                .find(|task| task["id"] == id.to_string())
                .unwrap()["archived"],
            expected
        );
    }
    assert_eq!(
        archive(&mut bridge, 14, as_of)["error"],
        "Load an archive preview before confirming"
    );
}

#[test]
fn local_context_mismatch_invalidates_confirmation_and_preview_is_handle_specific() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("context.db");
    let as_of = Utc::now();
    let mut bridge = open_fixture(&path).unwrap();
    let mut second = open_at(&path).unwrap();
    assert!(archive(&mut second, 14, as_of).get("error").is_some());
    for (days, when) in [(7, as_of), (14, as_of + Duration::seconds(1))] {
        assert!(preview(&mut bridge, 14, as_of).get("error").is_none());
        assert_eq!(
            archive(&mut bridge, days, when)["error"],
            "Archive preview changed. Load a new preview before confirming"
        );
        assert_eq!(
            archive(&mut bridge, 14, as_of)["error"],
            "Load an archive preview before confirming"
        );
    }
    assert!(
        bridge
            .application
            .tasks(TaskOrdering::default())
            .iter()
            .all(|item| !item.task.is_archived())
    );
}

#[test]
fn context_compares_utc_microseconds_across_offsets_and_submicrosecond_inputs() {
    let directory = tempfile::tempdir().unwrap();
    let mut bridge = open_fixture(&directory.path().join("precision.db")).unwrap();
    let as_of = c"2026-10-07T19:22:38.123456789+02:00";
    let confirmation = c"2026-10-07T17:22:38.123456Z";
    // SAFETY: The bridge and both strings remain live with exclusive access.
    unsafe {
        let selected = response(tt_bridge_preview_inactive_tasks_at(
            &mut bridge,
            14,
            as_of.as_ptr(),
        ));
        assert_eq!(selected["data"]["asOf"], "2026-10-07T17:22:38.123456Z");
        let result = response(tt_bridge_archive_inactive_tasks_at(
            &mut bridge,
            14,
            confirmation.as_ptr(),
        ));
        assert_eq!(result["data"]["archivedCount"], 3);
    }
}

#[test]
fn malformed_inputs_clear_retained_preview_without_changes() {
    let directory = tempfile::tempdir().unwrap();
    let mut bridge = open_fixture(&directory.path().join("inputs.db")).unwrap();
    let as_of = Utc::now();
    let valid = CString::new(timestamp(as_of)).unwrap();
    let invalid = c"invalid";
    let non_utf8 = CString::new(vec![0xff]).unwrap();
    for operation in [
        tt_bridge_preview_inactive_tasks_at,
        tt_bridge_archive_inactive_tasks_at,
    ] {
        // SAFETY: All pointers are null or live C strings and access is exclusive.
        unsafe {
            assert_eq!(
                response(operation(ptr::null_mut(), 14, valid.as_ptr()))["error"],
                "Database is not open"
            );
            for pointer in [ptr::null(), invalid.as_ptr(), non_utf8.as_ptr()] {
                assert!(preview(&mut bridge, 14, as_of).get("error").is_none());
                assert_eq!(
                    response(operation(&mut bridge, 14, pointer))["error"],
                    "Invalid archive preview timestamp"
                );
                assert!(bridge.inactive_preview.is_none());
            }
            assert!(preview(&mut bridge, 14, as_of).get("error").is_none());
            assert_eq!(
                response(operation(&mut bridge, 0, valid.as_ptr()))["error"],
                "inactivity period must be at least one day"
            );
            assert!(bridge.inactive_preview.is_none());
        }
    }
    assert!(
        bridge
            .application
            .tasks(TaskOrdering::default())
            .iter()
            .all(|item| !item.task.is_archived())
    );
}

#[test]
fn local_confirmation_rechecks_candidates_and_consumes_failed_preview() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("changed.db");
    let mut bridge = open_fixture(&path).unwrap();
    let as_of = Utc::now();
    let selected = preview(&mut bridge, 14, as_of);
    let id: TaskId = selected["data"]["tasks"][0]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let mut writer = TrackerApplication::load(SqliteRepository::open(&path).unwrap()).unwrap();
    writer
        .rename_task(id, TaskName::new("Recently renamed").unwrap(), as_of)
        .unwrap();
    let failure = archive(&mut bridge, 14, as_of);
    assert!(failure.get("error").is_some(), "{failure}");
    assert_eq!(failure["uncertain"], false);
    assert_eq!(failure["requiresRefresh"], true);
    assert_eq!(
        archive(&mut bridge, 14, as_of)["error"],
        "Load an archive preview before confirming"
    );
    assert!(
        bridge
            .application
            .tasks(TaskOrdering::default())
            .iter()
            .all(|item| !item.task.is_archived())
    );
}

#[test]
fn remote_preview_and_archive_use_server_candidates_and_captured_revision() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("remote.db");
    let mut writer = open_fixture(&path).unwrap();
    let id = writer.application.tasks(TaskOrdering::default())[0]
        .task
        .id();
    let as_of = Utc::now();
    writer
        .application
        .set_active_task(id, as_of - Duration::hours(1))
        .unwrap();
    let server = super::super::report_tests::Server::start(
        tracker_server::router_for_database(&path).unwrap(),
    );
    let mut bridge = server.client();
    bridge.application.refresh_resources(3).unwrap();
    let selected = preview(&mut bridge, 7, as_of);
    assert_eq!(selected["data"]["tasks"].as_array().unwrap().len(), 2);
    assert!(
        selected["data"]["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|task| task["id"] != id.to_string())
    );
    // Polling a snapshot does not replace the retained preview's revision.
    bridge.application.refresh_resources(3).unwrap();
    let result = archive(&mut bridge, 7, as_of);
    assert_eq!(result["data"]["archivedCount"], 2, "{result}");
    assert_eq!(
        crate::tests::test_active_value(&bridge.application)["taskId"],
        id.to_string()
    );
    assert_eq!(
        archive(&mut bridge, 7, as_of)["error"],
        "Load an archive preview before confirming"
    );
}

#[test]
fn remote_revision_conflict_is_not_hidden_by_polling_and_blocks_another_write() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("conflict.db");
    let _writer = open_fixture(&path).unwrap();
    let server = super::super::report_tests::Server::start(
        tracker_server::router_for_database(&path).unwrap(),
    );
    let mut first = server.client();
    let mut second = server.client();
    first.application.refresh_resources(3).unwrap();
    second.application.refresh_resources(3).unwrap();
    let as_of = Utc::now();
    assert!(preview(&mut first, 14, as_of).get("error").is_none());
    second
        .application
        .create_task(TaskName::new("Concurrent task").unwrap(), as_of)
        .unwrap();
    first.application.refresh_resources(3).unwrap();
    let failure = archive(&mut first, 14, as_of);
    assert_eq!(failure["kind"], "conflict", "{failure}");
    assert_eq!(failure["uncertain"], false);
    assert_eq!(failure["requiresRefresh"], true);
    assert!(preview(&mut first, 14, as_of).get("error").is_none());
    let blocked = archive(&mut first, 14, as_of);
    assert_eq!(blocked["kind"], "unavailable");
    assert_eq!(blocked["uncertain"], false);
    first.application.refresh_resources(3).unwrap();
    assert!(preview(&mut first, 14, as_of).get("error").is_none());
    assert_eq!(archive(&mut first, 14, as_of)["data"]["archivedCount"], 3);
}

#[test]
fn unavailable_preview_and_confirmation_invalidate_context_and_preserve_confirmed_state() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("offline.db");
    let _writer = open_fixture(&path).unwrap();
    for fail_preview in [false, true] {
        let server = super::super::report_tests::Server::start(
            tracker_server::router_for_database(&path).unwrap(),
        );
        let mut bridge = server.client();
        bridge.application.refresh_resources(3).unwrap();
        let as_of = Utc::now();
        assert!(preview(&mut bridge, 14, as_of).get("error").is_none());
        let before = crate::tests::test_resource_values(&bridge.application);
        drop(server);
        let failure = if fail_preview {
            preview(&mut bridge, 14, as_of)
        } else {
            archive(&mut bridge, 14, as_of)
        };
        assert_eq!(failure["kind"], "unavailable");
        assert_eq!(failure["uncertain"], !fail_preview);
        assert_eq!(failure["requiresRefresh"], true);
        assert_eq!(
            crate::tests::test_resource_values(&bridge.application),
            before
        );
        assert_eq!(
            archive(&mut bridge, 14, as_of)["error"],
            "Load an archive preview before confirming"
        );
    }
}
