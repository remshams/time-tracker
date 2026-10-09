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
    bridge
        .application
        .worklogs_for_task(task, None)
        .unwrap()
        .worklogs
        .into_iter()
        .find(|worklog| worklog.id() == id)
        .unwrap()
}

fn move_to(bridge: &mut Bridge, expected: &Worklog, destination: TaskId) -> Value {
    let id = CString::new(expected.id().to_string()).unwrap();
    let source = CString::new(expected.task_id().to_string()).unwrap();
    let start = CString::new(timestamp(expected.start())).unwrap();
    let end = expected
        .end()
        .map(|value| CString::new(timestamp(value)).unwrap());
    let destination = CString::new(destination.to_string()).unwrap();
    // SAFETY: The bridge and all strings remain live, with exclusive bridge access.
    response(unsafe {
        tt_bridge_move_worklog(
            bridge,
            id.as_ptr(),
            source.as_ptr(),
            start.as_ptr(),
            end.as_ref().map_or(ptr::null(), |value| value.as_ptr()),
            destination.as_ptr(),
        )
    })
}

fn candidates(bridge: &mut Bridge, source: TaskId, query: &str) -> Value {
    let source = CString::new(source.to_string()).unwrap();
    let query = CString::new(query).unwrap();
    // SAFETY: The bridge and strings remain live, with exclusive bridge access.
    response(unsafe { tt_bridge_move_candidates(bridge, source.as_ptr(), query.as_ptr()) })
}

#[test]
fn completed_and_running_moves_preserve_identity_times_and_update_history_and_totals() {
    for end in [Some(200), None] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("move.db");
        let mut bridge = open_fixture(&path).unwrap();
        let tasks = bridge.application.tasks(TaskOrdering::default());
        let source = tasks[0].task.id();
        let destination = tasks[1].task.id();
        let original = record(&mut bridge, source, 100, end);
        let unrelated = if end.is_some() {
            Some(record(&mut bridge, tasks[2].task.id(), 300, None))
        } else {
            None
        };
        let result = move_to(&mut bridge, &original, destination);
        assert!(result.get("error").is_none(), "{result}");
        let moved = original.moved_to(destination).unwrap();
        assert_eq!(
            result["data"]["worklog"],
            serde_json::to_value(worklog_json(&moved)).unwrap()
        );
        let active = if end.is_none() {
            &moved
        } else {
            unrelated.as_ref().unwrap()
        };
        assert_eq!(
            result["data"]["snapshot"]["active"],
            serde_json::to_value(worklog_json(active)).unwrap()
        );
        assert!(
            bridge
                .application
                .worklogs_for_task(source, None)
                .unwrap()
                .worklogs
                .is_empty()
        );
        assert_eq!(
            bridge
                .application
                .worklogs_for_task(destination, None)
                .unwrap()
                .worklogs,
            vec![moved.clone()]
        );
        let totals = bridge
            .application
            .report_totals(at(0), at(250), at(250))
            .unwrap_or_else(|error| panic!("{}", error.message));
        assert_eq!(totals.rows.len(), 1);
        assert_eq!(totals.rows[0].task.id(), destination);
        assert_eq!(
            totals.rows[0].duration.num_seconds(),
            if end.is_some() { 100 } else { 150 }
        );
        drop(bridge);
        let mut reopened = open_at(&path).unwrap();
        assert_eq!(
            reopened
                .application
                .worklogs_for_task(destination, None)
                .unwrap()
                .worklogs,
            vec![moved]
        );
    }
}

