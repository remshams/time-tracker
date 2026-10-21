use super::*;
use tracker_domain::Worklog;

fn response(pointer: *mut c_char) -> Value {
    assert!(!pointer.is_null());
    // SAFETY: This test owns the returned bridge string and frees it once.
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
    history(bridge, task)
        .into_iter()
        .find(|worklog| worklog.id() == id)
        .unwrap()
}

fn history(bridge: &mut Bridge, task: TaskId) -> Vec<Worklog> {
    bridge
        .application
        .worklogs_for_task(task, None)
        .unwrap()
        .worklogs
}

fn delete(bridge: &mut Bridge, id: WorklogId, task: TaskId, start: i64, end: i64) -> Value {
    let id = CString::new(id.to_string()).unwrap();
    let task = CString::new(task.to_string()).unwrap();
    let start = CString::new(timestamp(at(start))).unwrap();
    let end = CString::new(timestamp(at(end))).unwrap();
    // SAFETY: The bridge and all strings remain live, with exclusive bridge access.
    response(unsafe {
        tt_bridge_delete_worklog(
            bridge,
            id.as_ptr(),
            task.as_ptr(),
            start.as_ptr(),
            end.as_ptr(),
        )
    })
}

fn delete_expected(bridge: &mut Bridge, expected: &Worklog) -> Value {
    delete(
        bridge,
        expected.id(),
        expected.task_id(),
        expected.start().timestamp(),
        expected.end().unwrap().timestamp(),
    )
}

#[test]
fn completed_deletion_returns_the_worklog_and_updates_history_and_totals() {
    let directory = tempfile::tempdir().unwrap();
    let mut bridge = open_fixture(&directory.path().join("delete.db")).unwrap();
    let task = bridge.application.tasks(TaskOrdering::default())[0]
        .task
        .id();
    let kept = record(&mut bridge, task, 100, Some(200));
    let deleted = record(&mut bridge, task, 300, Some(400));

    let result = delete_expected(&mut bridge, &deleted);

    assert!(result.get("error").is_none(), "{result}");
    assert_eq!(
        result["data"]["worklog"],
        serde_json::to_value(worklog_json(&deleted)).unwrap()
    );
    assert!(result["data"]["receipt"].is_null());
    assert_eq!(history(&mut bridge, task), vec![kept]);
    let (rows, _) = bridge
        .application
        .report_rows(at(0), at(500), at(500))
        .unwrap_or_else(|error| panic!("{}", error.message));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].task_id, task.to_string());
    assert_eq!(rows[0].duration_microseconds, 100_000_000);
}

#[test]
fn deletion_rejects_changed_missing_and_running_worklogs_without_writing() {
    let directory = tempfile::tempdir().unwrap();
    let mut bridge = open_fixture(&directory.path().join("stale.db")).unwrap();
    let tasks = bridge.application.tasks(TaskOrdering::default());
    let task = tasks[0].task.id();
    let other_task = tasks[1].task.id();
    let completed = record(&mut bridge, task, 100, Some(200));

    let changed_end = delete(&mut bridge, completed.id(), task, 100, 210);
    assert_eq!(changed_end["kind"], "worklog_changed", "{changed_end}");
    assert_eq!(changed_end["uncertain"], false);
    assert_eq!(changed_end["requiresRefresh"], true);
    let changed_task = delete(&mut bridge, completed.id(), other_task, 100, 200);
    assert_eq!(changed_task["kind"], "worklog_changed", "{changed_task}");
    assert_eq!(history(&mut bridge, task), vec![completed.clone()]);

    let running = record(&mut bridge, task, 300, None);
    let active = delete(&mut bridge, running.id(), task, 300, 400);
    assert_eq!(active["kind"], "worklog_active", "{active}");
    assert_eq!(active["uncertain"], false);
    assert_eq!(active["requiresRefresh"], true);
    assert_eq!(history(&mut bridge, task), vec![running, completed.clone()]);

    assert!(
        delete_expected(&mut bridge, &completed)
            .get("error")
            .is_none()
    );
    let missing = delete_expected(&mut bridge, &completed);
    assert_eq!(missing["kind"], "worklog_not_found", "{missing}");
    assert_eq!(missing["uncertain"], false);
    assert_eq!(missing["requiresRefresh"], true);
}

#[test]
fn deletion_rejects_null_invalid_and_non_utf8_inputs_without_writing() {
    let directory = tempfile::tempdir().unwrap();
    let mut bridge = open_fixture(&directory.path().join("inputs.db")).unwrap();
    let task = bridge.application.tasks(TaskOrdering::default())[0]
        .task
        .id();
    let expected = record(&mut bridge, task, 100, Some(200));
    let id = CString::new(expected.id().to_string()).unwrap();
    let task_id = CString::new(task.to_string()).unwrap();
    let start = CString::new(timestamp(expected.start())).unwrap();
    let end = CString::new(timestamp(expected.end().unwrap())).unwrap();
    let malformed = c"invalid";
    let non_utf8 = CString::new(vec![0xff]).unwrap();
    let baseline = [id.as_ptr(), task_id.as_ptr(), start.as_ptr(), end.as_ptr()];
    // SAFETY: All arguments are valid C strings or null and access is exclusive.
    unsafe {
        assert_eq!(
            response(tt_bridge_delete_worklog(
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                ptr::null()
            ))["error"],
            "Database is not open"
        );
        for (index, message) in [
            "Invalid worklog ID",
            "Invalid task ID",
            "Invalid original start timestamp",
            "Invalid original end timestamp",
        ]
        .into_iter()
        .enumerate()
        {
            for invalid in [ptr::null(), malformed.as_ptr(), non_utf8.as_ptr()] {
                let mut args = baseline;
                args[index] = invalid;
                assert_eq!(
                    response(tt_bridge_delete_worklog(
                        &mut bridge,
                        args[0],
                        args[1],
                        args[2],
                        args[3]
                    ))["error"],
                    message
                );
            }
        }
    }
    assert_eq!(history(&mut bridge, task), vec![expected]);
}

