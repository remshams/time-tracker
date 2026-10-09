use std::{net::SocketAddr, path::PathBuf, sync::mpsc, thread};

use axum::{Json, Router, routing::get};
use chrono::{DateTime, Duration, Utc};
use tempfile::TempDir;
use tokio::sync::oneshot;
use tracker_application::{
    ClearActiveTaskOutcome, SetActiveTaskOutcome, TaskOrdering, WorklogCursor,
};
use tracker_domain::{TaskName, WorklogId, WorklogTimes};
use tracker_protocol::{HealthDto, VERSION};
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
        let router = Router::new().route(
            "/v1/health",
            get(move || {
                let health = health.clone();
                async move { Json(health) }
            }),
        );
        Self::from_router(router)
    }

    fn from_router(router: Router) -> Self {
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

async fn connected(endpoint: &str) -> Result<RemoteApplication, tracker_remote::RemoteError> {
    let mut client = RemoteApplication::connect(endpoint).await?;
    client.refresh().await?;
    Ok(client)
}

fn name(raw: &str) -> TaskName {
    TaskName::new(raw).unwrap()
}

#[tokio::test]
async fn remote_inactive_archive_uses_the_preview_revision() {
    let server = TestServer::start();
    let mut client = connected(&server.endpoint()).await.unwrap();
    let mut other = connected(&server.endpoint()).await.unwrap();
    let as_of = Utc::now();
    client
        .create_task(name("Old remote task"), as_of - Duration::days(20))
        .await
        .unwrap();
    other.refresh().await.unwrap();

    let preview = client.preview_inactive_tasks(as_of).await.unwrap();
    assert_eq!(preview.count, 1);
    assert_eq!(
        client.resolve_preview_tasks(&preview).await.unwrap()[0].name,
        "Old remote task"
    );
    other
        .create_task(name("New remote task"), as_of)
        .await
        .unwrap();
    client.refresh().await.unwrap();

    let error = client.archive_inactive_tasks(&preview).await.unwrap_err();
    assert_eq!(
        error.failure().message(),
        "Tracker state changed. Refresh and retry."
    );
    assert!(
        !client
            .tasks(TaskOrdering::RecentlyCreated)
            .into_iter()
            .find(|item| item.task.name().as_str() == "Old remote task")
            .unwrap()
            .task
            .is_archived()
    );

    let current = client.preview_inactive_tasks(Utc::now()).await.unwrap();
    assert_eq!(client.archive_inactive_tasks(&current).await.unwrap(), 1);
    assert!(
        client
            .tasks(TaskOrdering::RecentlyCreated)
            .into_iter()
            .find(|item| item.task.name().as_str() == "Old remote task")
            .unwrap()
            .task
            .is_archived()
    );
}

#[tokio::test]
async fn remote_client_rejects_each_invalid_health_field_before_adopting_a_snapshot() {
    for health in [
        HealthDto {
            status: "ok".into(),
            protocol_version: VERSION - 1,
        },
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
            client.refresh().await,
            Err(tracker_remote::RemoteError::Protocol(_))
        ));
        assert_eq!(client.last_failure(), Some(RemoteFailureKind::Protocol));
        assert!(client.snapshot().task_items.is_empty());
    }
}

#[tokio::test]
async fn uncommitted_task_create_retries_after_server_restart() {
    let mut server = TestServer::start();
    let mut client = connected(&server.endpoint()).await.unwrap();
    let at = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    server.stop();
    assert!(
        client
            .create_task(name("Created after restart"), at)
            .await
            .is_err()
    );

    server.restart();
    let created = client
        .create_task(name("Created after restart"), at)
        .await
        .unwrap();
    assert_eq!(created.name().as_str(), "Created after restart");
    let observer = connected(&server.endpoint()).await.unwrap();
    assert_eq!(
        observer
            .tasks(TaskOrdering::RecentlyCreated)
            .into_iter()
            .filter(|item| item.task.name().as_str() == "Created after restart")
            .count(),
        1
    );
}

#[tokio::test]
async fn remote_client_round_trips_every_application_operation() {
    fn assert_send<T: Send>() {}
    assert_send::<RemoteApplication>();

    let server = TestServer::start();
    let mut client = connected(&server.endpoint()).await.unwrap();
    let at = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let first = client.create_task(name("First project"), at).await.unwrap();
    let second = client
        .create_task(name("Second project"), at + Duration::seconds(1))
        .await
        .unwrap();
    let renamed = client
        .rename_task(
            first.id(),
            name("Renamed project"),
            at + Duration::seconds(2),
        )
        .await
        .unwrap();
    assert_eq!(renamed.name().as_str(), "Renamed project");
    assert_eq!(
        client.task(first.id()).unwrap().name().as_str(),
        "Renamed project"
    );

    let started = match client
        .set_active_task(first.id(), at + Duration::seconds(3))
        .await
        .unwrap()
    {
        SetActiveTaskOutcome::Started { worklog } => worklog,
        other => panic!("expected started worklog, got {other:?}"),
    };
    assert!(matches!(
        client
            .set_active_task(first.id(), at + Duration::seconds(4))
            .await
            .unwrap(),
        SetActiveTaskOutcome::AlreadyActive { .. }
    ));
    let switched = client
        .set_active_task(second.id(), at + Duration::seconds(10))
        .await
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
            .await
            .unwrap(),
        ClearActiveTaskOutcome::Stopped { .. }
    ));
    assert!(matches!(
        client.current_tracking(),
        tracker_domain::TrackingState::Idle
    ));

    let page = client.worklogs_for_task(first.id(), None).await.unwrap();
    assert_eq!(page.worklogs.len(), 1);
    assert_eq!(page.worklogs[0].id(), started.id());
    let all = client.all_worklogs(None).await.unwrap();
    assert!(
        all.worklogs
            .iter()
            .any(|worklog| worklog.id() == second_active.id())
    );
    let report = client
        .report_totals(at, at + Duration::seconds(30), at + Duration::seconds(30))
        .await
        .unwrap();
    assert_eq!(report.total, Duration::seconds(17));

    let individual = client.read_worklog(started.id()).await.unwrap();
    assert_eq!(client.cached_worklog(started.id()), Some(&individual));
    let original = page.worklogs[0].times();
    let invalid = client
        .correct_worklog(
            started.id(),
            original,
            WorklogTimes::new(at + Duration::seconds(12), Some(at + Duration::seconds(11))),
            at + Duration::seconds(21),
        )
        .await
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
        .await
        .unwrap();
    let moved = client
        .move_worklog(corrected.id(), first.id(), corrected.times(), second.id())
        .await
        .unwrap();
    assert_eq!(moved.task_id(), second.id());
    let deleted = client
        .delete_completed_worklog(moved.id(), second.id(), moved.times())
        .await
        .unwrap();
    assert_eq!(deleted.id(), started.id());
    assert!(
        client
            .worklogs_for_task(first.id(), None)
            .await
            .unwrap()
            .worklogs
            .is_empty()
    );

    client
        .archive_task(first.id(), at + Duration::seconds(22))
        .await
        .unwrap();
    assert!(client.task(first.id()).unwrap().is_archived());
    let archived = client
        .set_active_task(first.id(), at + Duration::seconds(23))
        .await
        .unwrap_err();
    assert_eq!(
        archived.failure().message(),
        format!("task {} is archived", first.id())
    );
    client
        .unarchive_task(first.id(), at + Duration::seconds(23))
        .await
        .unwrap();
    assert!(!client.task(first.id()).unwrap().is_archived());
    assert!(
        client
            .tasks(TaskOrdering::RecentlyWorked)
            .iter()
            .any(|item| item.task.id() == first.id())
    );
}