#[test]
fn candidates_use_cached_tasks_filter_search_and_exclude_source_and_archived_rows() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.db");
    let mut bridge = open_fixture(&path).unwrap();
    let tasks = bridge.application.tasks(TaskOrdering::default());
    let source = tasks[0].task.id();
    let destination = tasks[1].task.id();
    bridge
        .application
        .archive_task(tasks[2].task.id(), at(0))
        .unwrap();
    let first = candidates(&mut bridge, source, "");
    assert_eq!(
        first["data"],
        json!([{ "id": destination.to_string(), "name": tasks[1].task.name().as_str() }])
    );
    assert_eq!(
        candidates(&mut bridge, source, "PLAN")["data"],
        first["data"]
    );
    assert_eq!(
        candidates(&mut bridge, source, "missing")["data"],
        json!([])
    );
    let mut other = open_at(&path).unwrap();
    other
        .application
        .create_task(TaskName::new("Fresh task").unwrap(), at(1))
        .unwrap_or_else(|error| panic!("{}", error.message));
    assert_eq!(candidates(&mut bridge, source, "")["data"], first["data"]);
    bridge
        .application
        .refresh()
        .unwrap_or_else(|error| panic!("{}", error.message));
    assert_eq!(
        candidates(&mut bridge, source, "fresh")["data"][0]["name"],
        "Fresh task"
    );
}

#[test]
fn stale_source_times_missing_rows_overlap_and_unavailable_destination_are_classified() {
    let directory = tempfile::tempdir().unwrap();
    let mut bridge = open_fixture(&directory.path().join("failure.db")).unwrap();
    let tasks = bridge.application.tasks(TaskOrdering::default());
    let source = tasks[0].task.id();
    let destination = tasks[1].task.id();
    let original = record(&mut bridge, source, 100, Some(200));
    let changed_source = original.moved_to(tasks[2].task.id()).unwrap();
    let changed_times = original
        .corrected(WorklogTimes::new(at(90), Some(at(200))), at(300))
        .unwrap();
    for expected in [&changed_source, &changed_times] {
        let result = move_to(&mut bridge, expected, destination);
        assert_eq!(result["kind"], "worklog_changed");
        assert_eq!(result["uncertain"], false);
        assert_eq!(result["requiresRefresh"], true);
    }
    let missing = Worklog::new(
        WorklogId::generate(),
        source,
        original.start(),
        original.end(),
    )
    .unwrap();
    assert_eq!(
        move_to(&mut bridge, &missing, destination)["kind"],
        "worklog_not_found"
    );
    let overlap = record(&mut bridge, destination, 150, Some(250));
    assert_eq!(
        move_to(&mut bridge, &original, destination)["kind"],
        "worklog_overlap"
    );
    assert_eq!(
        move_to(&mut bridge, &original, TaskId::generate())["kind"],
        "destination_unavailable"
    );
    bridge
        .application
        .archive_task(tasks[2].task.id(), at(300))
        .unwrap();
    assert_eq!(
        move_to(&mut bridge, &original, tasks[2].task.id())["kind"],
        "destination_unavailable"
    );
    assert_eq!(move_to(&mut bridge, &original, source)["kind"], "general");
    assert_eq!(
        bridge
            .application
            .worklogs_for_task(source, None)
            .unwrap()
            .worklogs,
        vec![original]
    );
    assert_eq!(
        bridge
            .application
            .worklogs_for_task(destination, None)
            .unwrap()
            .worklogs,
        vec![overlap]
    );
}

