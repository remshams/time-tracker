use std::{net::SocketAddr, path::PathBuf, sync::mpsc, thread};

use axum::{
    Json, Router,
    routing::{get, patch},
};
use chrono::{DateTime, Duration, Utc};
use tempfile::TempDir;
use tokio::sync::oneshot;
use tracker_application::{
    ClearActiveTaskOutcome, SetActiveTaskOutcome, TaskOrdering, WorklogCursor,
};
use tracker_domain::{TaskName, WorklogId, WorklogTimes};
use tracker_protocol::{DailyOverviewDto, HealthDto, TaskListStateDto, VERSION};
use tracker_remote::RemoteApplication;
use tracker_remote::RemoteFailureKind;

/// Checks that a collapsed report interval fails before attempting a server connection.
#[tokio::test]
async fn report_range_validation_uses_microseconds_before_connecting() {
    let mut client = RemoteApplication::disconnected("http://127.0.0.1:1/").unwrap();
    let start = DateTime::from_timestamp(100, 100).unwrap();
    let end = DateTime::from_timestamp(100, 900).unwrap();

    assert_eq!(
        client.task_totals(start, end, end).await,
        Err(tracker_application::ApplicationError::InvalidReportRange)
    );
    assert!(client.report_observation().is_none());
    assert!(matches!(
        client.refresh_task_list_with_totals(start, end, end).await,
        Err(tracker_remote::RemoteError::Protocol(message)) if message == "invalid report range"
    ));
    assert!(client.report_observation().is_none());
    assert!(client.task_observation().is_none());
    assert!(client.tracking_observation().is_none());
}

/// Invalid canonical input preserves network probes and existing availability state.
#[tokio::test]
async fn invalid_report_refreshes_do_not_contact_the_server_or_start_an_outage_cooldown() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    let reads = Arc::new(AtomicUsize::new(0));
    let health_reads = reads.clone();
    let server = StubServer::from_router(Router::new().route(
        "/v1/health",
        get(move || {
            health_reads.fetch_add(1, Ordering::SeqCst);
            async { axum::http::StatusCode::SERVICE_UNAVAILABLE }
        }),
    ));
    let start = DateTime::from_timestamp(100, 100).unwrap();
    let end = DateTime::from_timestamp(100, 900).unwrap();
    let mut client = RemoteApplication::disconnected(&server.endpoint()).unwrap();
    let failure = client.last_failure();
    assert!(matches!(
        client.refresh_task_list_with_totals(start, end, end).await,
        Err(tracker_remote::RemoteError::Protocol(message)) if message == "invalid report range"
    ));
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(client.last_failure(), failure);
    let later = end + Duration::seconds(1);
    assert!(
        client
            .refresh_task_list_with_totals(start, later, later)
            .await
            .unwrap_err()
            .is_unavailable()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert!(matches!(
        client.refresh_task_list_with_totals(start, end, end).await,
        Err(tracker_remote::RemoteError::Protocol(message)) if message == "invalid report range"
    ));
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(client.last_failure(), Some(RemoteFailureKind::Unavailable));
}

/// Checks that request bounds, returned cutoffs, and the retained report use identical precision.
#[tokio::test]
async fn report_request_and_echo_use_the_same_microsecond_context() {
    let server = TestServer::start();
    let mut client = RemoteApplication::connect(&server.endpoint())
        .await
        .unwrap();
    let start = DateTime::from_timestamp(100, 900).unwrap();
    let end = DateTime::from_timestamp(100, 2_100).unwrap();
    let now = DateTime::from_timestamp(100, 1_900).unwrap();

    let report = client.task_totals(start, end, now).await.unwrap();

    assert_eq!(report.start, DateTime::from_timestamp(100, 0).unwrap());
    assert_eq!(report.end, DateTime::from_timestamp(100, 2_000).unwrap());
    assert_eq!(report.now, DateTime::from_timestamp(100, 1_000).unwrap());
    assert_eq!(client.report_observation(), Some(&report));
    assert!(client.task_observation().is_none());
    assert!(client.tracking_observation().is_none());

    client
        .refresh_task_list_with_totals(start, end, now)
        .await
        .unwrap();
    assert_eq!(client.report_observation().unwrap().start, report.start);
    assert_eq!(client.report_observation().unwrap().end, report.end);
    assert_eq!(client.report_observation().unwrap().now, report.now);
    assert!(client.task_observation().is_some());
    assert!(client.tracking_observation().is_some());
    assert_eq!(client.last_failure(), None);
    let confirmed = client.report_observation().cloned();
    assert!(
        client
            .refresh_task_list_with_totals(start, start, now)
            .await
            .is_err()
    );
    assert_eq!(client.last_failure(), None);
    assert_eq!(client.report_observation(), confirmed.as_ref());
}

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
    client.refresh_task_list().await?;
    Ok(client)
}

fn name(raw: &str) -> TaskName {
    TaskName::new(raw).unwrap()
}

/// Exercises persisted preview confirmation and verifies the server archives exactly the eligible tasks.
#[tokio::test]
async fn serialized_preview_confirms_the_chosen_period_and_returns_the_persisted_count() {
    let server = TestServer::start();
    let mut client = connected(&server.endpoint()).await.unwrap();
    let as_of = tracker_application::timestamps::canonical_timestamp(Utc::now());
    let first = client
        .create_task(name("Old planning task"), as_of - Duration::days(10))
        .await
        .unwrap();
    let second = client
        .create_task(name("Old review task"), as_of - Duration::days(8))
        .await
        .unwrap();
    let recent = client
        .create_task(name("Recent task"), as_of - Duration::days(2))
        .await
        .unwrap();
    let preview = client
        .preview_inactive_tasks_with_period(
            as_of,
            tracker_domain::InactivityPeriod::new(7).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(preview.as_of(), as_of);
    assert_eq!(preview.inactive_days(), 7);
    assert_eq!(preview.count(), 2);
    let mut expected = vec![first.id().to_string(), second.id().to_string()];
    expected.sort();
    let token = serde_json::to_value(&preview).unwrap();
    let mut candidate_ids: Vec<String> =
        serde_json::from_value(token["candidate_task_ids"].clone()).unwrap();
    candidate_ids.sort();
    assert_eq!(candidate_ids, expected);
    assert!(token.get("revision").is_none());
    let serialized = serde_json::to_string(&preview).unwrap();
    let restored = serde_json::from_str(&serialized).unwrap();
    let count = client
        .archive_inactive_tasks_with_period(&restored)
        .await
        .unwrap();
    assert_eq!(count, 2);
    client.refresh_task_list().await.unwrap();
    assert!(client.task(first.id()).unwrap().is_archived());
    assert!(client.task(second.id()).unwrap().is_archived());
    assert!(!client.task(recent.id()).unwrap().is_archived());
}

/// Confirms an older preview after other clients' writes and archives the tasks eligible at run time.
#[tokio::test]
async fn remote_inactive_archive_applies_the_preview_period_when_the_command_runs() {
    let server = TestServer::start();
    let mut client = connected(&server.endpoint()).await.unwrap();
    let mut other = connected(&server.endpoint()).await.unwrap();
    let as_of = Utc::now();
    let reviewed = client
        .create_task(name("Old remote task"), as_of - Duration::days(20))
        .await
        .unwrap();

    let preview = client.preview_inactive_tasks(as_of).await.unwrap();
    assert_eq!(preview.count(), 1);
    assert_eq!(
        client.resolve_preview_tasks(&preview).await.unwrap()[0]
            .name()
            .as_str(),
        "Old remote task"
    );
    let recent = other
        .create_task(name("New remote task"), as_of)
        .await
        .unwrap();
    let unreviewed = other
        .create_task(name("Unreviewed old task"), as_of - Duration::days(30))
        .await
        .unwrap();

    assert_eq!(client.archive_inactive_tasks(&preview).await.unwrap(), 2);
    assert!(client.last_write_attempted());
    assert!(client.task(reviewed.id()).unwrap().is_archived());
    assert!(client.task(unreviewed.id()).unwrap().is_archived());
    assert!(!client.task(recent.id()).unwrap().is_archived());
}

#[tokio::test]
async fn remote_client_rejects_each_invalid_health_field_before_adopting_resources() {
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
            client.refresh_task_list().await,
            Err(tracker_remote::RemoteError::Protocol(_))
        ));
        assert_eq!(client.last_failure(), Some(RemoteFailureKind::Protocol));
        assert!(client.task_items().is_empty());
    }
}