#[test]
fn remote_deletion_removes_the_worklog_and_rejects_stale_clients() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("remote.db");
    let mut writer = open_fixture(&path).unwrap();
    let task = writer.application.tasks(TaskOrdering::default())[0]
        .task
        .id();
    let original = record(&mut writer, task, 100, Some(200));
    let server = report_tests::Server::start(tracker_server::router_for_database(&path).unwrap());
    let mut first = server.client();
    let mut second = server.client();
    for client in [&mut first, &mut second] {
        client
            .application
            .refresh_resources(3)
            .unwrap_or_else(|error| panic!("{}", error.message));
    }

    let deleted = delete_expected(&mut first, &original);

    assert!(deleted.get("error").is_none(), "{deleted}");
    assert_eq!(
        deleted["data"]["worklog"],
        serde_json::to_value(worklog_json(&original)).unwrap()
    );
    assert!(deleted["data"]["receipt"]["requestId"].is_string());
    assert!(history(&mut first, task).is_empty());
    let failure = delete_expected(&mut second, &original);
    assert_eq!(failure["kind"], "worklog_not_found", "{failure}");
    assert_eq!(failure["uncertain"], false);
    assert_eq!(failure["requiresRefresh"], true);
}

#[test]
fn remote_deletion_sends_the_expected_worklog_and_classifies_failures() {
    use axum::{
        Json, Router,
        http::StatusCode,
        routing::{delete, get},
    };
    use std::sync::{Arc, Mutex};
    use tracker_protocol::{ErrorCode, ErrorDto, HealthDto};

    let directory = tempfile::tempdir().unwrap();
    let mut writer = open_fixture(&directory.path().join("payload.db")).unwrap();
    let task = writer.application.tasks(TaskOrdering::default())[0]
        .task
        .id();
    let original = record(&mut writer, task, 100, Some(200));
    let resources =
        report_tests::resource_dtos(writer.application.tasks(TaskOrdering::default()), None);
    for (code, message, status, kind, uncertain) in [
        (
            ErrorCode::WorklogChanged,
            "Worklog changed in another client",
            StatusCode::CONFLICT,
            "worklog_changed",
            false,
        ),
        (
            ErrorCode::NotFound,
            "Worklog not found",
            StatusCode::NOT_FOUND,
            "worklog_not_found",
            false,
        ),
        (
            ErrorCode::ActiveWorklog,
            "Running worklogs cannot be deleted",
            StatusCode::CONFLICT,
            "worklog_active",
            false,
        ),
        (
            ErrorCode::Conflict,
            "Tracker state changed in another client",
            StatusCode::CONFLICT,
            "conflict",
            false,
        ),
        (
            ErrorCode::InvalidRequest,
            "Invalid deletion",
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
        let resources = resources.clone();
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
                resources,
                vec![tracker_protocol::WorklogDto::from(&original)],
            ))
            .route(
                "/v1/worklogs/{id}",
                delete(
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
        let failure = delete_expected(&mut bridge, &original);
        assert_eq!(failure["kind"], kind, "{failure}");
        assert_eq!(failure["uncertain"], uncertain);
        assert_eq!(failure["requiresRefresh"], true);
        assert!(failure.get("data").is_none());
        let guard = received.lock().unwrap();
        let (id, body) = guard.as_ref().unwrap();
        assert_eq!(id, &original.id().to_string());
        assert_eq!(body["expected_task_id"], task.to_string());
        assert_eq!(body["expected_start"], timestamp(original.start()));
        assert_eq!(body["expected_end"], timestamp(original.end().unwrap()));
        assert!(body.get("expected_revision").is_none());
        assert!(body["request_id"].as_str().is_some_and(|id| !id.is_empty()));
        assert_eq!(body.as_object().unwrap().len(), 4);
    }
}

#[test]
fn remote_deletion_connection_failure_is_certain_and_does_not_delete() {
    let directory = tempfile::tempdir().unwrap();
    let mut writer = open_fixture(&directory.path().join("unavailable.db")).unwrap();
    let task = writer.application.tasks(TaskOrdering::default())[0]
        .task
        .id();
    let original = record(&mut writer, task, 100, Some(200));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let mut bridge = Bridge::new(Backend::remote(&endpoint).unwrap());
    let failure = delete_expected(&mut bridge, &original);
    assert_eq!(failure["kind"], "unavailable");
    assert_eq!(failure["uncertain"], false);
    assert_eq!(failure["requiresRefresh"], true);
    assert!(failure.get("data").is_none());
    assert_eq!(history(&mut writer, task), vec![original]);
}