#[tokio::test]
async fn stale_client_refreshes_after_conflicting_write() {
    let server = TestServer::start();
    let mut first = connected(&server.endpoint()).await.unwrap();
    let at = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let task = first.create_task(name("Shared task"), at).await.unwrap();
    let mut second = connected(&server.endpoint()).await.unwrap();

    first
        .rename_task(
            task.id(),
            name("Updated by first"),
            at + Duration::seconds(1),
        )
        .await
        .unwrap();
    assert!(
        second
            .rename_task(
                task.id(),
                name("Updated by second"),
                at + Duration::seconds(2)
            )
            .await
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
        .await
        .unwrap();
    first.refresh().await.unwrap();
    assert_eq!(
        first.task(task.id()).unwrap().name().as_str(),
        "Updated by second"
    );
}

#[tokio::test]
async fn a_history_page_does_not_authorize_writes_from_a_stale_task_catalog() {
    let server = TestServer::start();
    let mut first = connected(&server.endpoint()).await.unwrap();
    let at = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let task = first.create_task(name("Shared task"), at).await.unwrap();
    let mut second = connected(&server.endpoint()).await.unwrap();

    first
        .rename_task(task.id(), name("New name"), at + Duration::seconds(1))
        .await
        .unwrap();
    second.worklogs_for_task(task.id(), None).await.unwrap();
    assert_eq!(
        second.task(task.id()).unwrap().name().as_str(),
        "Shared task"
    );
    assert!(
        second
            .rename_task(task.id(), name("Stale name"), at + Duration::seconds(2))
            .await
            .is_err()
    );
    assert_eq!(second.task(task.id()).unwrap().name().as_str(), "New name");
}

#[tokio::test]
async fn disconnected_client_keeps_last_confirmed_state_and_rejects_writes() {
    let server = TestServer::start();
    let mut client = connected(&server.endpoint()).await.unwrap();
    let at = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    client
        .create_task(name("Confirmed task"), at)
        .await
        .unwrap();
    let count = client.tasks(TaskOrdering::RecentlyCreated).len();
    assert_eq!(count, 1);
    drop(server);

    let error = client.refresh().await.unwrap_err();
    assert!(error.is_unavailable());
    assert_eq!(client.last_failure(), Some(RemoteFailureKind::Unavailable));
    assert_eq!(client.tasks(TaskOrdering::RecentlyCreated).len(), count);
    assert!(client.create_task(name("Offline task"), at).await.is_err());
    assert_eq!(client.tasks(TaskOrdering::RecentlyCreated).len(), count);
    let failure = client
        .report_totals(at, at + Duration::seconds(1), at)
        .await
        .unwrap_err()
        .failure();
    assert_eq!(
        failure.source(),
        tracker_application::ApplicationFailureSource::RemoteUnavailable
    );
    assert_eq!(failure.message(), "Tracker server is unavailable");
    assert!(!failure.recovery_failed());
}

#[tokio::test]
async fn task_history_rejects_other_scope_and_keeps_task_and_tracking_caches() {
    let server = TestServer::start();
    let mut first = connected(&server.endpoint()).await.unwrap();
    let at = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let requested = first.create_task(name("Requested"), at).await.unwrap();
    let active = first
        .create_task(name("Active"), at + Duration::seconds(1))
        .await
        .unwrap();
    let mut second = connected(&server.endpoint()).await.unwrap();
    let cursor = WorklogCursor {
        task_id: active.id(),
        start: at,
        id: WorklogId::generate(),
        revision: 0,
    };
    assert!(
        second
            .worklogs_for_task(requested.id(), Some(&cursor))
            .await
            .is_err()
    );

    let requested_at = at + Duration::seconds(2);
    let requested_worklog = match first
        .set_active_task(requested.id(), requested_at)
        .await
        .unwrap()
    {
        SetActiveTaskOutcome::Started { worklog } => worklog,
        other => panic!("expected started worklog, got {other:?}"),
    };
    first
        .clear_active_task(requested_worklog.id(), at + Duration::seconds(3))
        .await
        .unwrap();
    let started_at = at + Duration::seconds(10);
    first
        .set_active_task(active.id(), started_at)
        .await
        .unwrap();
    let page = second
        .worklogs_for_task(requested.id(), None)
        .await
        .unwrap();
    assert_eq!(page.worklogs.len(), 1);
    assert_eq!(page.snapshot.requested_task_latest_work_start, None);
    assert_eq!(page.snapshot.active_task_latest_work_start, None);
    assert!(matches!(
        second.current_tracking(),
        tracker_domain::TrackingState::Idle
    ));
    assert!(
        second
            .tasks(TaskOrdering::RecentlyWorked)
            .iter()
            .all(|item| item.latest_work_start.is_none())
    );
    second.refresh().await.unwrap();
    assert_eq!(
        second
            .tasks(TaskOrdering::RecentlyWorked)
            .iter()
            .find(|item| item.task.id() == active.id())
            .unwrap()
            .latest_work_start,
        Some(started_at)
    );
    assert_eq!(
        second
            .tasks(TaskOrdering::RecentlyWorked)
            .iter()
            .find(|item| item.task.id() == requested.id())
            .unwrap()
            .latest_work_start,
        Some(requested_at)
    );
    let current_page = second
        .worklogs_for_task(requested.id(), None)
        .await
        .unwrap();
    assert_eq!(
        current_page.snapshot.requested_task_latest_work_start,
        Some(requested_at)
    );
    assert_eq!(
        current_page.snapshot.active_task_latest_work_start,
        Some(started_at)
    );
}

#[tokio::test]
async fn task_history_returns_a_cursor_for_a_full_page() {
    let server = TestServer::start();
    let mut client = connected(&server.endpoint()).await.unwrap();
    let at = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let task = client.create_task(name("Paged history"), at).await.unwrap();
    for index in 0..51 {
        let start = at + Duration::seconds(2 * index + 1);
        let worklog = match client.set_active_task(task.id(), start).await.unwrap() {
            SetActiveTaskOutcome::Started { worklog } => worklog,
            other => panic!("expected started worklog, got {other:?}"),
        };
        client
            .clear_active_task(worklog.id(), start + Duration::seconds(1))
            .await
            .unwrap();
    }
    let first = client.worklogs_for_task(task.id(), None).await.unwrap();
    assert_eq!(first.worklogs.len(), 50);
    let cursor = first.next_cursor.expect("more worklogs remain");
    assert_eq!(cursor.task_id, task.id());
    let second = client
        .worklogs_for_task(task.id(), Some(&cursor))
        .await
        .unwrap();
    assert_eq!(second.worklogs.len(), 1);
    assert_ne!(first.worklogs[49].id(), second.worklogs[0].id());
}

#[tokio::test]
async fn global_history_keeps_task_cache_and_reports_exclude_the_end_instant() {
    let server = TestServer::start();
    let mut first = connected(&server.endpoint()).await.unwrap();
    let mut second = connected(&server.endpoint()).await.unwrap();
    let at = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let task = first.create_task(name("Shared report"), at).await.unwrap();
    let started = match first
        .set_active_task(task.id(), at + Duration::seconds(10))
        .await
        .unwrap()
    {
        SetActiveTaskOutcome::Started { worklog } => worklog,
        other => panic!("expected started worklog, got {other:?}"),
    };
    first
        .clear_active_task(started.id(), at + Duration::seconds(20))
        .await
        .unwrap();

    let feed = second.all_worklogs(None).await.unwrap();
    assert_eq!(feed.worklogs[0].id(), started.id());
    assert!(feed.snapshot.task_items.is_empty());
    assert!(second.task(task.id()).is_none());
    assert_eq!(
        second
            .report_totals(
                at + Duration::seconds(15),
                at + Duration::seconds(20),
                at + Duration::seconds(30)
            )
            .await
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
            .await
            .unwrap()
            .total,
        Duration::zero()
    );
}

#[tokio::test]
async fn connection_checks_health_without_initializing_resources() {
    let server = StubServer::with_health(HealthDto {
        status: "ok".into(),
        protocol_version: VERSION,
    });
    let client = RemoteApplication::connect(&server.endpoint())
        .await
        .unwrap();
    assert!(client.snapshot().task_items.is_empty());
    assert!(client.task_revision().is_empty());
    assert!(client.tracking_revision().is_empty());
    assert_eq!(client.last_failure(), None);
    assert!(!client.last_write_attempted());
}

#[tokio::test]
async fn report_and_history_reads_never_authorize_stale_task_or_tracking_intent() {
    let server = TestServer::start();
    let mut first = connected(&server.endpoint()).await.unwrap();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let task = first.create_task(name("Reviewed task"), at).await.unwrap();
    let mut second = connected(&server.endpoint()).await.unwrap();
    let tasks_revision = second.task_revision().to_owned();
    let tracking_revision = second.tracking_revision().to_owned();
    first
        .rename_task(
            task.id(),
            name("Changed by other client"),
            at + Duration::seconds(1),
        )
        .await
        .unwrap();
    second
        .task_totals(at, at + Duration::seconds(10), at)
        .await
        .unwrap();
    second.all_worklogs(None).await.unwrap();
    assert_eq!(second.task_revision(), tasks_revision);
    assert_eq!(second.tracking_revision(), tracking_revision);
    assert_ne!(second.report_revision(), tasks_revision);
    assert_eq!(
        second.cached_task_totals().unwrap().revision,
        second.report_revision()
    );
    assert!(second.cached_history(None).is_some());
    assert!(
        second
            .rename_task(task.id(), name("Reviewed draft"), at + Duration::seconds(2))
            .await
            .is_err()
    );
    assert_eq!(
        second.task(task.id()).unwrap().name().as_str(),
        "Changed by other client"
    );
    second.refresh().await.unwrap();
    first
        .set_active_task(task.id(), at + Duration::seconds(3))
        .await
        .unwrap();
    assert!(
        second
            .set_active_task(task.id(), at + Duration::seconds(4))
            .await
            .is_err()
    );
    assert!(matches!(
        second.current_tracking(),
        tracker_domain::TrackingState::Running { .. }
    ));
}

#[tokio::test]
async fn concurrent_restart_between_stop_and_archive_is_rejected() {
    let server = TestServer::start();
    let mut first = connected(&server.endpoint()).await.unwrap();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let task = first.create_task(name("Timer task"), at).await.unwrap();
    let worklog = match first
        .set_active_task(task.id(), at + Duration::seconds(1))
        .await
        .unwrap()
    {
        SetActiveTaskOutcome::Started { worklog } => worklog,
        _ => panic!("expected start"),
    };
    first
        .clear_active_task(worklog.id(), at + Duration::seconds(2))
        .await
        .unwrap();
    let mut second = connected(&server.endpoint()).await.unwrap();
    second
        .set_active_task(task.id(), at + Duration::seconds(3))
        .await
        .unwrap();
    let error = first
        .archive_task(task.id(), at + Duration::seconds(4))
        .await
        .unwrap_err();
    assert_eq!(
        error.failure().category(),
        tracker_application::ApplicationFailureCategory::ActiveTask
    );
    assert!(!first.task(task.id()).unwrap().is_archived());
}

#[tokio::test]
async fn coherent_refresh_retries_once_and_retains_the_confirmed_view_during_writes() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tracker_protocol::{TasksDto, TrackingDto};
    let task_reads = Arc::new(AtomicUsize::new(0));
    let tracking_reads = Arc::new(AtomicUsize::new(0));
    let tasks_count = task_reads.clone();
    let tracking_count = tracking_reads.clone();
    let router = Router::new()
        .route(
            "/v1/health",
            get(|| async {
                Json(HealthDto {
                    status: "ok".into(),
                    protocol_version: VERSION,
                })
            }),
        )
        .route(
            "/v1/tasks",
            get(move || {
                let counter = tasks_count.clone();
                async move {
                    let i = counter.fetch_add(1, Ordering::SeqCst);
                    Json(TasksDto {
                        tasks: vec![],
                        revision: format!("tasks-{i}"),
                    })
                }
            }),
        )
        .route(
            "/v1/tracking",
            get(move || {
                let counter = tracking_count.clone();
                async move {
                    let i = counter.fetch_add(1, Ordering::SeqCst);
                    Json(TrackingDto {
                        active_worklog: None,
                        revision: format!("tracking-{i}"),
                    })
                }
            }),
        );
    let server = StubServer::from_router(router);
    let mut client = RemoteApplication::connect(&server.endpoint())
        .await
        .unwrap();
    assert!(client.refresh().await.is_err());
    assert_eq!(task_reads.load(Ordering::SeqCst), 2);
    assert_eq!(tracking_reads.load(Ordering::SeqCst), 2);
    assert!(client.task_revision().is_empty());
    assert!(client.tracking_revision().is_empty());
    assert_eq!(client.last_failure(), Some(RemoteFailureKind::Conflict));
}

