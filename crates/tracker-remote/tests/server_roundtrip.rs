use std::{net::SocketAddr, path::PathBuf, sync::mpsc, thread};

use axum::{Json, Router, routing::get};
use chrono::{DateTime, Duration, Utc};
use tempfile::TempDir;
use tokio::sync::oneshot;
use tracker_application::{
    ApplicationError, ClearActiveTaskOutcome, ReportQueries, RepositoryError, SetActiveTaskOutcome,
    TaskOperations, TaskOrdering, TaskQueries, TrackingOperations, WorklogCursor,
    WorklogOperations, WorklogQueries,
};
use tracker_domain::{TaskName, WorklogId, WorklogTimes};
use tracker_protocol::{HealthDto, SnapshotDto, VERSION};
use tracker_remote::RemoteApplication;
use tracker_remote::RemoteFailureKind;

struct TestServer {
    _directory: TempDir,
    address: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl TestServer {
    fn start() -> Self {
        let directory = TempDir::new().unwrap();
        let database = directory.path().join("remote.sqlite3");
        let (address, shutdown, thread) = Self::launch(database, "127.0.0.1:0".parse().unwrap());
        Self {
            _directory: directory,
            address,
            shutdown: Some(shutdown),
            thread: Some(thread),
        }
    }

    fn launch(
        database: PathBuf,
        bind: SocketAddr,
    ) -> (SocketAddr, oneshot::Sender<()>, thread::JoinHandle<()>) {
        let (address_tx, address_rx) = mpsc::channel();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let thread = thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let router = tracker_server::router_for_database(&database).unwrap();
                let listener = tokio::net::TcpListener::bind(bind).await.unwrap();
                address_tx.send(listener.local_addr().unwrap()).unwrap();
                axum::serve(listener, router)
                    .with_graceful_shutdown(async {
                        let _ = shutdown_rx.await;
                    })
                    .await
                    .unwrap();
            });
        });
        let address = address_rx.recv().unwrap();
        (address, shutdown_tx, thread)
    }

    fn endpoint(&self) -> String {
        format!("http://{}/", self.address)
    }

    fn stop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }

    fn restart(&mut self) {
        self.stop();
        let database = self._directory.path().join("remote.sqlite3");
        let (address, shutdown, thread) = Self::launch(database, self.address);
        self.address = address;
        self.shutdown = Some(shutdown);
        self.thread = Some(thread);
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop();
    }
}

struct StubServer {
    address: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl StubServer {
    fn with_health(health: HealthDto) -> Self {
        let router = Router::new()
            .route(
                "/v1/health",
                get(move || {
                    let health = health.clone();
                    async move { Json(health) }
                }),
            )
            .route(
                "/v1/snapshot",
                get(|| async {
                    Json(SnapshotDto {
                        task_items: Vec::new(),
                        active_worklog: None,
                        revision: "stub-revision".into(),
                    })
                }),
            );
        let (address_tx, address_rx) = mpsc::channel();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let thread = thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                address_tx.send(listener.local_addr().unwrap()).unwrap();
                axum::serve(listener, router)
                    .with_graceful_shutdown(async {
                        let _ = shutdown_rx.await;
                    })
                    .await
                    .unwrap();
            });
        });
        Self {
            address: address_rx.recv().unwrap(),
            shutdown: Some(shutdown_tx),
            thread: Some(thread),
        }
    }

    fn endpoint(&self) -> String {
        format!("http://{}/", self.address)
    }
}

