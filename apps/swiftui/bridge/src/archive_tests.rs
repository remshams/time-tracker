use super::*;

fn response(pointer: *mut c_char) -> Value {
    assert!(!pointer.is_null());
    // SAFETY: This test owns the returned string and releases it once.
    unsafe {
        let value = serde_json::from_str(CStr::from_ptr(pointer).to_str().unwrap()).unwrap();
        tt_bridge_string_free(pointer);
        value
    }
}

fn at(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 123_456_000).unwrap()
}

/// Archives under the cached name, as the macOS app does, or restores the task.
fn change(bridge: &mut Bridge, id: TaskId, archived: bool) -> Value {
    let expected =
        CString::new(crate::tests::cached_task_name(&bridge.application, id).as_str()).unwrap();
    let id = CString::new(id.to_string()).unwrap();
    let instant = CString::new(timestamp(at(400))).unwrap();
    // SAFETY: The bridge and strings remain live with exclusive access.
    response(unsafe {
        if archived {
            tt_bridge_archive_task_with_expected_name_at(
                bridge,
                id.as_ptr(),
                expected.as_ptr(),
                instant.as_ptr(),
            )
        } else {
            tt_bridge_unarchive_task_at(bridge, id.as_ptr(), instant.as_ptr())
        }
    })
}

#[test]
fn archive_and_restore_preserve_history_other_tracking_and_concurrent_names() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("archive.db");
    let mut bridge = open_fixture(&path).unwrap();
    let tasks = bridge.application.tasks(TaskOrdering::default());
    let target = tasks[0].task.id();
    bridge.application.set_active_task(target, at(100)).unwrap();
    let TrackingState::Running { worklog } = bridge.application.current_tracking() else {
        panic!("tracking should be running")
    };
    bridge
        .application
        .clear_active_task(worklog.id(), at(200))
        .unwrap();
    let history = bridge
        .application
        .worklogs_for_task(target, None)
        .unwrap()
        .worklogs;
    let mut writer = TrackerApplication::load(SqliteRepository::open(&path).unwrap()).unwrap();

    writer
        .rename_task(target, TaskName::new("Concurrent rename").unwrap(), at(250))
        .unwrap();
    writer.set_active_task(tasks[1].task.id(), at(300)).unwrap();
    let expected_active = writer.current_tracking().clone();
    let stale = change(&mut bridge, target, true);
    assert_eq!(stale["kind"], "conflict", "{stale}");
    assert_eq!(stale["requiresRefresh"], true);
    assert!(!writer.task(target).unwrap().is_archived());
    for archived in [true, true, false, false] {
        let result = change(&mut bridge, target, archived);
        assert!(result.get("error").is_none(), "{result}");
        let item = &result["data"]["task"];
        assert_eq!(item["name"], "Concurrent rename");
        assert_eq!(item["archived"], archived);
        let observed_tasks = crate::tests::test_task_values(&bridge.application);
        let observed = observed_tasks
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["id"] == target.to_string())
            .unwrap();
        assert_eq!(observed["latestStart"], timestamp(at(100)));
        assert_eq!(bridge.application.current_tracking(), &expected_active);
        assert_eq!(
            bridge
                .application
                .worklogs_for_task(target, None)
                .unwrap()
                .worklogs,
            history
        );
    }
    drop(bridge);
    let stored = TrackerApplication::load(SqliteRepository::open(path).unwrap()).unwrap();
    assert!(!stored.task(target).unwrap().is_archived());
    assert_eq!(
        stored.task(target).unwrap().name().as_str(),
        "Concurrent rename"
    );
    assert_eq!(stored.task(target).unwrap().updated_at(), at(400));
    assert_eq!(stored.current_tracking(), &expected_active);
}