#[tokio::test]
async fn coherent_refresh_rejects_missing_or_archived_active_tasks_without_adopting_resources() {
    use tracker_protocol::{TaskDto, TasksDto, TrackingDto, WorklogDto};
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let active_task_id = tracker_domain::TaskId::generate();
    for archived in [false, true] {
        let unrelated = TaskDto {
            id: tracker_domain::TaskId::generate().to_string(),
            name: "Unrelated task".into(),
            archived: false,
            created_at: at,
            updated_at: at,
            latest_work_start: None,
        };
        let mut tasks = vec![unrelated.clone()];
        if archived {
            tasks.push(TaskDto {
                id: active_task_id.to_string(),
                name: "Archived active task".into(),
                archived: true,
                ..unrelated
            });
        }
        let catalog = TasksDto {
            tasks,
            revision: "same-revision".into(),
        };
        let tracking = TrackingDto {
            active_worklog: Some(WorklogDto {
                id: WorklogId::generate().to_string(),
                task_id: active_task_id.to_string(),
                start: at,
                end: None,
            }),
            revision: "same-revision".into(),
        };
        let router = Router::new()
            .route(
                "/v1/health",
                get(|| async {
                    Json(HealthDto {
                        status: "ok".into(),
                        protocol_version: VERSION,
                    })
                }),
            )
            .route(
                "/v1/tasks",
                get(move || {
                    let catalog = catalog.clone();
                    async move { Json(catalog) }
                }),
            )
            .route(
                "/v1/tracking",
                get(move || {
                    let tracking = tracking.clone();
                    async move { Json(tracking) }
                }),
            );
        let server = StubServer::from_router(router);
        let mut client = RemoteApplication::connect(&server.endpoint())
            .await
            .unwrap();
        assert!(matches!(
            client.refresh().await,
            Err(tracker_remote::RemoteError::Protocol(_))
        ));
        assert_eq!(client.last_failure(), Some(RemoteFailureKind::Protocol));
        assert!(client.snapshot().task_items.is_empty());
        assert!(client.snapshot().active_worklog.is_none());
        assert!(client.task_revision().is_empty());
        assert!(client.tracking_revision().is_empty());
    }
}

#[tokio::test]
async fn candidate_metadata_keeps_original_preview_revision() {
    let server = TestServer::start();
    let mut client = connected(&server.endpoint()).await.unwrap();
    let as_of = Utc::now();
    client
        .create_task(name("Old task"), as_of - Duration::days(20))
        .await
        .unwrap();
    let preview = client.preview_inactive_tasks(as_of).await.unwrap();
    let mut other = connected(&server.endpoint()).await.unwrap();
    other
        .create_task(name("Intervening write"), as_of)
        .await
        .unwrap();
    assert!(client.resolve_preview_tasks(&preview).await.is_err());
    assert!(client.archive_inactive_tasks(&preview).await.is_err());
}

#[tokio::test]
async fn replayed_receipt_returns_original_result_without_overwriting_a_newer_read() {
    use std::sync::{Arc, Mutex};
    use tracker_protocol::{CreateTaskRequest, MutationDto, MutationResultDto, TaskDto, TasksDto};
    let current = Arc::new(Mutex::new(Vec::<TaskDto>::new()));
    let for_get = current.clone();
    let for_post = current.clone();
    let router = Router::new()
        .route(
            "/v1/health",
            get(|| async {
                Json(HealthDto {
                    status: "ok".into(),
                    protocol_version: VERSION,
                })
            }),
        )
        .route(
            "/v1/tasks",
            get(move || {
                let current = for_get.clone();
                async move {
                    Json(TasksDto {
                        tasks: current.lock().unwrap().clone(),
                        revision: "newer-read".into(),
                    })
                }
            })
            .post(move |Json(body): Json<CreateTaskRequest>| {
                let current = for_post.clone();
                async move {
                    let task = TaskDto {
                        id: body.task_id,
                        name: body.name,
                        archived: false,
                        created_at: body.occurred_at,
                        updated_at: body.occurred_at,
                        latest_work_start: None,
                    };
                    *current.lock().unwrap() = vec![TaskDto {
                        name: "Renamed after original command".into(),
                        updated_at: task.updated_at + Duration::seconds(1),
                        ..task.clone()
                    }];
                    Json(MutationDto {
                        request_id: body.guard.request_id,
                        applied_revision: "original-receipt".into(),
                        replayed: true,
                        result: MutationResultDto::Task(task),
                    })
                }
            }),
        );
    let server = StubServer::from_router(router);
    let mut client = RemoteApplication::connect(&server.endpoint())
        .await
        .unwrap();
    let task = client
        .create_task(name("Original command result"), Utc::now())
        .await
        .unwrap();
    assert_eq!(task.name().as_str(), "Original command result");
    assert_eq!(
        client.task(task.id()).unwrap().name().as_str(),
        "Renamed after original command"
    );
    assert_eq!(client.task_revision(), "newer-read");
}

