use super::*;
use tracker_application::{TaskOperations, TaskQueries, TrackingOperations, WORKLOG_PAGE_SIZE};

fn response(pointer: *mut c_char) -> Value {
    assert!(!pointer.is_null());
    // SAFETY: The pointer comes from the bridge and is released once.
    unsafe {
        let value = serde_json::from_str(CStr::from_ptr(pointer).to_str().unwrap()).unwrap();
        tt_bridge_string_free(pointer);
        value
    }
}

fn at(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 0).unwrap()
}

fn report(bridge: &mut Bridge, start: i64, end: i64, now: i64) -> Value {
    let start = CString::new(timestamp(at(start))).unwrap();
    let end = CString::new(timestamp(at(end))).unwrap();
    let now = CString::new(timestamp(at(now))).unwrap();
    // SAFETY: The bridge and all strings remain live for this call.
    response(unsafe { tt_bridge_report(bridge, start.as_ptr(), end.as_ptr(), now.as_ptr()) })
}

fn completed(
    application: &mut TrackerApplication<SqliteRepository>,
    task_id: TaskId,
    start: i64,
    end: i64,
) {
    application.set_active_task(task_id, at(start)).unwrap();
    let TrackingState::Running { worklog } = application.current_tracking() else {
        panic!("tracking should be running");
    };
    application
        .clear_active_task(worklog.id(), at(end))
        .unwrap();
}

pub(super) fn resource_router(
    snapshot: tracker_protocol::SnapshotDto,
    worklogs: Vec<tracker_protocol::WorklogDto>,
) -> axum::Router {
    resource_router_state(
        std::sync::Arc::new(std::sync::Mutex::new(snapshot)),
        worklogs,
    )
}

pub(super) fn resource_router_state(
    snapshot: std::sync::Arc<std::sync::Mutex<tracker_protocol::SnapshotDto>>,
    worklogs: Vec<tracker_protocol::WorklogDto>,
) -> axum::Router {
    resource_router_resources(
        snapshot,
        std::sync::Arc::new(std::sync::Mutex::new(worklogs)),
    )
}

