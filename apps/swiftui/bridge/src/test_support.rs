//! Helpers shared by the bridge's unit-test modules.

use std::ffi::{CStr, c_char};
use std::sync::{Arc, Mutex};

use axum::{Json, Router, extract::Path, http::StatusCode, response::IntoResponse, routing::get};
use chrono::{DateTime, Utc};
use serde_json::Value;
use tracker_domain::{TaskId, TrackingState, Worklog};
use tracker_protocol::{
    ErrorCode, ErrorDto, HealthDto, TaskDto, TaskListStateDto, TaskResourceDto, TasksDto,
    TrackingDto, WorklogDto, WorklogResourceDto,
};

use crate::backend::Backend;
use crate::lifecycle::open_fixture;
use crate::{Bridge, tt_bridge_refresh_resources, tt_bridge_string_free};

/// Task and tracking resources served by the mock resource routes.
pub(crate) type Resources = (TasksDto, TrackingDto);

/// Decodes the bridge response and releases its C string allocation.
pub(crate) fn response(pointer: *mut c_char) -> Value {
    assert!(!pointer.is_null());
    // SAFETY: The pointer comes from the bridge and is released once.
    unsafe {
        let value = serde_json::from_str(CStr::from_ptr(pointer).to_str().unwrap()).unwrap();
        tt_bridge_string_free(pointer);
        value
    }
}

/// Returns a timestamp with sub-second precision, so tests catch truncation.
pub(crate) fn at(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 123_456_000).unwrap()
}

/// Records a worklog on the bridge's local application and returns it from history.
pub(crate) fn record(bridge: &mut Bridge, task: TaskId, start: i64, end: Option<i64>) -> Worklog {
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

/// Serves `/v1/health` as a compatible server.
pub(crate) fn healthy_router() -> Router {
    Router::new().route(
        "/v1/health",
        get(|| async {
            Json(HealthDto {
                status: "ok".into(),
                protocol_version: tracker_protocol::VERSION,
            })
        }),
    )
}

pub(crate) fn resource_dtos(
    items: Vec<tracker_application::TaskListItem>,
    active: Option<Worklog>,
) -> Resources {
    (
        TasksDto {
            tasks: items.iter().map(TaskDto::from).collect(),
        },
        TrackingDto {
            active_worklog: active.as_ref().map(WorklogDto::from),
        },
    )
}

pub(crate) fn resource_router(resources: Resources, worklogs: Vec<WorklogDto>) -> Router {
    resource_router_state(Arc::new(Mutex::new(resources)), worklogs)
}

pub(crate) fn resource_router_state(
    resources: Arc<Mutex<Resources>>,
    worklogs: Vec<WorklogDto>,
) -> Router {
    resource_router_resources(resources, Arc::new(Mutex::new(worklogs)))
}

pub(crate) fn resource_router_resources(
    resources: Arc<Mutex<Resources>>,
    worklogs: Arc<Mutex<Vec<WorklogDto>>>,
) -> Router {
    let aggregate_resources = resources.clone();
    let task_list = resources.clone();
    let tracking_state = resources.clone();
    Router::new()
        .route(
            "/v1/state/task-list",
            get(move || {
                let resources = aggregate_resources.lock().unwrap();
                let dto = TaskListStateDto {
                    tasks: resources.0.tasks.clone(),
                    active_worklog: resources.1.active_worklog.clone(),
                };
                async move { Json(dto) }
            }),
        )
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
                    match tasks.tasks.into_iter().find(|task| task.id == id) {
                        Some(task) => Json(TaskResourceDto { task }).into_response(),
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
                async move {
                    match worklogs.into_iter().find(|worklog| worklog.id == id) {
                        Some(worklog) => Json(WorklogResourceDto { worklog }).into_response(),
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

/// Runs a router on a local port until stopped or dropped.
pub(crate) struct Server {
    endpoint: String,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
    completed: std::sync::mpsc::Receiver<()>,
}

impl Server {
    pub(crate) fn start(router: Router) -> Self {
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

    pub(crate) fn client(&self) -> Bridge {
        Bridge::new(Backend::remote(&self.endpoint).unwrap())
    }

    pub(crate) fn stop(&mut self) {
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

/// Runs the real tracker server on a fixture database.
pub(crate) struct FixtureServer {
    // Declared before `directory` so the server stops before the database is removed.
    server: Server,
    directory: tempfile::TempDir,
}

impl FixtureServer {
    pub(crate) fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("server.db");
        drop(open_fixture(&path).unwrap());
        let server = Server::start(tracker_server::router_for_database(&path).unwrap());
        Self { server, directory }
    }

    pub(crate) fn database_path(&self) -> std::path::PathBuf {
        self.directory.path().join("server.db")
    }

    /// Returns a remote bridge that has loaded the server's resources.
    pub(crate) fn client(&self) -> Bridge {
        let mut bridge = self.server.client();
        // SAFETY: The bridge is live and uniquely accessed.
        let loaded = response(unsafe { tt_bridge_refresh_resources(&mut bridge) });
        assert!(loaded.get("error").is_none(), "{loaded}");
        bridge
    }

    pub(crate) fn stop(&mut self) {
        self.server.stop();
    }
}