#[test]
fn concurrent_running_task_and_missing_target_have_typed_errors() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("running.db");
    let mut bridge = open_fixture(&path).unwrap();
    let id = bridge.application.tasks(TaskOrdering::default())[0]
        .task
        .id();
    let mut writer = open_at(&path).unwrap();
    writer.application.set_active_task(id, at(100)).unwrap();
    let failure = change(&mut bridge, id, true);
    assert_eq!(failure["kind"], "task_active");
    assert_eq!(failure["uncertain"], false);
    assert_eq!(failure["requiresRefresh"], true);
    assert_eq!(
        bridge.application.current_tracking(),
        writer.application.current_tracking()
    );
    for archived in [true, false] {
        let failure = change(&mut bridge, TaskId::generate(), archived);
        assert_eq!(failure["kind"], "task_not_found");
        assert_eq!(failure["uncertain"], false);
        assert_eq!(failure["requiresRefresh"], true);
    }
}

#[test]
fn archive_and_restore_reject_invalid_c_inputs_without_mutating_state() {
    let directory = tempfile::tempdir().unwrap();
    let mut bridge = open_fixture(&directory.path().join("inputs.db")).unwrap();
    let before = crate::tests::test_resource_values(&bridge.application);
    let task = bridge.application.tasks(TaskOrdering::default())[0]
        .task
        .clone();
    let id = CString::new(task.id().to_string()).unwrap();
    let expected = CString::new(task.name().as_str()).unwrap();
    let instant = CString::new(timestamp(at(400))).unwrap();
    let malformed = CString::new("invalid").unwrap();
    let non_utf8 = CString::new(vec![0xff]).unwrap();
    let archive = |bridge, id, at| {
        // SAFETY: The caller passes live C strings or null and exclusive bridge access.
        unsafe { tt_bridge_archive_task_with_expected_name_at(bridge, id, expected.as_ptr(), at) }
    };
    let restore = |bridge, id, at| {
        // SAFETY: The caller passes live C strings or null and exclusive bridge access.
        unsafe { tt_bridge_unarchive_task_at(bridge, id, at) }
    };
    let operations: [&dyn Fn(*mut Bridge, *const c_char, *const c_char) -> *mut c_char; 2] =
        [&archive, &restore];
    for operation in operations {
        assert_eq!(
            response(operation(ptr::null_mut(), id.as_ptr(), instant.as_ptr()))["error"],
            "Database is not open"
        );
        for invalid in [ptr::null(), malformed.as_ptr(), non_utf8.as_ptr()] {
            assert_eq!(
                response(operation(&mut bridge, invalid, instant.as_ptr()))["error"],
                "Invalid task ID"
            );
            assert_eq!(
                response(operation(&mut bridge, id.as_ptr(), invalid))["error"],
                "Invalid archive timestamp"
            );
        }
    }
    assert_eq!(
        crate::tests::test_resource_values(&bridge.application),
        before
    );
}

#[test]
fn remote_archive_and_restore_require_refresh_and_preserve_committed_tracking_and_names() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("remote.db");
    let mut writer = open_fixture(&path).unwrap();
    let tasks = writer.application.tasks(TaskOrdering::default());
    let target = tasks[0].task.id();
    writer
        .application
        .set_active_task(tasks[1].task.id(), at(100))
        .unwrap();
    let server = report_tests::Server::start(tracker_server::router_for_database(&path).unwrap());
    let mut bridge = server.client();
    assert_eq!(change(&mut bridge, target, true)["kind"], "unavailable");
    bridge.application.refresh_resources().unwrap();
    let mut second = server.client();
    second.application.refresh_resources().unwrap();
    second
        .application
        .rename_task_with_expected_name(
            target,
            tasks[0].task.name().clone(),
            TaskName::new("Server rename").unwrap(),
            at(200),
        )
        .unwrap();
    let stale = change(&mut bridge, target, true);
    assert_eq!(stale["kind"], "conflict");
    assert_eq!(stale["uncertain"], false);
    assert_eq!(stale["requiresRefresh"], true);
    let expected_active = writer.application.current_tracking().clone();
    for archived in [true, true, false, false] {
        bridge.application.refresh_resources().unwrap();
        let result = change(&mut bridge, target, archived);
        assert!(result.get("error").is_none(), "{result}");
        let task = &result["data"]["task"];
        assert_eq!(task["name"], "Server rename");
        assert_eq!(task["archived"], archived);
        assert_eq!(bridge.application.current_tracking(), &expected_active);
    }
    bridge.application.refresh_resources().unwrap();
    let failure = change(&mut bridge, tasks[1].task.id(), true);
    assert_eq!(failure["kind"], "task_active");
    assert_eq!(failure["uncertain"], false);
    assert_eq!(failure["requiresRefresh"], true);
    bridge.application.refresh_resources().unwrap();
    assert_eq!(
        change(&mut bridge, TaskId::generate(), false)["kind"],
        "task_not_found"
    );
}

