use std::{
    ffi::{CStr, CString, c_char},
    net::SocketAddr,
    path::PathBuf,
    ptr,
    sync::{Arc, Mutex, mpsc},
    thread,
};

use axum::{Json, Router, http::StatusCode, routing::get};
use chrono::{DateTime, Duration, Utc};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::sync::oneshot;
use tracker_application::{TaskOperations, TaskQueries, TrackerApplication, TrackingOperations};
use tracker_domain::{TaskName, TrackingState};
use tracker_storage::SqliteRepository;
use tracker_swift_bridge::{
    Bridge, tt_bridge_close, tt_bridge_create_task_at, tt_bridge_history, tt_bridge_open_remote,
    tt_bridge_refresh_resources, tt_bridge_rename_task_at, tt_bridge_start_tracking_at,
    tt_bridge_start_tracking_if_active_at, tt_bridge_stop_tracking_at, tt_bridge_string_free,
    tt_bridge_tasks, tt_bridge_tracking,
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
        let repository = SqliteRepository::open(directory.path().join("server.db")).unwrap();
        let mut application = TrackerApplication::load(repository).unwrap();
        for name in ["Review backlog", "Plan release", "Write documentation"] {
            application
                .create_task(TaskName::new(name).unwrap(), at())
                .unwrap();
        }
        drop(application);
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
    fn task_list(&mut self, refresh: bool) -> (Value, Value) {
        // SAFETY: This client owns and serializes access to its bridge.
        unsafe {
            if refresh {
                let status = response(tt_bridge_refresh_resources(self.0, 3));
                if status.get("error").is_some() {
                    return (status.clone(), status);
                }
            }
            (
                response(tt_bridge_tasks(self.0)),
                response(tt_bridge_tracking(self.0)),
            )
        }
    }
    fn create(&mut self, name: &str, at: DateTime<Utc>) -> Value {
        let name = CString::new(name).unwrap();
        let at = CString::new(at.to_rfc3339()).unwrap();
        // SAFETY: The client owns the bridge and both strings remain live.
        response(unsafe { tt_bridge_create_task_at(self.0, name.as_ptr(), at.as_ptr()) })
    }
    fn rename(&mut self, id: &str, name: &str, at: DateTime<Utc>) -> Value {
        let id = CString::new(id).unwrap();
        let name = CString::new(name).unwrap();
        let at = CString::new(at.to_rfc3339()).unwrap();
        // SAFETY: The client owns the bridge and every input string remains live.
        response(unsafe {
            tt_bridge_rename_task_at(self.0, id.as_ptr(), name.as_ptr(), at.as_ptr())
        })
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
    let unloaded = client.task_list(false);
    assert!(unloaded.0["data"].is_null());
    assert!(unloaded.1["data"].is_null());
    let disconnected = client.start("00000000-0000-0000-0000-000000000001", at());
    assert_eq!(disconnected["requiresRefresh"], true);
    let snapshot = client.task_list(true);
    let tasks = snapshot.0["data"]["value"].as_array().unwrap();
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
    assert_eq!(
        reopened.task_list(true).1["data"]["value"],
        stopped["data"]["active"]
    );
    assert!(server.path().exists());
}

#[test]
fn remote_creation_persists_client_ids_timestamps_and_preserves_tracking() {
    let server = Server::start();
    let mut client = Client::open(&server.endpoint());
    assert_eq!(
        client.create("Before refresh", at())["requiresRefresh"],
        true
    );
    let initial = client.task_list(true);
    let active = client.start(initial.0["data"]["value"][0]["id"].as_str().unwrap(), at());
    let created = client.create("  Server project 🛠  ", at() + Duration::seconds(5));
    assert!(created.get("error").is_none(), "{created}");
    let id: tracker_domain::TaskId = created["data"]["task"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(id.as_uuid().get_version_num(), 7);
    assert!(created["data"].get("snapshot").is_none());
    assert_eq!(
        client.task_list(false).1["data"]["value"],
        active["data"]["active"]
    );
    let refreshed = client.task_list(true);
    assert!(
        refreshed.0["data"]["value"]
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["id"] == created["data"]["task"]["id"])
    );
    assert_eq!(refreshed.1["data"]["value"], active["data"]["active"]);
    assert_eq!(
        refreshed.0["data"]["revision"],
        refreshed.1["data"]["revision"]
    );
    let stored = TrackerApplication::load(SqliteRepository::open(server.path()).unwrap()).unwrap();
    let task = stored.task(id).unwrap();
    assert_eq!(task.name().as_str(), "Server project 🛠");
    assert_eq!(task.created_at(), at() + Duration::seconds(5));
    let duplicate = client.create("Server project 🛠", at() + Duration::seconds(6));
    assert_ne!(
        duplicate["data"]["task"]["id"],
        created["data"]["task"]["id"]
    );
}

#[test]
fn uncertain_creation_recovers_the_same_id_after_polling_or_retries_the_original_intent() {
    for committed in [false, true] {
        for failure in [
            "malformed",
            "wrong-result",
            "server-error",
            "empty-revision",
        ] {
            let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
            let task = Arc::new(Mutex::new(None::<Value>));
            let recovered = Arc::new(Mutex::new(false));
            let request_state = Arc::clone(&requests);
            let task_for_get = Arc::clone(&task);
            let task_for_post = Arc::clone(&task);
            let recovered_for_post = Arc::clone(&recovered);
            let task_list_state = Arc::clone(&task);
            let server = Server::launch(
                TempDir::new().unwrap(),
                "127.0.0.1:0".parse().unwrap(),
                move || {
                    Router::new()
                    .route("/v1/health", get(|| async { Json(json!({"status":"ok","protocol_version":tracker_protocol::VERSION})) }))
                    .route("/v1/tasks", get(move || {
                        let task = Arc::clone(&task_list_state);
                        async move {
                            let tasks = task.lock().unwrap().clone().map(|task| vec![task]).unwrap_or_default();
                            Json(json!({"tasks":tasks,"revision":"recovered"}))
                        }
                    }).post(move |Json(body): Json<Value>| {
                        let requests = Arc::clone(&request_state);
                        let task = Arc::clone(&task_for_post);
                        let recovered = Arc::clone(&recovered_for_post);
                        async move {
                            requests.lock().unwrap().push(body.clone());
                            let healthy = *recovered.lock().unwrap();
                            let created = json!({"id":body["task_id"],"name":body["name"],"archived":false,"created_at":body["occurred_at"],"updated_at":body["occurred_at"],"latest_work_start":null});
                            if committed || healthy { *task.lock().unwrap() = Some(created.clone()); }
                            let receipt = json!({"request_id":body["request_id"],"applied_revision":"created","replayed":false,"result":{"kind":"task","value":created}});
                            if healthy {
                                (StatusCode::OK, Json(receipt.clone()))
                            } else if failure == "empty-revision" {
                                (StatusCode::OK, Json(json!({"request_id":body["request_id"],"applied_revision":"","replayed":false,"result":{"kind":"task","value":created}})))
                            } else if failure == "wrong-result" {
                                (StatusCode::OK, Json(json!({"request_id":body["request_id"],"applied_revision":"created","replayed":false,"result":{"kind":"tracking_already_idle"}})))
                            } else {
                                (if failure == "server-error" { StatusCode::INTERNAL_SERVER_ERROR } else { StatusCode::OK }, Json(json!({"unexpected":true})))
                            }
                        }
                    }))
                    .route("/v1/tracking", get(|| async { Json(json!({"active_worklog":null,"revision":"recovered"})) }))
                    .route("/v1/tasks/{id}", get(move || {
                        let task = task_for_get.lock().unwrap().clone().unwrap();
                        async move { Json(json!({"task":task,"revision":"recovered"})) }
                    }))
                },
            );
            let mut client = Client::open(&server.endpoint());
            client.task_list(true);
            let failed = client.create("Recovery project", at());
            assert_eq!(failed["uncertain"], true, "{failure}: {failed}");
            assert_eq!(failed["requiresRefresh"], true);
            let first = requests.lock().unwrap()[0].clone();
            assert_eq!(client.create("Recovery project", at())["uncertain"], false);
            client.task_list(true);
            *recovered.lock().unwrap() = true;
            let count = requests.lock().unwrap().len();
            let confirmed = client.create("Recovery project", at() + Duration::seconds(90));
            assert!(confirmed.get("error").is_none(), "{failure}: {confirmed}");
            assert_eq!(confirmed["data"]["task"]["id"], first["task_id"]);
            let sent = requests.lock().unwrap();
            assert_eq!(sent.len(), count + usize::from(!committed));
            for request in sent.iter() {
                assert_eq!(request["task_id"], first["task_id"]);
                assert_eq!(request["occurred_at"], first["occurred_at"]);
            }
            for request in sent.iter().take(count) {
                assert_eq!(request["request_id"], first["request_id"]);
            }
            if !committed {
                assert_eq!(sent.last().unwrap()["request_id"], first["request_id"]);
                assert_eq!(sent.last().unwrap()["expected_revision"], "recovered");
            }
        }
    }
}

#[test]
fn creation_refreshes_its_task_guard_after_another_client_adds_an_unrelated_task() {
    let server = Server::start();
    let mut first = Client::open(&server.endpoint());
    let mut second = Client::open(&server.endpoint());
    let initial = first.task_list(true);
    second.task_list(true);
    let concurrent = second.create("Other client", at());
    assert!(concurrent.get("error").is_none(), "{concurrent}");
    let created = first.create("My project", at());
    assert!(created.get("error").is_none(), "{created}");
    assert_eq!(
        first.task_list(false).0["data"]["value"]
            .as_array()
            .unwrap()
            .len(),
        initial.0["data"]["value"].as_array().unwrap().len() + 2
    );
    let stored = TrackerApplication::load(SqliteRepository::open(server.path()).unwrap()).unwrap();
    assert_eq!(
        stored
            .tasks(tracker_application::TaskOrdering::default())
            .iter()
            .filter(|item| item.task.name().as_str() == "My project")
            .count(),
        1
    );
}

#[test]
fn remote_rename_preserves_active_and_archived_tasks_and_existing_worklogs() {
    let server = Server::start();
    let mut client = Client::open(&server.endpoint());
    let initial = client.task_list(true);
    let id = initial.0["data"]["value"][0]["id"].as_str().unwrap();
    let archived_id = initial.0["data"]["value"][1]["id"].as_str().unwrap();
    let started = client.start(id, at());
    let history = client.history(id, None);
    let renamed_at = Utc::now() + Duration::seconds(5);
    let renamed = client.rename(id, "  Shared name 🛠  ", renamed_at);
    assert!(renamed.get("error").is_none(), "{renamed}");
    assert_eq!(
        client.task_list(false).1["data"]["value"],
        started["data"]["active"]
    );
    assert_eq!(client.history(id, None)["data"], history["data"]);
    let repository = SqliteRepository::open(server.path()).unwrap();
    let mut application = TrackerApplication::load(repository).unwrap();
    let stored = application.task(id.parse().unwrap()).unwrap();
    assert_eq!(stored.name().as_str(), "Shared name 🛠");
    assert_eq!(
        stored.updated_at(),
        DateTime::from_timestamp_micros(renamed_at.timestamp_micros()).unwrap()
    );
    application
        .archive_task(archived_id.parse().unwrap(), renamed_at)
        .unwrap();
    drop(application);
    client.task_list(true);
    let renamed_archived = client.rename(archived_id, "Shared name 🛠", renamed_at);
    assert!(
        renamed_archived.get("error").is_none(),
        "{renamed_archived}"
    );
    let task = &renamed_archived["data"]["task"];
    assert_eq!(task["name"], "Shared name 🛠");
    assert_eq!(task["archived"], true);
    assert_eq!(
        client.task_list(false).1["data"]["value"],
        started["data"]["active"]
    );
}

#[test]
fn remote_rename_conflicts_and_outages_require_reconciliation_without_changing_confirmed_state() {
    let mut server = Server::start();
    let mut first = Client::open(&server.endpoint());
    let mut second = Client::open(&server.endpoint());
    let initial = first.task_list(true);
    second.task_list(true);
    let id = initial.0["data"]["value"][0]["id"].as_str().unwrap();
    let concurrent = second.rename(id, "Other client name", Utc::now());
    assert!(concurrent.get("error").is_none(), "{concurrent}");
    let conflict = first.rename(id, "Updated name", Utc::now());
    assert_eq!(conflict["kind"], "conflict", "{conflict}");
    assert_eq!(conflict["uncertain"], false);
    assert_eq!(conflict["requiresRefresh"], true);
    assert_eq!(
        first.rename(id, "Updated name", Utc::now())["requiresRefresh"],
        true
    );
    first.task_list(true);
    let renamed = first.rename(id, "Updated name", Utc::now());
    assert!(renamed.get("error").is_none(), "{renamed}");
    let confirmed = first.task_list(false);
    server.stop();
    let offline = first.rename(id, "Offline name", Utc::now());
    assert_eq!(offline["kind"], "unavailable");
    assert_eq!(offline["uncertain"], false);
    assert_eq!(offline["requiresRefresh"], true);
    assert_eq!(first.task_list(false), confirmed);
    assert_eq!(
        first.rename(id, "Offline name", Utc::now())["uncertain"],
        false
    );
    assert_eq!(first.task_list(true).0["kind"], "unavailable");
}

#[test]
fn remote_rename_rejects_inconsistent_result_names_and_invalid_receipts() {
    for failure in ["empty-revision", "wrong-request", "result-name"] {
        let id = tracker_domain::TaskId::generate().to_string();
        let id_for_post = id.clone();
        let initial_task = json!({"id":id,"name":"Original","archived":false,"created_at":at(),"updated_at":at(),"latest_work_start":null});
        let task_for_get = initial_task.clone();
        let server = Server::launch(
            TempDir::new().unwrap(),
            "127.0.0.1:0".parse().unwrap(),
            move || {
                Router::new()
                .route("/v1/health", get(|| async { Json(json!({"status":"ok","protocol_version":tracker_protocol::VERSION})) }))
                .route("/v1/tasks", get(move || {
                    let task = initial_task.clone();
                    async move { Json(json!({"tasks":[task],"revision":"initial"})) }
                }))
                .route("/v1/tracking", get(|| async { Json(json!({"active_worklog":null,"revision":"initial"})) }))
                .route("/v1/tasks/{id}", get(move || {
                    let task = task_for_get.clone();
                    async move { Json(json!({"task":task,"revision":"initial"})) }
                }).patch(move |Json(body): Json<Value>| {
                    let id = id_for_post.clone();
                    async move {
                        let task = json!({"id":id,"name":if failure == "result-name" {"Wrong name"} else {"Updated"},"archived":false,"created_at":at(),"updated_at":at(),"latest_work_start":null});
                        Json(json!({"request_id":if failure == "wrong-request" {json!(tracker_domain::WorklogId::generate().to_string())} else {body["request_id"].clone()},"applied_revision":if failure == "empty-revision" {""} else {"renamed"},"replayed":false,"result":{"kind":"task","value":task}}))
                    }
                }))
            },
        );
        let mut client = Client::open(&server.endpoint());
        client.task_list(true);
        let rejected = client.rename(&id, "Updated", at());
        assert_eq!(rejected["kind"], "protocol", "{failure}: {rejected}");
        assert_eq!(rejected["uncertain"], true);
        assert_eq!(rejected["requiresRefresh"], true);
        assert_eq!(client.rename(&id, "Updated", at())["uncertain"], false);
        assert!(client.task_list(true).0.get("error").is_none());
    }
}

#[test]
fn stale_remote_writes_require_refresh_and_never_stop_another_clients_timer() {
    let server = Server::start();
    let mut first = Client::open(&server.endpoint());
    let mut second = Client::open(&server.endpoint());
    let initial = first.task_list(true);
    second.task_list(true);
    let tasks = initial.0["data"]["value"].as_array().unwrap();
    let started = first.start(tasks[0]["id"].as_str().unwrap(), at());
    second.task_list(true);
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
        first.task_list(true).1["data"]["value"],
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
    let snapshot = first.task_list(true);
    second.task_list(true);
    let first_task = snapshot.0["data"]["value"][0]["id"].as_str().unwrap();
    let second_task = snapshot.0["data"]["value"][1]["id"].as_str().unwrap();
    let started = second.start(first_task, at());
    let active_id = started["data"]["active"]["id"].as_str().unwrap();
    first.history(first_task, None);
    let rejected = first.start_if_active(second_task, None, at() + Duration::seconds(1));
    assert_eq!(rejected["kind"], "conflict", "{rejected}");
    assert_eq!(rejected["requiresRefresh"], true);
    assert_eq!(
        second.task_list(true).1["data"]["value"],
        started["data"]["active"]
    );
    first.task_list(true);
    let switched = first.start_if_active(second_task, Some(active_id), at() + Duration::seconds(2));
    assert_eq!(
        switched["data"]["active"]["taskId"], second_task,
        "{switched}"
    );
    let new_id = switched["data"]["active"]["id"].as_str().unwrap();
    let stale = first.start_if_active(first_task, Some(active_id), at() + Duration::seconds(3));
    assert_eq!(stale["kind"], "conflict");
    first.task_list(true);
    let stopped = first.stop(new_id, at() + Duration::seconds(4));
    assert!(stopped["data"]["active"].is_null());
    let running_while_idle =
        first.start_if_active(first_task, Some(new_id), at() + Duration::seconds(5));
    assert_eq!(running_while_idle["kind"], "conflict");
    first.task_list(true);
    let idle_start = first.start_if_active(first_task, None, at() + Duration::seconds(6));
    assert_eq!(idle_start["data"]["active"]["taskId"], first_task);
}

#[test]
fn unavailable_server_keeps_confirmed_cache_and_blocks_writes_without_local_fallback() {
    let mut server = Server::start();
    let mut client = Client::open(&server.endpoint());
    let snapshot = client.task_list(true);
    server.stop();
    let task = snapshot.0["data"]["value"][0]["id"].as_str().unwrap();
    let failure = client.start(task, at());
    assert_eq!(failure["kind"], "unavailable", "{failure}");
    assert_eq!(failure["uncertain"], false);
    assert_eq!(failure["requiresRefresh"], true);
    assert_eq!(client.task_list(false), snapshot);
    assert_eq!(client.task_list(true).0["kind"], "unavailable");
    assert_eq!(client.start(task, at())["uncertain"], false);
    let repository = SqliteRepository::open(server.path()).unwrap();
    let application = TrackerApplication::load(repository).unwrap();
    assert!(matches!(
        application.current_tracking(),
        TrackingState::Idle
    ));
}

#[test]
fn incompatible_health_protocol_is_reported_before_loading_resources() {
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
    let failure = client.task_list(true).0;
    assert_eq!(failure["kind"], "protocol", "{failure}");
    assert_eq!(failure["uncertain"], false);
    assert_eq!(failure["requiresRefresh"], true);
    assert!(client.task_list(false).0["data"].is_null());
}

#[test]
fn failed_write_response_remains_uncertain_after_a_successful_recovery_read() {
    let server = Server::launch(
        TempDir::new().unwrap(),
        "127.0.0.1:0".parse().unwrap(),
        || {
            Router::new()
            .route("/v1/health", get(|| async { Json(json!({"status": "ok", "protocol_version": tracker_protocol::VERSION})) }))
            .route("/v1/tasks", get(|| async { Json(json!({"tasks": [{"id":"00000000-0000-0000-0000-000000000001","name":"Tracking project","archived":false,"created_at":at(),"updated_at":at(),"latest_work_start":null}], "revision": "confirmed"})) }))
            .route("/v1/tasks/{id}", get(|| async { Json(json!({"task":{"id":"00000000-0000-0000-0000-000000000001","name":"Tracking project","archived":false,"created_at":at(),"updated_at":at(),"latest_work_start":null},"revision":"confirmed"})) }))
            .route("/v1/tracking", get(|| async { Json(json!({"active_worklog":null,"revision":"confirmed"})) }).put(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "Unconfirmed write") }))
        },
    );
    let mut client = Client::open(&server.endpoint());
    assert!(client.task_list(true).0.get("error").is_none());
    let failure = client.start("00000000-0000-0000-0000-000000000001", at());
    assert_eq!(failure["kind"], "unavailable", "{failure}");
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
    client.task_list(true);
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
    let snapshot = client.task_list(true);
    let task = CString::new(snapshot.0["data"]["value"][0]["id"].as_str().unwrap()).unwrap();
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
    assert!(client.task_list(false).1["data"]["value"].is_null());
}
