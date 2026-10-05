use super::*;

fn response(pointer: *mut c_char) -> Value {
    assert!(!pointer.is_null());
    // SAFETY: The pointer comes from the bridge and is released once.
    unsafe {
        let value = serde_json::from_str(CStr::from_ptr(pointer).to_str().unwrap()).unwrap();
        tt_bridge_string_free(pointer);
        value
    }
}

fn pause(bridge: &mut Bridge, worklog_id: &str, at: &str) -> Value {
    let worklog_id = CString::new(worklog_id).unwrap();
    let at = CString::new(at).unwrap();
    // SAFETY: The bridge and both strings remain live for this call.
    response(unsafe { tt_bridge_pause_tracking_at(bridge, worklog_id.as_ptr(), at.as_ptr()) })
}

fn resume(bridge: &mut Bridge, task_id: TaskId, at: &str) -> Value {
    let task_id = CString::new(task_id.to_string()).unwrap();
    let at = CString::new(at).unwrap();
    // SAFETY: The bridge and both strings remain live for this call.
    response(unsafe { tt_bridge_resume_tracking_at(bridge, task_id.as_ptr(), at.as_ptr()) })
}

fn refresh(bridge: &mut Bridge) -> Value {
    // SAFETY: The bridge is live and uniquely accessed.
    response(unsafe { tt_bridge_snapshot(bridge, true) })
}

#[test]
fn pause_owns_only_a_successful_stop_and_resume_excludes_the_locked_interval() {
    let directory = tempfile::tempdir().unwrap();
    let mut bridge = open_fixture(&directory.path().join("tracker.db")).unwrap();
    let task_id = bridge.application.tasks(TaskOrdering::default())[0]
        .task
        .id();
    let started = resume(&mut bridge, task_id, "2026-10-04T10:00:00.123456789Z");
    assert!(started.get("error").is_none(), "{started}");
    let worklog_id = started["data"]["active"]["id"].as_str().unwrap();
    let paused = pause(&mut bridge, worklog_id, "2026-10-04T10:10:00.987654321Z");
    assert_eq!(paused["data"]["didStop"], true);
    assert!(paused["data"]["snapshot"]["active"].is_null());
    assert_eq!(
        paused["data"]["snapshot"]["tasks"],
        started["data"]["tasks"]
    );
    let repeated = pause(&mut bridge, worklog_id, "2026-10-04T10:11:00Z");
    assert_eq!(repeated["data"]["didStop"], false);
    let resumed = resume(&mut bridge, task_id, "2026-10-04T10:20:00Z");
    assert!(resumed.get("error").is_none(), "{resumed}");
    assert_eq!(resumed["data"]["active"]["taskId"], task_id.to_string());
    assert_eq!(
        resumed["data"]["active"]["start"],
        "2026-10-04T10:20:00.000000Z"
    );
    assert_ne!(resumed["data"]["active"]["id"], worklog_id);
    let page = bridge.application.worklogs_for_task(task_id, None).unwrap();
    assert_eq!(page.worklogs.len(), 2);
    let stopped = page
        .worklogs
        .iter()
        .find(|item| item.id().to_string() == worklog_id)
        .unwrap();
    assert_eq!(timestamp(stopped.start()), "2026-10-04T10:00:00.123456Z");
    assert_eq!(
        timestamp(stopped.end().unwrap()),
        "2026-10-04T10:10:00.987654Z"
    );
}

#[test]
fn local_automation_never_stops_or_switches_a_competing_clients_timer() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tracker.db");
    let mut first = open_fixture(&path).unwrap();
    let tasks = first.application.tasks(TaskOrdering::default());
    let started = resume(&mut first, tasks[0].task.id(), "2026-10-04T10:00:00Z");
    let old_id = started["data"]["active"]["id"].as_str().unwrap();
    let mut second = open_fixture(&path).unwrap();
    second
        .application
        .set_active_task(tasks[1].task.id(), "2026-10-04T10:10:00Z".parse().unwrap())
        .unwrap_or_else(|error| panic!("{}", error.message));
    let foreign = serde_json::to_value(snapshot(&second.application)).unwrap()["active"].clone();

    let rejected = pause(&mut first, old_id, "2026-10-04T10:20:00Z");
    assert!(rejected.get("error").is_some());
    assert!(rejected.get("data").is_none());
    let rejected = resume(&mut first, tasks[0].task.id(), "2026-10-04T10:30:00Z");
    assert!(rejected.get("error").is_some());
    assert_eq!(refresh(&mut first)["data"]["active"], foreign);
    let page = second
        .application
        .worklogs_for_task(tasks[1].task.id(), None)
        .unwrap();
    assert_eq!(page.worklogs.len(), 1);
    assert!(page.worklogs[0].is_active());
}