fn mock_server(
    initial: (tracker_protocol::TasksDto, tracker_protocol::TrackingDto),
    reply: Value,
    requests: std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
) -> report_tests::Server {
    use axum::{
        Json, Router,
        routing::{get, patch},
    };
    let state = std::sync::Arc::new(std::sync::Mutex::new(initial));
    let write_state = state.clone();
    report_tests::Server::start(
        Router::new()
            .route(
                "/v1/health",
                get(|| async {
                    Json(tracker_protocol::HealthDto {
                        status: "ok".into(),
                        protocol_version: tracker_protocol::VERSION,
                    })
                }),
            )
            .merge(report_tests::resource_router_state(state, vec![]))
            .route(
                "/v1/tasks/{id}",
                patch(move |Json(body): Json<Value>| {
                    let reply = reply.clone();
                    let requests = requests.clone();
                    let state = write_state.clone();
                    async move {
                        requests.lock().unwrap().push(body.clone());
                        let mut reply = reply;
                        if reply["request_id"] == "" {
                            reply["request_id"] = body["request_id"].clone();
                        }
                        if let Ok(receipt) =
                            serde_json::from_value::<tracker_protocol::MutationDto>(reply.clone())
                            && let tracker_protocol::MutationResultDto::Task(task) = receipt.result
                        {
                            let mut resources = state.lock().unwrap();
                            if let Some(item) =
                                resources.0.tasks.iter_mut().find(|item| item.id == task.id)
                            {
                                *item = task;
                            }
                        }
                        Json(reply)
                    }
                }),
            ),
    )
}