pub(super) fn resource_router_resources(
    snapshot: std::sync::Arc<std::sync::Mutex<tracker_protocol::SnapshotDto>>,
    worklogs: std::sync::Arc<std::sync::Mutex<Vec<tracker_protocol::WorklogDto>>>,
) -> axum::Router {
    use axum::{Json, Router, extract::Path, http::StatusCode, routing::get};
    use tracker_protocol::{
        ErrorCode, ErrorDto, TaskResourceDto, TasksDto, TrackingDto, WorklogResourceDto,
    };
    let task_list = snapshot.clone();
    let tracking_state = snapshot.clone();
    let worklog_state = snapshot.clone();
    Router::new()
        .route(
            "/v1/tasks",
            get(move || {
                let snapshot = snapshot.lock().unwrap().clone();
                async move {
                    Json(TasksDto {
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
                }
            }),
        )
        .route(
            "/v1/tracking",
            get(move || {
                let snapshot = tracking_state.lock().unwrap().clone();
                async move {
                    Json(TrackingDto {
                        active_worklog: snapshot.active_worklog,
                        revision: snapshot.revision,
                    })
                }
            }),
        )
        .route(
            "/v1/tasks/{id}",
            get(move |Path(id): Path<String>| {
                let snapshot = task_list.lock().unwrap().clone();
                async move {
                    use axum::response::IntoResponse;
                    match snapshot
                        .task_items
                        .into_iter()
                        .find(|item| item.task.id == id)
                    {
                        Some(item) => {
                            let mut task = item.task;
                            task.latest_work_start = item.latest_work_start;
                            Json(TaskResourceDto {
                                task,
                                revision: snapshot.revision,
                            })
                            .into_response()
                        }
                        None => (
                            StatusCode::NOT_FOUND,
                            Json(ErrorDto {
                                code: ErrorCode::NotFound,
                                message: "Task not found".into(),
                            }),
                        )
                            .into_response(),
                    }
                }
            }),
        )
        .route(
            "/v1/worklogs/{id}",
            get(move |Path(id): Path<String>| {
                let worklogs = worklogs.lock().unwrap().clone();
                let revision = worklog_state.lock().unwrap().revision.clone();
                async move {
                    use axum::response::IntoResponse;
                    match worklogs.into_iter().find(|worklog| worklog.id == id) {
                        Some(worklog) => {
                            Json(WorklogResourceDto { worklog, revision }).into_response()
                        }
                        None => (
                            StatusCode::NOT_FOUND,
                            Json(ErrorDto {
                                code: ErrorCode::NotFound,
                                message: "Worklog not found".into(),
                            }),
                        )
                            .into_response(),
                    }
                }
            }),
        )
}

#[test]
fn report_aggregates_all_history_and_clips_completed_and_active_work() {
    let repository = SqliteRepository::open_in_memory().unwrap();
    let mut application = TrackerApplication::load(repository).unwrap();
    let archived = application
        .create_task(TaskName::new("Archived work").unwrap(), at(0))
        .unwrap();
    let finished = application
        .create_task(TaskName::new("Finished work").unwrap(), at(0))
        .unwrap();
    let active = application
        .create_task(TaskName::new("Active work").unwrap(), at(0))
        .unwrap();
    let unused = application
        .create_task(TaskName::new("Unused work").unwrap(), at(0))
        .unwrap();
    completed(&mut application, archived.id(), 990, 1005);
    for index in 0..=WORKLOG_PAGE_SIZE as i64 {
        completed(
            &mut application,
            archived.id(),
            1010 + index * 2,
            1011 + index * 2,
        );
    }
    application.archive_task(archived.id(), at(1120)).unwrap();
    completed(&mut application, finished.id(), 1180, 1205);
    application.set_active_task(active.id(), at(1210)).unwrap();
    let mut bridge = Bridge::new(Backend::Local(application));

    let value = report(&mut bridge, 1000, 1200, 1190);
    assert!(value.get("error").is_none(), "{value}");
    assert_eq!(
        value["data"]["rows"],
        json!([
            {"taskId": archived.id().to_string(), "durationMicroseconds": 56_000_000},
            {"taskId": finished.id().to_string(), "durationMicroseconds": 20_000_000}
        ])
    );
    assert_eq!(
        value["data"]["snapshot"]["active"]["taskId"],
        active.id().to_string()
    );
    assert!(
        value["data"]["snapshot"]["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["id"] == archived.id().to_string() && task["archived"] == true)
    );
    assert!(
        !value["data"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["taskId"] == unused.id().to_string())
    );

    let value = report(&mut bridge, 1200, 1300, 1215);
    assert_eq!(
        value["data"]["rows"],
        json!([
            {"taskId": finished.id().to_string(), "durationMicroseconds": 5_000_000},
            {"taskId": active.id().to_string(), "durationMicroseconds": 5_000_000}
        ])
        .as_array()
        .map(|rows| {
            let mut rows = rows.clone();
            rows.sort_by_key(|row| row["taskId"].as_str().unwrap().to_owned());
            Value::Array(rows)
        })
        .unwrap()
    );
    let value = report(&mut bridge, 1200, 1212, 1220);
    assert!(value["data"]["rows"].as_array().unwrap().contains(&json!({
        "taskId": active.id().to_string(), "durationMicroseconds": 2_000_000
    })));
}

#[test]
fn report_returns_the_snapshot_adopted_from_other_clients() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tracker.db");
    let mut first = open_fixture(&path).unwrap();
    let mut second = open_fixture(&path).unwrap();
    let task_id = second.application.tasks(TaskOrdering::default())[0]
        .task
        .id();
    second
        .application
        .set_active_task(task_id, at(1005))
        .unwrap_or_else(|error| panic!("{}", error.message));
    assert!(matches!(
        first.application.current_tracking(),
        TrackingState::Idle
    ));

    let value = report(&mut first, 1000, 1100, 1010);
    assert_eq!(
        value["data"]["rows"],
        json!([
            {"taskId": task_id.to_string(), "durationMicroseconds": 5_000_000}
        ])
    );
    assert_eq!(
        value["data"]["snapshot"]["active"]["taskId"],
        task_id.to_string()
    );
    assert_eq!(
        value["data"]["snapshot"],
        serde_json::to_value(snapshot(&second.application)).unwrap()
    );
}

#[test]
fn report_preserves_microseconds_and_accepts_utc_offsets() {
    let repository = SqliteRepository::open_in_memory().unwrap();
    let mut application = TrackerApplication::load(repository).unwrap();
    let task = application
        .create_task(TaskName::new("Precise work").unwrap(), at(0))
        .unwrap();
    application
        .set_active_task(task.id(), "2026-10-04T00:00:00.123456Z".parse().unwrap())
        .unwrap();
    let TrackingState::Running { worklog } = application.current_tracking() else {
        panic!("tracking should be running");
    };
    application
        .clear_active_task(worklog.id(), "2026-10-04T00:00:00.987654Z".parse().unwrap())
        .unwrap();
    let mut bridge = Bridge::new(Backend::Local(application));
    let start = c"2026-10-04T02:00:00+02:00";
    let end = c"2026-10-05T02:00:00+02:00";
    let now = c"2026-10-04T00:00:00.500000Z";
    // SAFETY: The bridge and all strings remain live for this call.
    let value = response(unsafe {
        tt_bridge_report(&mut bridge, start.as_ptr(), end.as_ptr(), now.as_ptr())
    });
    assert_eq!(
        value["data"]["rows"],
        json!([
            {"taskId": task.id().to_string(), "durationMicroseconds": 864_198}
        ])
    );
}

#[test]
fn report_rejects_null_malformed_and_invalid_ranges_and_returns_empty_totals() {
    let repository = SqliteRepository::open_in_memory().unwrap();
    let mut bridge = Bridge::new(Backend::Local(
        TrackerApplication::load(repository).unwrap(),
    ));
    let valid = c"2026-10-04T00:00:00Z";
    let malformed = c"invalid";
    let non_utf8 = CString::new(vec![0xff]).unwrap();
    // SAFETY: All handles and strings are live or null, as the API supports.
    unsafe {
        assert_eq!(
            response(tt_bridge_report(
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                ptr::null()
            ))["error"],
            "Database is not open"
        );
        for invalid in [ptr::null(), malformed.as_ptr(), non_utf8.as_ptr()] {
            assert_eq!(
                response(tt_bridge_report(
                    &mut bridge,
                    invalid,
                    valid.as_ptr(),
                    valid.as_ptr()
                ))["error"],
                "Invalid report start"
            );
            assert_eq!(
                response(tt_bridge_report(
                    &mut bridge,
                    valid.as_ptr(),
                    invalid,
                    valid.as_ptr()
                ))["error"],
                "Invalid report end"
            );
            assert_eq!(
                response(tt_bridge_report(
                    &mut bridge,
                    valid.as_ptr(),
                    valid.as_ptr(),
                    invalid
                ))["error"],
                "Invalid report timestamp"
            );
        }
    }
    for (start, end) in [(1000, 1000), (1001, 1000)] {
        let value = report(&mut bridge, start, end, 1000);
        assert_eq!(value["error"], "Report end must be later than start");
        assert_eq!(value["uncertain"], false);
        assert!(value.get("data").is_none());
    }
    let value = report(&mut bridge, 1000, 1100, 1050);
    assert_eq!(
        value["data"],
        json!({"snapshot":{"tasks":[],"active":null,"tasksRevision":null,"trackingRevision":null}, "rows":[], "revision":null, "now":timestamp(at(1050))})
    );
}

pub(super) struct Server {
    endpoint: String,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
    completed: std::sync::mpsc::Receiver<()>,
}

impl Server {
    pub(super) fn start(router: axum::Router) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let (shutdown, cancelled) = tokio::sync::oneshot::channel();
        let (finished, completed) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async move {
                    let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                    axum::serve(listener, router)
                        .with_graceful_shutdown(async {
                            let _ = cancelled.await;
                        })
                        .await
                        .unwrap();
                });
            let _ = finished.send(());
        });
        Self {
            endpoint,
            shutdown: Some(shutdown),
            thread: Some(thread),
            completed,
        }
    }

    pub(super) fn client(&self) -> Bridge {
        Bridge::new(Backend::remote(&self.endpoint).unwrap())
    }

    fn stop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(thread) = self.thread.take() {
            self.completed
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("server shutdown timed out");
            thread.join().unwrap();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

#[test]
fn remote_report_uses_server_totals_and_composes_task_and_tracking_resources() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server.db");
    let server = Server::start(tracker_server::router_for_database(&path).unwrap());
    let mut bridge = server.client();
    let mut writer = open_fixture(&path).unwrap();
    let tasks = writer.application.tasks(TaskOrdering::default());
    let task_id = tasks[0].task.id();
    writer
        .application
        .set_active_task(task_id, at(900))
        .unwrap_or_else(|error| panic!("{}", error.message));

    let value = report(&mut bridge, 1000, 1100, 1050);
    assert_eq!(
        value["data"]["rows"],
        json!([
            {"taskId": task_id.to_string(), "durationMicroseconds": 50_000_000}
        ])
    );
    let mut expected = serde_json::to_value(snapshot(&writer.application)).unwrap();
    expected["tasksRevision"] = value["data"]["revision"].clone();
    expected["trackingRevision"] = value["data"]["revision"].clone();
    assert!(
        expected["tasksRevision"]
            .as_str()
            .is_some_and(|revision| !revision.is_empty())
    );
    assert_eq!(value["data"]["snapshot"], expected);
    let invalid = report(&mut bridge, 1100, 1000, 1050);
    assert_eq!(invalid["requiresRefresh"], false);
    bridge
        .application
        .set_active_task(tasks[1].task.id(), at(1060))
        .unwrap_or_else(|error| panic!("{}", error.message));
    assert!(
        matches!(bridge.application.current_tracking(), TrackingState::Running { worklog } if worklog.task_id() == tasks[1].task.id())
    );
}

