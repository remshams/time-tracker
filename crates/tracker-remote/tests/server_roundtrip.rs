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