#[test]
fn remote_archive_payload_reuses_public_commands_and_rejects_contradictory_successes() {
    use std::sync::{Arc, Mutex};
    use tracker_protocol::{MutationDto, MutationResultDto, TaskDto};
    let directory = tempfile::tempdir().unwrap();
    for archived in [true, false] {
        let mut writer =
            open_fixture(&directory.path().join(format!("protocol-{archived}.db"))).unwrap();
        let tasks = writer.application.tasks(TaskOrdering::default());
        let target = tasks[0].task.id();
        let reviewed_name = tasks[0].task.name().as_str().to_owned();
        if !archived {
            crate::tests::archive_cached_task(&mut writer.application, target, at(50)).unwrap();
        }
        writer
            .application
            .set_active_task(tasks[1].task.id(), at(100))
            .unwrap();
        let TrackingState::Running { worklog } = writer.application.current_tracking() else {
            panic!("tracking should be running")
        };
        let initial = report_tests::resource_dtos(
            writer.application.tasks(TaskOrdering::default()),
            Some(tracker_domain::Worklog::begin(
                worklog.id(),
                worklog.task_id(),
                worklog.start(),
            )),
        );
        let mut updated = initial.clone();
        let task = updated
            .0
            .tasks
            .iter_mut()
            .find(|item| item.id == target.to_string())
            .unwrap();
        task.archived = archived;
        task.updated_at = at(400);
        let result_task: TaskDto = task.clone();
        let baseline = serde_json::to_value(MutationDto {
            result: MutationResultDto::Task(result_task),
            request_id: String::new(),
            replayed: false,
        })
        .unwrap();
        for index in 0..8 {
            let mut reply = baseline.clone();
            match index {
                0 => {}
                1 => reply["result"]["value"]["id"] = json!(TaskId::generate().to_string()),
                2 => reply["result"]["value"]["archived"] = json!(!archived),
                3 => reply["request_id"] = json!(WorklogId::generate().to_string()),
                4 => {
                    reply.as_object_mut().unwrap().remove("replayed");
                }
                5 => reply["unexpected"] = json!(true),
                6 => reply["replayed"] = json!("invalid"),
                7 => reply["result"]["kind"] = json!("tracking_already_idle"),
                _ => unreachable!(),
            }
            let requests = Arc::new(Mutex::new(Vec::new()));
            let server = mock_server(initial.clone(), reply, requests.clone());
            let mut bridge = server.client();
            bridge.application.refresh_resources().unwrap();
            let response = change(&mut bridge, target, archived);
            if index == 0 {
                assert!(response.get("error").is_none(), "{response}");
                assert_eq!(
                    crate::tests::test_active_value(&bridge.application)["id"],
                    worklog.id().to_string()
                );
                assert_eq!(requests.lock().unwrap().len(), 1);
                let body = &requests.lock().unwrap()[0];
                assert_eq!(body["action"], if archived { "archive" } else { "restore" });
                assert_eq!(body["occurred_at"], timestamp(at(400)));
                assert!(body.get("expected_revision").is_none());
                assert!(body["request_id"].as_str().is_some());
                if archived {
                    assert_eq!(body["expected_name"], reviewed_name.as_str());
                    assert_eq!(body.as_object().unwrap().len(), 4);
                } else {
                    assert_eq!(body.as_object().unwrap().len(), 3);
                }
            } else {
                assert_eq!(response["kind"], "protocol", "{index}: {response}");
                assert_eq!(response["uncertain"], true);
                assert_eq!(response["requiresRefresh"], true);
                assert!(response.get("data").is_none());
                let blocked = change(&mut bridge, target, archived);
                assert_eq!(blocked["kind"], "unavailable");
                assert_eq!(blocked["uncertain"], false);
                assert_eq!(requests.lock().unwrap().len(), 1);
                bridge.application.refresh_resources().unwrap();
            }
        }
    }
}

#[test]
fn unavailable_archive_and_restore_keep_confirmed_state_and_block_writes_until_refresh() {
    let directory = tempfile::tempdir().unwrap();
    for archived in [true, false] {
        let path = directory.path().join(format!("offline-{archived}.db"));
        let mut writer = open_fixture(&path).unwrap();
        let target = writer.application.tasks(TaskOrdering::default())[0]
            .task
            .id();
        if !archived {
            crate::tests::archive_cached_task(&mut writer.application, target, at(100)).unwrap();
        }
        let server =
            report_tests::Server::start(tracker_server::router_for_database(&path).unwrap());
        let mut bridge = server.client();
        bridge.application.refresh_resources().unwrap();
        let before = crate::tests::test_resource_values(&bridge.application);
        drop(server);
        let failure = change(&mut bridge, target, archived);
        assert_eq!(failure["kind"], "unavailable");
        assert_eq!(failure["uncertain"], false);
        assert_eq!(failure["requiresRefresh"], true);
        assert_eq!(
            crate::tests::test_resource_values(&bridge.application),
            before
        );
        let blocked = change(&mut bridge, target, archived);
        assert_eq!(blocked["uncertain"], false);
        assert_eq!(blocked["requiresRefresh"], true);
        assert!(bridge.application.refresh_resources().is_err());
    }
}