#[tokio::test]
async fn possibly_committed_creation_rereads_tasks_and_recovers_the_original_identity() {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    use tracker_protocol::{CreateTaskRequest, TaskDto, TasksDto};
    let current = Arc::new(Mutex::new(Vec::<TaskDto>::new()));
    let for_get = current.clone();
    let for_post = current.clone();
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let router = Router::new()
        .route(
            "/v1/health",
            get(|| async {
                Json(HealthDto {
                    status: "ok".into(),
                    protocol_version: VERSION,
                })
            }),
        )
        .route(
            "/v1/tasks",
            get(move || {
                let current = for_get.clone();
                async move {
                    Json(TasksDto {
                        tasks: current.lock().unwrap().clone(),
                        revision: "confirmed".into(),
                    })
                }
            })
            .post(move |Json(body): Json<CreateTaskRequest>| {
                let current = for_post.clone();
                let count = count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    *current.lock().unwrap() = vec![TaskDto {
                        id: body.task_id,
                        name: body.name,
                        archived: false,
                        created_at: body.occurred_at,
                        updated_at: body.occurred_at,
                        latest_work_start: None,
                    }];
                    "invalid receipt after commit"
                }
            }),
        );
    let server = StubServer::from_router(router);
    let mut client = RemoteApplication::connect(&server.endpoint())
        .await
        .unwrap();
    let clicked_at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let error = client
        .create_task(name("Uncertain create"), clicked_at)
        .await
        .unwrap_err();
    assert_eq!(
        error.failure().source(),
        tracker_application::ApplicationFailureSource::RemoteProtocol
    );
    assert!(client.last_write_attempted());
    let original = client.tasks(TaskOrdering::RecentlyCreated)[0].task.clone();
    let recovered = client
        .create_task(name("Uncertain create"), clicked_at + Duration::seconds(20))
        .await
        .unwrap();
    assert_eq!(recovered, original);
    assert_eq!(recovered.created_at(), clicked_at);
    assert_eq!(writes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn rename_preserves_the_original_reviewed_name_after_background_refresh() {
    let server = TestServer::start();
    let mut first = connected(&server.endpoint()).await.unwrap();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let task = first
        .create_task(name("Original reviewed name"), at)
        .await
        .unwrap();
    let mut second = connected(&server.endpoint()).await.unwrap();
    first
        .rename_task(
            task.id(),
            name("Renamed during editing"),
            at + Duration::seconds(1),
        )
        .await
        .unwrap();
    second.refresh_tasks().await.unwrap();
    assert!(
        second
            .rename_task_with_expected_name(
                task.id(),
                task.name(),
                name("Draft name"),
                at + Duration::seconds(2)
            )
            .await
            .is_err()
    );
    first.refresh_tasks().await.unwrap();
    assert_eq!(
        first.task(task.id()).unwrap().name().as_str(),
        "Renamed during editing"
    );
    second
        .rename_task_with_expected_name(
            task.id(),
            &name("Renamed during editing"),
            name("Reviewed again"),
            at + Duration::seconds(3),
        )
        .await
        .unwrap();
    first.refresh_tasks().await.unwrap();
    assert_eq!(
        first.task(task.id()).unwrap().name().as_str(),
        "Reviewed again"
    );
}

#[tokio::test]
async fn independent_task_totals_keep_valid_rows_without_metadata_or_tracking_reads() {
    use tracker_protocol::{ReportDto, ReportRowDto};
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let id = tracker_domain::TaskId::generate();
    let expected = ReportDto {
        start: at,
        end: at + Duration::seconds(10),
        now: at,
        rows: vec![ReportRowDto {
            task_id: id.to_string(),
            duration_us: 5,
        }],
        total_us: 5,
        revision: "report-observation".into(),
    };
    let returned = expected.clone();
    let router = Router::new()
        .route(
            "/v1/health",
            get(|| async {
                Json(HealthDto {
                    status: "ok".into(),
                    protocol_version: VERSION,
                })
            }),
        )
        .route(
            "/v1/reports/task-totals",
            get(move || {
                let response = returned.clone();
                async move { Json(response) }
            }),
        );
    let server = StubServer::from_router(router);
    let mut client = RemoteApplication::connect(&server.endpoint())
        .await
        .unwrap();
    let report = client
        .task_totals(at, at + Duration::seconds(10), at)
        .await
        .unwrap();
    assert_eq!(report, expected);
    assert_eq!(client.cached_task_totals(), Some(&expected));
    assert!(client.task(id).is_none());
    assert!(client.task_revision().is_empty());
    assert!(client.tracking_revision().is_empty());
    assert!(
        client
            .report_totals(at, at + Duration::seconds(10), at)
            .await
            .is_err()
    );
    assert_eq!(client.cached_task_totals(), Some(&expected));
}

fn with_json_resource(path: &str, response: serde_json::Value) -> StubServer {
    StubServer::from_router(
        Router::new()
            .route(
                "/v1/health",
                get(|| async {
                    Json(HealthDto {
                        status: "ok".into(),
                        protocol_version: VERSION,
                    })
                }),
            )
            .route(
                path,
                get(move || {
                    let response = response.clone();
                    async move { Json(response) }
                }),
            ),
    )
}

#[tokio::test]
async fn individual_resources_reject_wrong_ids_revisions_and_domain_values() {
    use tracker_protocol::{TaskDto, TaskResourceDto, WorklogDto, WorklogResourceDto};
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let task_id = tracker_domain::TaskId::generate();
    let worklog_id = WorklogId::generate();
    let task = TaskDto {
        id: task_id.to_string(),
        name: "Individual task".into(),
        archived: false,
        created_at: at,
        updated_at: at,
        latest_work_start: None,
    };
    for dto in [
        TaskResourceDto {
            task: TaskDto {
                id: tracker_domain::TaskId::generate().to_string(),
                ..task.clone()
            },
            revision: "r1".into(),
        },
        TaskResourceDto {
            task: task.clone(),
            revision: String::new(),
        },
        TaskResourceDto {
            task: TaskDto {
                name: "Bad\u{1b}name".into(),
                ..task
            },
            revision: "r1".into(),
        },
    ] {
        let server = with_json_resource(
            &format!("/v1/tasks/{task_id}"),
            serde_json::to_value(dto).unwrap(),
        );
        let mut client = RemoteApplication::connect(&server.endpoint())
            .await
            .unwrap();
        assert!(client.read_task(task_id).await.is_err());
        assert!(client.task(task_id).is_none());
        assert_eq!(client.last_failure(), Some(RemoteFailureKind::Protocol));
    }
    let worklog = WorklogDto {
        id: worklog_id.to_string(),
        task_id: task_id.to_string(),
        start: at,
        end: Some(at + Duration::seconds(10)),
    };
    for dto in [
        WorklogResourceDto {
            worklog: WorklogDto {
                id: WorklogId::generate().to_string(),
                ..worklog.clone()
            },
            revision: "r1".into(),
        },
        WorklogResourceDto {
            worklog: worklog.clone(),
            revision: String::new(),
        },
        WorklogResourceDto {
            worklog: WorklogDto {
                end: Some(at - Duration::seconds(1)),
                ..worklog
            },
            revision: "r1".into(),
        },
    ] {
        let server = with_json_resource(
            &format!("/v1/worklogs/{worklog_id}"),
            serde_json::to_value(dto).unwrap(),
        );
        let mut client = RemoteApplication::connect(&server.endpoint())
            .await
            .unwrap();
        assert!(client.read_worklog(worklog_id).await.is_err());
        assert!(client.cached_worklog(worklog_id).is_none());
        assert_eq!(client.last_failure(), Some(RemoteFailureKind::Protocol));
    }
}

#[tokio::test]
async fn report_echoes_and_preview_echoes_must_match_the_original_query() {
    use tracker_protocol::{InactiveTaskPreviewDto, ReportDto};
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let valid = ReportDto {
        start: at,
        end: at + Duration::seconds(10),
        now: at,
        rows: vec![],
        total_us: 0,
        revision: "r1".into(),
    };
    for dto in [
        ReportDto {
            start: at - Duration::seconds(1),
            ..valid.clone()
        },
        ReportDto {
            end: at + Duration::seconds(11),
            ..valid.clone()
        },
        ReportDto {
            now: at + Duration::seconds(1),
            ..valid
        },
    ] {
        let server = with_json_resource(
            "/v1/reports/task-totals",
            serde_json::to_value(dto).unwrap(),
        );
        let mut client = RemoteApplication::connect(&server.endpoint())
            .await
            .unwrap();
        assert!(
            client
                .task_totals(at, at + Duration::seconds(10), at)
                .await
                .is_err()
        );
        assert!(client.cached_task_totals().is_none());
    }
    let valid = InactiveTaskPreviewDto {
        as_of: at,
        inactive_days: 7,
        count: 0,
        candidate_task_ids: vec![],
        revision: "r1".into(),
    };
    for dto in [
        InactiveTaskPreviewDto {
            as_of: at + Duration::seconds(1),
            ..valid.clone()
        },
        InactiveTaskPreviewDto {
            inactive_days: 14,
            ..valid
        },
    ] {
        let server = with_json_resource(
            "/v1/tasks/inactive-preview",
            serde_json::to_value(dto).unwrap(),
        );
        let mut client = RemoteApplication::connect(&server.endpoint())
            .await
            .unwrap();
        assert!(
            client
                .preview_inactive_tasks_with_period(
                    at,
                    tracker_domain::InactivityPeriod::new(7).unwrap()
                )
                .await
                .is_err()
        );
        assert_eq!(client.last_failure(), Some(RemoteFailureKind::Protocol));
    }
}

#[tokio::test]
async fn uncertain_uncommitted_creation_preserves_id_and_timestamp_under_a_new_guard() {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    use tracker_protocol::{CreateTaskRequest, MutationDto, MutationResultDto, TaskDto, TasksDto};
    for advance_revision in [false, true] {
        let bodies = Arc::new(Mutex::new(Vec::<CreateTaskRequest>::new()));
        let received = bodies.clone();
        let current = Arc::new(Mutex::new(Vec::<TaskDto>::new()));
        let for_get = current.clone();
        let for_post = current.clone();
        let phase = Arc::new(AtomicUsize::new(0));
        let get_phase = phase.clone();
        let post_phase = phase.clone();
        let router = Router::new()
            .route(
                "/v1/health",
                get(|| async {
                    Json(HealthDto {
                        status: "ok".into(),
                        protocol_version: VERSION,
                    })
                }),
            )
            .route(
                "/v1/tasks",
                get(move || {
                    let current = for_get.clone();
                    let phase = get_phase.clone();
                    async move {
                        Json(TasksDto {
                            tasks: current.lock().unwrap().clone(),
                            revision: format!(
                                "epoch-{}",
                                if advance_revision || phase.load(Ordering::SeqCst) > 1 {
                                    phase.load(Ordering::SeqCst)
                                } else {
                                    0
                                }
                            ),
                        })
                    }
                })
                .post(move |Json(body): Json<CreateTaskRequest>| {
                    let current = for_post.clone();
                    let phase = post_phase.clone();
                    let bodies = received.clone();
                    async move {
                        use axum::response::IntoResponse;
                        bodies.lock().unwrap().push(body.clone());
                        if phase.fetch_add(1, Ordering::SeqCst) == 0 {
                            return "lost uncommitted receipt".into_response();
                        }
                        let task = TaskDto {
                            id: body.task_id,
                            name: body.name,
                            archived: false,
                            created_at: body.occurred_at,
                            updated_at: body.occurred_at,
                            latest_work_start: None,
                        };
                        *current.lock().unwrap() = vec![task.clone()];
                        Json(MutationDto {
                            request_id: body.guard.request_id,
                            applied_revision: "epoch-2".into(),
                            replayed: false,
                            result: MutationResultDto::Task(task),
                        })
                        .into_response()
                    }
                }),
            );
        let server = StubServer::from_router(router);
        let mut client = RemoteApplication::connect(&server.endpoint())
            .await
            .unwrap();
        let clicked_at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
        assert!(
            client
                .create_task(name("Original draft"), clicked_at)
                .await
                .is_err()
        );
        assert!(
            client
                .create_task(name("Different draft"), clicked_at + Duration::seconds(1))
                .await
                .is_err()
        );
        let task = client
            .create_task(name("Original draft"), clicked_at + Duration::seconds(2))
            .await
            .unwrap();
        let bodies = bodies.lock().unwrap();
        assert_eq!(bodies.len(), 2);
        assert_eq!(bodies[0].task_id, bodies[1].task_id);
        assert_eq!(bodies[0].occurred_at, bodies[1].occurred_at);
        assert_eq!(bodies[0].name, bodies[1].name);
        if advance_revision {
            assert_ne!(
                bodies[0].guard.expected_revision,
                bodies[1].guard.expected_revision
            );
            assert_ne!(bodies[0].guard.request_id, bodies[1].guard.request_id);
        } else {
            assert_eq!(bodies[0], bodies[1]);
        }
        assert_eq!(task.created_at(), clicked_at);
    }
}

#[tokio::test]
async fn queued_switch_preserves_the_active_worklog_seen_at_click_time() {
    let server = TestServer::start();
    let mut first = connected(&server.endpoint()).await.unwrap();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let source = first.create_task(name("Original timer"), at).await.unwrap();
    let destination = first
        .create_task(name("Selected destination"), at)
        .await
        .unwrap();
    let concurrent = first
        .create_task(name("Concurrent timer"), at)
        .await
        .unwrap();
    let reviewed = match first
        .set_active_task(source.id(), at + Duration::seconds(1))
        .await
        .unwrap()
    {
        SetActiveTaskOutcome::Started { worklog } => worklog.id(),
        _ => panic!("expected start"),
    };
    let mut second = connected(&server.endpoint()).await.unwrap();
    first
        .set_active_task(concurrent.id(), at + Duration::seconds(2))
        .await
        .unwrap();
    second.refresh().await.unwrap();
    assert!(
        second
            .set_active_task_with_expected_active(
                destination.id(),
                Some(reviewed),
                at + Duration::seconds(3)
            )
            .await
            .is_err()
    );
    first.refresh_tracking().await.unwrap();
    assert!(
        matches!(first.current_tracking(),tracker_domain::TrackingState::Running {worklog} if worklog.task_id()==concurrent.id())
    );
    let current = first.snapshot().active_worklog.as_ref().unwrap().id();
    second
        .set_active_task_with_expected_active(
            destination.id(),
            Some(current),
            at + Duration::seconds(4),
        )
        .await
        .unwrap();
    first.refresh_tracking().await.unwrap();
    assert!(
        matches!(first.current_tracking(),tracker_domain::TrackingState::Running {worklog} if worklog.task_id()==destination.id())
    );
}

#[tokio::test]
async fn archive_confirmation_preserves_the_name_reviewed_before_polling() {
    let server = TestServer::start();
    let mut first = connected(&server.endpoint()).await.unwrap();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let task = first
        .create_task(name("Reviewed archive task"), at)
        .await
        .unwrap();
    let mut second = connected(&server.endpoint()).await.unwrap();
    first
        .rename_task(
            task.id(),
            name("Renamed archive task"),
            at + Duration::seconds(1),
        )
        .await
        .unwrap();
    second.refresh_tasks().await.unwrap();
    assert!(
        second
            .archive_task_with_expected_name(task.id(), task.name(), at + Duration::seconds(2))
            .await
            .is_err()
    );
    first.refresh_tasks().await.unwrap();
    assert!(!first.task(task.id()).unwrap().is_archived());
    second
        .archive_task_with_expected_name(
            task.id(),
            &name("Renamed archive task"),
            at + Duration::seconds(3),
        )
        .await
        .unwrap();
    first.refresh_tasks().await.unwrap();
    assert!(first.task(task.id()).unwrap().is_archived());
}

#[tokio::test]
async fn individual_task_reads_keep_the_catalog_revision_and_catalog_data_separate() {
    let server = TestServer::start();
    let mut first = connected(&server.endpoint()).await.unwrap();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let task = first
        .create_task(name("Catalog observation"), at)
        .await
        .unwrap();
    let mut second = connected(&server.endpoint()).await.unwrap();
    let catalog_revision = second.task_revision().to_owned();
    first
        .rename_task(
            task.id(),
            name("Individual observation"),
            at + Duration::seconds(1),
        )
        .await
        .unwrap();
    let dto = second.read_task(task.id()).await.unwrap();
    assert_ne!(dto.revision, catalog_revision);
    assert_eq!(second.task_revision(), catalog_revision);
    assert_eq!(
        second.cached_task_revision(task.id()),
        Some(dto.revision.as_str())
    );
    assert_eq!(
        second.tasks(TaskOrdering::RecentlyCreated)[0]
            .task
            .name()
            .as_str(),
        "Catalog observation"
    );
    assert_eq!(
        second.task(task.id()).unwrap().name().as_str(),
        "Individual observation"
    );
    first
        .rename_task(task.id(), name("Newest catalog"), at + Duration::seconds(2))
        .await
        .unwrap();
    second.refresh_tasks().await.unwrap();
    assert!(second.cached_task_revision(task.id()).is_none());
    assert_eq!(
        second.task(task.id()).unwrap().name().as_str(),
        "Newest catalog"
    );
    assert_eq!(
        second.tasks(TaskOrdering::RecentlyCreated)[0]
            .task
            .name()
            .as_str(),
        "Newest catalog"
    );
}

#[tokio::test]
async fn source_changed_preflight_is_certain_and_recovers_affected_caches() {
    let server = TestServer::start();
    let mut first = connected(&server.endpoint()).await.unwrap();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let source = first.create_task(name("Source history"), at).await.unwrap();
    let timer = first.create_task(name("Other timer"), at).await.unwrap();
    let original = match first
        .set_active_task(source.id(), at + Duration::seconds(1))
        .await
        .unwrap()
    {
        SetActiveTaskOutcome::Started { worklog } => worklog,
        _ => panic!("expected start"),
    };
    first
        .clear_active_task(original.id(), at + Duration::seconds(5))
        .await
        .unwrap();
    let mut second = connected(&server.endpoint()).await.unwrap();
    let reviewed = second
        .worklogs_for_task(source.id(), None)
        .await
        .unwrap()
        .worklogs[0]
        .clone();
    first
        .correct_worklog(
            reviewed.id(),
            reviewed.times(),
            WorklogTimes::new(at + Duration::seconds(2), Some(at + Duration::seconds(5))),
            at + Duration::seconds(6),
        )
        .await
        .unwrap();
    first
        .set_active_task(timer.id(), at + Duration::seconds(7))
        .await
        .unwrap();
    let error = second
        .correct_worklog(
            reviewed.id(),
            reviewed.times(),
            WorklogTimes::new(at + Duration::seconds(3), Some(at + Duration::seconds(5))),
            at + Duration::seconds(8),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.failure().category(),
        tracker_application::ApplicationFailureCategory::WorklogChanged
    );
    assert!(!error.failure().recovery_failed());
    assert!(!second.last_write_attempted());
    assert!(
        matches!(second.current_tracking(),tracker_domain::TrackingState::Running {worklog} if worklog.task_id()==timer.id())
    );
    assert_eq!(
        second.cached_worklog(reviewed.id()).unwrap().worklog.start,
        at + Duration::seconds(2)
    );
}

fn command_receipt_server(
    tasks: Vec<tracker_protocol::TaskDto>,
    active: Option<tracker_protocol::WorklogDto>,
    worklog: tracker_protocol::WorklogDto,
    result: impl Fn(serde_json::Value) -> serde_json::Value + Send + Sync + 'static,
) -> StubServer {
    use axum::extract::Path;
    use std::sync::Arc;
    use tracker_protocol::{TaskResourceDto, TasksDto, TrackingDto, WorklogResourceDto};
    let result = Arc::new(result);
    let write = move |Json(body): Json<serde_json::Value>| {
        let result = result.clone();
        async move {
            Json(serde_json::json!({
                "request_id": body["request_id"],
                "applied_revision": "receipt-revision",
                "replayed": false,
                "result": result(body),
            }))
        }
    };
    let individual_tasks = tasks.clone();
    let router = Router::new()
        .route(
            "/v1/health",
            get(|| async {
                Json(HealthDto {
                    status: "ok".into(),
                    protocol_version: VERSION,
                })
            }),
        )
        .route(
            "/v1/tasks",
            get(move || {
                let tasks = tasks.clone();
                async move {
                    Json(TasksDto {
                        tasks,
                        revision: "read-revision".into(),
                    })
                }
            })
            .post(write.clone()),
        )
        .route(
            "/v1/tasks/{id}",
            get(move |Path(id): Path<String>| {
                let task = individual_tasks
                    .iter()
                    .find(|task| task.id == id)
                    .unwrap()
                    .clone();
                async move {
                    Json(TaskResourceDto {
                        task,
                        revision: "read-revision".into(),
                    })
                }
            })
            .patch(write.clone()),
        )
        .route(
            "/v1/tracking",
            get(move || {
                let active_worklog = active.clone();
                async move {
                    Json(TrackingDto {
                        active_worklog,
                        revision: "read-revision".into(),
                    })
                }
            })
            .put(write.clone()),
        )
        .route(
            "/v1/worklogs/{id}",
            get(move || {
                let worklog = worklog.clone();
                async move {
                    Json(WorklogResourceDto {
                        worklog,
                        revision: "read-revision".into(),
                    })
                }
            })
            .patch(write.clone())
            .delete(write),
        );
    StubServer::from_router(router)
}

#[tokio::test]
async fn command_receipts_reject_individual_changes_to_reviewed_result_fields() {
    use serde_json::json;
    use tracker_protocol::{TaskDto, WorklogDto};
    #[derive(Clone, Copy, Debug)]
    enum Command {
        Create,
        Rename,
        Archive,
        Restore,
        Start,
        Switch,
        AlreadyActive,
        Stop,
        Move,
        Correct,
        Delete,
    }
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let source_id = tracker_domain::TaskId::generate();
    let target_id = tracker_domain::TaskId::generate();
    let worklog_id = WorklogId::generate();
    let other_id = tracker_domain::TaskId::generate().to_string();
    let commands = [
        (
            Command::Create,
            vec!["id", "name", "archived", "created_at", "updated_at"],
        ),
        (Command::Rename, vec!["id", "name"]),
        (Command::Archive, vec!["id", "archived"]),
        (Command::Restore, vec!["id", "archived"]),
        (Command::Start, vec!["id", "task_id", "start", "end"]),
        (
            Command::Switch,
            vec![
                "stopped.id",
                "stopped.task_id",
                "stopped.start",
                "stopped.end",
                "started.id",
                "started.task_id",
                "started.start",
                "started.end",
            ],
        ),
        (
            Command::AlreadyActive,
            vec!["id", "task_id", "start", "end"],
        ),
        (Command::Stop, vec!["id", "task_id", "start", "end"]),
        (Command::Move, vec!["id", "task_id", "start", "end"]),
        (Command::Correct, vec!["id", "task_id", "start", "end"]),
        (Command::Delete, vec!["id", "task_id", "start", "end"]),
    ];
    for (command, fields) in commands {
        for field in fields {
            let source = TaskDto {
                id: source_id.to_string(),
                name: "Source task".into(),
                archived: matches!(command, Command::Restore),
                created_at: at,
                updated_at: at,
                latest_work_start: None,
            };
            let target = TaskDto {
                id: target_id.to_string(),
                name: "Target task".into(),
                archived: false,
                ..source.clone()
            };
            let running = matches!(
                command,
                Command::Switch | Command::AlreadyActive | Command::Stop
            );
            let original = WorklogDto {
                id: worklog_id.to_string(),
                task_id: source_id.to_string(),
                start: at,
                end: (!running).then_some(at + Duration::seconds(10)),
            };
            let original_for_receipt = original.clone();
            let source_for_receipt = source.clone();
            let other_id = other_id.clone();
            let server = command_receipt_server(
                vec![source, target],
                running.then_some(original.clone()),
                original,
                move |body| {
                    let mut value = match command {
                        Command::Create => json!({
                            "id": body["task_id"], "name": body["name"], "archived": false,
                            "created_at": body["occurred_at"], "updated_at": body["occurred_at"], "latest_work_start": null,
                        }),
                        Command::Rename | Command::Archive | Command::Restore => {
                            let mut value = serde_json::to_value(&source_for_receipt).unwrap();
                            match command {
                                Command::Rename => value["name"] = body["name"].clone(),
                                Command::Archive => value["archived"] = json!(true),
                                Command::Restore => value["archived"] = json!(false),
                                _ => unreachable!(),
                            }
                            value
                        }
                        Command::Start => {
                            json!({"id":body["worklog_id"], "task_id":body["task_id"], "start":body["occurred_at"], "end":null})
                        }
                        Command::Switch => {
                            let mut stopped = serde_json::to_value(&original_for_receipt).unwrap();
                            stopped["end"] = body["occurred_at"].clone();
                            json!({"stopped":stopped,"started":{"id":body["worklog_id"],"task_id":body["task_id"],"start":body["occurred_at"],"end":null}})
                        }
                        Command::AlreadyActive | Command::Delete => {
                            serde_json::to_value(&original_for_receipt).unwrap()
                        }
                        Command::Stop => {
                            let mut value = serde_json::to_value(&original_for_receipt).unwrap();
                            value["end"] = body["occurred_at"].clone();
                            value
                        }
                        Command::Move => {
                            let mut value = serde_json::to_value(&original_for_receipt).unwrap();
                            value["task_id"] = body["destination_task_id"].clone();
                            value
                        }
                        Command::Correct => {
                            let mut value = serde_json::to_value(&original_for_receipt).unwrap();
                            value["start"] = body["replacement_start"].clone();
                            value["end"] = body["replacement_end"].clone();
                            value
                        }
                    };
                    let mut parts = field.split('.');
                    let first = parts.next().unwrap();
                    let altered = if let Some(second) = parts.next() {
                        &mut value[first][second]
                    } else {
                        &mut value[first]
                    };
                    *altered = match field.rsplit('.').next().unwrap() {
                        "id" | "task_id" => json!(other_id),
                        "name" => json!("Contradictory name"),
                        "archived" => json!(!altered.as_bool().unwrap()),
                        "created_at" => json!(at - Duration::seconds(1)),
                        "updated_at" => json!(at + Duration::seconds(21)),
                        "start" => {
                            let old: DateTime<Utc> =
                                serde_json::from_value(altered.clone()).unwrap();
                            json!(old + Duration::seconds(1))
                        }
                        "end" => {
                            if altered.is_null() {
                                json!(at + Duration::seconds(21))
                            } else {
                                let old: DateTime<Utc> =
                                    serde_json::from_value(altered.clone()).unwrap();
                                json!(old + Duration::seconds(1))
                            }
                        }
                        _ => unreachable!(),
                    };
                    let kind = match command {
                        Command::Create | Command::Rename | Command::Archive | Command::Restore => {
                            "task"
                        }
                        Command::Switch => "tracking_switched",
                        Command::AlreadyActive => "tracking_already_active",
                        _ => "worklog",
                    };
                    json!({"kind":kind,"value":value})
                },
            );
            let mut client = connected(&server.endpoint()).await.unwrap();
            let expected = WorklogTimes::new(at, Some(at + Duration::seconds(10)));
            let write_at = at + Duration::seconds(20);
            let error = match command {
                Command::Create => client
                    .create_task(name("Created task"), write_at)
                    .await
                    .map(|_| ()),
                Command::Rename => client
                    .rename_task(source_id, name("Renamed task"), write_at)
                    .await
                    .map(|_| ()),
                Command::Archive => client.archive_task(source_id, write_at).await.map(|_| ()),
                Command::Restore => client.unarchive_task(source_id, write_at).await.map(|_| ()),
                Command::Start | Command::Switch => client
                    .set_active_task(target_id, write_at)
                    .await
                    .map(|_| ()),
                Command::AlreadyActive => client
                    .set_active_task(source_id, write_at)
                    .await
                    .map(|_| ()),
                Command::Stop => client
                    .clear_active_task(worklog_id, write_at)
                    .await
                    .map(|_| ()),
                Command::Move => client
                    .move_worklog(worklog_id, source_id, expected, target_id)
                    .await
                    .map(|_| ()),
                Command::Correct => client
                    .correct_worklog(
                        worklog_id,
                        expected,
                        WorklogTimes::new(
                            at + Duration::seconds(1),
                            Some(at + Duration::seconds(9)),
                        ),
                        write_at,
                    )
                    .await
                    .map(|_| ()),
                Command::Delete => client
                    .delete_completed_worklog(worklog_id, source_id, expected)
                    .await
                    .map(|_| ()),
            }
            .expect_err(&format!("{command:?} receipt changed {field}"));
            assert!(client.last_write_attempted(), "{command:?} {field}");
            assert_eq!(
                error.failure().source(),
                tracker_application::ApplicationFailureSource::RemoteProtocol,
                "{command:?} {field}"
            );
            assert_eq!(client.task_revision(), "read-revision");
            assert_eq!(client.tracking_revision(), "read-revision");
        }
    }
}

#[tokio::test]
async fn raced_worklog_recovery_retains_the_last_coherent_view() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use tracker_protocol::{
        MutationDto, MutationResultDto, TaskDto, TasksDto, TrackingDto, WorklogChangeRequest,
        WorklogDto, WorklogResourceDto,
    };
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let task_id = tracker_domain::TaskId::generate();
    let id = WorklogId::generate();
    let committed = Arc::new(AtomicBool::new(false));
    let tasks_committed = committed.clone();
    let tracking_committed = committed.clone();
    let worklog_committed = committed.clone();
    let command_committed = committed.clone();
    let original = WorklogDto {
        id: id.to_string(),
        task_id: task_id.to_string(),
        start: at,
        end: Some(at + Duration::seconds(10)),
    };
    let for_get = original.clone();
    let for_patch = original.clone();
    let router = Router::new()
        .route(
            "/v1/health",
            get(|| async {
                Json(HealthDto {
                    status: "ok".into(),
                    protocol_version: VERSION,
                })
            }),
        )
        .route(
            "/v1/tasks",
            get(move || {
                let committed = tasks_committed.clone();
                async move {
                    let changed = committed.load(Ordering::SeqCst);
                    Json(TasksDto {
                        tasks: vec![TaskDto {
                            id: task_id.to_string(),
                            name: if changed {
                                "Later task name"
                            } else {
                                "Confirmed task name"
                            }
                            .into(),
                            archived: false,
                            created_at: at,
                            updated_at: at,
                            latest_work_start: None,
                        }],
                        revision: if changed { "racing-tasks" } else { "confirmed" }.into(),
                    })
                }
            }),
        )
        .route(
            "/v1/tracking",
            get(move || {
                let committed = tracking_committed.clone();
                async move {
                    Json(TrackingDto {
                        active_worklog: None,
                        revision: if committed.load(Ordering::SeqCst) {
                            "racing-tracking"
                        } else {
                            "confirmed"
                        }
                        .into(),
                    })
                }
            }),
        )
        .route(
            "/v1/worklogs/{id}",
            get(move || {
                let committed = worklog_committed.clone();
                let mut worklog = for_get.clone();
                async move {
                    let changed = committed.load(Ordering::SeqCst);
                    if changed {
                        worklog.start = at + Duration::seconds(1);
                    }
                    Json(WorklogResourceDto {
                        worklog,
                        revision: if changed {
                            "committed-worklog"
                        } else {
                            "confirmed"
                        }
                        .into(),
                    })
                }
            })
            .patch(move |Json(body): Json<WorklogChangeRequest>| {
                let committed = command_committed.clone();
                let mut worklog = for_patch.clone();
                async move {
                    let WorklogChangeRequest::Correct {
                        replacement_start,
                        replacement_end,
                        guard,
                        ..
                    } = body
                    else {
                        panic!("expected correction")
                    };
                    worklog.start = replacement_start;
                    worklog.end = replacement_end;
                    committed.store(true, Ordering::SeqCst);
                    Json(MutationDto {
                        request_id: guard.request_id,
                        applied_revision: "committed-worklog".into(),
                        replayed: false,
                        result: MutationResultDto::Worklog(worklog),
                    })
                }
            }),
        );
    let server = StubServer::from_router(router);
    let mut client = connected(&server.endpoint()).await.unwrap();
    let error = client
        .correct_worklog(
            id,
            WorklogTimes::new(at, original.end),
            WorklogTimes::new(at + Duration::seconds(1), original.end),
            at + Duration::seconds(20),
        )
        .await
        .unwrap_err();
    assert!(committed.load(Ordering::SeqCst));
    assert!(client.last_write_attempted());
    assert_eq!(
        error.failure().source(),
        tracker_application::ApplicationFailureSource::Operation
    );
    assert_eq!(client.task_revision(), "confirmed");
    assert_eq!(client.tracking_revision(), "confirmed");
    assert_eq!(
        client.task(task_id).unwrap().name().as_str(),
        "Confirmed task name"
    );
    assert!(client.snapshot().active_worklog.is_none());
    assert_eq!(
        client.cached_worklog(id).unwrap().worklog.start,
        at + Duration::seconds(1)
    );
    assert_eq!(
        client.cached_worklog(id).unwrap().revision,
        "committed-worklog"
    );
}