#[tokio::test]
async fn prewrite_compatibility_failure_does_not_retry_or_recover_resources() {
    use axum::{http::StatusCode, response::IntoResponse};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tracker_protocol::{InactiveTaskPreviewDto, TasksDto};

    for unavailable in [false, true] {
        let health_reads = Arc::new(AtomicUsize::new(0));
        let resource_reads = Arc::new(AtomicUsize::new(0));
        let health_counter = health_reads.clone();
        let resource_counter = resource_reads.clone();
        let server = StubServer::from_router(
            Router::new()
                .route(
                    "/v1/health",
                    get(move || {
                        let first = health_counter.fetch_add(1, Ordering::SeqCst) == 0;
                        async move {
                            if first && unavailable {
                                return StatusCode::SERVICE_UNAVAILABLE.into_response();
                            }
                            Json(HealthDto {
                                status: "ok".into(),
                                protocol_version: if first { VERSION + 1 } else { VERSION },
                            })
                            .into_response()
                        }
                    }),
                )
                .route(
                    "/v1/tasks",
                    get(move || {
                        resource_counter.fetch_add(1, Ordering::SeqCst);
                        async { Json(TasksDto { tasks: vec![] }) }
                    }),
                ),
        );
        let mut client = RemoteApplication::disconnected(&server.endpoint()).unwrap();
        let preview = InactiveTaskPreviewDto {
            as_of: DateTime::UNIX_EPOCH,
            inactive_days: 7,
            count: 0,
            candidate_task_ids: vec![],
        };
        let preview = serde_json::from_value(serde_json::to_value(preview).unwrap()).unwrap();
        let error = client.archive_inactive_tasks(&preview).await.unwrap_err();
        assert_eq!(health_reads.load(Ordering::SeqCst), 1);
        assert_eq!(resource_reads.load(Ordering::SeqCst), 0);
        assert!(!client.last_write_attempted());
        assert!(client.last_command_receipt().is_none());
        assert!(client.task_observation().is_none());
        assert!(client.tracking_observation().is_none());
        assert!(!error.failure().recovery_failed());
        assert_eq!(
            client.last_failure(),
            Some(if unavailable {
                RemoteFailureKind::Unavailable
            } else {
                RemoteFailureKind::Protocol
            })
        );
    }
}

/// The server rejects an outdated reviewed name after the previous command's receipt is cleared.
#[tokio::test]
async fn rejected_rename_reports_the_attempt_and_clears_the_previous_receipt() {
    let server = TestServer::start();
    let mut client = connected(&server.endpoint()).await.unwrap();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let task = client.create_task(name("Project work"), at).await.unwrap();
    assert!(client.last_write_attempted());
    assert!(client.last_command_receipt().is_some());

    let error = client
        .rename_task_with_expected_name(
            task.id(),
            &name("Previously reviewed name"),
            name("Renamed work"),
            at + Duration::seconds(1),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.failure().category(),
        tracker_application::ApplicationFailureCategory::General
    );
    assert_eq!(error.failure().message(), "Task changed in another client");
    assert!(!error.failure().recovery_failed());
    assert!(client.last_write_attempted());
    assert!(client.last_command_receipt().is_none());
    assert_eq!(
        client.task(task.id()).unwrap().name().as_str(),
        "Project work"
    );
}

#[tokio::test]
async fn unavailable_totals_reads_keep_the_cooldown_without_another_request() {
    use axum::http::StatusCode;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    let reads = Arc::new(AtomicUsize::new(0));
    let counter = reads.clone();
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
                "/v1/state/daily-overview",
                get(move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                    async { StatusCode::SERVICE_UNAVAILABLE }
                }),
            ),
    );
    let mut client = RemoteApplication::connect(&server.endpoint())
        .await
        .unwrap();
    let start = DateTime::UNIX_EPOCH;
    let end = start + Duration::hours(24);
    for _ in 0..2 {
        assert!(matches!(
            client
                .refresh_task_list_with_totals(start, end, start)
                .await,
            Err(tracker_remote::RemoteError::Unavailable(_))
                | Err(tracker_remote::RemoteError::Http {
                    status: StatusCode::SERVICE_UNAVAILABLE,
                    ..
                })
        ));
        assert_eq!(client.last_failure(), Some(RemoteFailureKind::Unavailable));
    }
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert!(client.report_observation().is_none());
    assert!(client.tracking_observation().is_none());
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
async fn canceling_a_fresh_creation_during_health_allows_a_different_draft() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use tokio::sync::Notify;

    let health_started = Arc::new(Notify::new());
    let release_health = Arc::new(Notify::new());
    let first_health = Arc::new(AtomicBool::new(true));
    let writes = Arc::new(AtomicUsize::new(0));
    let started = health_started.clone();
    let release = release_health.clone();
    let count = writes.clone();
    let directory = TempDir::new().unwrap();
    let router =
        tracker_server::router_for_database(&directory.path().join("cancellation.sqlite3"))
            .unwrap()
            .layer(axum::middleware::from_fn(
                move |request: axum::extract::Request, next: axum::middleware::Next| {
                    let first_health = first_health.clone();
                    let started = started.clone();
                    let release = release.clone();
                    let count = count.clone();
                    async move {
                        if request.uri().path() == "/v1/health"
                            && first_health.swap(false, Ordering::SeqCst)
                        {
                            started.notify_one();
                            release.notified().await;
                        }
                        if request.method() == axum::http::Method::POST {
                            count.fetch_add(1, Ordering::SeqCst);
                        }
                        next.run(request).await
                    }
                },
            ));
    let server = StubServer::from_router(router);
    let mut client = RemoteApplication::disconnected(&server.endpoint()).unwrap();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    {
        let command = client.create_task(name("Canceled draft"), at);
        tokio::pin!(command);
        tokio::select! {
            result = &mut command => panic!("creation completed before health was released: {result:?}"),
            started = tokio::time::timeout(std::time::Duration::from_secs(5), health_started.notified()) => {
                started.expect("creation must wait for the initial health response");
            }
        }
    }
    release_health.notify_one();
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    assert!(!client.last_write_attempted());

    let created = client
        .create_task(name("Different draft"), at + Duration::seconds(1))
        .await
        .unwrap();
    assert_eq!(created.name().as_str(), "Different draft");
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    assert!(client.last_write_attempted());
}

#[tokio::test]
async fn a_refused_fresh_creation_allows_a_different_draft_after_server_restart() {
    let mut server = TestServer::start();
    let mut client = connected(&server.endpoint()).await.unwrap();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    server.stop();
    assert!(
        client
            .create_task(name("Original draft"), at)
            .await
            .is_err()
    );
    assert!(!client.last_write_attempted());

    server.restart();
    let created = client
        .create_task(name("Different draft"), at + Duration::seconds(1))
        .await
        .unwrap();
    assert_eq!(created.name().as_str(), "Different draft");
    let observer = connected(&server.endpoint()).await.unwrap();
    let items = observer.tasks(TaskOrdering::RecentlyCreated);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].task, created);
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

    let candidates = client.move_candidates(first.id(), "Second");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].id, second.id());

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
        .task_totals(at, at + Duration::seconds(30), at + Duration::seconds(30))
        .await
        .unwrap();
    assert_eq!(report.total_us, 17_000_000);

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
        task_worklog(&mut client, second.id(), deleted.id())
            .await
            .is_none()
    );
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
            .rename_task_with_expected_name(
                task.id(),
                task.name(),
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
    first.refresh_task_list().await.unwrap();
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
    let cached_name = second.task(task.id()).unwrap().name().clone();
    assert!(
        second
            .rename_task_with_expected_name(
                task.id(),
                &cached_name,
                name("Stale name"),
                at + Duration::seconds(2)
            )
            .await
            .is_err()
    );
    assert_eq!(second.task(task.id()).unwrap().name().as_str(), "New name");
}