#[test]
fn remote_report_reconciles_changed_tracking_once_and_keeps_matching_totals() {
    use axum::{Json, routing::get};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tracker_protocol::{HealthDto, ReportDto, ReportRowDto, SnapshotDto};

    for initial_revision in ["41", "42"] {
        let repository = SqliteRepository::open_in_memory().unwrap();
        let mut application = TrackerApplication::load(repository).unwrap();
        let task_id = application
            .create_task(TaskName::new("Measured task").unwrap(), at(0))
            .unwrap()
            .id();
        let resources = SnapshotDto::from_snapshot(
            &tracker_application::TrackerSnapshot {
                task_items: application.tasks(TaskOrdering::default()),
                active_worklog: None,
            },
            "42".into(),
        );
        let requests = Arc::new(AtomicUsize::new(0));
        let report_requests = requests.clone();
        let router = resource_router(resources, vec![])
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
                "/v1/reports/task-totals",
                get(move || {
                    let first = report_requests.fetch_add(1, Ordering::SeqCst) == 0;
                    async move {
                        let duration_us = if first { 50_000_000 } else { 60_000_000 };
                        Json(ReportDto {
                            start: at(1000),
                            end: at(1100),
                            now: at(1050),
                            rows: vec![ReportRowDto {
                                task_id: task_id.to_string(),
                                duration_us,
                            }],
                            total_us: duration_us,
                            revision: if first { initial_revision } else { "42" }.into(),
                        })
                    }
                }),
            );
        let server = Server::start(router);
        let mut bridge = server.client();
        let value = report(&mut bridge, 1000, 1100, 1050);
        assert!(value.get("error").is_none(), "{value}");
        assert_eq!(value["data"]["revision"], "42");
        assert_eq!(value["data"]["snapshot"]["trackingRevision"], "42");
        let changed = initial_revision != "42";
        assert_eq!(requests.load(Ordering::SeqCst), if changed { 2 } else { 1 });
        assert_eq!(
            value["data"]["rows"][0]["durationMicroseconds"],
            if changed { 60_000_000 } else { 50_000_000 }
        );
    }
}