#[test]
fn move_and_candidate_c_inputs_reject_null_malformed_and_non_utf8_strings() {
    let directory = tempfile::tempdir().unwrap();
    let mut bridge = open_fixture(&directory.path().join("inputs.db")).unwrap();
    let tasks = bridge.application.tasks(TaskOrdering::default());
    let original = record(&mut bridge, tasks[0].task.id(), 100, Some(200));
    let strings = [
        original.id().to_string(),
        original.task_id().to_string(),
        timestamp(original.start()),
        timestamp(original.end().unwrap()),
        tasks[1].task.id().to_string(),
    ]
    .map(|text| CString::new(text).unwrap());
    let baseline = strings.each_ref().map(|value| value.as_ptr());
    let malformed = CString::new("bad input").unwrap();
    let non_utf8 = CString::new(vec![255]).unwrap();
    // SAFETY: All non-null pointers reference live C strings, with exclusive bridge access.
    unsafe {
        assert_eq!(
            response(tt_bridge_move_worklog(
                ptr::null_mut(),
                baseline[0],
                baseline[1],
                baseline[2],
                baseline[3],
                baseline[4]
            ))["error"],
            "Database is not open"
        );
        assert_eq!(
            response(tt_bridge_move_candidates(
                ptr::null_mut(),
                baseline[1],
                malformed.as_ptr()
            ))["error"],
            "Database is not open"
        );
        for (index, message) in [
            "Invalid worklog ID",
            "Invalid source task ID",
            "Invalid original start timestamp",
            "Invalid original end timestamp",
            "Invalid destination task ID",
        ]
        .iter()
        .enumerate()
        {
            for invalid in [ptr::null(), malformed.as_ptr(), non_utf8.as_ptr()] {
                if index == 3 && invalid.is_null() {
                    continue;
                }
                let mut args = baseline;
                args[index] = invalid;
                assert_eq!(
                    response(tt_bridge_move_worklog(
                        &mut bridge,
                        args[0],
                        args[1],
                        args[2],
                        args[3],
                        args[4]
                    ))["error"],
                    *message
                );
            }
        }
        for invalid in [ptr::null(), malformed.as_ptr(), non_utf8.as_ptr()] {
            assert_eq!(
                response(tt_bridge_move_candidates(
                    &mut bridge,
                    invalid,
                    malformed.as_ptr()
                ))["error"],
                "Invalid source task ID"
            );
        }
        for invalid in [ptr::null(), non_utf8.as_ptr()] {
            assert_eq!(
                response(tt_bridge_move_candidates(&mut bridge, baseline[1], invalid))["error"],
                "Invalid move search query"
            );
        }
    }
    assert_eq!(
        bridge
            .application
            .worklogs_for_task(original.task_id(), None)
            .unwrap()
            .worklogs,
        vec![original]
    );
}

#[test]
fn remote_moves_refresh_revision_keep_original_guards_and_candidates_never_use_http() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("remote.db");
    let mut writer = open_fixture(&path).unwrap();
    let tasks = writer.application.tasks(TaskOrdering::default());
    let source = tasks[0].task.id();
    let destination = tasks[1].task.id();
    let original = record(&mut writer, source, 100, None);
    let server = report_tests::Server::start(tracker_server::router_for_database(&path).unwrap());
    let mut first = server.client();
    let mut second = server.client();
    first
        .application
        .refresh()
        .unwrap_or_else(|error| panic!("{}", error.message));
    second
        .application
        .refresh()
        .unwrap_or_else(|error| panic!("{}", error.message));
    second
        .application
        .create_task(TaskName::new("Concurrent task").unwrap(), at(500))
        .unwrap_or_else(|error| panic!("{}", error.message));
    let moved = move_to(&mut first, &original, destination);
    assert!(moved.get("error").is_none(), "{moved}");
    assert_eq!(
        moved["data"]["snapshot"]["tasks"].as_array().unwrap().len(),
        4
    );
    assert_eq!(
        moved["data"]["snapshot"]["active"],
        moved["data"]["worklog"]
    );
    let failure = move_to(&mut second, &original, tasks[2].task.id());
    assert_eq!(failure["kind"], "worklog_changed");
    assert_eq!(failure["uncertain"], false);
    assert_eq!(failure["requiresRefresh"], true);
    let latest = first
        .application
        .worklogs_for_task(destination, None)
        .unwrap()
        .worklogs
        .remove(0);
    first
        .application
        .clear_active_task(latest.id(), at(200))
        .unwrap_or_else(|error| panic!("{}", error.message));
    assert_eq!(
        move_to(&mut second, &latest, source)["kind"],
        "worklog_changed"
    );
    let completed = first
        .application
        .worklogs_for_task(destination, None)
        .unwrap()
        .worklogs
        .remove(0);
    let moved = move_to(&mut second, &completed, source);
    assert!(moved.get("error").is_none(), "{moved}");
    assert!(moved["data"]["snapshot"]["active"].is_null());
    drop(server);
    assert_eq!(
        candidates(&mut second, source, "CONCURRENT")["data"][0]["name"],
        "Concurrent task"
    );
    assert_eq!(
        candidates(&mut second, source, "release")["data"][0]["name"],
        "Plan release"
    );
}