#[test]
fn task_write_recovery_adopts_an_aggregate_or_preserves_the_last_coherent_pair() {
    use axum::{Json, Router, routing::get};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    use tracker_protocol::{
        HealthDto, MutationDto, MutationResultDto, TaskDto, TaskResourceDto, WorklogDto,
    };

    for settles in [false, true] {
        let mut application =
            TrackerApplication::load(SqliteRepository::open_in_memory().unwrap()).unwrap();
        let task = application
            .create_task(TaskName::new("Original name").unwrap(), at(1))
            .unwrap();
        let initial = TaskDto::from(&application.tasks(TaskOrdering::default())[0]);
        let mut applied = initial.clone();
        applied.name = "Requested name".into();
        applied.updated_at = at(400);
        let active = WorklogDto {
            id: WorklogId::generate().to_string(),
            task_id: task.id().to_string(),
            start: at(399),
            end: None,
        };
        let mut latest = applied.clone();
        latest.latest_work_start = Some(active.start);
        let written = Arc::new(AtomicBool::new(false));
        let attempts = Arc::new(AtomicUsize::new(0));
        let task_written = written.clone();
        let task_attempts = attempts.clone();
        let original_task = initial.clone();

        let write_written = written.clone();
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
            .route(
                "/v1/state/task-list",
                get(move || {
                    let after = task_written.load(Ordering::SeqCst);
                    if after {
                        task_attempts.fetch_add(1, Ordering::SeqCst);
                    }
                    let dto = tracker_protocol::TaskListStateDto {
                        tasks: vec![if after {
                            latest.clone()
                        } else {
                            original_task.clone()
                        }],
                        active_worklog: after.then(|| active.clone()),
                    };
                    async move {
                        (
                            if after && !settles {
                                axum::http::StatusCode::CONFLICT
                            } else {
                                axum::http::StatusCode::OK
                            },
                            Json(dto),
                        )
                    }
                }),
            )
            .route(
                "/v1/tasks/{id}",
                get(move || {
                    let task = initial.clone();
                    async move { Json(TaskResourceDto { task }) }
                })
                .patch(move |Json(body): Json<Value>| {
                    write_written.store(true, Ordering::SeqCst);
                    let task = applied.clone();
                    async move {
                        Json(MutationDto {
                            request_id: body["request_id"].as_str().unwrap().into(),
                            replayed: false,
                            result: MutationResultDto::Task(task),
                        })
                    }
                }),
            );
        let server = report_tests::Server::start(router);
        let mut bridge = server.client();
        bridge.application.refresh_resources().unwrap();
        let before = crate::tests::test_resource_values(&bridge.application);
        let result = bridge.application.rename_task_with_expected_name(
            task.id(),
            TaskName::new("Original name").unwrap(),
            TaskName::new("Requested name").unwrap(),
            at(400),
        );
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        if settles {
            assert!(result.is_ok());
            assert_eq!(
                bridge.application.tasks(TaskOrdering::default())[0]
                    .task
                    .name()
                    .as_str(),
                "Requested name"
            );
            assert!(matches!(
                bridge.application.current_tracking(),
                TrackingState::Running { .. }
            ));
        } else {
            assert!(result.is_ok());
            let error = bridge.application.refresh_resources().unwrap_err();
            assert!(error.requires_refresh);
            assert!(!error.uncertain);
            assert_eq!(
                crate::tests::test_resource_values(&bridge.application),
                before
            );
        }
    }
}

