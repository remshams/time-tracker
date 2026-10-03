use std::{
    ffi::{CStr, CString, c_char},
    net::SocketAddr,
    path::PathBuf,
    ptr,
    sync::mpsc,
    thread,
};

use axum::{
    Json, Router,
    http::StatusCode,
    routing::{get, put},
};
use chrono::{DateTime, Duration, Utc};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::sync::oneshot;
use tracker_application::{TaskOperations, TrackerApplication, TrackingOperations};
use tracker_domain::{TaskName, TrackingState};
use tracker_storage::SqliteRepository;
use tracker_swift_bridge::{
    Bridge, tt_bridge_close, tt_bridge_history, tt_bridge_open_remote, tt_bridge_snapshot,
    tt_bridge_start_tracking_at, tt_bridge_start_tracking_if_active_at, tt_bridge_stop_tracking_at,
    tt_bridge_string_free,
};

struct Server {
    directory: TempDir,
    address: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Server {
    fn start() -> Self {
        let directory = TempDir::new().unwrap();
        Self::with_database(directory, "127.0.0.1:0".parse().unwrap())
    }

    fn with_database(directory: TempDir, bind: SocketAddr) -> Self {
        let path = directory.path().join("server.db");
        Self::launch(directory, bind, move || {
            tracker_server::router_for_database(&path).unwrap()
        })
    }

    fn launch(
        directory: TempDir,
        bind: SocketAddr,
        router: impl FnOnce() -> Router + Send + 'static,
    ) -> Self {
        let (address_tx, address_rx) = mpsc::channel();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let thread = thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async move {
                    let listener = tokio::net::TcpListener::bind(bind).await.unwrap();
                    address_tx.send(listener.local_addr().unwrap()).unwrap();
                    axum::serve(listener, router())
                        .with_graceful_shutdown(async {
                            let _ = shutdown_rx.await;
                        })
                        .await
                        .unwrap();
                });
        });
        Self {
            directory,
            address: address_rx.recv().unwrap(),
            shutdown: Some(shutdown_tx),
            thread: Some(thread),
        }
    }

    fn endpoint(&self) -> String {
        format!("http://{}", self.address)
    }
    fn path(&self) -> PathBuf {
        self.directory.path().join("server.db")
    }
    fn stop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Client(*mut Bridge);

impl Client {
    fn open(endpoint: &str) -> Self {
        let endpoint = CString::new(endpoint).unwrap();
        let mut error = ptr::null_mut();
        // SAFETY: Inputs are live C strings and writable pointer storage.
        let bridge = unsafe { tt_bridge_open_remote(endpoint.as_ptr(), &mut error) };
        assert!(
            !bridge.is_null(),
            "{}",
            if error.is_null() {
                String::new()
            } else {
                take_string(error)
            }
        );
        assert!(error.is_null());
        Self(bridge)
    }
    fn snapshot(&mut self, refresh: bool) -> Value {
        // SAFETY: This client owns and serializes access to its bridge.
        response(unsafe { tt_bridge_snapshot(self.0, refresh) })
    }
    fn start(&mut self, task: &str, at: DateTime<Utc>) -> Value {
        let task = CString::new(task).unwrap();
        let at = CString::new(at.to_rfc3339()).unwrap();
        // SAFETY: All inputs are live, and the client owns exclusive bridge access.
        response(unsafe { tt_bridge_start_tracking_at(self.0, task.as_ptr(), at.as_ptr()) })
    }
    fn stop(&mut self, id: &str, at: DateTime<Utc>) -> Value {
        let id = CString::new(id).unwrap();
        let at = CString::new(at.to_rfc3339()).unwrap();
        // SAFETY: All inputs are live, and the client owns exclusive bridge access.
        response(unsafe { tt_bridge_stop_tracking_at(self.0, id.as_ptr(), at.as_ptr()) })
    }
    fn start_if_active(&mut self, task: &str, expected: Option<&str>, at: DateTime<Utc>) -> Value {
        let task = CString::new(task).unwrap();
        let expected = expected.map(|id| CString::new(id).unwrap());
        let at = CString::new(at.to_rfc3339()).unwrap();
        // SAFETY: Inputs are live, and bridge access belongs exclusively to this client.
        response(unsafe {
            tt_bridge_start_tracking_if_active_at(
                self.0,
                task.as_ptr(),
                expected.as_ref().map_or(ptr::null(), |id| id.as_ptr()),
                at.as_ptr(),
            )
        })
    }
    fn history(&mut self, task: &str, cursor: Option<&str>) -> Value {
        let task = CString::new(task).unwrap();
        let cursor = cursor.map(|value| CString::new(value).unwrap());
        // SAFETY: All inputs are live, and the client owns exclusive bridge access.
        response(unsafe {
            tt_bridge_history(
                self.0,
                task.as_ptr(),
                cursor.as_ref().map_or(ptr::null(), |value| value.as_ptr()),
            )
        })
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // SAFETY: This client owns the bridge, released once after its last call.
        unsafe { tt_bridge_close(self.0) };
    }
}