fn mock_server(
    snapshot: tracker_protocol::SnapshotDto,
    original: &Worklog,
    reply: Value,
) -> report_tests::Server {
    use axum::{
        Json, Router,
        routing::{get, patch},
    };
    let state = std::sync::Arc::new(std::sync::Mutex::new(snapshot));
    let write_state = state.clone();
    let router = Router::new()
        .route(
            "/v1/health",
            get(|| async {
                Json(tracker_protocol::HealthDto {
                    status: "ok".into(),
                    protocol_version: tracker_protocol::VERSION,
                })
            }),
        )
        .merge(report_tests::resource_router_state(
            state,
            vec![tracker_protocol::WorklogDto::from(original)],
        ))
        .route(
            "/v1/worklogs/{id}",
            patch(move |Json(body): Json<Value>| {
                let mut reply = reply.clone();
                if reply["request_id"] == "" {
                    reply["request_id"] = body["request_id"].clone();
                }
                let state = write_state.clone();
                async move {
                    if let Ok(receipt) =
                        serde_json::from_value::<tracker_protocol::MutationDto>(reply.clone())
                    {
                        if let tracker_protocol::MutationResultDto::Worklog(worklog) =
                            receipt.result
                        {
                            let mut snapshot = state.lock().unwrap();
                            if snapshot
                                .active_worklog
                                .as_ref()
                                .is_some_and(|active| active.id == worklog.id)
                            {
                                snapshot.active_worklog = worklog.end.is_none().then_some(worklog);
                            }
                            snapshot.revision = receipt.applied_revision;
                        }
                    }
                    Json(reply)
                }
            }),
        );
    report_tests::Server::start(router)
}

#[test]
fn remote_move_rejects_wrong_identity_destination_times_and_invalid_receipts() {
    use tracker_protocol::{MutationDto, MutationResultDto, SnapshotDto, WorklogDto};
    let directory = tempfile::tempdir().unwrap();
    for end in [Some(200), None] {
        let mut writer =
            open_fixture(&directory.path().join(format!("protocol-{end:?}.db"))).unwrap();
        let tasks = writer.application.tasks(TaskOrdering::default());
        let source = tasks[0].task.id();
        let destination = tasks[1].task.id();
        let original = record(&mut writer, source, 100, end);
        let moved = original.moved_to(destination).unwrap();
        let initial = SnapshotDto::from_snapshot(
            &tracker_application::TrackerSnapshot {
                task_items: writer.application.tasks(TaskOrdering::default()),
                active_worklog: end.is_none().then(|| original.clone()),
            },
            "42".into(),
        );
        let mut updated = initial.clone();
        updated.active_worklog = end.is_none().then(|| WorklogDto::from(&moved));
        updated.revision = "43".into();
        let baseline = serde_json::to_value(MutationDto {
            result: MutationResultDto::Worklog(WorklogDto::from(&moved)),
            request_id: String::new(),
            applied_revision: updated.revision,
            replayed: false,
        })
        .unwrap();
        for index in 0..11 {
            let mut reply = baseline.clone();
            match index {
                0 => reply["result"]["value"]["id"] = json!(WorklogId::generate().to_string()),
                1 => reply["result"]["value"]["task_id"] = json!(source.to_string()),
                2 => reply["result"]["value"]["start"] = json!(timestamp(at(90))),
                3 => {
                    reply["result"]["value"]["end"] = if end.is_some() {
                        Value::Null
                    } else {
                        json!(timestamp(at(200)))
                    }
                }
                4 => reply["request_id"] = json!(WorklogId::generate().to_string()),
                5 => reply["applied_revision"] = json!(""),
                6 => reply["unexpected"] = json!(true),
                7 => reply["result"]["kind"] = json!("tracking_already_idle"),
                8 => reply["replayed"] = json!("invalid"),
                9 => reply["result"]["value"]["start"] = json!(timestamp(at(110))),
                10 => reply["result"]["value"]["unexpected"] = json!(true),
                _ => unreachable!(),
            }
            let server = mock_server(initial.clone(), &original, reply);
            let mut bridge = server.client();
            let failure = move_to(&mut bridge, &original, destination);
            assert_eq!(failure["kind"], "protocol", "{index}: {failure}");
            assert_eq!(failure["uncertain"], true);
            assert_eq!(failure["requiresRefresh"], true);
            assert!(failure.get("data").is_none());
        }
        let server = mock_server(initial.clone(), &original, baseline);
        let mut bridge = server.client();
        assert!(
            move_to(&mut bridge, &original, destination)
                .get("error")
                .is_none()
        );
    }
}

