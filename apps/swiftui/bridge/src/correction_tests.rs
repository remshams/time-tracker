use super::*;
use tracker_domain::Worklog;

fn response(pointer: *mut c_char) -> Value {
    assert!(!pointer.is_null());
    // SAFETY: This test owns the returned bridge string and releases it once.
    unsafe {
        let value = serde_json::from_str(CStr::from_ptr(pointer).to_str().unwrap()).unwrap();
        tt_bridge_string_free(pointer);
        value
    }
}

fn at(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 123_456_000).unwrap()
}

fn record(bridge: &mut Bridge, task: TaskId, start: i64, end: Option<i64>) -> Worklog {
    bridge
        .application
        .set_active_task(task, at(start))
        .unwrap_or_else(|error| panic!("{}", error.message));
    let TrackingState::Running { worklog } = bridge.application.current_tracking() else {
        panic!("tracking should be running")
    };
    let id = worklog.id();
    if let Some(end) = end {
        bridge
            .application
            .clear_active_task(id, at(end))
            .unwrap_or_else(|error| panic!("{}", error.message));
    }
    bridge
        .application
        .worklogs_for_task(task, None)
        .unwrap()
        .worklogs
        .into_iter()
        .find(|worklog| worklog.id() == id)
        .unwrap()
}

fn correct(bridge: &mut Bridge, expected: &Worklog, start: i64, end: Option<i64>) -> Value {
    let id = CString::new(expected.id().to_string()).unwrap();
    let original_start = CString::new(timestamp(expected.start())).unwrap();
    let original_end = expected
        .end()
        .map(|end| CString::new(timestamp(end)).unwrap());
    let start = CString::new(timestamp(at(start))).unwrap();
    let end = end.map(|end| CString::new(timestamp(at(end))).unwrap());
    let occurred_at = CString::new(timestamp(at(1000))).unwrap();
    // SAFETY: Strings are live or null and bridge access is exclusive.
    response(unsafe {
        tt_bridge_correct_worklog_at(
            bridge,
            id.as_ptr(),
            original_start.as_ptr(),
            original_end
                .as_ref()
                .map_or(ptr::null(), |value| value.as_ptr()),
            start.as_ptr(),
            end.as_ref().map_or(ptr::null(), |value| value.as_ptr()),
            occurred_at.as_ptr(),
        )
    })
}

#[test]
fn correction_preserves_identity_precision_and_archived_task_and_updates_totals() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("correction.db");
    let mut bridge = open_fixture(&path).unwrap();
    let task = bridge.application.tasks(TaskOrdering::default())[0]
        .task
        .id();
    let expected = record(&mut bridge, task, 100, Some(200));
    bridge.application.archive_task(task, at(300)).unwrap();
    let corrected = correct(&mut bridge, &expected, 90, Some(220));
    assert!(corrected.get("error").is_none(), "{corrected}");
    assert_eq!(
        corrected["data"]["worklog"],
        json!({
            "id": expected.id().to_string(), "taskId": task.to_string(),
            "start": timestamp(at(90)), "end": timestamp(at(220))
        })
    );
    assert!(crate::tests::test_active_value(&bridge.application).is_null());
    let observed_tasks = crate::tests::test_task_values(&bridge.application);
    let updated_task = observed_tasks
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == task.to_string())
        .unwrap();
    assert_eq!(updated_task["archived"], true);
    assert_eq!(updated_task["latestStart"], timestamp(at(90)));
    let (rows, _, _) = bridge
        .application
        .report_rows(at(0), at(1000), at(1000))
        .unwrap_or_else(|error| panic!("{}", error.message));
    assert_eq!(rows[0].duration_microseconds, 130_000_000);
    drop(bridge);
    let mut reopened = open_at(&path).unwrap();
    let stored = reopened.application.worklogs_for_task(task, None).unwrap();
    assert_eq!(stored.worklogs[0].id(), expected.id());
    assert_eq!(stored.worklogs[0].start(), at(90));
    assert_eq!(stored.worklogs[0].end(), Some(at(220)));
}

