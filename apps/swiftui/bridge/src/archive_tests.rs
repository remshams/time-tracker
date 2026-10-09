use super::*;
use tracker_application::TaskQueries;

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

fn change(bridge: &mut Bridge, id: TaskId, archived: bool) -> Value {
    let id = CString::new(id.to_string()).unwrap();
    let instant = CString::new(timestamp(at(400))).unwrap();
    // SAFETY: The bridge and strings remain live with exclusive access.
    response(unsafe {
        if archived {
            tt_bridge_archive_task_at(bridge, id.as_ptr(), instant.as_ptr())
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
    use tracker_application::{TaskOperations, TrackingOperations};
    writer
        .rename_task(target, TaskName::new("Concurrent rename").unwrap(), at(250))
        .unwrap();
    writer.set_active_task(tasks[1].task.id(), at(300)).unwrap();
    let expected_active = writer.current_tracking().clone();
    for archived in [true, true, false, false] {
        let result = change(&mut bridge, target, archived);
        assert!(result.get("error").is_none(), "{result}");
        let item = result["data"]["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["id"] == target.to_string())
            .unwrap();
        assert_eq!(item["name"], "Concurrent rename");
        assert_eq!(item["archived"], archived);
        assert_eq!(item["latestStart"], timestamp(at(100)));
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
    let before = serde_json::to_value(snapshot(&bridge.application)).unwrap();
    let id = CString::new(
        bridge.application.tasks(TaskOrdering::default())[0]
            .task
            .id()
            .to_string(),
    )
    .unwrap();
    let instant = CString::new(timestamp(at(400))).unwrap();
    let malformed = CString::new("invalid").unwrap();
    let non_utf8 = CString::new(vec![0xff]).unwrap();
    for operation in [tt_bridge_archive_task_at, tt_bridge_unarchive_task_at] {
        // SAFETY: Every non-null input is a live C string and access is exclusive.
        unsafe {
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
    }
    assert_eq!(
        serde_json::to_value(snapshot(&bridge.application)).unwrap(),
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
    bridge.application.refresh().unwrap();
    let mut second = server.client();
    second.application.refresh().unwrap();
    second
        .application
        .rename_task(target, TaskName::new("Server rename").unwrap(), at(200))
        .unwrap();
    let stale = change(&mut bridge, target, true);
    assert_eq!(stale["kind"], "general");
    assert_eq!(stale["uncertain"], false);
    assert_eq!(stale["requiresRefresh"], true);
    let expected_active = writer.application.current_tracking().clone();
    for archived in [true, true, false, false] {
        bridge.application.refresh().unwrap();
        let result = change(&mut bridge, target, archived);
        assert!(result.get("error").is_none(), "{result}");
        let task = result["data"]["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["id"] == target.to_string())
            .unwrap();
        assert_eq!(task["name"], "Server rename");
        assert_eq!(task["archived"], archived);
        assert_eq!(bridge.application.current_tracking(), &expected_active);
    }
    bridge.application.refresh().unwrap();
    let failure = change(&mut bridge, tasks[1].task.id(), true);
    assert_eq!(failure["kind"], "task_active");
    assert_eq!(failure["uncertain"], false);
    assert_eq!(failure["requiresRefresh"], true);
    bridge.application.refresh().unwrap();
    assert_eq!(
        change(&mut bridge, TaskId::generate(), false)["kind"],
        "task_not_found"
    );
}

fn mock_server(
    initial: tracker_protocol::SnapshotDto,
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
                            let mut snapshot = state.lock().unwrap();
                            if let Some(item) = snapshot
                                .task_items
                                .iter_mut()
                                .find(|item| item.task.id == task.id)
                            {
                                item.task = task;
                            }
                            if !receipt.applied_revision.is_empty() {
                                snapshot.revision = receipt.applied_revision;
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
    use tracker_protocol::{MutationDto, MutationResultDto, SnapshotDto, TaskDto};
    let directory = tempfile::tempdir().unwrap();
    for archived in [true, false] {
        let mut writer =
            open_fixture(&directory.path().join(format!("protocol-{archived}.db"))).unwrap();
        let tasks = writer.application.tasks(TaskOrdering::default());
        let target = tasks[0].task.id();
        if !archived {
            writer.application.archive_task(target, at(50)).unwrap();
        }
        writer
            .application
            .set_active_task(tasks[1].task.id(), at(100))
            .unwrap();
        let TrackingState::Running { worklog } = writer.application.current_tracking() else {
            panic!("tracking should be running")
        };
        let initial = SnapshotDto::from_snapshot(
            &tracker_application::TrackerSnapshot {
                task_items: writer.application.tasks(TaskOrdering::default()),
                active_worklog: Some(tracker_domain::Worklog::begin(
                    worklog.id(),
                    worklog.task_id(),
                    worklog.start(),
                )),
            },
            "42".into(),
        );
        let mut updated = initial.clone();
        updated.revision = "43".into();
        let task = updated
            .task_items
            .iter_mut()
            .find(|item| item.task.id == target.to_string())
            .unwrap();
        task.task.archived = archived;
        task.task.updated_at = at(400);
        let result_task: TaskDto = task.task.clone();
        let baseline = serde_json::to_value(MutationDto {
            result: MutationResultDto::Task(result_task),
            request_id: String::new(),
            applied_revision: updated.revision,
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
                4 => reply["applied_revision"] = json!(""),
                5 => reply["unexpected"] = json!(true),
                6 => reply["replayed"] = json!("invalid"),
                7 => reply["result"]["kind"] = json!("tracking_already_idle"),
                _ => unreachable!(),
            }
            let requests = Arc::new(Mutex::new(Vec::new()));
            let server = mock_server(initial.clone(), reply, requests.clone());
            let mut bridge = server.client();
            bridge.application.refresh().unwrap();
            let response = change(&mut bridge, target, archived);
            if index == 0 {
                assert!(response.get("error").is_none(), "{response}");
                assert_eq!(response["data"]["active"]["id"], worklog.id().to_string());
                assert_eq!(requests.lock().unwrap().len(), 1);
                let body = &requests.lock().unwrap()[0];
                assert_eq!(body["action"], if archived { "archive" } else { "restore" });
                assert_eq!(body["occurred_at"], timestamp(at(400)));
                assert_eq!(body["expected_revision"], "42");
                assert!(body["request_id"].as_str().is_some());
                assert_eq!(body.as_object().unwrap().len(), 4);
            } else {
                assert_eq!(response["kind"], "protocol", "{index}: {response}");
                assert_eq!(response["uncertain"], true);
                assert_eq!(response["requiresRefresh"], true);
                assert!(response.get("data").is_none());
                let blocked = change(&mut bridge, target, archived);
                assert_eq!(blocked["kind"], "unavailable");
                assert_eq!(blocked["uncertain"], false);
                assert_eq!(requests.lock().unwrap().len(), 1);
                bridge.application.refresh().unwrap();
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
            writer.application.archive_task(target, at(100)).unwrap();
        }
        let server =
            report_tests::Server::start(tracker_server::router_for_database(&path).unwrap());
        let mut bridge = server.client();
        bridge.application.refresh().unwrap();
        let before = serde_json::to_value(snapshot(&bridge.application)).unwrap();
        drop(server);
        let failure = change(&mut bridge, target, archived);
        assert_eq!(failure["kind"], "unavailable");
        assert_eq!(failure["uncertain"], false);
        assert_eq!(failure["requiresRefresh"], true);
        assert_eq!(
            serde_json::to_value(snapshot(&bridge.application)).unwrap(),
            before
        );
        let blocked = change(&mut bridge, target, archived);
        assert_eq!(blocked["uncertain"], false);
        assert_eq!(blocked["requiresRefresh"], true);
        assert!(bridge.application.refresh().is_err());
    }
}

#[test]
fn task_receipts_allow_newer_metadata_and_archive_state_from_recovery_reads() {
    use axum::{
        Json, Router,
        routing::{get, patch},
    };
    use std::sync::{Arc, Mutex};
    use tracker_application::TaskOperations;
    use tracker_protocol::{MutationDto, MutationResultDto, SnapshotDto, TaskDto};
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
            let initial = SnapshotDto::from_snapshot(
                &tracker_application::TrackerSnapshot {
                    task_items: application.tasks(TaskOrdering::default()),
                    active_worklog: None,
                },
                "before".into(),
            );
            let state = Arc::new(Mutex::new(initial));
            let changed = state.clone();
            let mut committed_task = state.lock().unwrap().task_items[0].task.clone();
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
                                state.task_items[0].task = latest;
                                state.revision = "later".into();
                            }
                            Json(MutationDto {
                                request_id: body["request_id"].as_str().unwrap().into(),
                                applied_revision: "applied".into(),
                                replayed,
                                result: MutationResultDto::Task(committed_task),
                            })
                        }
                    }),
                );
            let server = report_tests::Server::start(router);
            let mut bridge = server.client();
            bridge.application.refresh().unwrap();
            let value = if action == "rename" {
                let id = CString::new(task.id().to_string()).unwrap();
                let name = CString::new("Requested name").unwrap();
                let timestamp = CString::new(timestamp(at(400))).unwrap();
                // SAFETY: The bridge and input strings are live for this call.
                response(unsafe {
                    tt_bridge_rename_task_at(
                        &mut bridge,
                        id.as_ptr(),
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
            assert_eq!(value["data"]["tasks"][0]["name"], "Later writer name");
            assert_eq!(value["data"]["tasksRevision"], "later");
            if action != "rename" {
                assert_eq!(value["data"]["tasks"][0]["archived"], action != "archive");
            }
        }
    }
}