#[test]
fn remote_move_sends_original_task_and_microsecond_times_and_classifies_failures() {
    use axum::{
        Json, Router,
        http::StatusCode,
        routing::{get, patch},
    };
    use std::sync::{Arc, Mutex};
    use tracker_protocol::{ErrorCode, ErrorDto, HealthDto, SnapshotDto};
    let directory = tempfile::tempdir().unwrap();
    let mut writer = open_fixture(&directory.path().join("payload.db")).unwrap();
    let tasks = writer.application.tasks(TaskOrdering::default());
    let original = record(&mut writer, tasks[0].task.id(), 100, Some(200));
    let snapshot = SnapshotDto::from_snapshot(
        &tracker_application::TrackerSnapshot {
            task_items: writer.application.tasks(TaskOrdering::default()),
            active_worklog: None,
        },
        "42".into(),
    );
    for (code, status, kind, uncertain) in [
        (
            ErrorCode::WorklogChanged,
            StatusCode::CONFLICT,
            "worklog_changed",
            false,
        ),
        (
            ErrorCode::NotFound,
            StatusCode::NOT_FOUND,
            "worklog_not_found",
            false,
        ),
        (
            ErrorCode::WorklogOverlap,
            StatusCode::CONFLICT,
            "worklog_overlap",
            false,
        ),
        (
            ErrorCode::StaleRevision,
            StatusCode::CONFLICT,
            "conflict",
            false,
        ),
        (
            ErrorCode::InvalidRequest,
            StatusCode::BAD_REQUEST,
            "general",
            false,
        ),
        (
            ErrorCode::Internal,
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
                        let message = if code == ErrorCode::StaleRevision {
                            "Tracker state changed. Refresh and retry."
                        } else {
                            "Move failed"
                        };
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
        let failure = move_to(&mut bridge, &original, tasks[1].task.id());
        assert_eq!(failure["kind"], kind, "{failure}");
        assert_eq!(failure["uncertain"], uncertain);
        assert_eq!(failure["requiresRefresh"], true);
        let received = received.lock().unwrap();
        let (id, body) = received.as_ref().unwrap();
        assert_eq!(id, &original.id().to_string());
        assert_eq!(body["action"], "move");
        assert_eq!(body["expected_task_id"], original.task_id().to_string());
        assert_eq!(body["expected_start"], timestamp(original.start()));
        assert_eq!(body["expected_end"], timestamp(original.end().unwrap()));
        assert_eq!(body["destination_task_id"], tasks[1].task.id().to_string());
        assert_eq!(body["expected_revision"], "42");
        assert!(body["request_id"].as_str().is_some_and(|id| !id.is_empty()));
        assert_eq!(body.as_object().unwrap().len(), 7);
    }
}

#[test]
fn remote_move_preflight_failure_is_certain_and_does_not_write() {
    let directory = tempfile::tempdir().unwrap();
    let mut writer = open_fixture(&directory.path().join("unavailable.db")).unwrap();
    let tasks = writer.application.tasks(TaskOrdering::default());
    let original = record(&mut writer, tasks[0].task.id(), 100, None);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let mut bridge = Bridge::new(Backend::remote(&endpoint).unwrap());
    let failure = move_to(&mut bridge, &original, tasks[1].task.id());
    assert_eq!(failure["kind"], "unavailable");
    assert_eq!(failure["uncertain"], false);
    assert_eq!(failure["requiresRefresh"], true);
    assert_eq!(
        writer
            .application
            .worklogs_for_task(original.task_id(), None)
            .unwrap()
            .worklogs,
        vec![original]
    );
}

#[test]
fn remote_move_classifies_archived_and_missing_destination_from_recovered_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("destinations.db");
    let mut writer = open_fixture(&path).unwrap();
    let tasks = writer.application.tasks(TaskOrdering::default());
    let original = record(&mut writer, tasks[0].task.id(), 100, Some(200));
    writer
        .application
        .archive_task(tasks[1].task.id(), at(300))
        .unwrap();
    let server = report_tests::Server::start(tracker_server::router_for_database(&path).unwrap());
    let mut bridge = server.client();
    for destination in [tasks[1].task.id(), TaskId::generate()] {
        let result = move_to(&mut bridge, &original, destination);
        assert_eq!(result["kind"], "destination_unavailable", "{result}");
        assert_eq!(result["uncertain"], false);
        assert_eq!(result["requiresRefresh"], true);
    }
    assert_eq!(
        writer
            .application
            .worklogs_for_task(original.task_id(), None)
            .unwrap()
            .worklogs,
        vec![original]
    );
}