fn take_string(pointer: *mut c_char) -> String {
    assert!(!pointer.is_null());
    // SAFETY: Only owned bridge strings are passed here and released once.
    unsafe {
        let string = CStr::from_ptr(pointer).to_str().unwrap().to_owned();
        tt_bridge_string_free(pointer);
        string
    }
}

fn response(pointer: *mut c_char) -> Value {
    serde_json::from_str(&take_string(pointer)).unwrap()
}
fn at() -> DateTime<Utc> {
    "2026-09-20T12:00:00Z".parse().unwrap()
}

#[test]
fn remote_mode_uses_only_server_tasks_and_preserves_click_timestamps() {
    let server = Server::start();
    let mut client = Client::open(&server.endpoint());
    assert_eq!(
        client.snapshot(false)["data"],
        json!({"tasks": [], "active": null})
    );
    let disconnected = client.start("00000000-0000-0000-0000-000000000001", at());
    assert_eq!(disconnected["requiresRefresh"], true);
    let snapshot = client.snapshot(true);
    let tasks = snapshot["data"]["tasks"].as_array().unwrap();
    let first = tasks[0]["id"].as_str().unwrap();
    let second = tasks[1]["id"].as_str().unwrap();
    let started = client.start(first, at());
    assert_eq!(
        started["data"]["active"]["start"],
        "2026-09-20T12:00:00.000000Z"
    );
    let old_id = started["data"]["active"]["id"].as_str().unwrap();
    let switched = client.start(second, at() + Duration::seconds(10));
    assert_eq!(switched["data"]["active"]["taskId"], second);
    assert_ne!(switched["data"]["active"]["id"], old_id);
    let first_history = client.history(first, None);
    assert_eq!(
        first_history["data"]["worklogs"][0]["end"],
        switched["data"]["active"]["start"]
    );
    let active = switched["data"]["active"]["id"].as_str().unwrap();
    let stopped = client.stop(active, at() + Duration::seconds(25));
    assert!(stopped["data"]["active"].is_null(), "{stopped}");
    let history = client.history(second, None);
    assert_eq!(
        history["data"]["worklogs"][0]["end"],
        "2026-09-20T12:00:25.000000Z"
    );
    drop(client);
    let mut reopened = Client::open(&server.endpoint());
    assert_eq!(reopened.snapshot(true)["data"], stopped["data"]);
    assert!(server.path().exists());
}

#[test]
fn stale_remote_writes_require_refresh_and_never_stop_another_clients_timer() {
    let server = Server::start();
    let mut first = Client::open(&server.endpoint());
    let mut second = Client::open(&server.endpoint());
    let initial = first.snapshot(true);
    second.snapshot(true);
    let tasks = initial["data"]["tasks"].as_array().unwrap();
    let started = first.start(tasks[0]["id"].as_str().unwrap(), at());
    second.snapshot(true);
    let switched = second.start(
        tasks[1]["id"].as_str().unwrap(),
        at() + Duration::seconds(5),
    );
    let rejected = first.stop(
        started["data"]["active"]["id"].as_str().unwrap(),
        at() + Duration::seconds(10),
    );
    assert_eq!(rejected["kind"], "conflict", "{rejected}");
    assert_eq!(rejected["uncertain"], false);
    assert_eq!(rejected["requiresRefresh"], true);
    assert_eq!(
        first.start(
            tasks[0]["id"].as_str().unwrap(),
            at() + Duration::seconds(11)
        )["requiresRefresh"],
        true
    );
    assert_eq!(
        first.snapshot(true)["data"]["active"],
        switched["data"]["active"]
    );
    assert!(
        first.stop(
            switched["data"]["active"]["id"].as_str().unwrap(),
            at() + Duration::seconds(15)
        )["data"]["active"]
            .is_null()
    );
}