#[test]
fn local_resume_rejects_a_timer_started_after_the_idle_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tracker.db");
    let mut first = open_fixture(&path).unwrap();
    let tasks = first.application.tasks(TaskOrdering::default());
    let mut second = open_fixture(&path).unwrap();
    let foreign = resume(&mut second, tasks[1].task.id(), "2026-10-04T10:00:00Z");
    let rejected = resume(&mut first, tasks[0].task.id(), "2026-10-04T10:10:00Z");
    assert!(rejected.get("error").is_some());
    assert_eq!(
        refresh(&mut first)["data"]["active"],
        foreign["data"]["active"]
    );
}

#[test]
fn local_resume_rejects_missing_and_archived_tasks() {
    let directory = tempfile::tempdir().unwrap();
    let mut bridge = open_fixture(&directory.path().join("tracker.db")).unwrap();
    let task_id = bridge.application.tasks(TaskOrdering::default())[0]
        .task
        .id();
    bridge
        .application
        .archive_task(task_id, "2026-10-04T10:00:00Z".parse().unwrap())
        .unwrap();
    assert!(
        resume(&mut bridge, task_id, "2026-10-04T10:10:00Z")
            .get("error")
            .is_some()
    );
    assert_eq!(
        resume(&mut bridge, TaskId::generate(), "2026-10-04T10:10:00Z")["error"],
        "Task not found"
    );
    assert!(refresh(&mut bridge)["data"]["active"].is_null());
}

#[test]
fn automation_commands_reject_null_bridges_identifiers_and_timestamps() {
    let directory = tempfile::tempdir().unwrap();
    let mut bridge = open_fixture(&directory.path().join("tracker.db")).unwrap();
    let task_id = CString::new(
        bridge.application.tasks(TaskOrdering::default())[0]
            .task
            .id()
            .to_string(),
    )
    .unwrap();
    let worklog_id = CString::new(WorklogId::generate().to_string()).unwrap();
    let at = c"2026-10-04T10:00:00Z";
    let invalid = c"invalid";
    let non_utf8 = CString::new(vec![0xff]).unwrap();
    // SAFETY: Null handles and strings are supported by both entry points.
    unsafe {
        assert_eq!(
            response(tt_bridge_pause_tracking_at(
                ptr::null_mut(),
                ptr::null(),
                ptr::null()
            ))["error"],
            "Database is not open"
        );
        assert_eq!(
            response(tt_bridge_resume_tracking_at(
                ptr::null_mut(),
                ptr::null(),
                ptr::null()
            ))["error"],
            "Database is not open"
        );
        for input in [ptr::null(), invalid.as_ptr(), non_utf8.as_ptr()] {
            assert_eq!(
                response(tt_bridge_pause_tracking_at(&mut bridge, input, at.as_ptr()))["error"],
                "Invalid worklog ID"
            );
            assert_eq!(
                response(tt_bridge_resume_tracking_at(
                    &mut bridge,
                    input,
                    at.as_ptr()
                ))["error"],
                "Invalid task ID"
            );
            assert_eq!(
                response(tt_bridge_pause_tracking_at(
                    &mut bridge,
                    worklog_id.as_ptr(),
                    input
                ))["error"],
                "Invalid tracking timestamp"
            );
            assert_eq!(
                response(tt_bridge_resume_tracking_at(
                    &mut bridge,
                    task_id.as_ptr(),
                    input
                ))["error"],
                "Invalid tracking timestamp"
            );
        }
    }
    assert!(refresh(&mut bridge)["data"]["active"].is_null());
}

#[test]
fn remote_resume_requires_an_authoritative_snapshot_before_writing() {
    let mut bridge = Bridge {
        application: Backend::remote("http://127.0.0.1:12345").unwrap(),
    };
    let rejected = resume(&mut bridge, TaskId::generate(), "2026-10-04T10:00:00Z");
    assert_eq!(rejected["kind"], "unavailable");
    assert_eq!(rejected["uncertain"], false);
    assert_eq!(rejected["requiresRefresh"], true);
}

struct Server {
    directory: tempfile::TempDir,
    endpoint: String,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
    completed: std::sync::mpsc::Receiver<()>,
}