#[tokio::test]
async fn history_rejects_empty_revision_and_cursor_scope_before_caching() {
    use tracker_protocol::{WorklogCursorDto, WorklogPageDto};
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let requested = tracker_domain::TaskId::generate();
    for (revision, cursor_task) in [
        ("", None),
        (
            "valid",
            Some(tracker_domain::TaskId::generate().to_string()),
        ),
        ("valid", None),
    ] {
        let next_cursor = if revision.is_empty() {
            None
        } else {
            Some(WorklogCursorDto {
                task_id: cursor_task,
                start: at,
                id: WorklogId::generate().to_string(),
                revision: 1,
            })
        };
        let dto = WorklogPageDto {
            worklogs: vec![],
            next_cursor,
            revision: revision.into(),
        };
        let server = with_json_resource("/v1/worklogs", serde_json::to_value(dto).unwrap());
        let mut client = RemoteApplication::connect(&server.endpoint())
            .await
            .unwrap();
        let error = client.worklogs_for_task(requested, None).await.unwrap_err();
        assert_eq!(
            error.failure().source(),
            tracker_application::ApplicationFailureSource::RemoteProtocol
        );
        assert!(client.cached_history(Some(requested)).is_none());
        assert!(client.task_revision().is_empty());
        assert!(client.tracking_revision().is_empty());
    }
}