#[test]
fn start_never_switches_a_timer_learned_from_history_but_not_rendered_yet() {
    let server = Server::start();
    let mut first = Client::open(&server.endpoint());
    let mut second = Client::open(&server.endpoint());
    let snapshot = first.snapshot(true);
    second.snapshot(true);
    let first_task = snapshot["data"]["tasks"][0]["id"].as_str().unwrap();
    let second_task = snapshot["data"]["tasks"][1]["id"].as_str().unwrap();
    let started = second.start(first_task, at());
    let active_id = started["data"]["active"]["id"].as_str().unwrap();
    first.history(first_task, None);
    let rejected = first.start_if_active(second_task, None, at() + Duration::seconds(1));
    assert_eq!(rejected["kind"], "conflict", "{rejected}");
    assert_eq!(rejected["requiresRefresh"], true);
    assert_eq!(
        second.snapshot(true)["data"]["active"],
        started["data"]["active"]
    );
    first.snapshot(true);
    let switched = first.start_if_active(second_task, Some(active_id), at() + Duration::seconds(2));
    assert_eq!(
        switched["data"]["active"]["taskId"], second_task,
        "{switched}"
    );
    let new_id = switched["data"]["active"]["id"].as_str().unwrap();
    let stale = first.start_if_active(first_task, Some(active_id), at() + Duration::seconds(3));
    assert_eq!(stale["kind"], "conflict");
    first.snapshot(true);
    let stopped = first.stop(new_id, at() + Duration::seconds(4));
    assert!(stopped["data"]["active"].is_null());
    let running_while_idle =
        first.start_if_active(first_task, Some(new_id), at() + Duration::seconds(5));
    assert_eq!(running_while_idle["kind"], "conflict");
    first.snapshot(true);
    let idle_start = first.start_if_active(first_task, None, at() + Duration::seconds(6));
    assert_eq!(idle_start["data"]["active"]["taskId"], first_task);
}

#[test]
fn unavailable_server_keeps_confirmed_cache_and_blocks_writes_without_local_fallback() {
    let mut server = Server::start();
    let mut client = Client::open(&server.endpoint());
    let snapshot = client.snapshot(true);
    server.stop();
    let task = snapshot["data"]["tasks"][0]["id"].as_str().unwrap();
    let failure = client.start(task, at());
    assert_eq!(failure["kind"], "unavailable", "{failure}");
    assert_eq!(failure["uncertain"], true);
    assert_eq!(failure["requiresRefresh"], true);
    assert_eq!(client.snapshot(false)["data"], snapshot["data"]);
    assert_eq!(client.snapshot(true)["kind"], "unavailable");
    assert_eq!(client.start(task, at())["uncertain"], false);
    let repository = SqliteRepository::open(server.path()).unwrap();
    let application = TrackerApplication::load(repository).unwrap();
    assert!(matches!(
        application.current_tracking(),
        TrackingState::Idle
    ));
}

#[test]
fn incompatible_health_protocol_is_reported_before_loading_a_snapshot() {
    let server = Server::launch(
        TempDir::new().unwrap(),
        "127.0.0.1:0".parse().unwrap(),
        || {
            Router::new().route(
                "/v1/health",
                get(|| async {
                    Json(json!({"status": "ok", "protocol_version": tracker_protocol::VERSION + 1}))
                }),
            )
        },
    );
    let mut client = Client::open(&server.endpoint());
    let failure = client.snapshot(true);
    assert_eq!(failure["kind"], "protocol", "{failure}");
    assert_eq!(failure["uncertain"], false);
    assert_eq!(failure["requiresRefresh"], true);
    assert_eq!(client.snapshot(false)["data"]["tasks"], json!([]));
}

#[test]
fn failed_write_response_remains_uncertain_after_a_successful_recovery_read() {
    let server = Server::launch(
        TempDir::new().unwrap(),
        "127.0.0.1:0".parse().unwrap(),
        || {
            Router::new()
            .route("/v1/health", get(|| async { Json(json!({"status": "ok", "protocol_version": tracker_protocol::VERSION})) }))
            .route("/v1/snapshot", get(|| async { Json(json!({"task_items": [], "active_worklog": null, "revision": "confirmed"})) }))
            .route("/v1/tracking", put(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "Unconfirmed write") }))
        },
    );
    let mut client = Client::open(&server.endpoint());
    assert!(client.snapshot(true).get("error").is_none());
    let failure = client.start("00000000-0000-0000-0000-000000000001", at());
    assert_eq!(failure["kind"], "protocol", "{failure}");
    assert_eq!(failure["uncertain"], true);
    assert_eq!(failure["requiresRefresh"], true);
    let blocked = client.start("00000000-0000-0000-0000-000000000001", at());
    assert_eq!(blocked["uncertain"], false);
    assert_eq!(blocked["requiresRefresh"], true);
}