#[test]
fn correcting_active_start_keeps_timer_running_and_updates_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let mut bridge = open_fixture(&directory.path().join("active.db")).unwrap();
    let task = bridge.application.tasks(TaskOrdering::default())[0]
        .task
        .id();
    let expected = record(&mut bridge, task, 100, None);
    let corrected = correct(&mut bridge, &expected, 80, None);
    assert!(corrected.get("error").is_none(), "{corrected}");
    assert_eq!(
        crate::tests::test_active_value(&bridge.application),
        corrected["data"]["worklog"]
    );
    assert_eq!(corrected["data"]["worklog"]["start"], timestamp(at(80)));
    assert!(corrected["data"]["worklog"]["end"].is_null());
    assert_eq!(
        corrected["data"]["worklog"]["id"],
        expected.id().to_string()
    );
}

#[test]
fn correction_rejects_other_clients_timestamp_changes_and_stopped_timer() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("concurrent.db");
    let mut first = open_fixture(&path).unwrap();
    let task = first.application.tasks(TaskOrdering::default())[0]
        .task
        .id();
    let original = record(&mut first, task, 100, None);
    let mut second = open_at(&path).unwrap();
    assert!(
        correct(&mut second, &original, 80, None)
            .get("error")
            .is_none()
    );
    let failure = correct(&mut first, &original, 90, None);
    assert_eq!(failure["kind"], "worklog_changed");
    assert_eq!(failure["uncertain"], false);
    assert_eq!(failure["requiresRefresh"], true);
    assert!(failure.get("data").is_none());
    assert_eq!(
        tracking_json(&first.application)
            .unwrap()
            .value
            .unwrap()
            .start,
        timestamp(at(80))
    );
    let current = second
        .application
        .worklogs_for_task(task, None)
        .unwrap()
        .worklogs
        .remove(0);
    second
        .application
        .clear_active_task(current.id(), at(200))
        .unwrap_or_else(|error| panic!("{}", error.message));
    let failure = correct(&mut first, &current, 70, None);
    assert_eq!(failure["kind"], "worklog_changed");
    assert!(tracking_json(&first.application).unwrap().value.is_none());
    let stored = first
        .application
        .worklogs_for_task(task, None)
        .unwrap()
        .worklogs
        .remove(0);
    assert_eq!(stored.start(), at(80));
    assert_eq!(stored.end(), Some(at(200)));
}

#[test]
fn correction_rejects_invalid_times_completion_changes_and_overlap_without_writing() {
    let directory = tempfile::tempdir().unwrap();
    let mut bridge = open_fixture(&directory.path().join("validation.db")).unwrap();
    let task = bridge.application.tasks(TaskOrdering::default())[0]
        .task
        .id();
    let expected = record(&mut bridge, task, 100, Some(200));
    let other = record(&mut bridge, task, 300, Some(400));
    for (start, end, message) in [
        (
            201,
            Some(200),
            "corrected worklog end must not precede its start",
        ),
        (
            100,
            None,
            "a correction must preserve whether the worklog is active or completed",
        ),
        (
            1001,
            Some(1100),
            "corrected worklog start must not be later than occurred_at",
        ),
        (
            100,
            Some(1001),
            "corrected worklog end must not be later than occurred_at",
        ),
    ] {
        let failure = correct(&mut bridge, &expected, start, end);
        assert_eq!(failure["error"], message);
        assert_eq!(failure["kind"], "general");
        assert_eq!(failure["requiresRefresh"], false);
        assert_eq!(failure["uncertain"], false);
    }
    let failure = correct(&mut bridge, &expected, 100, Some(301));
    assert_eq!(failure["kind"], "worklog_overlap");
    assert_eq!(failure["error"], "The worklog overlaps another worklog");
    assert_eq!(failure["uncertain"], false);
    assert_eq!(failure["requiresRefresh"], false);
    assert_eq!(
        bridge
            .application
            .worklogs_for_task(task, None)
            .unwrap()
            .worklogs,
        vec![other, expected]
    );
}