#[test]
fn remote_move_failed_recovery_keeps_uncertain_transport_error() {
    use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::get};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tracker_protocol::{ErrorCode, ErrorDto, HealthDto, SnapshotDto};
    let directory = tempfile::tempdir().unwrap();
    let mut writer = open_fixture(&directory.path().join("recovery.db")).unwrap();
    let tasks = writer.application.tasks(TaskOrdering::default());
    let original = record(&mut writer, tasks[0].task.id(), 100, Some(200));
    let snapshot = SnapshotDto::from_snapshot(
        &tracker_application::TrackerSnapshot {
            task_items: writer.application.tasks(TaskOrdering::default()),
            active_worklog: None,
        },
        "42".into(),
    );
    let reads = Arc::new(AtomicUsize::new(0));
    let task_resources = snapshot.task_items.clone();
    let original_dto = tracker_protocol::WorklogDto::from(&original);
    let router = Router::new()
        .route(
            "/v1/tasks/{id}",
            get(
                move |axum::extract::Path(id): axum::extract::Path<String>| {
                    let task = task_resources
                        .iter()
                        .find(|item| item.task.id == id)
                        .unwrap()
                        .task
                        .clone();
                    async move {
                        Json(tracker_protocol::TaskResourceDto {
                            task,
                            revision: "42".into(),
                        })
                    }
                },
            ),
        )
        .route(
            "/v1/health",
            get(|| async {
                Json(HealthDto {
                    status: "ok".into(),
                    protocol_version: tracker_protocol::VERSION,
                })
            }),
        )
        .route(
            "/v1/tasks",
            get(move || {
                let fail = reads.fetch_add(1, Ordering::SeqCst) > 0;
                let snapshot = snapshot.clone();
                async move {
                    if fail {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(ErrorDto {
                                code: ErrorCode::Internal,
                                message: "Recovery unavailable".into(),
                            }),
                        )
                            .into_response()
                    } else {
                        Json(tracker_protocol::TasksDto {
                            tasks: snapshot
                                .task_items
                                .into_iter()
                                .map(|item| {
                                    let mut task = item.task;
                                    task.latest_work_start = item.latest_work_start;
                                    task
                                })
                                .collect(),
                            revision: snapshot.revision,
                        })
                        .into_response()
                    }
                }
            }),
        )
        .route(
            "/v1/tracking",
            get(|| async {
                Json(tracker_protocol::TrackingDto {
                    active_worklog: None,
                    revision: "42".into(),
                })
            }),
        )
        .route(
            "/v1/worklogs/{id}",
            get(move || {
                let worklog = original_dto.clone();
                async move {
                    Json(tracker_protocol::WorklogResourceDto {
                        worklog,
                        revision: "42".into(),
                    })
                }
            })
            .patch(|| async {
                (
                    StatusCode::CONFLICT,
                    Json(ErrorDto {
                        code: ErrorCode::WorklogChanged,
                        message: "Worklog changed".into(),
                    }),
                )
            }),
        );
    let server = report_tests::Server::start(router);
    let mut bridge = server.client();
    bridge.application.refresh().unwrap();
    let failure = move_to(&mut bridge, &original, tasks[1].task.id());
    assert_eq!(failure["kind"], "unavailable");
    assert_eq!(failure["uncertain"], true);
    assert_eq!(failure["requiresRefresh"], true);
}