impl Drop for StubServer {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

fn name(raw: &str) -> TaskName {
    TaskName::new(raw).unwrap()
}

#[test]
fn remote_client_rejects_each_invalid_health_field_before_adopting_a_snapshot() {
    for health in [
        HealthDto {
            status: "ok".into(),
            protocol_version: VERSION + 1,
        },
        HealthDto {
            status: "maintenance".into(),
            protocol_version: VERSION,
        },
    ] {
        let server = StubServer::with_health(health);
        let mut client = RemoteApplication::disconnected(&server.endpoint()).unwrap();
        assert!(matches!(
            client.refresh(),
            Err(tracker_remote::RemoteError::Protocol(_))
        ));
        assert_eq!(client.last_failure(), Some(RemoteFailureKind::Protocol));
        assert!(client.snapshot().task_items.is_empty());
    }
}

#[test]
fn uncommitted_task_create_retries_after_server_restart() {
    let mut server = TestServer::start();
    let mut client = RemoteApplication::connect(&server.endpoint()).unwrap();
    let at = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    server.stop();
    assert!(
        client
            .create_task(name("Created after restart"), at)
            .is_err()
    );

    server.restart();
    let created = client
        .create_task(name("Created after restart"), at)
        .unwrap();
    assert_eq!(created.name().as_str(), "Created after restart");
    let observer = RemoteApplication::connect(&server.endpoint()).unwrap();
    assert_eq!(
        observer
            .tasks(TaskOrdering::RecentlyCreated)
            .into_iter()
            .filter(|item| item.task.name().as_str() == "Created after restart")
            .count(),
        1
    );
}

#[test]
fn remote_client_round_trips_every_application_operation() {
    fn assert_send<T: Send>() {}
    assert_send::<RemoteApplication>();

    let server = TestServer::start();
    let mut client = RemoteApplication::connect(&server.endpoint()).unwrap();
    let at = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let first = client.create_task(name("First project"), at).unwrap();
    let second = client
        .create_task(name("Second project"), at + Duration::seconds(1))
        .unwrap();
    let renamed = client
        .rename_task(
            first.id(),
            name("Renamed project"),
            at + Duration::seconds(2),
        )
        .unwrap();
    assert_eq!(renamed.name().as_str(), "Renamed project");
    assert_eq!(
        client.task(first.id()).unwrap().name().as_str(),
        "Renamed project"
    );

    let started = match client
        .set_active_task(first.id(), at + Duration::seconds(3))
        .unwrap()
    {
        SetActiveTaskOutcome::Started { worklog } => worklog,
        other => panic!("expected started worklog, got {other:?}"),
    };
    assert!(matches!(
        client
            .set_active_task(first.id(), at + Duration::seconds(4))
            .unwrap(),
        SetActiveTaskOutcome::AlreadyActive { .. }
    ));
    let switched = client
        .set_active_task(second.id(), at + Duration::seconds(10))
        .unwrap();
    let second_active = match switched {
        SetActiveTaskOutcome::Switched {
            stopped,
            started: next,
        } => {
            assert_eq!(stopped.id(), started.id());
            next
        }
        other => panic!("expected switched worklogs, got {other:?}"),
    };
    assert!(matches!(
        client
            .clear_active_task(second_active.id(), at + Duration::seconds(20))
            .unwrap(),
        ClearActiveTaskOutcome::Stopped { .. }
    ));
    assert!(matches!(
        client.current_tracking(),
        tracker_domain::TrackingState::Idle
    ));

    let page = client.worklogs_for_task(first.id(), None).unwrap();
    assert_eq!(page.worklogs.len(), 1);
    assert_eq!(page.worklogs[0].id(), started.id());
    let all = client.all_worklogs(None).unwrap();
    assert!(
        all.worklogs
            .iter()
            .any(|worklog| worklog.id() == second_active.id())
    );
    let report = client
        .report_totals(at, at + Duration::seconds(30), at + Duration::seconds(30))
        .unwrap();
    assert_eq!(report.total, Duration::seconds(17));

    let original = page.worklogs[0].times();
    let invalid = client
        .correct_worklog(
            started.id(),
            original,
            WorklogTimes::new(at + Duration::seconds(12), Some(at + Duration::seconds(11))),
            at + Duration::seconds(21),
        )
        .unwrap_err();
    assert_eq!(
        invalid.failure().message(),
        "corrected worklog end must not precede its start"
    );
    let corrected = client
        .correct_worklog(
            started.id(),
            original,
            WorklogTimes::new(at + Duration::seconds(4), Some(at + Duration::seconds(9))),
            at + Duration::seconds(21),
        )
        .unwrap();
    let moved = client
        .move_worklog(corrected.id(), first.id(), corrected.times(), second.id())
        .unwrap();
    assert_eq!(moved.task_id(), second.id());
    let deleted = client
        .delete_completed_worklog(moved.id(), second.id(), moved.times())
        .unwrap();
    assert_eq!(deleted.id(), started.id());
    assert!(
        client
            .worklogs_for_task(first.id(), None)
            .unwrap()
            .worklogs
            .is_empty()
    );

    client
        .archive_task(first.id(), at + Duration::seconds(22))
        .unwrap();
    assert!(client.task(first.id()).unwrap().is_archived());
    let archived = client
        .set_active_task(first.id(), at + Duration::seconds(23))
        .unwrap_err();
    assert_eq!(
        archived.failure().message(),
        format!("task {} is archived", first.id())
    );
    client
        .unarchive_task(first.id(), at + Duration::seconds(23))
        .unwrap();
    assert!(!client.task(first.id()).unwrap().is_archived());
    assert!(
        client
            .tasks(TaskOrdering::RecentlyWorked)
            .iter()
            .any(|item| item.task.id() == first.id())
    );
}

#[test]
fn stale_client_refreshes_after_conflicting_write() {
    let server = TestServer::start();
    let mut first = RemoteApplication::connect(&server.endpoint()).unwrap();
    let at = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let task = first.create_task(name("Shared task"), at).unwrap();
    let mut second = RemoteApplication::connect(&server.endpoint()).unwrap();

    first
        .rename_task(
            task.id(),
            name("Updated by first"),
            at + Duration::seconds(1),
        )
        .unwrap();
    assert!(
        second
            .rename_task(
                task.id(),
                name("Updated by second"),
                at + Duration::seconds(2)
            )
            .is_err()
    );
    assert_eq!(
        second.task(task.id()).unwrap().name().as_str(),
        "Updated by first"
    );
    second
        .rename_task(
            task.id(),
            name("Updated by second"),
            at + Duration::seconds(3),
        )
        .unwrap();
    first.refresh().unwrap();
    assert_eq!(
        first.task(task.id()).unwrap().name().as_str(),
        "Updated by second"
    );
}

#[test]
fn a_history_page_does_not_authorize_writes_from_a_stale_task_catalog() {
    let server = TestServer::start();
    let mut first = RemoteApplication::connect(&server.endpoint()).unwrap();
    let at = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let task = first.create_task(name("Shared task"), at).unwrap();
    let mut second = RemoteApplication::connect(&server.endpoint()).unwrap();

    first
        .rename_task(task.id(), name("New name"), at + Duration::seconds(1))
        .unwrap();
    second.worklogs_for_task(task.id(), None).unwrap();
    assert_eq!(
        second.task(task.id()).unwrap().name().as_str(),
        "Shared task"
    );
    assert!(
        second
            .rename_task(task.id(), name("Stale name"), at + Duration::seconds(2))
            .is_err()
    );
    assert_eq!(second.task(task.id()).unwrap().name().as_str(), "New name");
}

#[test]
fn disconnected_client_keeps_last_confirmed_state_and_rejects_writes() {
    let server = TestServer::start();
    let mut client = RemoteApplication::connect(&server.endpoint()).unwrap();
    let count = client.tasks(TaskOrdering::RecentlyCreated).len();
    assert!(count > 0);
    drop(server);

    let error = client.refresh().unwrap_err();
    assert!(error.is_unavailable());
    assert_eq!(client.last_failure(), Some(RemoteFailureKind::Unavailable));
    assert_eq!(client.tasks(TaskOrdering::RecentlyCreated).len(), count);
    let at = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    assert!(client.create_task(name("Offline task"), at).is_err());
    assert_eq!(client.tasks(TaskOrdering::RecentlyCreated).len(), count);
    assert!(matches!(
        client
            .report_totals(at, at + Duration::seconds(1), at)
            .unwrap_err(),
        ApplicationError::Repository(RepositoryError::Backend { message })
            if message == "tracker server is unavailable"
    ));
}

#[test]
fn task_history_rejects_a_cursor_from_another_task_and_refreshes_active_aggregates() {
    let server = TestServer::start();
    let mut first = RemoteApplication::connect(&server.endpoint()).unwrap();
    let at = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let requested = first.create_task(name("Requested"), at).unwrap();
    let active = first
        .create_task(name("Active"), at + Duration::seconds(1))
        .unwrap();
    let mut second = RemoteApplication::connect(&server.endpoint()).unwrap();
    let cursor = WorklogCursor {
        task_id: active.id(),
        start: at,
        id: WorklogId::generate(),
        revision: 0,
    };
    assert!(
        second
            .worklogs_for_task(requested.id(), Some(&cursor))
            .is_err()
    );

    let requested_at = at + Duration::seconds(2);
    let requested_worklog = match first.set_active_task(requested.id(), requested_at).unwrap() {
        SetActiveTaskOutcome::Started { worklog } => worklog,
        other => panic!("expected started worklog, got {other:?}"),
    };
    first
        .clear_active_task(requested_worklog.id(), at + Duration::seconds(3))
        .unwrap();
    let started_at = at + Duration::seconds(10);
    first.set_active_task(active.id(), started_at).unwrap();
    let page = second.worklogs_for_task(requested.id(), None).unwrap();
    assert_eq!(page.worklogs.len(), 1);
    assert_eq!(
        page.snapshot.requested_task_latest_work_start,
        Some(requested_at)
    );
    assert_eq!(
        page.snapshot.active_task_latest_work_start,
        Some(started_at)
    );
    assert_eq!(
        second
            .tasks(TaskOrdering::RecentlyWorked)
            .into_iter()
            .find(|item| item.task.id() == active.id())
            .unwrap()
            .latest_work_start,
        Some(started_at)
    );
    assert_eq!(
        second
            .tasks(TaskOrdering::RecentlyWorked)
            .into_iter()
            .find(|item| item.task.id() == requested.id())
            .unwrap()
            .latest_work_start,
        Some(requested_at)
    );
}

#[test]
fn task_history_returns_a_cursor_for_a_full_page() {
    let server = TestServer::start();
    let mut client = RemoteApplication::connect(&server.endpoint()).unwrap();
    let at = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let task = client.create_task(name("Paged history"), at).unwrap();
    for index in 0..51 {
        let start = at + Duration::seconds(2 * index + 1);
        let worklog = match client.set_active_task(task.id(), start).unwrap() {
            SetActiveTaskOutcome::Started { worklog } => worklog,
            other => panic!("expected started worklog, got {other:?}"),
        };
        client
            .clear_active_task(worklog.id(), start + Duration::seconds(1))
            .unwrap();
    }
    let first = client.worklogs_for_task(task.id(), None).unwrap();
    assert_eq!(first.worklogs.len(), 50);
    let cursor = first.next_cursor.expect("more worklogs remain");
    assert_eq!(cursor.task_id, task.id());
    let second = client.worklogs_for_task(task.id(), Some(&cursor)).unwrap();
    assert_eq!(second.worklogs.len(), 1);
    assert_ne!(first.worklogs[49].id(), second.worklogs[0].id());
}

#[test]
fn global_history_adopts_other_client_changes_and_reports_exclude_the_end_instant() {
    let server = TestServer::start();
    let mut first = RemoteApplication::connect(&server.endpoint()).unwrap();
    let mut second = RemoteApplication::connect(&server.endpoint()).unwrap();
    let at = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let task = first.create_task(name("Shared report"), at).unwrap();
    let started = match first
        .set_active_task(task.id(), at + Duration::seconds(10))
        .unwrap()
    {
        SetActiveTaskOutcome::Started { worklog } => worklog,
        other => panic!("expected started worklog, got {other:?}"),
    };
    first
        .clear_active_task(started.id(), at + Duration::seconds(20))
        .unwrap();

    let feed = second.all_worklogs(None).unwrap();
    assert_eq!(feed.worklogs[0].id(), started.id());
    assert!(
        feed.snapshot
            .task_items
            .iter()
            .any(|item| item.task.id() == task.id())
    );
    assert!(second.task(task.id()).is_some());
    assert_eq!(
        second
            .report_totals(
                at + Duration::seconds(15),
                at + Duration::seconds(20),
                at + Duration::seconds(30)
            )
            .unwrap()
            .total,
        Duration::seconds(5)
    );
    assert_eq!(
        second
            .report_totals(
                at + Duration::seconds(20),
                at + Duration::seconds(30),
                at + Duration::seconds(30)
            )
            .unwrap()
            .total,
        Duration::zero()
    );
}