/// Unguarded commands send no reviewed name, so they apply after another client's rename and
/// need no task read first, even without a cached catalog.
#[tokio::test]
async fn unguarded_rename_and_archive_apply_after_another_clients_rename_without_a_read() {
    use std::sync::{Arc, Mutex};
    let directory = TempDir::new().unwrap();
    let router =
        tracker_server::router_for_database(&directory.path().join("remote.sqlite3")).unwrap();
    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let recorder = requests.clone();
    let router = router.layer(axum::middleware::from_fn(
        move |request: axum::extract::Request, next: axum::middleware::Next| {
            let recorder = recorder.clone();
            async move {
                if request.uri().path() != "/v1/health" {
                    recorder.lock().unwrap().push(format!(
                        "{} {}",
                        request.method(),
                        request.uri().path()
                    ));
                }
                next.run(request).await
            }
        },
    ));
    let server = StubServer::from_router(router);
    let at = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let mut other = connected(&server.endpoint()).await.unwrap();
    let task = other.create_task(name("Reviewed task"), at).await.unwrap();
    other
        .rename_task(
            task.id(),
            name("Other client task"),
            at + Duration::seconds(1),
        )
        .await
        .unwrap();
    let mut client = RemoteApplication::connect(&server.endpoint())
        .await
        .unwrap();
    let command = format!("PATCH /v1/tasks/{}", task.id());

    requests.lock().unwrap().clear();
    let renamed = client
        .rename_task(
            task.id(),
            name("Unguarded rename"),
            at + Duration::seconds(2),
        )
        .await
        .unwrap();
    assert_eq!(renamed.name().as_str(), "Unguarded rename");
    assert_eq!(requests.lock().unwrap().first(), Some(&command));

    requests.lock().unwrap().clear();
    let archived = client
        .archive_task(task.id(), at + Duration::seconds(3))
        .await
        .unwrap();
    assert!(archived.is_archived());
    assert_eq!(archived.name().as_str(), "Unguarded rename");
    assert_eq!(requests.lock().unwrap().first(), Some(&command));

    other.refresh_task_list().await.unwrap();
    let stored = other.task(task.id()).unwrap();
    assert_eq!(stored.name().as_str(), "Unguarded rename");
    assert!(stored.is_archived());
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

    let error = client.refresh_task_list().await.unwrap_err();
    assert!(error.is_unavailable());
    assert_eq!(client.last_failure(), Some(RemoteFailureKind::Unavailable));
    assert_eq!(client.tasks(TaskOrdering::RecentlyCreated).len(), count);
    assert!(client.create_task(name("Offline task"), at).await.is_err());
    assert_eq!(client.tasks(TaskOrdering::RecentlyCreated).len(), count);
    let failure = client
        .task_totals(at, at + Duration::seconds(1), at)
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
    assert!(page.adoption.is_none());
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
    second.refresh_task_list().await.unwrap();
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
    assert!(current_page.adoption.is_none());
    assert_eq!(
        second.task_item(requested.id()).unwrap().latest_work_start,
        Some(requested_at)
    );
    assert_eq!(
        second.task_item(active.id()).unwrap().latest_work_start,
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
    assert_eq!(cursor.id, first.worklogs.last().unwrap().id());
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
    assert!(feed.task_items.is_empty());
    assert!(second.task(task.id()).is_none());
    assert_eq!(
        second
            .task_totals(
                at + Duration::seconds(15),
                at + Duration::seconds(20),
                at + Duration::seconds(30)
            )
            .await
            .unwrap()
            .total_us,
        5_000_000
    );
    assert_eq!(
        second
            .task_totals(
                at + Duration::seconds(20),
                at + Duration::seconds(30),
                at + Duration::seconds(30)
            )
            .await
            .unwrap()
            .total_us,
        0
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
    assert!(client.task_items().is_empty());
    assert!(client.task_observation().is_none());
    assert!(client.tracking_observation().is_none());
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
    let reviewed_tasks = second.task_observation().unwrap().to_vec();
    let reviewed_tracking = second.tracking_observation().cloned();
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
    assert!(second.all_worklogs(None).await.unwrap().worklogs.is_empty());
    assert_eq!(second.task_observation(), Some(reviewed_tasks.as_slice()));
    assert_eq!(second.tracking_observation().cloned(), reviewed_tracking);
    assert!(second.report_observation().is_some());
    assert!(
        second
            .rename_task_with_expected_name(
                task.id(),
                task.name(),
                name("Reviewed draft"),
                at + Duration::seconds(2)
            )
            .await
            .is_err()
    );
    assert_eq!(
        second.task(task.id()).unwrap().name().as_str(),
        "Changed by other client"
    );
    second.refresh_task_list().await.unwrap();
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
async fn aggregate_refresh_uses_one_read_and_preserves_confirmed_state_on_missing_route() {
    use axum::http::StatusCode;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let reads = Arc::new(AtomicUsize::new(0));
    let count = reads.clone();
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
                "/v1/state/task-list",
                get(move || {
                    let i = count.fetch_add(1, Ordering::SeqCst);
                    async move {
                        (
                            if i == 0 {
                                StatusCode::OK
                            } else {
                                StatusCode::NOT_FOUND
                            },
                            Json(TaskListStateDto {
                                tasks: vec![],
                                active_worklog: None,
                            }),
                        )
                    }
                }),
            ),
    );
    let mut client = RemoteApplication::connect(&server.endpoint())
        .await
        .unwrap();
    assert!(client.tracking_observation().is_none());
    client.refresh_task_list().await.unwrap();
    let tasks = client.task_observation().map(<[_]>::to_vec);
    let tracking = client.tracking_observation().cloned();
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(
        client.current_tracking(),
        &tracker_domain::TrackingState::Idle
    );
    assert!(matches!(
        client.refresh_task_list().await,
        Err(tracker_remote::RemoteError::Http {
            status: StatusCode::NOT_FOUND,
            ..
        })
    ));
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    assert_eq!(client.task_observation(), tasks.as_deref());
    assert_eq!(client.tracking_observation(), tracking.as_ref());
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
        let catalog = TasksDto { tasks };
        let tracking = TrackingDto {
            active_worklog: Some(WorklogDto {
                id: WorklogId::generate().to_string(),
                task_id: active_task_id.to_string(),
                start: at,
                end: None,
            }),
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
                "/v1/state/task-list",
                get(move || {
                    let state = TaskListStateDto {
                        tasks: catalog.tasks.clone(),
                        active_worklog: tracking.active_worklog.clone(),
                    };
                    async move { Json(state) }
                }),
            );
        let server = StubServer::from_router(router);
        let mut client = RemoteApplication::connect(&server.endpoint())
            .await
            .unwrap();
        assert!(matches!(
            client.refresh_task_list().await,
            Err(tracker_remote::RemoteError::Protocol(_))
        ));
        assert_eq!(client.last_failure(), Some(RemoteFailureKind::Protocol));
        assert!(client.task_items().is_empty());
        assert!(active_worklog(&client).is_none());
        assert!(client.task_observation().is_none());
        assert!(client.tracking_observation().is_none());
    }
}

/// Another client's write between the preview and the candidate reads does not invalidate the
/// preview; only an archived candidate does.
#[tokio::test]
async fn preview_candidates_resolve_after_another_client_writes() {
    let server = TestServer::start();
    let mut client = connected(&server.endpoint()).await.unwrap();
    let as_of = Utc::now();
    let candidate = client
        .create_task(name("Old task"), as_of - Duration::days(20))
        .await
        .unwrap();
    let preview = client.preview_inactive_tasks(as_of).await.unwrap();
    let mut other = connected(&server.endpoint()).await.unwrap();
    other
        .create_task(name("Intervening write"), as_of)
        .await
        .unwrap();
    other
        .rename_task(candidate.id(), name("Renamed old task"), as_of)
        .await
        .unwrap();

    let resolved = client.resolve_preview_tasks(&preview).await.unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].id(), candidate.id());
    assert_eq!(resolved[0].name().as_str(), "Renamed old task");
    assert_eq!(client.last_failure(), None);

    other.archive_task(candidate.id(), as_of).await.unwrap();
    assert_eq!(
        client
            .resolve_preview_tasks(&preview)
            .await
            .unwrap_err()
            .to_string(),
        "Tracker state changed. Refresh and retry."
    );
    assert_eq!(client.last_failure(), Some(RemoteFailureKind::Conflict));
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
    let receipt = client.last_command_receipt().unwrap();
    assert_eq!(
        receipt.result,
        MutationResultDto::Task(TaskDto {
            id: task.id().to_string(),
            name: "Original command result".into(),
            archived: false,
            created_at: task.created_at(),
            updated_at: task.updated_at(),
            latest_work_start: None,
        })
    );
    assert!(receipt.replayed);
    assert!(!receipt.request_id.is_empty());
}

#[tokio::test]
async fn possibly_committed_creation_rereads_tasks_and_recovers_the_original_identity() {
    use axum::{http::StatusCode, response::IntoResponse};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use tracker_protocol::{CreateTaskRequest, TaskDto, TasksDto};
    let current = Arc::new(Mutex::new(Vec::<TaskDto>::new()));
    let for_get = current.clone();
    let for_post = current.clone();
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let fail_recovery = Arc::new(AtomicBool::new(false));
    let fail_get = fail_recovery.clone();
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
                let fail_get = fail_get.clone();
                async move {
                    if fail_get.swap(false, Ordering::SeqCst) {
                        return StatusCode::SERVICE_UNAVAILABLE.into_response();
                    }
                    Json(TasksDto {
                        tasks: current.lock().unwrap().clone(),
                    })
                    .into_response()
                }
            })
            .post(move |Json(body): Json<CreateTaskRequest>| {
                let current = for_post.clone();
                let count = count.clone();
                let fail_recovery = fail_recovery.clone();
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
                    fail_recovery.store(true, Ordering::SeqCst);
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
        tracker_application::ApplicationFailureSource::RemoteUnavailable
    );
    assert!(matches!(
        &error,
        tracker_application::ApplicationError::Recovery { primary, recovery }
            if matches!(primary.as_ref(), tracker_application::ApplicationError::RemoteProtocol(_))
                && matches!(recovery.as_ref(), tracker_application::ApplicationError::RemoteUnavailable(_))
    ));
    assert!(client.last_write_attempted());
    assert!(error.failure().recovery_failed());
    assert!(client.task_items().is_empty());
    let original = current.lock().unwrap()[0].clone();
    let recovered = client
        .create_task(name("Uncertain create"), clicked_at + Duration::seconds(20))
        .await
        .unwrap();
    assert_eq!(recovered.id().to_string(), original.id);
    assert_eq!(recovered.name().as_str(), original.name);
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
    assert_eq!(client.report_observation(), Some(&expected));
    assert!(client.task(id).is_none());
    assert!(client.task_observation().is_none());
    assert!(client.tracking_observation().is_none());
    assert!(client.read_task_item(id).await.is_err());
    assert_eq!(client.report_observation(), Some(&expected));
}