#[test]
fn correction_rejects_null_invalid_and_non_utf8_inputs_without_writing() {
    let directory = tempfile::tempdir().unwrap();
    let mut bridge = open_fixture(&directory.path().join("inputs.db")).unwrap();
    let task = bridge.application.tasks(TaskOrdering::default())[0]
        .task
        .id();
    let expected = record(&mut bridge, task, 100, Some(200));
    let id = CString::new(expected.id().to_string()).unwrap();
    let valid = c"1970-01-01T00:01:40.123456Z";
    let end = c"1970-01-01T00:03:20.123456Z";
    let now = c"1970-01-01T00:16:40.123456Z";
    let malformed = c"invalid";
    let non_utf8 = CString::new(vec![0xff]).unwrap();
    let baseline = [
        id.as_ptr(),
        valid.as_ptr(),
        end.as_ptr(),
        valid.as_ptr(),
        end.as_ptr(),
        now.as_ptr(),
    ];
    // SAFETY: All arguments are valid C strings or null and access is exclusive.
    unsafe {
        assert_eq!(
            response(tt_bridge_correct_worklog_at(
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                ptr::null()
            ))["error"],
            "Database is not open"
        );
        for (index, message) in [
            "Invalid worklog ID",
            "Invalid original start timestamp",
            "Invalid original end timestamp",
            "Invalid replacement start timestamp",
            "Invalid replacement end timestamp",
            "Invalid correction timestamp",
        ]
        .into_iter()
        .enumerate()
        {
            for invalid in [ptr::null(), malformed.as_ptr(), non_utf8.as_ptr()] {
                if invalid.is_null() && matches!(index, 2 | 4) {
                    continue;
                }
                let mut args = baseline;
                args[index] = invalid;
                assert_eq!(
                    response(tt_bridge_correct_worklog_at(
                        &mut bridge,
                        args[0],
                        args[1],
                        args[2],
                        args[3],
                        args[4],
                        args[5]
                    ))["error"],
                    message
                );
            }
        }
    }
    assert_eq!(
        bridge
            .application
            .worklogs_for_task(task, None)
            .unwrap()
            .worklogs,
        vec![expected]
    );
}

#[test]
fn remote_correction_refreshes_revision_but_keeps_original_timestamp_guard() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("remote.db");
    let mut writer = open_fixture(&path).unwrap();
    let task = writer.application.tasks(TaskOrdering::default())[0]
        .task
        .id();
    let original = record(&mut writer, task, 100, None);
    let server = report_tests::Server::start(tracker_server::router_for_database(&path).unwrap());
    let mut first = server.client();
    let mut second = server.client();
    first
        .application
        .refresh_resources(3)
        .unwrap_or_else(|error| panic!("{}", error.message));
    second
        .application
        .refresh_resources(3)
        .unwrap_or_else(|error| panic!("{}", error.message));
    second
        .application
        .create_task(TaskName::new("Concurrent task").unwrap(), at(500))
        .unwrap_or_else(|error| panic!("{}", error.message));
    let corrected = correct(&mut first, &original, 90, None);
    assert!(corrected.get("error").is_none(), "{corrected}");
    assert_eq!(
        crate::tests::test_task_values(&first.application)
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        crate::tests::test_active_value(&first.application)["start"],
        timestamp(at(90))
    );
    let failure = correct(&mut second, &original, 80, None);
    assert_eq!(failure["kind"], "worklog_changed");
    assert_eq!(failure["uncertain"], false);
    assert_eq!(failure["requiresRefresh"], true);
    assert_eq!(
        tracking_json(&second.application)
            .unwrap()
            .value
            .unwrap()
            .start,
        timestamp(at(90))
    );
    let latest = first
        .application
        .worklogs_for_task(task, None)
        .unwrap()
        .worklogs
        .remove(0);
    first
        .application
        .clear_active_task(latest.id(), at(200))
        .unwrap_or_else(|error| panic!("{}", error.message));
    let failure = correct(&mut second, &latest, 70, None);
    assert_eq!(failure["kind"], "worklog_changed");
    let completed = first
        .application
        .worklogs_for_task(task, None)
        .unwrap()
        .worklogs
        .remove(0);
    let corrected = correct(&mut second, &completed, 60, Some(210));
    assert!(corrected.get("error").is_none(), "{corrected}");
    assert_eq!(corrected["data"]["worklog"]["end"], timestamp(at(210)));
    assert!(crate::tests::test_active_value(&second.application).is_null());
}