impl Server {
    fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        drop(open_fixture(&directory.path().join("server.db")).unwrap());
        let router =
            tracker_server::router_for_database(&directory.path().join("server.db")).unwrap();
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
            directory,
            endpoint,
            shutdown: Some(shutdown),
            thread: Some(thread),
            completed,
        }
    }

    fn client(&self) -> Bridge {
        let mut bridge = Bridge {
            application: Backend::remote(&self.endpoint).unwrap(),
        };
        let loaded = refresh(&mut bridge);
        assert!(loaded.get("error").is_none(), "{loaded}");
        bridge
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
fn remote_pause_reports_ownership_and_resume_commits_the_captured_timestamp() {
    let server = Server::start();
    let mut bridge = server.client();
    let task_id = bridge.application.tasks(TaskOrdering::default())[0]
        .task
        .id();
    let started = resume(&mut bridge, task_id, "2026-10-04T10:00:00Z");
    let worklog_id = started["data"]["active"]["id"].as_str().unwrap();
    let stopped = pause(&mut bridge, worklog_id, "2026-10-04T10:10:00Z");
    assert_eq!(stopped["data"]["didStop"], true);
    assert!(stopped["data"]["snapshot"]["active"].is_null());
    assert_eq!(
        pause(&mut bridge, worklog_id, "2026-10-04T10:11:00Z")["data"]["didStop"],
        false
    );
    let resumed = resume(&mut bridge, task_id, "2026-10-04T10:20:00Z");
    assert!(resumed.get("error").is_none(), "{resumed}");
    assert_ne!(resumed["data"]["active"]["id"], worklog_id);
    assert_eq!(
        resumed["data"]["active"]["start"],
        "2026-10-04T10:20:00.000000Z"
    );
    let repository = SqliteRepository::open(&server.directory.path().join("server.db")).unwrap();
    assert_eq!(
        timestamp(
            repository
                .find_worklog(worklog_id.parse().unwrap())
                .unwrap()
                .unwrap()
                .end()
                .unwrap()
        ),
        "2026-10-04T10:10:00.000000Z"
    );
}

#[test]
fn remote_resume_revision_guard_preserves_a_competing_clients_timer() {
    let server = Server::start();
    let mut first = server.client();
    let tasks = first.application.tasks(TaskOrdering::default());
    let mut second = server.client();
    let foreign = resume(&mut second, tasks[1].task.id(), "2026-10-04T10:00:00Z");
    let rejected = resume(&mut first, tasks[0].task.id(), "2026-10-04T10:10:00Z");
    assert_eq!(rejected["kind"], "conflict");
    assert_eq!(rejected["uncertain"], false);
    assert_eq!(rejected["requiresRefresh"], true);
    assert_eq!(
        refresh(&mut first)["data"]["active"],
        foreign["data"]["active"]
    );
    let cached_rejection = resume(&mut first, tasks[0].task.id(), "2026-10-04T10:20:00Z");
    assert_eq!(cached_rejection["kind"], "conflict");
    assert_eq!(cached_rejection["requiresRefresh"], true);
    assert_eq!(
        refresh(&mut second)["data"]["active"],
        foreign["data"]["active"]
    );
}

#[test]
fn remote_pause_conflict_does_not_claim_another_clients_stop() {
    let server = Server::start();
    let mut first = server.client();
    let task_id = first.application.tasks(TaskOrdering::default())[0]
        .task
        .id();
    let started = resume(&mut first, task_id, "2026-10-04T10:00:00Z");
    let worklog_id = started["data"]["active"]["id"].as_str().unwrap();
    let mut second = server.client();
    assert_eq!(
        pause(&mut second, worklog_id, "2026-10-04T10:10:00Z")["data"]["didStop"],
        true
    );
    let rejected = pause(&mut first, worklog_id, "2026-10-04T10:20:00Z");
    assert_eq!(rejected["kind"], "conflict");
    assert_eq!(rejected["uncertain"], false);
    assert!(rejected.get("data").is_none());
    assert!(refresh(&mut first)["data"]["active"].is_null());
    assert_eq!(
        pause(&mut first, worklog_id, "2026-10-04T10:30:00Z")["data"]["didStop"],
        false
    );
}

#[test]
fn remote_pause_connection_failure_is_uncertain_and_requires_reconciliation() {
    let mut server = Server::start();
    let mut bridge = server.client();
    let task_id = bridge.application.tasks(TaskOrdering::default())[0]
        .task
        .id();
    let started = resume(&mut bridge, task_id, "2026-10-04T10:00:00Z");
    let worklog_id = started["data"]["active"]["id"].as_str().unwrap();
    server.stop();
    let rejected = pause(&mut bridge, worklog_id, "2026-10-04T10:10:00Z");
    assert_eq!(rejected["kind"], "unavailable");
    assert_eq!(rejected["uncertain"], true);
    assert_eq!(rejected["requiresRefresh"], true);
    assert!(rejected.get("data").is_none());
    let refused = resume(&mut bridge, task_id, "2026-10-04T10:20:00Z");
    assert_eq!(refused["uncertain"], false);
    assert_eq!(refused["requiresRefresh"], true);
}