/// Returns the active worklog in the client's confirmed tracking state.
fn active_worklog(client: &RemoteApplication) -> Option<tracker_domain::Worklog> {
    match client.current_tracking() {
        tracker_domain::TrackingState::Idle => None,
        tracker_domain::TrackingState::Running { worklog } => Some(worklog.to_worklog()),
    }
}

/// Finds one worklog in the first history page of its task.
async fn task_worklog(
    client: &mut RemoteApplication,
    task_id: tracker_domain::TaskId,
    id: WorklogId,
) -> Option<tracker_domain::Worklog> {
    client
        .worklogs_for_task(task_id, None)
        .await
        .unwrap()
        .worklogs
        .into_iter()
        .find(|worklog| worklog.id() == id)
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
async fn individual_resources_reject_wrong_ids_and_domain_values() {
    use tracker_protocol::{TaskDto, TaskResourceDto};
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let task_id = tracker_domain::TaskId::generate();
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
        },
        TaskResourceDto {
            task: TaskDto {
                name: "Bad\u{1b}name".into(),
                ..task
            },
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
        assert!(client.report_observation().is_none());
    }
    let valid = InactiveTaskPreviewDto {
        as_of: at,
        inactive_days: 7,
        count: 0,
        candidate_task_ids: vec![],
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

/// Keeps the original creation request across canceled reconciliation.
#[tokio::test]
async fn uncertain_uncommitted_creation_resends_the_same_request() {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use tokio::sync::Notify;
    use tracker_protocol::{CreateTaskRequest, MutationDto, MutationResultDto, TaskDto, TasksDto};
    let bodies = Arc::new(Mutex::new(Vec::<CreateTaskRequest>::new()));
    let received = bodies.clone();
    let current = Arc::new(Mutex::new(Vec::<TaskDto>::new()));
    let for_get = current.clone();
    let for_post = current.clone();
    let phase = Arc::new(AtomicUsize::new(0));
    let post_phase = phase.clone();
    let hold_reconciliation = Arc::new(AtomicBool::new(false));
    let reconciliation_started = Arc::new(Notify::new());
    let release_reconciliation = Arc::new(Notify::new());
    let hold_read = hold_reconciliation.clone();
    let read_started = reconciliation_started.clone();
    let release_read = release_reconciliation.clone();
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
                let hold_read = hold_read.clone();
                let read_started = read_started.clone();
                let release_read = release_read.clone();
                async move {
                    if hold_read.swap(false, Ordering::SeqCst) {
                        read_started.notify_one();
                        release_read.notified().await;
                    }
                    Json(TasksDto {
                        tasks: current.lock().unwrap().clone(),
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
    hold_reconciliation.store(true, Ordering::SeqCst);
    {
        let command = client.create_task(name("Original draft"), clicked_at);
        tokio::pin!(command);
        tokio::select! {
            result = &mut command => panic!("creation completed before reconciliation was released: {result:?}"),
            started = tokio::time::timeout(std::time::Duration::from_secs(5), reconciliation_started.notified()) => {
                started.expect("pending creation must wait for reconciliation");
            }
        }
    }
    release_reconciliation.notify_one();
    assert!(!client.last_write_attempted());
    assert_eq!(bodies.lock().unwrap().len(), 1);
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
    assert_eq!(bodies[0], bodies[1]);
    assert_eq!(task.created_at(), clicked_at);
}

#[tokio::test]
async fn an_uncertain_creation_keeps_its_identity_after_a_refused_resend() {
    use std::{
        io::{BufRead, BufReader, Read, Write},
        net::{TcpListener, TcpStream},
        time::{Duration as Timeout, Instant},
    };
    use tracker_protocol::{CreateTaskRequest, MutationDto, MutationResultDto, TaskDto, TasksDto};

    fn receive(listener: &TcpListener, expected: &str) -> (TcpStream, Vec<u8>) {
        let deadline = Instant::now() + Timeout::from_secs(5);
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "expected {expected}");
                    thread::sleep(Timeout::from_millis(10));
                }
                Err(error) => panic!("listener failed: {error}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Timeout::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Timeout::from_secs(5)))
            .unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        assert!(line.starts_with(&format!("{expected} ")), "{line}");
        let mut length = 0;
        loop {
            line.clear();
            assert!(reader.read_line(&mut line).unwrap() > 0);
            if line == "\r\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':')
                && name.eq_ignore_ascii_case("content-length")
            {
                length = value.trim().parse().unwrap();
            }
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        (reader.into_inner(), body)
    }

    fn respond(mut stream: TcpStream, body: &[u8]) {
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .unwrap();
        stream.write_all(body).unwrap();
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let listener = listener.into_std().unwrap();
    let (restart_tx, restart_rx) = mpsc::channel::<TcpListener>();
    let server = thread::spawn(move || {
        let empty_catalog = serde_json::to_vec(&TasksDto { tasks: vec![] }).unwrap();
        let (stream, _) = receive(&listener, "GET /v1/health");
        respond(
            stream,
            &serde_json::to_vec(&HealthDto {
                status: "ok".into(),
                protocol_version: VERSION,
            })
            .unwrap(),
        );
        let (stream, original) = receive(&listener, "POST /v1/tasks");
        respond(stream, b"invalid receipt");
        let (stream, _) = receive(&listener, "GET /v1/tasks");
        respond(stream, &empty_catalog);
        let (stream, _) = receive(&listener, "GET /v1/tasks");
        // Close the listener before completing reconciliation so both resend connections fail.
        drop(listener);
        respond(stream, &empty_catalog);

        let listener = restart_rx.recv_timeout(Timeout::from_secs(5)).unwrap();
        let (stream, _) = receive(&listener, "GET /v1/tasks");
        respond(stream, &empty_catalog);
        let (stream, resent) = receive(&listener, "POST /v1/tasks");
        let body: CreateTaskRequest = serde_json::from_slice(&resent).unwrap();
        let task = TaskDto {
            id: body.task_id,
            name: body.name,
            archived: false,
            created_at: body.occurred_at,
            updated_at: body.occurred_at,
            latest_work_start: None,
        };
        respond(
            stream,
            &serde_json::to_vec(&MutationDto {
                request_id: body.guard.request_id,
                replayed: false,
                result: MutationResultDto::Task(task.clone()),
            })
            .unwrap(),
        );
        let (stream, _) = receive(&listener, "GET /v1/tasks");
        respond(
            stream,
            &serde_json::to_vec(&TasksDto { tasks: vec![task] }).unwrap(),
        );
        (original, resent)
    });
    let mut client = RemoteApplication::connect(&format!("http://{address}/"))
        .await
        .unwrap();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    assert!(
        client
            .create_task(name("Original draft"), at)
            .await
            .is_err()
    );
    assert!(client.last_write_attempted());
    assert!(
        client
            .create_task(name("Original draft"), at + Duration::seconds(1))
            .await
            .is_err()
    );
    assert!(!client.last_write_attempted());
    let listener = tokio::net::TcpListener::bind(address).await.unwrap();
    restart_tx.send(listener.into_std().unwrap()).unwrap();
    let created = client
        .create_task(name("Original draft"), at + Duration::seconds(2))
        .await
        .unwrap();
    assert_eq!(created.created_at(), at);
    let (original, resent) = server.join().unwrap();
    assert_eq!(original, resent);
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
    second.refresh_task_list().await.unwrap();
    let error = second
        .set_active_task_with_expected_active(
            destination.id(),
            Some(reviewed),
            at + Duration::seconds(3),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.failure().message(),
        "Tracking state changed in another client. Refreshed state."
    );
    assert!(second.last_write_attempted());
    first.refresh_tracking().await.unwrap();
    assert!(
        matches!(first.current_tracking(),tracker_domain::TrackingState::Running {worklog} if worklog.task_id()==concurrent.id())
    );
    let current = active_worklog(&first).as_ref().unwrap().id();
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
    let error = second
        .archive_task_with_expected_name(task.id(), task.name(), at + Duration::seconds(2))
        .await
        .unwrap_err();
    assert_eq!(error.failure().message(), "Task changed in another client");
    assert!(second.last_write_attempted());
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
async fn individual_task_reads_keep_the_catalog_data_separate() {
    let server = TestServer::start();
    let mut first = connected(&server.endpoint()).await.unwrap();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let task = first
        .create_task(name("Catalog observation"), at)
        .await
        .unwrap();
    let mut second = connected(&server.endpoint()).await.unwrap();
    first
        .rename_task(
            task.id(),
            name("Individual observation"),
            at + Duration::seconds(1),
        )
        .await
        .unwrap();
    let dto = second.read_task(task.id()).await.unwrap();
    assert_eq!(dto.task.name, "Individual observation");
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

/// The server rejects a correction of outdated times, and recovery refreshes the affected caches.
#[tokio::test]
async fn outdated_correction_is_rejected_by_the_server_and_recovers_affected_caches() {
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
    assert!(second.last_write_attempted());
    assert!(
        matches!(second.current_tracking(),tracker_domain::TrackingState::Running {worklog} if worklog.task_id()==timer.id())
    );
    assert_eq!(
        task_worklog(&mut second, source.id(), reviewed.id())
            .await
            .unwrap()
            .start(),
        at + Duration::seconds(2)
    );
}

/// The server rejects moves and deletions based on outdated worklog times.
#[tokio::test]
async fn outdated_worklog_moves_and_deletions_are_rejected_by_the_server() {
    use tracker_application::ApplicationFailureCategory;
    let server = TestServer::start();
    let mut first = connected(&server.endpoint()).await.unwrap();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let source = first.create_task(name("Source task"), at).await.unwrap();
    let destination = first
        .create_task(name("Destination task"), at)
        .await
        .unwrap();
    let started = match first
        .set_active_task(source.id(), at + Duration::seconds(1))
        .await
        .unwrap()
    {
        SetActiveTaskOutcome::Started { worklog } => worklog,
        _ => panic!("expected start"),
    };
    let ClearActiveTaskOutcome::Stopped { worklog: reviewed } = first
        .clear_active_task(started.id(), at + Duration::seconds(5))
        .await
        .unwrap()
    else {
        panic!("expected stop");
    };
    let mut second = connected(&server.endpoint()).await.unwrap();
    let corrected = first
        .correct_worklog(
            reviewed.id(),
            reviewed.times(),
            WorklogTimes::new(at + Duration::seconds(2), Some(at + Duration::seconds(5))),
            at + Duration::seconds(6),
        )
        .await
        .unwrap();

    let moved = second
        .move_worklog(
            reviewed.id(),
            source.id(),
            reviewed.times(),
            destination.id(),
        )
        .await
        .unwrap_err();
    assert_eq!(
        moved.failure().category(),
        ApplicationFailureCategory::WorklogChanged
    );
    assert!(second.last_write_attempted());
    let deleted = second
        .delete_completed_worklog(reviewed.id(), source.id(), reviewed.times())
        .await
        .unwrap_err();
    assert_eq!(
        deleted.failure().category(),
        ApplicationFailureCategory::WorklogChanged
    );
    assert!(second.last_write_attempted());

    let stored = task_worklog(&mut first, source.id(), reviewed.id())
        .await
        .unwrap();
    assert_eq!(stored.task_id(), source.id());
    assert_eq!(stored.start(), corrected.start());
}

/// Another client's write to an unrelated task between this client's read and write does not reject it.
#[tokio::test]
async fn unrelated_writes_by_another_client_do_not_reject_commands() {
    let server = TestServer::start();
    let mut client = connected(&server.endpoint()).await.unwrap();
    let mut other = connected(&server.endpoint()).await.unwrap();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let mut step = 0;
    let mut next_at = || {
        step += 1;
        at + Duration::seconds(step)
    };
    let task = client
        .create_task(name("Reviewed task"), next_at())
        .await
        .unwrap();
    let destination = client
        .create_task(name("Destination task"), next_at())
        .await
        .unwrap();
    async fn interfere(other: &mut RemoteApplication, at: DateTime<Utc>) {
        other.create_task(name("Unrelated task"), at).await.unwrap();
    }

    interfere(&mut other, next_at()).await;
    let renamed = client
        .rename_task(task.id(), name("Renamed task"), next_at())
        .await
        .unwrap();
    assert_eq!(renamed.name().as_str(), "Renamed task");

    interfere(&mut other, next_at()).await;
    let started = match client.set_active_task(task.id(), next_at()).await.unwrap() {
        SetActiveTaskOutcome::Started { worklog } => worklog,
        _ => panic!("expected start"),
    };

    interfere(&mut other, next_at()).await;
    let switched_at = next_at();
    let (stopped, running) = match client
        .set_active_task_with_expected_active(destination.id(), Some(started.id()), switched_at)
        .await
        .unwrap()
    {
        SetActiveTaskOutcome::Switched { stopped, started } => (stopped, started),
        _ => panic!("expected switch"),
    };
    assert_eq!(stopped.id(), started.id());

    interfere(&mut other, next_at()).await;
    let cleared_at = next_at();
    assert!(matches!(
        client
            .clear_active_task(running.id(), cleared_at)
            .await
            .unwrap(),
        ClearActiveTaskOutcome::Stopped { .. }
    ));

    interfere(&mut other, next_at()).await;
    let replacement =
        WorklogTimes::new(stopped.start() + Duration::milliseconds(500), stopped.end());
    let corrected = client
        .correct_worklog(stopped.id(), stopped.times(), replacement, next_at())
        .await
        .unwrap();
    assert_eq!(corrected.start(), replacement.start());

    interfere(&mut other, next_at()).await;
    let moved = client
        .move_worklog(stopped.id(), task.id(), corrected.times(), destination.id())
        .await
        .unwrap();
    assert_eq!(moved.task_id(), destination.id());

    interfere(&mut other, next_at()).await;
    let deleted = client
        .delete_completed_worklog(stopped.id(), destination.id(), moved.times())
        .await
        .unwrap();
    assert_eq!(deleted.id(), stopped.id());

    interfere(&mut other, next_at()).await;
    assert!(
        client
            .archive_task(task.id(), next_at())
            .await
            .unwrap()
            .is_archived()
    );

    interfere(&mut other, next_at()).await;
    assert!(
        !client
            .unarchive_task(task.id(), next_at())
            .await
            .unwrap()
            .is_archived()
    );
}

/// Stopping a timer that another client already stopped succeeds as a no-op.
#[tokio::test]
async fn stopping_a_timer_another_client_stopped_reports_already_idle() {
    let server = TestServer::start();
    let mut client = connected(&server.endpoint()).await.unwrap();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let task = client.create_task(name("Shared timer"), at).await.unwrap();
    let started = match client
        .set_active_task(task.id(), at + Duration::seconds(1))
        .await
        .unwrap()
    {
        SetActiveTaskOutcome::Started { worklog } => worklog,
        _ => panic!("expected start"),
    };
    let mut other = connected(&server.endpoint()).await.unwrap();
    other
        .clear_active_task(started.id(), at + Duration::seconds(2))
        .await
        .unwrap();

    let outcome = client
        .clear_active_task(started.id(), at + Duration::seconds(3))
        .await
        .unwrap();

    assert!(matches!(outcome, ClearActiveTaskOutcome::AlreadyIdle));
    assert!(client.last_write_attempted());
    assert!(active_worklog(&client).is_none());
}

/// A write to a server that stopped after connecting fails before delivery, so its failure is certain.
#[tokio::test]
async fn writes_refused_by_a_stopped_server_are_not_attempted() {
    let mut server = TestServer::start();
    let mut client = connected(&server.endpoint()).await.unwrap();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let task = client.create_task(name("Offline task"), at).await.unwrap();
    assert!(client.last_write_attempted());
    server.stop();

    let renamed = client
        .rename_task(task.id(), name("Offline rename"), at + Duration::seconds(1))
        .await
        .unwrap_err();
    assert_eq!(
        renamed.failure().source(),
        tracker_application::ApplicationFailureSource::RemoteUnavailable
    );
    assert!(!client.last_write_attempted());
    assert!(client.last_command_receipt().is_none());
    let started = client
        .set_active_task(task.id(), at + Duration::seconds(2))
        .await
        .unwrap_err();
    assert_eq!(
        started.failure().source(),
        tracker_application::ApplicationFailureSource::RemoteUnavailable
    );
    assert!(!client.last_write_attempted());
    assert_eq!(client.last_failure(), Some(RemoteFailureKind::Unavailable));
    assert_eq!(
        client.task(task.id()).unwrap().name().as_str(),
        "Offline task"
    );
}

fn command_receipt_server(
    tasks: Vec<tracker_protocol::TaskDto>,
    active: Option<tracker_protocol::WorklogDto>,
    invalid_receipt_field: Option<&'static str>,
    result: impl Fn(serde_json::Value) -> serde_json::Value + Send + Sync + 'static,
) -> (
    StubServer,
    std::sync::Arc<std::sync::Mutex<Vec<axum::http::Method>>>,
) {
    use axum::extract::Path;
    use std::sync::{Arc, Mutex};
    use tracker_protocol::{TaskResourceDto, TasksDto, TrackingDto};
    let result = Arc::new(result);
    let write = move |Json(body): Json<serde_json::Value>| {
        let result = result.clone();
        async move {
            let mut receipt = serde_json::json!({
                "request_id": body["request_id"],
                "replayed": false,
                "result": result(body),
            });
            if let Some(field) = invalid_receipt_field {
                receipt[field] = serde_json::json!("another-command");
            }
            Json(receipt)
        }
    };
    let individual_tasks = tasks.clone();
    let aggregate_tasks = tasks.clone();
    let aggregate_active = active.clone();
    let router = Router::new()
        .route(
            "/v1/state/task-list",
            get(move || {
                let dto = TaskListStateDto {
                    tasks: aggregate_tasks.clone(),
                    active_worklog: aggregate_active.clone(),
                };
                async move { Json(dto) }
            }),
        )
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
                async move { Json(TasksDto { tasks }) }
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
                async move { Json(TaskResourceDto { task }) }
            })
            .patch(write.clone()),
        )
        .route(
            "/v1/tracking",
            get(move || {
                let active_worklog = active.clone();
                async move { Json(TrackingDto { active_worklog }) }
            })
            .put(write.clone()),
        )
        .route("/v1/worklogs/{id}", patch(write.clone()).delete(write));
    // Records resource request methods so tests can check that writes skip pre-write reads.
    let requests = Arc::new(Mutex::new(Vec::new()));
    let recorder = requests.clone();
    let router = router.layer(axum::middleware::from_fn(
        move |request: axum::extract::Request, next: axum::middleware::Next| {
            let recorder = recorder.clone();
            async move {
                if request.uri().path() != "/v1/health" {
                    recorder.lock().unwrap().push(request.method().clone());
                }
                next.run(request).await
            }
        },
    ));
    (StubServer::from_router(router), requests)
}

#[tokio::test]
async fn command_receipts_reject_results_that_contradict_the_command() {
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
                "stopped.end",
                "started.id",
                "started.task_id",
                "started.start",
                "started.end",
            ],
        ),
        (Command::AlreadyActive, vec!["id", "task_id", "end"]),
        (Command::Stop, vec!["id", "end"]),
        (Command::Move, vec!["id", "task_id", "start", "end"]),
        (Command::Correct, vec!["id", "start", "end"]),
        (Command::Delete, vec!["id", "task_id", "start", "end"]),
    ];
    for (command, fields) in commands {
        for field in fields.into_iter().chain(["receipt.request_id"]) {
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
            let (server, requests) = command_receipt_server(
                vec![source, target],
                running.then_some(original.clone()),
                field.strip_prefix("receipt."),
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
                    let kind = match command {
                        Command::Create | Command::Rename | Command::Archive | Command::Restore => {
                            "task"
                        }
                        Command::Switch => "tracking_switched",
                        Command::AlreadyActive => "tracking_already_active",
                        _ => "worklog",
                    };
                    if field.starts_with("receipt.") {
                        return json!({"kind":kind,"value":value});
                    }
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
                    json!({"kind":kind,"value":value})
                },
            );
            let mut client = connected(&server.endpoint()).await.unwrap();
            let reads_before_command = requests.lock().unwrap().len();
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
            assert_ne!(
                requests.lock().unwrap()[reads_before_command],
                axum::http::Method::GET,
                "{command:?} {field}"
            );
            assert_eq!(
                error.failure().source(),
                tracker_application::ApplicationFailureSource::RemoteProtocol,
                "{command:?} {field}"
            );
            assert_eq!(client.task_observation().map(<[_]>::len), Some(2));
            assert!(client.tracking_observation().is_some());
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
        MutationDto, MutationResultDto, TaskDto, TrackingDto, WorklogChangeRequest, WorklogDto,
    };
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let task_id = tracker_domain::TaskId::generate();
    let id = WorklogId::generate();
    let committed = Arc::new(AtomicBool::new(false));
    let tasks_committed = committed.clone();
    let command_committed = committed.clone();
    let original = WorklogDto {
        id: id.to_string(),
        task_id: task_id.to_string(),
        start: at,
        end: Some(at + Duration::seconds(10)),
    };
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
            "/v1/state/task-list",
            get(move || {
                let committed = tasks_committed.clone();
                async move {
                    let changed = committed.load(Ordering::SeqCst);
                    (
                        if changed {
                            axum::http::StatusCode::CONFLICT
                        } else {
                            axum::http::StatusCode::OK
                        },
                        Json(TaskListStateDto {
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
                            active_worklog: None,
                        }),
                    )
                }
            }),
        )
        .route(
            "/v1/tracking",
            get(|| async {
                Json(TrackingDto {
                    active_worklog: None,
                })
            }),
        )
        .route(
            "/v1/worklogs/{id}",
            patch(move |Json(body): Json<WorklogChangeRequest>| {
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
                        replayed: false,
                        result: MutationResultDto::Worklog(worklog),
                    })
                }
            }),
        );
    let server = StubServer::from_router(router);
    let mut client = connected(&server.endpoint()).await.unwrap();
    let corrected = client
        .correct_worklog(
            id,
            WorklogTimes::new(at, original.end),
            WorklogTimes::new(at + Duration::seconds(1), original.end),
            at + Duration::seconds(20),
        )
        .await
        .unwrap();
    assert!(committed.load(Ordering::SeqCst));
    assert!(client.last_write_attempted());
    assert_eq!(corrected.start(), at + Duration::seconds(1));
    assert_eq!(client.last_failure(), Some(RemoteFailureKind::Conflict));
    assert!(client.last_command_receipt().is_some());
    assert_eq!(
        client.task(task_id).unwrap().name().as_str(),
        "Confirmed task name"
    );
    assert!(active_worklog(&client).is_none());
}