#[test]
fn remote_correction_sends_original_and_replacement_times_and_classifies_failures() {
    use axum::{
        Json, Router,
        http::StatusCode,
        routing::{get, patch},
    };
    use std::sync::{Arc, Mutex};
    use tracker_protocol::{ErrorCode, ErrorDto, HealthDto};

    let directory = tempfile::tempdir().unwrap();
    let mut writer = open_fixture(&directory.path().join("payload.db")).unwrap();
    let task = writer.application.tasks(TaskOrdering::default())[0]
        .task
        .id();
    let original = record(&mut writer, task, 100, Some(200));
    let snapshot = report_tests::resource_dtos(
        writer.application.tasks(TaskOrdering::default()),
        None,
        "42".into(),
    );
    for (code, message, status, kind, uncertain) in [
        (
            ErrorCode::WorklogOverlap,
            "The worklog overlaps another worklog",
            StatusCode::CONFLICT,
            "worklog_overlap",
            false,
        ),
        (
            ErrorCode::StaleRevision,
            "Tracker state changed. Refresh and retry.",
            StatusCode::CONFLICT,
            "conflict",
            false,
        ),
        (
            ErrorCode::InvalidRequest,
            "Invalid correction",
            StatusCode::BAD_REQUEST,
            "general",
            false,
        ),
        (
            ErrorCode::Internal,
            "Storage unavailable",
            StatusCode::INTERNAL_SERVER_ERROR,
            "unavailable",
            true,
        ),
    ] {
        let received = Arc::new(Mutex::new(None));
        let captured = received.clone();
        let snapshot = snapshot.clone();
        let router = Router::new()
            .route(
                "/v1/health",
                get(|| async {
                    Json(HealthDto {
                        status: "ok".into(),
                        protocol_version: tracker_protocol::VERSION,
                    })
                }),
            )
            .merge(report_tests::resource_router(
                snapshot,
                vec![tracker_protocol::WorklogDto::from(&original)],
            ))
            .route(
                "/v1/worklogs/{id}",
                patch(
                    move |axum::extract::Path(id): axum::extract::Path<String>,
                          Json(body): Json<Value>| {
                        *captured.lock().unwrap() = Some((id, body));
                        async move {
                            (
                                status,
                                Json(ErrorDto {
                                    code,
                                    message: message.into(),
                                }),
                            )
                        }
                    },
                ),
            );
        let server = report_tests::Server::start(router);
        let mut bridge = server.client();
        let failure = correct(&mut bridge, &original, 90, Some(210));
        assert_eq!(failure["kind"], kind, "{failure}");
        assert_eq!(failure["uncertain"], uncertain);
        assert_eq!(failure["requiresRefresh"], true);
        assert!(failure.get("data").is_none());
        let guard = received.lock().unwrap();
        let (id, body) = guard.as_ref().unwrap();
        assert_eq!(id, &original.id().to_string());
        assert_eq!(body["action"], "correct");
        assert_eq!(body["expected_start"], timestamp(original.start()));
        assert_eq!(body["expected_end"], timestamp(original.end().unwrap()));
        assert_eq!(body["replacement_start"], timestamp(at(90)));
        assert_eq!(body["replacement_end"], timestamp(at(210)));
        assert_eq!(body["occurred_at"], timestamp(at(1000)));
        assert_eq!(body["expected_revision"], "42");
        assert!(body["request_id"].as_str().is_some_and(|id| !id.is_empty()));
        assert_eq!(body.as_object().unwrap().len(), 8);
    }
}

#[test]
fn remote_correction_preflight_failure_does_not_send_a_write() {
    let directory = tempfile::tempdir().unwrap();
    let mut writer = open_fixture(&directory.path().join("unavailable.db")).unwrap();
    let task = writer.application.tasks(TaskOrdering::default())[0]
        .task
        .id();
    let original = record(&mut writer, task, 100, None);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let mut bridge = Bridge::new(Backend::remote(&endpoint).unwrap());
    let failure = correct(&mut bridge, &original, 90, None);
    assert_eq!(failure["kind"], "unavailable");
    assert_eq!(failure["uncertain"], false);
    assert_eq!(failure["requiresRefresh"], true);
    assert!(failure.get("data").is_none());
    assert_eq!(
        writer
            .application
            .worklogs_for_task(task, None)
            .unwrap()
            .worklogs,
        vec![original]
    );
}