#[test]
fn move_receipts_allow_a_newer_timer_and_destination_state_from_recovery_reads() {
    use axum::{
        Json, Router,
        routing::{get, patch},
    };
    use std::sync::{Arc, Mutex};
    use tracker_protocol::{MutationDto, MutationResultDto, SnapshotDto, WorklogDto};
    let directory = tempfile::tempdir().unwrap();
    for end in [Some(200), None] {
        let mut writer =
            open_fixture(&directory.path().join(format!("recovery-{end:?}.db"))).unwrap();
        let tasks = writer.application.tasks(TaskOrdering::default());
        let source = tasks[0].task.id();
        let destination = tasks[1].task.id();
        let competing_task = tasks[2].task.id();
        let original = record(&mut writer, source, 100, end);
        let moved = original.moved_to(destination).unwrap();
        let initial = SnapshotDto::from_snapshot(
            &tracker_application::TrackerSnapshot {
                task_items: writer.application.tasks(TaskOrdering::default()),
                active_worklog: end.is_none().then(|| original.clone()),
            },
            "before".into(),
        );
        let state = Arc::new(Mutex::new(initial));
        let worklogs = Arc::new(Mutex::new(vec![WorklogDto::from(&original)]));
        let changed_state = state.clone();
        let changed_worklogs = worklogs.clone();
        let applied = moved.clone();
        let later_active = Worklog::begin(WorklogId::generate(), competing_task, at(250));
        let expected_active = later_active.clone();
        let router = Router::new()
            .route(
                "/v1/health",
                get(|| async {
                    Json(tracker_protocol::HealthDto {
                        status: "ok".into(),
                        protocol_version: tracker_protocol::VERSION,
                    })
                }),
            )
            .merge(report_tests::resource_router_resources(state, worklogs))
            .route(
                "/v1/worklogs/{id}",
                patch(move |Json(body): Json<Value>| {
                    let changed_state = changed_state.clone();
                    let changed_worklogs = changed_worklogs.clone();
                    let applied = applied.clone();
                    let later_active = later_active.clone();
                    async move {
                        {
                            let mut state = changed_state.lock().unwrap();
                            state.active_worklog = Some(WorklogDto::from(&later_active));
                            state
                                .task_items
                                .iter_mut()
                                .find(|item| item.task.id == destination.to_string())
                                .unwrap()
                                .task
                                .archived = true;
                            state.revision = "later".into();
                        }
                        changed_worklogs.lock().unwrap()[0] = WorklogDto::from(
                            &Worklog::new(
                                applied.id(),
                                destination,
                                applied.start(),
                                Some(at(250)),
                            )
                            .unwrap(),
                        );
                        Json(MutationDto {
                            request_id: body["request_id"].as_str().unwrap().into(),
                            applied_revision: "applied".into(),
                            replayed: true,
                            result: MutationResultDto::Worklog(WorklogDto::from(&applied)),
                        })
                    }
                }),
            );
        let server = report_tests::Server::start(router);
        let mut bridge = server.client();
        let result = move_to(&mut bridge, &original, destination);
        assert!(result.get("error").is_none(), "{end:?}: {result}");
        assert_eq!(result["data"]["worklog"]["id"], moved.id().to_string());
        assert_eq!(
            result["data"]["snapshot"]["active"]["id"],
            expected_active.id().to_string()
        );
        assert_eq!(result["data"]["snapshot"]["tasksRevision"], "later");
        assert_eq!(result["data"]["snapshot"]["trackingRevision"], "later");
        assert_eq!(
            bridge
                .application
                .tasks(TaskOrdering::default())
                .iter()
                .find(|item| item.task.id() == destination)
                .unwrap()
                .task
                .is_archived(),
            true
        );
    }
}