#[tokio::test]
async fn history_rejects_an_unanchored_cursor() {
    use tracker_protocol::{WorklogCursorDto, WorklogPageDto};
    let requested = tracker_domain::TaskId::generate();
    let dto = WorklogPageDto {
        worklogs: vec![],
        next_cursor: Some(WorklogCursorDto {
            id: WorklogId::generate().to_string(),
            revision: 1,
        }),
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
    assert!(client.task_observation().is_none());
    assert!(client.tracking_observation().is_none());
}

#[tokio::test]
async fn history_rejects_malformed_pages_after_a_valid_read() {
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
    };
    let mut invalid_id = valid.clone();
    invalid_id.worklogs[0].id = "invalid worklog id".into();
    let mut invalid_task = valid.clone();
    invalid_task.worklogs[0].task_id = "invalid task id".into();
    let mut backwards = valid.clone();
    backwards.worklogs[0].end = Some(at - Duration::seconds(1));
    let mut zero = valid.clone();
    zero.worklogs[0].end = Some(at);
    let mut collapsed = valid.clone();
    collapsed.worklogs[0].end = Some(at + Duration::nanoseconds(999));
    let mut wrong_scope = valid.clone();
    wrong_scope.worklogs[0].task_id = tracker_domain::TaskId::generate().to_string();
    let mut invalid_cursor = valid.clone();
    invalid_cursor.next_cursor = Some(WorklogCursorDto {
        id: "invalid cursor id".into(),
        revision: 1,
    });
    let invalid_global_cursor = invalid_cursor.clone();
    let mut negative_cursor = valid.clone();
    negative_cursor.next_cursor = Some(WorklogCursorDto {
        id: valid.worklogs[0].id.clone(),
        revision: -1,
    });
    let mut wrong_anchor = valid.clone();
    wrong_anchor.next_cursor = Some(WorklogCursorDto {
        id: WorklogId::generate().to_string(),
        revision: 0,
    });
    for (global, invalid) in [
        (false, invalid_id),
        (false, invalid_task),
        (false, backwards),
        (false, zero),
        (false, collapsed),
        (false, negative_cursor),
        (false, wrong_anchor),
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
            let page = client.all_worklogs(None).await.unwrap();
            assert_eq!(page.worklogs[0].id().to_string(), valid.worklogs[0].id);
            assert!(client.all_worklogs(None).await.is_err());
        } else {
            let page = client.worklogs_for_task(requested, None).await.unwrap();
            assert_eq!(page.worklogs[0].id().to_string(), valid.worklogs[0].id);
            assert!(client.worklogs_for_task(requested, None).await.is_err());
        }
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
                "/v1/state/task-list",
                get({
                    let current = current.clone();
                    let coherent = coherent.clone();
                    move || {
                        let current = current.clone();
                        let coherent = coherent.clone();
                        async move {
                            (
                                if coherent.load(Ordering::SeqCst) {
                                    axum::http::StatusCode::OK
                                } else {
                                    axum::http::StatusCode::CONFLICT
                                },
                                Json(TaskListStateDto {
                                    tasks: current.lock().unwrap().clone(),
                                    active_worklog: None,
                                }),
                            )
                        }
                    }
                }),
            )
            .route(
                "/v1/tasks",
                get(move || {
                    let current = tasks_current.clone();
                    async move {
                        Json(TasksDto {
                            tasks: current.lock().unwrap().clone(),
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
                            replayed: false,
                            result: MutationResultDto::Task(task),
                        })
                    }
                }),
            )
            .route(
                "/v1/tracking",
                get(|| async {
                    Json(TrackingDto {
                        active_worklog: None,
                    })
                }),
            ),
    );
    let mut client = RemoteApplication::connect(&server.endpoint())
        .await
        .unwrap()
        .with_coherent_task_views();
    client.refresh_task_list().await.unwrap();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let created = client
        .create_task(name("Original creation"), at)
        .await
        .unwrap();
    assert_eq!(client.last_failure(), Some(RemoteFailureKind::Conflict));
    assert!(client.last_command_receipt().is_some());
    assert!(client.last_write_attempted());
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    assert_eq!(client.task_observation(), Some(&[][..]));
    assert_eq!(
        client.tracking_observation(),
        Some(&tracker_domain::TrackingState::Idle)
    );
    let original = current.lock().unwrap()[0].clone();
    coherent.store(true, Ordering::SeqCst);
    client.refresh_task_list().await.unwrap();
    assert_eq!(created.id().to_string(), original.id);
    assert_eq!(created.created_at(), at);
    assert_eq!(created.name().as_str(), "Original creation");
    assert_eq!(
        client.task(created.id()).unwrap().name().as_str(),
        "Original creation"
    );
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    assert_eq!(client.task_items().len(), 1);
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
                                    code: ErrorCode::Conflict,
                                    message: "Task changed in another client".into(),
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

