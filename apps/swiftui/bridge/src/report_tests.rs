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

fn selected_report(bridge: &mut Bridge, start: i64, end: i64, now: i64) -> Value {
    selected_totals(bridge, true, start, end, now)
}

fn selected_totals(
    bridge: &mut Bridge,
    include_tasks: bool,
    start: i64,
    end: i64,
    now: i64,
) -> Value {
    let start = CString::new(timestamp(at(start))).unwrap();
    let end = CString::new(timestamp(at(end))).unwrap();
    let now = CString::new(timestamp(at(now))).unwrap();
    // SAFETY: The bridge and all input strings remain live during each call.
    let result = response(unsafe {
        tt_bridge_refresh_totals(
            bridge,
            include_tasks,
            start.as_ptr(),
            end.as_ptr(),
            now.as_ptr(),
        )
    });
    if result.get("error").is_some() {
        return result;
    }
    response(unsafe { tt_bridge_report_observation(bridge) })
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

pub(super) fn resource_dtos(
    items: Vec<tracker_application::TaskListItem>,
    active: Option<tracker_domain::Worklog>,
    revision: String,
) -> (tracker_protocol::TasksDto, tracker_protocol::TrackingDto) {
    (
        tracker_protocol::TasksDto {
            tasks: items.iter().map(tracker_protocol::TaskDto::from).collect(),
            revision: revision.clone(),
        },
        tracker_protocol::TrackingDto {
            active_worklog: active.as_ref().map(tracker_protocol::WorklogDto::from),
            revision,
        },
    )
}

pub(super) fn resource_router(
    resources: (tracker_protocol::TasksDto, tracker_protocol::TrackingDto),
    worklogs: Vec<tracker_protocol::WorklogDto>,
) -> axum::Router {
    resource_router_state(
        std::sync::Arc::new(std::sync::Mutex::new(resources)),
        worklogs,
    )
}

pub(super) fn resource_router_state(
    resources: std::sync::Arc<
        std::sync::Mutex<(tracker_protocol::TasksDto, tracker_protocol::TrackingDto)>,
    >,
    worklogs: Vec<tracker_protocol::WorklogDto>,
) -> axum::Router {
    resource_router_resources(
        resources,
        std::sync::Arc::new(std::sync::Mutex::new(worklogs)),
    )
}

pub(super) fn resource_router_resources(
    resources: std::sync::Arc<
        std::sync::Mutex<(tracker_protocol::TasksDto, tracker_protocol::TrackingDto)>,
    >,
    worklogs: std::sync::Arc<std::sync::Mutex<Vec<tracker_protocol::WorklogDto>>>,
) -> axum::Router {
    use axum::{Json, Router, extract::Path, http::StatusCode, routing::get};
    use tracker_protocol::{ErrorCode, ErrorDto, TaskResourceDto, WorklogResourceDto};
    let task_list = resources.clone();
    let tracking_state = resources.clone();
    let worklog_state = resources.clone();
    Router::new()
        .route(
            "/v1/tasks",
            get(move || {
                let tasks = resources.lock().unwrap().0.clone();
                async move { Json(tasks) }
            }),
        )
        .route(
            "/v1/tracking",
            get(move || {
                let tracking = tracking_state.lock().unwrap().1.clone();
                async move { Json(tracking) }
            }),
        )
        .route(
            "/v1/tasks/{id}",
            get(move |Path(id): Path<String>| {
                let tasks = task_list.lock().unwrap().0.clone();
                async move {
                    use axum::response::IntoResponse;
                    match tasks.tasks.into_iter().find(|task| task.id == id) {
                        Some(task) => Json(TaskResourceDto {
                            task,
                            revision: tasks.revision,
                        })
                        .into_response(),
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
                let revision = worklog_state.lock().unwrap().0.revision.clone();
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
        crate::tests::test_active_value(&bridge.application)["taskId"],
        active.id().to_string()
    );
    assert!(
        crate::tests::test_task_values(&bridge.application)
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
fn report_adopts_local_tracking_and_returns_only_totals() {
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
        crate::tests::test_active_value(&first.application)["taskId"],
        task_id.to_string()
    );
    assert_eq!(
        crate::tests::test_resource_values(&first.application),
        crate::tests::test_resource_values(&second.application)
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
        json!({"rows":[], "revision":null, "now":timestamp(at(1050))})
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

    let value = selected_report(&mut bridge, 1000, 1100, 1050);
    assert_eq!(
        value["data"]["rows"],
        json!([
            {"taskId": task_id.to_string(), "durationMicroseconds": 50_000_000}
        ])
    );
    assert!(
        value["data"]["revision"]
            .as_str()
            .is_some_and(|revision| !revision.is_empty())
    );
    assert_eq!(
        crate::tests::test_resource_values(&bridge.application),
        crate::tests::test_resource_values(&writer.application)
    );
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
    use tracker_protocol::{HealthDto, ReportDto, ReportRowDto};

    for initial_revision in ["41", "42"] {
        let repository = SqliteRepository::open_in_memory().unwrap();
        let mut application = TrackerApplication::load(repository).unwrap();
        let task_id = application
            .create_task(TaskName::new("Measured task").unwrap(), at(0))
            .unwrap()
            .id();
        let resources = report_tests::resource_dtos(
            application.tasks(TaskOrdering::default()),
            None,
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
        let value = selected_report(&mut bridge, 1000, 1100, 1050);
        assert!(value.get("error").is_none(), "{value}");
        assert_eq!(value["data"]["revision"], "42");
        assert_eq!(
            tracking_json(&bridge.application).unwrap().revision,
            Some("42".into())
        );
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
    use tracker_protocol::{ErrorCode, ErrorDto, HealthDto, TasksDto, TrackingDto};

    let repository = SqliteRepository::open_in_memory().unwrap();
    let mut application = TrackerApplication::load(repository).unwrap();
    let task = application
        .create_task(TaskName::new("Changed elsewhere").unwrap(), at(0))
        .unwrap();
    let refreshed = report_tests::resource_dtos(
        application.tasks(TaskOrdering::default()),
        None,
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
                    (
                        TasksDto {
                            tasks: vec![],
                            revision: "42".into(),
                        },
                        TrackingDto {
                            active_worklog: None,
                            revision: "42".into(),
                        },
                    )
                } else {
                    refreshed.clone()
                };
                async move {
                    Json(TasksDto {
                        tasks: snapshot.0.tasks.into_iter().collect(),
                        revision: snapshot.0.revision,
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
    let initial = response(unsafe { tt_bridge_refresh_resources(&mut bridge, 3) });
    assert!(initial.get("error").is_none());
    assert_eq!(
        crate::tests::test_task_values(&bridge.application),
        json!([])
    );
    assert!(bridge.application.tasks(TaskOrdering::default()).is_empty());

    let value = report(&mut bridge, 1000, 1100, 1050);
    assert!(value.get("error").is_some());
    assert_eq!(value["requiresRefresh"], false);
    assert_eq!(value["uncertain"], false);
    assert!(bridge.application.tasks(TaskOrdering::default()).is_empty());

    // SAFETY: The bridge is live and uniquely accessed.
    let refreshed = response(unsafe { tt_bridge_refresh_resources(&mut bridge, 3) });
    assert!(refreshed.get("error").is_none());
    assert_eq!(
        crate::tests::test_task_values(&bridge.application)[0]["id"],
        task.id().to_string()
    );
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

#[test]
fn separate_resource_exports_distinguish_unloaded_empty_and_idle_observations() {
    use axum::{Json, routing::get};
    let server = Server::start(
        resource_router(resource_dtos(vec![], None, "r1".into()), vec![]).route(
            "/v1/health",
            get(|| async {
                Json(tracker_protocol::HealthDto {
                    status: "ok".into(),
                    protocol_version: tracker_protocol::VERSION,
                })
            }),
        ),
    );
    let mut bridge = server.client();
    // SAFETY: Each call has exclusive access to the live bridge.
    unsafe {
        assert!(response(tt_bridge_tasks(&mut bridge))["data"].is_null());
        assert!(response(tt_bridge_tracking(&mut bridge))["data"].is_null());
        assert!(response(tt_bridge_report_observation(&mut bridge))["data"].is_null());
        assert!(
            response(tt_bridge_refresh_resources(&mut bridge, 1))
                .get("error")
                .is_none()
        );
        assert_eq!(
            response(tt_bridge_tasks(&mut bridge))["data"],
            json!({"value":[], "revision":"r1"})
        );
        assert!(response(tt_bridge_tracking(&mut bridge))["data"].is_null());
        assert!(
            response(tt_bridge_refresh_resources(&mut bridge, 2))
                .get("error")
                .is_none()
        );
        assert_eq!(
            response(tt_bridge_tracking(&mut bridge))["data"],
            json!({"value":null, "revision":"r1"})
        );
        assert_eq!(
            response(tt_bridge_tasks(&mut bridge))["data"],
            json!({"value":[], "revision":"r1"})
        );
    }
}

#[test]
fn independent_remote_report_retains_totals_without_loading_task_or_tracking_metadata() {
    use axum::{Json, Router, routing::get};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let unrelated_reads = Arc::new(AtomicUsize::new(0));
    let tasks_reads = unrelated_reads.clone();
    let tracking_reads = unrelated_reads.clone();
    let task_id = TaskId::generate();
    let server = Server::start(
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
            .route(
                "/v1/tasks",
                get(move || {
                    tasks_reads.fetch_add(1, Ordering::SeqCst);
                    async { axum::http::StatusCode::NOT_FOUND }
                }),
            )
            .route(
                "/v1/tracking",
                get(move || {
                    tracking_reads.fetch_add(1, Ordering::SeqCst);
                    async { axum::http::StatusCode::NOT_FOUND }
                }),
            )
            .route(
                "/v1/reports/task-totals",
                get(move || async move {
                    Json(tracker_protocol::ReportDto {
                        start: at(1000),
                        end: at(1100),
                        now: at(1050),
                        rows: vec![tracker_protocol::ReportRowDto {
                            task_id: task_id.to_string(),
                            duration_us: 50_000_000,
                        }],
                        total_us: 50_000_000,
                        revision: "r1".into(),
                    })
                }),
            ),
    );
    let mut bridge = server.client();

    let value = report(&mut bridge, 1000, 1100, 1050);

    assert!(value.get("error").is_none(), "{value}");
    assert_eq!(
        value["data"]["rows"],
        json!([{"taskId":task_id.to_string(),"durationMicroseconds":50_000_000}])
    );
    assert_eq!(value["data"]["revision"], "r1");
    assert!(value["data"].get("snapshot").is_none());
    assert_eq!(unrelated_reads.load(Ordering::SeqCst), 0);
    // SAFETY: Each call has exclusive access to the live bridge.
    unsafe {
        assert!(response(tt_bridge_tasks(&mut bridge))["data"].is_null());
        assert!(response(tt_bridge_tracking(&mut bridge))["data"].is_null());
    }
}

#[test]
fn exhausted_selected_refresh_retains_each_confirmed_resource_observation() {
    use axum::{Json, routing::get};
    let server = Server::start(
        resource_router(
            (
                tracker_protocol::TasksDto {
                    tasks: vec![],
                    revision: "tasks-r1".into(),
                },
                tracker_protocol::TrackingDto {
                    active_worklog: None,
                    revision: "tracking-r2".into(),
                },
            ),
            vec![],
        )
        .route(
            "/v1/health",
            get(|| async {
                Json(tracker_protocol::HealthDto {
                    status: "ok".into(),
                    protocol_version: tracker_protocol::VERSION,
                })
            }),
        ),
    );
    let mut bridge = server.client();
    // SAFETY: Each call has exclusive access to the live bridge.
    unsafe {
        assert!(
            response(tt_bridge_refresh_resources(&mut bridge, 1))
                .get("error")
                .is_none()
        );
        assert!(
            response(tt_bridge_refresh_resources(&mut bridge, 2))
                .get("error")
                .is_none()
        );
        let tasks = response(tt_bridge_tasks(&mut bridge));
        let tracking = response(tt_bridge_tracking(&mut bridge));

        let failure = response(tt_bridge_refresh_resources(&mut bridge, 3));

        assert_eq!(failure["kind"], "conflict");
        assert_eq!(failure["requiresRefresh"], true);
        assert_eq!(response(tt_bridge_tasks(&mut bridge)), tasks);
        assert_eq!(response(tt_bridge_tracking(&mut bridge)), tracking);
    }
}

#[test]
fn resource_exports_and_selected_refresh_reject_null_handles_and_unknown_selections() {
    let repository = SqliteRepository::open_in_memory().unwrap();
    let mut bridge = Bridge::new(Backend::Local(
        TrackerApplication::load(repository).unwrap(),
    ));
    // SAFETY: Null handles are supported inputs and the local bridge stays live.
    unsafe {
        for returned in [
            tt_bridge_tasks(ptr::null_mut()),
            tt_bridge_tracking(ptr::null_mut()),
            tt_bridge_report_observation(ptr::null_mut()),
            tt_bridge_refresh_resources(ptr::null_mut(), 3),
            tt_bridge_refresh_totals(ptr::null_mut(), true, ptr::null(), ptr::null(), ptr::null()),
        ] {
            assert_eq!(response(returned)["error"], "Database is not open");
        }
        assert!(
            response(tt_bridge_refresh_resources(&mut bridge, 0))
                .get("error")
                .is_some()
        );
        assert!(
            response(tt_bridge_refresh_resources(&mut bridge, 4))
                .get("error")
                .is_some()
        );
    }
}

#[test]
fn local_selected_totals_refresh_only_the_requested_catalog_and_preserve_failed_reads() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("selected-local.db");
    let mut bridge = open_fixture(&path).unwrap();
    let mut writer = open_fixture(&path).unwrap();
    let original = writer.application.tasks(TaskOrdering::default());
    let added = writer
        .application
        .create_task(TaskName::new("Unrelated task").unwrap(), at(0))
        .unwrap();
    writer
        .application
        .set_active_task(original[0].task.id(), at(900))
        .unwrap();

    let daily = selected_totals(&mut bridge, false, 1000, 1100, 1050);
    assert_eq!(
        daily["data"]["rows"],
        json!([
            {"taskId": original[0].task.id().to_string(), "durationMicroseconds": 50_000_000}
        ])
    );
    assert!(daily["data"]["revision"].is_null());
    assert!(
        !bridge
            .application
            .tasks(TaskOrdering::default())
            .iter()
            .any(|item| item.task.id() == added.id())
    );
    assert_eq!(
        crate::tests::test_active_value(&bridge.application),
        crate::tests::test_active_value(&writer.application)
    );

    let with_catalog = selected_totals(&mut bridge, true, 1000, 1100, 1050);
    assert_eq!(with_catalog, daily);
    assert_eq!(
        crate::tests::test_resource_values(&bridge.application),
        crate::tests::test_resource_values(&writer.application)
    );
    let confirmed_resources = crate::tests::test_resource_values(&bridge.application);
    let confirmed_report = serde_json::to_value(&bridge.report_observation).unwrap();
    for include_tasks in [false, true] {
        let failure = selected_totals(&mut bridge, include_tasks, 1100, 1000, 1050);
        assert_eq!(failure["error"], "Report end must be later than start");
        assert_eq!(failure["uncertain"], false);
        assert_eq!(
            crate::tests::test_resource_values(&bridge.application),
            confirmed_resources
        );
        assert_eq!(
            serde_json::to_value(&bridge.report_observation).unwrap(),
            confirmed_report
        );
    }
}

#[test]
fn remote_selected_daily_totals_load_tracking_without_loading_the_task_catalog() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("selected-remote.db");
    let mut writer = open_fixture(&path).unwrap();
    let tasks = writer.application.tasks(TaskOrdering::default());
    writer
        .application
        .set_active_task(tasks[0].task.id(), at(900))
        .unwrap();
    let mut server = Server::start(tracker_server::router_for_database(&path).unwrap());
    let mut bridge = server.client();

    let daily = selected_totals(&mut bridge, false, 1000, 1100, 1050);
    assert!(daily.get("error").is_none(), "{daily}");
    assert!(tasks_json(&bridge.application).is_none());
    assert_eq!(
        tracking_json(&bridge.application)
            .unwrap()
            .revision
            .as_ref()
            .unwrap(),
        daily["data"]["revision"].as_str().unwrap()
    );
    assert_eq!(daily["data"]["rows"][0]["durationMicroseconds"], 50_000_000);

    let with_catalog = selected_totals(&mut bridge, true, 1000, 1100, 1050);
    assert_eq!(with_catalog, daily);
    assert_eq!(
        crate::tests::test_resource_values(&bridge.application),
        crate::tests::test_resource_values(&writer.application)
    );
    let confirmed_tasks = serde_json::to_value(tasks_json(&bridge.application)).unwrap();
    let confirmed_tracking = serde_json::to_value(tracking_json(&bridge.application)).unwrap();
    let confirmed_report = serde_json::to_value(&bridge.report_observation).unwrap();
    server.stop();
    for include_tasks in [false, true] {
        let failure = selected_totals(&mut bridge, include_tasks, 1000, 1100, 1050);
        assert_eq!(failure["kind"], "unavailable");
        assert_eq!(failure["uncertain"], false);
        assert_eq!(failure["requiresRefresh"], true);
        assert_eq!(
            serde_json::to_value(tasks_json(&bridge.application)).unwrap(),
            confirmed_tasks
        );
        assert_eq!(
            serde_json::to_value(tracking_json(&bridge.application)).unwrap(),
            confirmed_tracking
        );
        assert_eq!(
            serde_json::to_value(&bridge.report_observation).unwrap(),
            confirmed_report
        );
    }
}