#[tokio::test]
async fn malformed_history_retains_the_previous_confirmed_page() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tracker_protocol::{WorklogCursorDto, WorklogDto, WorklogPageDto};
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let requested = tracker_domain::TaskId::generate();
    let valid = WorklogPageDto {
        worklogs: vec![WorklogDto {
            id: WorklogId::generate().to_string(),
            task_id: requested.to_string(),
            start: at,
            end: Some(at + Duration::seconds(10)),
        }],
        next_cursor: None,
        revision: "confirmed-page".into(),
    };
    let mut invalid_id = valid.clone();
    invalid_id.worklogs[0].id = "invalid worklog id".into();
    let mut invalid_task = valid.clone();
    invalid_task.worklogs[0].task_id = "invalid task id".into();
    let mut backwards = valid.clone();
    backwards.worklogs[0].end = Some(at - Duration::seconds(1));
    let mut wrong_scope = valid.clone();
    wrong_scope.worklogs[0].task_id = tracker_domain::TaskId::generate().to_string();
    let mut invalid_cursor = valid.clone();
    invalid_cursor.next_cursor = Some(WorklogCursorDto {
        task_id: Some(requested.to_string()),
        start: at,
        id: "invalid cursor id".into(),
        revision: 1,
    });
    let mut invalid_global_cursor = invalid_cursor.clone();
    invalid_global_cursor.next_cursor.as_mut().unwrap().task_id = None;
    for (global, invalid) in [
        (false, invalid_id),
        (false, invalid_task),
        (false, backwards),
        (false, wrong_scope),
        (false, invalid_cursor),
        (true, invalid_global_cursor),
    ] {
        let reads = Arc::new(AtomicUsize::new(0));
        let valid_for_get = valid.clone();
        let server = StubServer::from_router(
            Router::new()
                .route(
                    "/v1/health",
                    get(|| async {
                        Json(HealthDto {
                            status: "ok".into(),
                            protocol_version: VERSION,
                        })
                    }),
                )
                .route(
                    "/v1/worklogs",
                    get(move || {
                        let reads = reads.clone();
                        let valid = valid_for_get.clone();
                        let invalid = invalid.clone();
                        async move {
                            Json(if reads.fetch_add(1, Ordering::SeqCst) == 0 {
                                valid
                            } else {
                                invalid
                            })
                        }
                    }),
                ),
        );
        let mut client = RemoteApplication::connect(&server.endpoint())
            .await
            .unwrap();
        if global {
            client.all_worklogs(None).await.unwrap();
            assert!(client.all_worklogs(None).await.is_err());
        } else {
            client.worklogs_for_task(requested, None).await.unwrap();
            assert!(client.worklogs_for_task(requested, None).await.is_err());
        }
        assert_eq!(
            client.cached_history((!global).then_some(requested)),
            Some(&valid)
        );
        assert_eq!(client.last_failure(), Some(RemoteFailureKind::Protocol));
    }
}