#[test]
fn remote_report_failure_retains_resource_caches_until_an_explicit_refresh() {
    use axum::{Json, Router, http::StatusCode, routing::get};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tracker_protocol::{ErrorCode, ErrorDto, HealthDto, SnapshotDto, TasksDto, TrackingDto};

    let repository = SqliteRepository::open_in_memory().unwrap();
    let mut application = TrackerApplication::load(repository).unwrap();
    let task = application
        .create_task(TaskName::new("Changed elsewhere").unwrap(), at(0))
        .unwrap();
    let refreshed = SnapshotDto::from_snapshot(
        &tracker_application::TrackerSnapshot {
            task_items: application.tasks(TaskOrdering::default()),
            active_worklog: None,
        },
        "42".into(),
    );
    let snapshots = Arc::new(AtomicUsize::new(0));
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
            "/v1/tasks",
            get(move || {
                let snapshot = if snapshots.fetch_add(1, Ordering::SeqCst) == 0 {
                    SnapshotDto {
                        task_items: vec![],
                        active_worklog: None,
                        revision: "42".into(),
                    }
                } else {
                    refreshed.clone()
                };
                async move {
                    Json(TasksDto {
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
                }
            }),
        )
        .route(
            "/v1/tracking",
            get(|| async {
                Json(TrackingDto {
                    active_worklog: None,
                    revision: "42".into(),
                })
            }),
        )
        .route(
            "/v1/reports/task-totals",
            get(|| async {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorDto {
                        code: ErrorCode::InvalidRequest,
                        message: "Report unavailable".into(),
                    }),
                )
            }),
        );
    let server = Server::start(router);
    let mut bridge = server.client();
    // SAFETY: The bridge is live and uniquely accessed.
    let initial = response(unsafe { tt_bridge_snapshot(&mut bridge, true) });
    assert_eq!(initial["data"]["tasks"], json!([]));
    assert!(bridge.application.tasks(TaskOrdering::default()).is_empty());

    let value = report(&mut bridge, 1000, 1100, 1050);
    assert!(value.get("error").is_some());
    assert_eq!(value["requiresRefresh"], false);
    assert_eq!(value["uncertain"], false);
    assert!(bridge.application.tasks(TaskOrdering::default()).is_empty());

    // SAFETY: The bridge is live and uniquely accessed.
    let refreshed = response(unsafe { tt_bridge_snapshot(&mut bridge, true) });
    assert_eq!(refreshed["data"]["tasks"][0]["id"], task.id().to_string());
}