/// The server rejects stopping a replaced timer, and recovery adopts the new coherent pair.
#[tokio::test]
async fn coherent_stop_of_a_replaced_timer_is_rejected_and_adopts_the_new_pair() {
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
    second.refresh_task_list().await.unwrap();
    let new_task = first
        .create_task(name("New timer task"), at + Duration::seconds(2))
        .await
        .unwrap();
    first
        .set_active_task(new_task.id(), at + Duration::seconds(3))
        .await
        .unwrap();

    let error = second
        .clear_active_task(original_timer.id(), at + Duration::seconds(4))
        .await
        .unwrap_err();
    assert_eq!(
        error.failure().category(),
        tracker_application::ApplicationFailureCategory::General
    );
    assert_eq!(
        error.failure().message(),
        "Tracking state changed in another client. Refreshed state."
    );
    assert!(second.last_write_attempted());
    assert_eq!(
        active_worklog(&second).as_ref().unwrap().task_id(),
        new_task.id()
    );
    assert!(
        second
            .task_items()
            .iter()
            .any(|item| item.task.id() == new_task.id())
    );
}

/// A fresh coherent client sends its command; the server rejects the unreviewed timer and recovery adopts a pair.
#[tokio::test]
async fn coherent_rejected_tracking_command_initializes_a_pair_during_recovery() {
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
        assert!(client.task_observation().is_none());
        assert!(client.tracking_observation().is_none());
        let error = if starting {
            client
                .set_active_task_with_expected_active(target.id(), None, at + Duration::seconds(2))
                .await
                .map(|_| ())
        } else {
            client
                .clear_active_task(WorklogId::generate(), at + Duration::seconds(2))
                .await
                .map(|_| ())
        }
        .unwrap_err();
        assert_eq!(
            error.failure().message(),
            "Tracking state changed in another client. Refreshed state."
        );
        assert!(client.last_write_attempted());
        assert!(client.task_observation().is_some());
        assert_eq!(active_worklog(&client).as_ref().unwrap().id(), running.id());
        let active_task_id = active_worklog(&client).as_ref().unwrap().task_id();
        assert!(
            client
                .task_items()
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
    assert!(initial.task_observation().is_some());
    assert_eq!(
        active_worklog(&initial).as_ref().unwrap().task_id(),
        target.id()
    );
}

/// Invalid daily overviews keep the confirmed task list, tracking and totals.
#[tokio::test]
async fn task_list_totals_refreshes_publish_together_and_keep_confirmed_resources_on_failure() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tracker_domain::{TaskId, TrackingState};
    use tracker_protocol::{ReportDto, TaskDto, WorklogDto};
    let mode = Arc::new(AtomicUsize::new(0));
    let reads = Arc::new(AtomicUsize::new(0));
    let response_mode = mode.clone();
    let response_reads = reads.clone();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let task_id = TaskId::generate();
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
                "/v1/state/daily-overview",
                get(move || {
                    let mode = response_mode.load(Ordering::SeqCst);
                    response_reads.fetch_add(1, Ordering::SeqCst);
                    async move {
                        Json(DailyOverviewDto {
                            tasks: if mode == 2 {
                                vec![]
                            } else {
                                vec![TaskDto {
                                    id: task_id.to_string(),
                                    name: "Timer task".into(),
                                    archived: mode == 3,
                                    created_at: at,
                                    updated_at: at,
                                    latest_work_start: Some(at),
                                }]
                            },
                            active_worklog: (mode >= 2).then(|| WorklogDto {
                                id: WorklogId::generate().to_string(),
                                task_id: task_id.to_string(),
                                start: at,
                                end: None,
                            }),
                            totals: ReportDto {
                                start: if mode == 6 {
                                    at - Duration::seconds(1)
                                } else {
                                    at
                                },
                                end: if mode == 7 {
                                    at + Duration::hours(2)
                                } else {
                                    at + Duration::hours(1)
                                },
                                now: if mode == 1 {
                                    at + Duration::seconds(1)
                                } else {
                                    at
                                },
                                rows: vec![],
                                total_us: if mode == 5 { 1 } else { 0 },
                            },
                        })
                    }
                }),
            ),
    );
    let mut client = RemoteApplication::disconnected(&server.endpoint()).unwrap();
    let end = at + Duration::hours(1);
    client
        .refresh_task_list_with_totals(at, end, at)
        .await
        .unwrap();
    assert_eq!(client.task_items()[0].task.id(), task_id);
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(client.tracking_observation(), Some(&TrackingState::Idle));
    assert_eq!(client.report_observation().unwrap().total_us, 0);
    let tasks = client.task_observation().map(<[_]>::to_vec);
    let tracking = client.tracking_observation().cloned();
    let report = client.report_observation().cloned();
    let confirmed = report
        .as_ref()
        .expect("the daily overview publishes a report observation");
    assert_eq!(confirmed.start, at);
    assert_eq!(confirmed.end, at + Duration::hours(1));
    assert_eq!(confirmed.now, at);
    for invalid_mode in [1, 2, 3, 5, 6, 7] {
        mode.store(invalid_mode, Ordering::SeqCst);
        let before = reads.load(Ordering::SeqCst);
        client
            .refresh_task_list_with_totals(at, end, at)
            .await
            .unwrap_err();
        assert_eq!(reads.load(Ordering::SeqCst), before + 1);
        assert_eq!(client.tracking_observation(), tracking.as_ref());
        assert_eq!(client.report_observation(), report.as_ref());
        assert_eq!(client.task_observation(), tasks.as_deref());
    }
    mode.store(4, Ordering::SeqCst);
    client
        .refresh_task_list_with_totals(at, end, at)
        .await
        .unwrap();
    assert_eq!(active_worklog(&client).unwrap().task_id(), task_id);
    assert_eq!(client.task_items()[0].task.id(), task_id);
    assert!(client.report_observation().is_some());
    assert_eq!(client.last_failure(), None);
}