#[tokio::test]
async fn coherent_task_creation_recovers_the_original_id_after_failed_post_write_reads() {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use tracker_protocol::{
        CreateTaskRequest, MutationDto, MutationResultDto, TaskDto, TasksDto, TrackingDto,
    };
    let current = Arc::new(Mutex::new(Vec::<TaskDto>::new()));
    let coherent = Arc::new(AtomicBool::new(true));
    let writes = Arc::new(AtomicUsize::new(0));
    let tasks_current = current.clone();
    let tasks_coherent = coherent.clone();
    let tracking_coherent = coherent.clone();
    let command_current = current.clone();
    let command_coherent = coherent.clone();
    let command_writes = writes.clone();
    let server = StubServer::from_router(
        Router::new()
            .route(
                "/v1/health",
                get(|| async {
                    Json(HealthDto {
                        status: "ok".into(),
                        protocol_version: VERSION,
                    })
                }),
            )
            .route(
                "/v1/tasks",
                get(move || {
                    let current = tasks_current.clone();
                    let coherent = tasks_coherent.clone();
                    async move {
                        Json(TasksDto {
                            tasks: current.lock().unwrap().clone(),
                            revision: if coherent.load(Ordering::SeqCst) {
                                "confirmed"
                            } else {
                                "racing-tasks"
                            }
                            .into(),
                        })
                    }
                })
                .post(move |Json(body): Json<CreateTaskRequest>| {
                    let current = command_current.clone();
                    let coherent = command_coherent.clone();
                    let writes = command_writes.clone();
                    async move {
                        writes.fetch_add(1, Ordering::SeqCst);
                        let task = TaskDto {
                            id: body.task_id,
                            name: body.name,
                            archived: false,
                            created_at: body.occurred_at,
                            updated_at: body.occurred_at,
                            latest_work_start: None,
                        };
                        *current.lock().unwrap() = vec![task.clone()];
                        coherent.store(false, Ordering::SeqCst);
                        Json(MutationDto {
                            request_id: body.guard.request_id,
                            applied_revision: "committed".into(),
                            replayed: false,
                            result: MutationResultDto::Task(task),
                        })
                    }
                }),
            )
            .route(
                "/v1/tracking",
                get(move || {
                    let coherent = tracking_coherent.clone();
                    async move {
                        Json(TrackingDto {
                            active_worklog: None,
                            revision: if coherent.load(Ordering::SeqCst) {
                                "confirmed"
                            } else {
                                "racing-tracking"
                            }
                            .into(),
                        })
                    }
                }),
            ),
    );
    let mut client = RemoteApplication::connect(&server.endpoint())
        .await
        .unwrap()
        .with_coherent_task_views();
    client.refresh().await.unwrap();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    client
        .create_task(name("Original creation"), at)
        .await
        .unwrap_err();
    assert!(client.last_write_attempted());
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    assert!(client.snapshot().task_items.is_empty());
    assert_eq!(client.task_revision(), "confirmed");
    assert_eq!(client.tracking_revision(), "confirmed");
    let original = current.lock().unwrap()[0].clone();
    coherent.store(true, Ordering::SeqCst);
    let recovered = client
        .create_task(name("Original creation"), at + Duration::seconds(20))
        .await
        .unwrap();
    assert_eq!(recovered.id().to_string(), original.id);
    assert_eq!(recovered.created_at(), at);
    assert_eq!(recovered.name().as_str(), "Original creation");
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    assert!(!client.last_write_attempted());
    assert_eq!(client.snapshot().task_items.len(), 1);
    assert_eq!(client.task_revision(), client.tracking_revision());
}

