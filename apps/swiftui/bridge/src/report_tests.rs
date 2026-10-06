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
    let mut bridge = Bridge {
        application: Backend::Local(application),
    };

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
    let mut bridge = Bridge {
        application: Backend::Local(application),
    };
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
    let mut bridge = Bridge {
        application: Backend::Local(TrackerApplication::load(repository).unwrap()),
    };
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
        json!({"snapshot":{"tasks":[],"active":null}, "rows":[]})
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
        Bridge {
            application: Backend::remote(&self.endpoint).unwrap(),
        }
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
fn remote_report_uses_server_totals_and_unblocks_tracking_with_the_returned_snapshot() {
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
    assert_eq!(
        value["data"]["snapshot"],
        serde_json::to_value(snapshot(&writer.application)).unwrap()
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
fn remote_report_failure_blocks_writes_until_a_snapshot_is_delivered() {
    use axum::{Json, Router, http::StatusCode, routing::get};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tracker_protocol::{ErrorCode, ErrorDto, HealthDto, SnapshotDto};

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
            "/v1/snapshot",
            get(move || {
                let snapshot = if snapshots.fetch_add(1, Ordering::SeqCst) == 0 {
                    SnapshotDto {
                        task_items: vec![],
                        active_worklog: None,
                        revision: "41".into(),
                    }
                } else {
                    refreshed.clone()
                };
                async move { Json(snapshot) }
            }),
        )
        .route(
            "/v1/reports",
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
    assert_eq!(value["requiresRefresh"], true);
    assert_eq!(value["uncertain"], false);
    assert_eq!(
        bridge.application.tasks(TaskOrdering::default())[0]
            .task
            .id(),
        task.id()
    );
    let rejected = bridge
        .application
        .set_active_task(task.id(), at(1050))
        .err()
        .unwrap();
    assert_eq!(rejected.kind, "unavailable");
    assert!(rejected.requires_refresh);
    assert!(!rejected.uncertain);

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