#[tokio::test]
async fn selected_empty_resources_are_loaded_independently() {
    let server = TestServer::start();
    let mut client = RemoteApplication::connect(&server.endpoint())
        .await
        .unwrap();
    client.refresh_tracking().await.unwrap();
    assert!(client.task_observation().is_none());
    assert!(client.tracking_observation().is_some());
    assert!(active_worklog(&client).is_none());
    let tracking = client.tracking_observation().cloned();
    client.refresh_tasks().await.unwrap();
    assert!(client.task_observation().unwrap().is_empty());
    assert_eq!(client.tracking_observation(), tracking.as_ref());
    client.refresh_task_list().await.unwrap();
    assert!(client.task_observation().is_some());
    assert!(client.tracking_observation().is_some());
}

#[tokio::test]
async fn coherent_tracking_command_adopts_the_task_pair_after_a_tracking_only_read() {
    let server = TestServer::start();
    let mut first = connected(&server.endpoint()).await.unwrap();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let task = first.create_task(name("Timer task"), at).await.unwrap();
    let mut client = RemoteApplication::connect(&server.endpoint())
        .await
        .unwrap()
        .with_coherent_task_views();
    client.refresh_tracking().await.unwrap();
    assert!(client.task_observation().is_none());
    let started = client
        .set_active_task_with_expected_active(task.id(), None, at)
        .await
        .unwrap();
    assert!(matches!(started, SetActiveTaskOutcome::Started { .. }));
    assert_eq!(client.task_items()[0].task.id(), task.id());
    assert!(client.tracking_observation().is_some());
}