#[test]
fn task_receipts_allow_newer_metadata_and_archive_state_from_recovery_reads() {
    use axum::{
        Json, Router,
        routing::{get, patch},
    };
    use std::sync::{Arc, Mutex};

    use tracker_protocol::{MutationDto, MutationResultDto, TaskDto};
    for action in ["rename", "archive", "restore"] {
        for replayed in [false, true] {
            let repository = SqliteRepository::open_in_memory().unwrap();
            let mut application = TrackerApplication::load(repository).unwrap();
            let task = application
                .create_task(TaskName::new("Original project").unwrap(), at(1))
                .unwrap();
            if action == "restore" {
                application.archive_task(task.id(), at(2)).unwrap();
            }
            let initial =
                report_tests::resource_dtos(application.tasks(TaskOrdering::default()), None);
            let state = Arc::new(Mutex::new(initial));
            let changed = state.clone();
            let mut committed_task = state.lock().unwrap().0.tasks[0].clone();
            committed_task.updated_at = at(400);
            if action == "rename" {
                committed_task.name = "Requested name".into();
            } else {
                committed_task.archived = action == "archive";
            }
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
                .merge(report_tests::resource_router_state(state, vec![]))
                .route(
                    "/v1/tasks/{id}",
                    patch(move |Json(body): Json<Value>| {
                        let changed = changed.clone();
                        let committed_task = committed_task.clone();
                        async move {
                            let mut latest: TaskDto = committed_task.clone();
                            latest.name = "Later writer name".into();
                            latest.archived = if action == "rename" {
                                latest.archived
                            } else {
                                !latest.archived
                            };
                            latest.updated_at = at(401);
                            {
                                let mut state = changed.lock().unwrap();
                                state.0.tasks[0] = latest;
                            }
                            Json(MutationDto {
                                request_id: body["request_id"].as_str().unwrap().into(),
                                replayed,
                                result: MutationResultDto::Task(committed_task),
                            })
                        }
                    }),
                );
            let server = report_tests::Server::start(router);
            let mut bridge = server.client();
            bridge.application.refresh_resources().unwrap();
            let value = if action == "rename" {
                let id = CString::new(task.id().to_string()).unwrap();
                let expected = c"Original project";
                let name = CString::new("Requested name").unwrap();
                let timestamp = CString::new(timestamp(at(400))).unwrap();
                // SAFETY: The bridge and input strings are live for this call.
                response(unsafe {
                    tt_bridge_rename_task_with_expected_name_at(
                        &mut bridge,
                        id.as_ptr(),
                        expected.as_ptr(),
                        name.as_ptr(),
                        timestamp.as_ptr(),
                    )
                })
            } else {
                change(&mut bridge, task.id(), action == "archive")
            };
            assert!(
                value.get("error").is_none(),
                "{action}, replayed={replayed}: {value}"
            );
            assert_eq!(
                value["data"]["task"]["name"],
                if action == "rename" {
                    "Requested name"
                } else {
                    "Original project"
                }
            );
            if action != "rename" {
                assert_eq!(value["data"]["task"]["archived"], action == "archive");
            }
            assert!(value["data"]["receipt"].get("appliedRevision").is_none());
            assert_eq!(value["data"]["receipt"]["replayed"], replayed);
            assert_eq!(
                crate::tests::test_task_values(&bridge.application)[0]["name"],
                "Later writer name"
            );
            if action != "rename" {
                assert_eq!(
                    crate::tests::test_task_values(&bridge.application)[0]["archived"],
                    action != "archive"
                );
            }
        }
    }
}