#[tokio::test]
async fn definitively_rejected_creation_allows_a_new_task_identity() {
    use axum::{http::StatusCode, response::IntoResponse};
    use std::sync::{Arc, Mutex};
    use tracker_protocol::{
        CreateTaskRequest, ErrorCode, ErrorDto, MutationDto, MutationResultDto, TaskDto, TasksDto,
    };
    let commands = Arc::new(Mutex::new(Vec::<CreateTaskRequest>::new()));
    let current = Arc::new(Mutex::new(Vec::<TaskDto>::new()));
    let for_get = current.clone();
    let for_post = current.clone();
    let captured = commands.clone();
    let server = StubServer::from_router(
        Router::new()
            .route(
                "/v1/health",
                get(|| async {
                    Json(HealthDto {
                        status: "ok".into(),
                        protocol_version: VERSION,
                    })
                }),
            )
            .route(
                "/v1/tasks",
                get(move || {
                    let current = for_get.clone();
                    async move {
                        Json(TasksDto {
                            tasks: current.lock().unwrap().clone(),
                            revision: "confirmed".into(),
                        })
                    }
                })
                .post(move |Json(body): Json<CreateTaskRequest>| {
                    let current = for_post.clone();
                    let captured = captured.clone();
                    async move {
                        let first = {
                            let mut commands = captured.lock().unwrap();
                            commands.push(body.clone());
                            commands.len() == 1
                        };
                        if first {
                            return (
                                StatusCode::CONFLICT,
                                Json(ErrorDto {
                                    code: ErrorCode::StaleRevision,
                                    message: "Tracker state changed. Refresh and retry.".into(),
                                }),
                            )
                                .into_response();
                        }
                        let task = TaskDto {
                            id: body.task_id,
                            name: body.name,
                            archived: false,
                            created_at: body.occurred_at,
                            updated_at: body.occurred_at,
                            latest_work_start: None,
                        };
                        *current.lock().unwrap() = vec![task.clone()];
                        Json(MutationDto {
                            request_id: body.guard.request_id,
                            applied_revision: "committed".into(),
                            replayed: false,
                            result: MutationResultDto::Task(task),
                        })
                        .into_response()
                    }
                }),
            ),
    );
    let mut client = RemoteApplication::connect(&server.endpoint())
        .await
        .unwrap();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let error = client
        .create_task(name("Rejected task"), at)
        .await
        .unwrap_err();
    assert_eq!(
        error.failure().source(),
        tracker_application::ApplicationFailureSource::Operation
    );
    let task = client
        .create_task(name("New reviewed task"), at + Duration::seconds(1))
        .await
        .unwrap();
    assert_eq!(task.name().as_str(), "New reviewed task");
    let commands = commands.lock().unwrap();
    assert_eq!(commands.len(), 2);
    assert_ne!(commands[0].task_id, commands[1].task_id);
    assert_ne!(commands[0].guard.request_id, commands[1].guard.request_id);
    assert_eq!(commands[1].task_id, task.id().to_string());
}

#[tokio::test]
async fn coherent_tracking_preflight_preserves_the_confirmed_view_when_a_new_task_is_running() {
    let server = TestServer::start();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let mut first = connected(&server.endpoint()).await.unwrap();
    let original_task = first
        .create_task(name("Original timer task"), at)
        .await
        .unwrap();
    let original_timer = match first
        .set_active_task(original_task.id(), at + Duration::seconds(1))
        .await
        .unwrap()
    {
        SetActiveTaskOutcome::Started { worklog } => worklog,
        _ => panic!("expected original timer"),
    };
    let mut second = RemoteApplication::connect(&server.endpoint())
        .await
        .unwrap()
        .with_coherent_task_views();
    second.refresh().await.unwrap();
    let revision = second.task_revision().to_owned();
    let confirmed_active = second.snapshot().active_worklog.clone();
    let new_task = first
        .create_task(name("New timer task"), at + Duration::seconds(2))
        .await
        .unwrap();
    first
        .set_active_task(new_task.id(), at + Duration::seconds(3))
        .await
        .unwrap();

    second
        .clear_active_task(original_timer.id(), at + Duration::seconds(4))
        .await
        .unwrap_err();
    assert!(!second.last_write_attempted());
    assert_eq!(second.task_revision(), second.tracking_revision());
    assert_eq!(second.task_revision(), revision);
    assert_eq!(second.snapshot().active_worklog, confirmed_active);
    let active_task_id = second.snapshot().active_worklog.as_ref().unwrap().task_id();
    assert!(
        second
            .snapshot()
            .task_items
            .iter()
            .any(|item| item.task.id() == active_task_id)
    );
}

#[tokio::test]
async fn coherent_tracking_preflight_initializes_a_pair_before_rejecting_an_unreviewed_timer() {
    let server = TestServer::start();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let mut first = connected(&server.endpoint()).await.unwrap();
    let running_task = first.create_task(name("Running task"), at).await.unwrap();
    let target = first.create_task(name("Target task"), at).await.unwrap();
    let running = match first
        .set_active_task(running_task.id(), at + Duration::seconds(1))
        .await
        .unwrap()
    {
        SetActiveTaskOutcome::Started { worklog } => worklog,
        _ => panic!("expected original timer"),
    };
    for starting in [false, true] {
        let mut client = RemoteApplication::connect(&server.endpoint())
            .await
            .unwrap()
            .with_coherent_task_views();
        assert!(client.task_revision().is_empty());
        assert!(client.tracking_revision().is_empty());
        if starting {
            client
                .set_active_task_with_expected_active(target.id(), None, at + Duration::seconds(2))
                .await
                .unwrap_err();
        } else {
            client
                .clear_active_task(WorklogId::generate(), at + Duration::seconds(2))
                .await
                .unwrap_err();
        }
        assert!(!client.last_write_attempted());
        assert!(!client.task_revision().is_empty());
        assert_eq!(client.task_revision(), client.tracking_revision());
        assert_eq!(
            client.snapshot().active_worklog.as_ref().unwrap().id(),
            running.id()
        );
        let active_task_id = client.snapshot().active_worklog.as_ref().unwrap().task_id();
        assert!(
            client
                .snapshot()
                .task_items
                .iter()
                .any(|item| item.task.id() == active_task_id)
        );
    }
    let mut initial = RemoteApplication::connect(&server.endpoint())
        .await
        .unwrap()
        .with_coherent_task_views();
    assert!(matches!(
        initial
            .set_active_task(target.id(), at + Duration::seconds(3))
            .await
            .unwrap(),
        SetActiveTaskOutcome::Switched { .. }
    ));
    assert_eq!(initial.task_revision(), initial.tracking_revision());
    assert_eq!(
        initial
            .snapshot()
            .active_worklog
            .as_ref()
            .unwrap()
            .task_id(),
        target.id()
    );
}