/// A start without a reviewed timer needs a baseline pair; an invalid or unavailable pair stops it
/// before the write.
#[tokio::test]
async fn coherent_start_without_a_reviewed_timer_rejects_an_unavailable_pair_before_writing() {
    use axum::http::StatusCode;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tracker_domain::TaskId;
    use tracker_protocol::{TaskDto, TaskResourceDto, TrackingDto, WorklogDto};

    for catalog_unavailable in [false, true] {
        let writes = Arc::new(AtomicUsize::new(0));
        let catalog_reads = Arc::new(AtomicUsize::new(0));
        let target_reads = Arc::new(AtomicUsize::new(0));
        let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
        let task = TaskDto {
            id: TaskId::generate().to_string(),
            name: "Timer task".into(),
            archived: false,
            created_at: at,
            updated_at: at,
            latest_work_start: None,
        };
        let target = task.id.parse().unwrap();
        let catalog_task = task.clone();
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
                "/v1/state/task-list",
                get({
                    let reads = catalog_reads.clone();
                    move || {
                        let task = catalog_task.clone();
                        reads.fetch_add(1, Ordering::SeqCst);
                        async move {
                            (
                                if catalog_unavailable {
                                    StatusCode::SERVICE_UNAVAILABLE
                                } else {
                                    StatusCode::OK
                                },
                                // The timer names a task outside the catalog, so the pair is invalid.
                                Json(TaskListStateDto {
                                    tasks: vec![task],
                                    active_worklog: Some(WorklogDto {
                                        id: WorklogId::generate().to_string(),
                                        task_id: TaskId::generate().to_string(),
                                        start: at,
                                        end: None,
                                    }),
                                }),
                            )
                        }
                    }
                }),
            )
            .route(
                "/v1/tasks/{id}",
                get({
                    let reads = target_reads.clone();
                    move || {
                        let task = task.clone();
                        reads.fetch_add(1, Ordering::SeqCst);
                        async move { Json(TaskResourceDto { task }) }
                    }
                }),
            )
            .route(
                "/v1/tracking",
                get(|| async {
                    Json(TrackingDto {
                        active_worklog: None,
                    })
                })
                .put({
                    let writes = writes.clone();
                    move || {
                        writes.fetch_add(1, Ordering::SeqCst);
                        async { StatusCode::INTERNAL_SERVER_ERROR }
                    }
                }),
            );
        let server = StubServer::from_router(router);
        let mut client = RemoteApplication::connect(&server.endpoint())
            .await
            .unwrap()
            .with_coherent_task_views();

        client.set_active_task(target, at).await.unwrap_err();

        assert!(!client.last_write_attempted());
        assert_eq!(writes.load(Ordering::SeqCst), 0);
        assert_eq!(target_reads.load(Ordering::SeqCst), 0);
        assert!(catalog_reads.load(Ordering::SeqCst) > 0);
        assert!(client.task_observation().is_none());
        assert!(client.tracking_observation().is_none());
    }
}

/// Another client's correction of the running start does not reject a stop by worklog id.
#[tokio::test]
async fn coherent_tracking_only_stop_is_not_rejected_by_another_clients_correction() {
    let server = TestServer::start();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let mut other = connected(&server.endpoint()).await.unwrap();
    let task = other.create_task(name("Timer task"), at).await.unwrap();
    let started = match other
        .set_active_task(task.id(), at + Duration::seconds(1))
        .await
        .unwrap()
    {
        SetActiveTaskOutcome::Started { worklog } => worklog,
        _ => panic!("expected a running timer"),
    };
    let mut client = RemoteApplication::connect(&server.endpoint())
        .await
        .unwrap()
        .with_coherent_task_views();
    client.refresh_tracking().await.unwrap();
    assert!(client.task_observation().is_none());
    assert_eq!(active_worklog(&client), Some(started.clone()));
    let corrected = other
        .correct_worklog(
            started.id(),
            started.times(),
            WorklogTimes::new(at + Duration::seconds(2), None),
            at + Duration::seconds(3),
        )
        .await
        .unwrap();

    let outcome = client
        .clear_active_task(started.id(), at + Duration::seconds(4))
        .await
        .unwrap();

    let ClearActiveTaskOutcome::Stopped { worklog } = outcome else {
        panic!("expected the corrected timer to stop");
    };
    assert_eq!(worklog.start(), corrected.start());
    assert_eq!(worklog.end(), Some(at + Duration::seconds(4)));
    assert!(client.last_write_attempted());
    assert!(active_worklog(&client).is_none());
    assert!(client.task_observation().is_some());
    other.refresh_tracking().await.unwrap();
    assert!(active_worklog(&other).is_none());
}

/// Another client's rename of the target task does not reject a start.
#[tokio::test]
async fn coherent_catalog_only_start_is_not_rejected_by_another_clients_rename() {
    let server = TestServer::start();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let mut other = connected(&server.endpoint()).await.unwrap();
    let task = other.create_task(name("Reviewed task"), at).await.unwrap();
    let mut client = RemoteApplication::connect(&server.endpoint())
        .await
        .unwrap()
        .with_coherent_task_views();
    client.refresh_tasks().await.unwrap();
    assert!(client.tracking_observation().is_none());
    assert_eq!(client.task(task.id()), Some(&task));
    let renamed = other
        .rename_task(task.id(), name("Changed task"), at + Duration::seconds(1))
        .await
        .unwrap();

    let outcome = client
        .set_active_task_with_expected_active(task.id(), None, at + Duration::seconds(2))
        .await
        .unwrap();

    assert!(matches!(outcome, SetActiveTaskOutcome::Started { .. }));
    assert!(client.last_write_attempted());
    assert_eq!(client.task(task.id()), Some(&renamed));
    assert!(client.tracking_observation().is_some());
    other.refresh_tracking().await.unwrap();
    assert_eq!(
        active_worklog(&other).as_ref().unwrap().task_id(),
        task.id()
    );
}

#[tokio::test]
#[ignore = "manual loopback measurement; timings are not a CI assertion"]
async fn measure_aggregate_reads_and_intervening_writes() {
    let server = TestServer::start();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let mut writer = connected(&server.endpoint()).await.unwrap();
    for index in 0..100 {
        writer
            .create_task(name(&format!("Measurement task {index:03}")), at)
            .await
            .unwrap();
    }
    let task = writer.task_items()[0].task.clone();
    writer.set_active_task(task.id(), at).await.unwrap();
    let http = reqwest::Client::new();
    let mut interval = reqwest::Url::parse(&server.endpoint()).unwrap();
    interval
        .query_pairs_mut()
        .append_pair("start", &at.to_rfc3339())
        .append_pair("end", &(at + Duration::hours(1)).to_rfc3339())
        .append_pair("now", &(at + Duration::minutes(5)).to_rfc3339());
    let query = format!("?{}", interval.query().unwrap());

    for (label, routes) in [
        (
            "split task list",
            vec!["v1/tasks".to_owned(), "v1/tracking".to_owned()],
        ),
        ("aggregate task list", vec!["v1/state/task-list".to_owned()]),
        (
            "split daily overview",
            vec![
                "v1/tasks".to_owned(),
                "v1/tracking".to_owned(),
                format!("v1/reports/task-totals{query}"),
            ],
        ),
        (
            "aggregate daily overview",
            vec![format!("v1/state/daily-overview{query}")],
        ),
    ] {
        let mut durations = vec![];
        let mut bytes = 0;
        for sample in 0..55 {
            let started = std::time::Instant::now();
            let mut sample_bytes = 0;
            for route in &routes {
                sample_bytes += http
                    .get(format!("{}{route}", server.endpoint()))
                    .send()
                    .await
                    .unwrap()
                    .error_for_status()
                    .unwrap()
                    .bytes()
                    .await
                    .unwrap()
                    .len();
            }
            if sample >= 5 {
                durations.push(started.elapsed());
                bytes = sample_bytes;
            }
        }
        durations.sort();
        println!(
            "{label}: {} requests, {bytes} JSON bytes, p50 {:.3} ms, p95 {:.3} ms, 50 samples after 5 warmups",
            routes.len(),
            durations[25].as_secs_f64() * 1000.0,
            durations[47].as_secs_f64() * 1000.0
        );
    }
    let tasks: tracker_protocol::TasksDto = http
        .get(format!("{}v1/tasks", server.endpoint()))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    writer
        .rename_task(
            task.id(),
            name("Renamed during refresh"),
            at + Duration::seconds(1),
        )
        .await
        .unwrap();
    let tracking: tracker_protocol::TrackingDto = http
        .get(format!("{}v1/tracking", server.endpoint()))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let split_task = tasks
        .tasks
        .iter()
        .find(|item| item.id == task.id().to_string())
        .unwrap();
    assert_eq!(split_task.name, task.name().as_str());
    assert!(tracking.active_worklog.is_some());
    writer.refresh_task_list().await.unwrap();
    assert_eq!(
        writer.task(task.id()).unwrap().name().as_str(),
        "Renamed during refresh"
    );
    println!(
        "An intervening rename left the split catalog stale; the next aggregate returned the renamed task with tracking."
    );
}

#[tokio::test]
async fn aggregate_reads_remain_coherent_while_http_commands_switch_tracking() {
    let server = TestServer::start();
    let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let mut writer = connected(&server.endpoint()).await.unwrap();
    let first = writer.create_task(name("First timer"), at).await.unwrap();
    let second = writer.create_task(name("Second timer"), at).await.unwrap();
    writer.set_active_task(first.id(), at).await.unwrap();
    let mut reader = RemoteApplication::connect(&server.endpoint())
        .await
        .unwrap();
    let write = async {
        for index in 1..=20 {
            let task = if index % 2 == 0 {
                first.id()
            } else {
                second.id()
            };
            writer
                .set_active_task(task, at + Duration::seconds(index))
                .await
                .unwrap();
        }
    };
    let read = async {
        for index in 0..40 {
            if index % 2 == 0 {
                reader.refresh_task_list().await.unwrap();
            } else {
                reader
                    .refresh_task_list_with_totals(
                        at,
                        at + Duration::minutes(1),
                        at + Duration::seconds(30),
                    )
                    .await
                    .unwrap();
            }
            let active = active_worklog(&reader).unwrap();
            let task = reader.task_item(active.task_id()).unwrap();
            assert!(!task.task.is_archived());
            assert_eq!(task.latest_work_start, Some(active.start()));
            if index % 2 == 1 {
                assert_eq!(reader.report_observation().unwrap().total_us, 30_000_000);
            }
        }
    };
    tokio::join!(write, read);
}