#[test]
fn remote_report_connection_failure_returns_a_certain_read_error_and_blocks_writes() {
    let directory = tempfile::tempdir().unwrap();
    let mut server = Server::start(
        tracker_server::router_for_database(&directory.path().join("server.db")).unwrap(),
    );
    let mut bridge = server.client();
    assert!(report(&mut bridge, 1000, 1100, 1050).get("error").is_none());
    server.stop();

    let value = report(&mut bridge, 1000, 1100, 1050);
    assert_eq!(value["kind"], "unavailable");
    assert_eq!(value["requiresRefresh"], true);
    assert_eq!(value["uncertain"], false);
    assert!(value.get("data").is_none());
    let error = bridge
        .application
        .set_active_task(TaskId::generate(), at(1060))
        .err()
        .unwrap();
    assert!(!error.uncertain);
    assert!(error.requires_refresh);
}

#[test]
fn connection_check_uses_only_health_and_rejects_incompatible_servers() {
    use axum::{Json, Router, routing::get};
    for version in [tracker_protocol::VERSION, 2] {
        let server = Server::start(Router::new().route(
            "/v1/health",
            get(move || async move {
                Json(tracker_protocol::HealthDto {
                    status: "ok".into(),
                    protocol_version: version,
                })
            }),
        ));
        let mut bridge = server.client();
        // SAFETY: The bridge is live and uniquely accessed.
        let value = response(unsafe { tt_bridge_check_connection(&mut bridge) });
        if version == tracker_protocol::VERSION {
            assert!(value.get("error").is_none(), "{value}");
            assert!(bridge.application.tasks(TaskOrdering::default()).is_empty());
            assert!(matches!(
                bridge.application.current_tracking(),
                TrackingState::Idle
            ));
        } else {
            assert_eq!(value["kind"], "protocol");
            assert_eq!(value["uncertain"], false);
        }
    }
}

#[test]
fn connection_check_supports_local_handles_and_rejects_null_handles() {
    let application =
        TrackerApplication::load(SqliteRepository::open_in_memory().unwrap()).unwrap();
    let mut bridge = Bridge::new(Backend::Local(application));
    // SAFETY: A live bridge and a null pointer are both supported inputs.
    unsafe {
        let value = response(tt_bridge_check_connection(&mut bridge));
        assert!(value.get("error").is_none(), "{value}");
        assert_eq!(
            response(tt_bridge_check_connection(std::ptr::null_mut()))["error"],
            "Database is not open"
        );
    }
}