#[test]
fn confirmed_receipt_survives_failed_recovery_and_blocks_writes_until_a_coherent_refresh() {
    use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::get};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    use tracker_protocol::{
        ErrorCode, ErrorDto, HealthDto, MutationDto, MutationResultDto, TaskResourceDto,
    };
    let repository = SqliteRepository::open_in_memory().unwrap();
    let mut application = TrackerApplication::load(repository).unwrap();
    let task = application
        .create_task(TaskName::new("Before name").unwrap(), at(1))
        .unwrap();
    let state = Arc::new(Mutex::new(report_tests::resource_dtos(
        application.tasks(TaskOrdering::default()),
        None,
    )));
    let tasks_state = state.clone();
    let tracking_state = state.clone();
    let task_state = state.clone();
    let write_state = state.clone();
    let failed_reads = Arc::new(AtomicBool::new(false));
    let tracking_failures = failed_reads.clone();
    let write_failures = failed_reads.clone();
    let writes = Arc::new(AtomicUsize::new(0));
    let write_count = writes.clone();
    let server = report_tests::Server::start(
        Router::new()
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
                "/v1/state/task-list",
                get({
                    let state = state.clone();
                    let failures = failed_reads.clone();
                    move || {
                        let state = state.lock().unwrap();
                        let dto = tracker_protocol::TaskListStateDto {
                            tasks: state.0.tasks.clone(),
                            active_worklog: state.1.active_worklog.clone(),
                        };
                        let fail = failures.load(Ordering::SeqCst);
                        async move {
                            (
                                if fail {
                                    StatusCode::INTERNAL_SERVER_ERROR
                                } else {
                                    StatusCode::OK
                                },
                                Json(dto),
                            )
                        }
                    }
                }),
            )
            .route(
                "/v1/tasks",
                get(move || {
                    let tasks = tasks_state.lock().unwrap().0.clone();
                    async move { Json(tasks) }
                }),
            )
            .route(
                "/v1/tracking",
                get(move || {
                    let tracking = tracking_state.lock().unwrap().1.clone();
                    let fail = tracking_failures.load(Ordering::SeqCst);
                    async move {
                        if fail {
                            (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                Json(ErrorDto {
                                    code: ErrorCode::Internal,
                                    message: "Refresh unavailable".into(),
                                }),
                            )
                                .into_response()
                        } else {
                            Json(tracking).into_response()
                        }
                    }
                }),
            )
            .route(
                "/v1/tasks/{id}",
                get(move || {
                    let tasks = task_state.lock().unwrap().0.clone();
                    async move {
                        Json(TaskResourceDto {
                            task: tasks.tasks[0].clone(),
                        })
                    }
                })
                .patch(move |Json(body): Json<Value>| {
                    write_count.fetch_add(1, Ordering::SeqCst);
                    let mut state = write_state.lock().unwrap();
                    state.0.tasks[0].name = body["name"].as_str().unwrap().to_owned();
                    state.0.tasks[0].updated_at =
                        body["occurred_at"].as_str().unwrap().parse().unwrap();
                    let result = MutationDto {
                        request_id: body["request_id"].as_str().unwrap().into(),
                        replayed: false,
                        result: MutationResultDto::Task(state.0.tasks[0].clone()),
                    };
                    write_failures.store(true, Ordering::SeqCst);
                    async move { Json(result) }
                }),
            ),
    );
    let mut bridge = server.client();
    bridge.application.refresh_resources().unwrap();
    let confirmed_before = crate::tests::test_resource_values(&bridge.application);
    let id = CString::new(task.id().to_string()).unwrap();
    let expected = c"Before name";
    let name = c"Committed name";
    let occurred_at = CString::new(timestamp(at(400))).unwrap();
    // SAFETY: The bridge and all input strings remain live during the call.
    let result = response(unsafe {
        tt_bridge_rename_task_with_expected_name_at(
            &mut bridge,
            id.as_ptr(),
            expected.as_ptr(),
            name.as_ptr(),
            occurred_at.as_ptr(),
        )
    });

    assert!(result.get("error").is_none(), "{result}");
    assert_eq!(result["data"]["task"]["name"], "Committed name");
    assert_eq!(result["data"]["receipt"]["replayed"], false);
    assert!(result["data"]["receipt"].get("appliedRevision").is_none());
    assert_eq!(
        crate::tests::test_resource_values(&bridge.application),
        confirmed_before
    );
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    let blocked = bridge
        .application
        .rename_task_with_expected_name(
            task.id(),
            TaskName::new("Committed name").unwrap(),
            TaskName::new("Next name").unwrap(),
            at(500),
        )
        .unwrap_err();
    assert_eq!(blocked.kind, "unavailable");
    assert!(blocked.requires_refresh);
    assert!(!blocked.uncertain);
    assert_eq!(writes.load(Ordering::SeqCst), 1);

    failed_reads.store(false, Ordering::SeqCst);
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    bridge.application.refresh_resources().unwrap();
    let next = bridge
        .application
        .rename_task_with_expected_name(
            task.id(),
            TaskName::new("Committed name").unwrap(),
            TaskName::new("Next name").unwrap(),
            at(500),
        )
        .unwrap();
    assert_eq!(next.name().as_str(), "Next name");
    assert_eq!(writes.load(Ordering::SeqCst), 2);
}