#[test]
fn remote_history_restarts_when_the_cursor_revision_is_stale() {
    let directory = TempDir::new().unwrap();
    let repository = SqliteRepository::open(directory.path().join("server.db")).unwrap();
    let mut application = TrackerApplication::load(repository).unwrap();
    let task = application
        .create_task(TaskName::new("History on the server").unwrap(), at())
        .unwrap();
    for index in 0..51 {
        let start = at() + Duration::seconds(index * 10);
        application.set_active_task(task.id(), start).unwrap();
        let TrackingState::Running { worklog } = application.current_tracking() else {
            panic!("timer must be running")
        };
        application
            .clear_active_task(worklog.id(), start + Duration::seconds(1))
            .unwrap();
    }
    drop(application);
    let server = Server::with_database(directory, "127.0.0.1:0".parse().unwrap());
    let mut client = Client::open(&server.endpoint());
    client.snapshot(true);
    let id = task.id().to_string();
    let first = client.history(&id, None);
    assert_eq!(first["data"]["worklogs"].as_array().unwrap().len(), 50);
    let cursor = first["data"]["nextCursor"].as_str().unwrap();
    let older = client.history(&id, Some(cursor));
    assert_eq!(older["data"]["worklogs"].as_array().unwrap().len(), 1);
    assert_eq!(older["data"]["reset"], false);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut other = tracker_remote::RemoteApplication::connect(&server.endpoint())
            .await
            .unwrap();
        let page = other.worklogs_for_task(task.id(), None).await.unwrap();
        let worklog = &page.worklogs[0];
        other
            .correct_worklog(
                worklog.id(),
                worklog.times(),
                tracker_domain::WorklogTimes::new(
                    worklog.start() + Duration::seconds(1),
                    worklog.end().map(|end| end + Duration::seconds(2)),
                ),
                at() + Duration::seconds(1_000),
            )
            .await
            .unwrap();
    });
    let reset = client.history(&id, Some(cursor));
    assert_eq!(reset["data"]["reset"], true, "{reset}");
    assert_eq!(reset["data"]["worklogs"].as_array().unwrap().len(), 50);
}

#[test]
fn invalid_remote_endpoints_and_tracking_timestamps_are_rejected() {
    for endpoint in [
        "not a URL",
        "ftp://127.0.0.1",
        "http://user:secret@localhost",
        "http://localhost/api",
    ] {
        let endpoint = CString::new(endpoint).unwrap();
        let mut error = ptr::null_mut();
        // SAFETY: Inputs are live C strings and writable pointer storage.
        let bridge = unsafe { tt_bridge_open_remote(endpoint.as_ptr(), &mut error) };
        assert!(bridge.is_null());
        assert!(!take_string(error).is_empty());
    }
    let mut error = ptr::null_mut();
    // SAFETY: Null inputs are supported and error storage is writable.
    unsafe {
        assert!(tt_bridge_open_remote(ptr::null(), &mut error).is_null());
        assert_eq!(take_string(error), "Invalid server URL");
        assert!(tt_bridge_open_remote(ptr::null(), ptr::null_mut()).is_null());
        assert!(
            response(tt_bridge_start_tracking_at(
                ptr::null_mut(),
                ptr::null(),
                ptr::null()
            ))
            .get("error")
            .is_some()
        );
        assert!(
            response(tt_bridge_stop_tracking_at(
                ptr::null_mut(),
                ptr::null(),
                ptr::null()
            ))
            .get("error")
            .is_some()
        );
        assert!(
            response(tt_bridge_start_tracking_if_active_at(
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                ptr::null()
            ))
            .get("error")
            .is_some()
        );
    }
    let server = Server::start();
    let mut client = Client::open(&server.endpoint());
    let snapshot = client.snapshot(true);
    let task = CString::new(snapshot["data"]["tasks"][0]["id"].as_str().unwrap()).unwrap();
    let invalid = CString::new("not a timestamp").unwrap();
    // SAFETY: Inputs are live C strings and bridge access is exclusive.
    let failure =
        response(unsafe { tt_bridge_start_tracking_at(client.0, task.as_ptr(), invalid.as_ptr()) });
    assert_eq!(failure["error"], "Invalid tracking timestamp");
    assert_eq!(failure["kind"], "general");
    assert_eq!(failure["uncertain"], false);
    // SAFETY: Inputs are live or null, and bridge access is exclusive.
    unsafe {
        assert_eq!(
            response(tt_bridge_start_tracking_if_active_at(
                client.0,
                task.as_ptr(),
                invalid.as_ptr(),
                ptr::null()
            ))["error"],
            "Invalid worklog ID"
        );
        assert_eq!(
            response(tt_bridge_start_tracking_if_active_at(
                client.0,
                task.as_ptr(),
                ptr::null(),
                invalid.as_ptr()
            ))["error"],
            "Invalid tracking timestamp"
        );
        assert_eq!(
            response(tt_bridge_start_tracking_if_active_at(
                client.0,
                ptr::null(),
                ptr::null(),
                invalid.as_ptr()
            ))["error"],
            "Invalid task ID"
        );
    }
    assert!(client.snapshot(false)["data"]["active"].is_null());
}
