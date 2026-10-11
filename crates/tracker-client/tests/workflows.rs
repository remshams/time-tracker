use std::{sync::mpsc, thread};

use chrono::{DateTime, TimeDelta, Utc};
use tempfile::TempDir;
use tokio::sync::oneshot;
use tracker_application::{
    ClearActiveTaskOutcome, SetActiveTaskOutcome, TaskOrdering, TrackerApplication,
};
use tracker_client::TrackerBackend;
use tracker_domain::{InactivityPeriod, TaskName, WorklogTimes};
use tracker_remote::ResourceSelection;
use tracker_storage::SqliteRepository;

struct Server {
    _directory: TempDir,
    endpoint: String,
    shutdown: Option<oneshot::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Server {
    fn start() -> Self {
        let directory = TempDir::new().unwrap();
        let database = directory.path().join("tracker.sqlite3");
        let router = tracker_server::router_for_database(&database).unwrap();
        Self::with_router(directory, router)
    }

    fn with_router(directory: TempDir, router: axum::Router) -> Self {
        let (address_tx, address_rx) = mpsc::channel();
        let (shutdown, stopped) = oneshot::channel();
        let thread = thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                    address_tx.send(listener.local_addr().unwrap()).unwrap();
                    axum::serve(listener, router)
                        .with_graceful_shutdown(async {
                            let _ = stopped.await;
                        })
                        .await
                        .unwrap();
                });
        });
        Self {
            _directory: directory,
            endpoint: format!("http://{}/", address_rx.recv().unwrap()),
            shutdown: Some(shutdown),
            thread: Some(thread),
        }
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

fn local() -> TrackerBackend {
    TrackerBackend::Local(
        TrackerApplication::load(SqliteRepository::open_in_memory().unwrap()).unwrap(),
    )
}

fn at(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(1_800_000_000 + seconds, 0).unwrap()
}
fn name(value: &str) -> TaskName {
    TaskName::new(value).unwrap()
}

#[tokio::test]
async fn selected_backends_preserve_task_tracking_history_and_report_workflows() {
    let server = Server::start();
    for mut backend in [local(), TrackerBackend::remote(&server.endpoint).unwrap()] {
        let remote = backend.is_remote();
        assert_eq!(backend.remote_application().is_some(), remote);
        backend.check_connection().await.unwrap();
        backend.refresh_task_list().await.unwrap();
        let first = backend
            .create_task(name("First project"), at(0))
            .await
            .unwrap();
        let second = backend
            .create_task(name("Second project"), at(1))
            .await
            .unwrap();
        assert_eq!(backend.command_receipt().is_some(), remote);
        let renamed = backend
            .rename_task(first.id(), name("Renamed project"), at(2))
            .await
            .unwrap();
        assert_eq!(renamed.name(), &name("Renamed project"));
        let renamed = backend
            .rename_task_with_expected_name(
                first.id(),
                renamed.name(),
                name("Reviewed project"),
                at(3),
            )
            .await
            .unwrap();
        assert_eq!(renamed.name(), &name("Reviewed project"));
        assert_eq!(backend.read_task(first.id()).await.unwrap().task, renamed);
        assert_eq!(
            backend.move_candidates(first.id(), "Second")[0].id,
            second.id()
        );

        let SetActiveTaskOutcome::Started { worklog: started } = backend
            .start_tracking_if_idle(first.id(), at(10))
            .await
            .unwrap()
        else {
            panic!("tracking should start");
        };
        assert_eq!(
            match backend.current_tracking() {
                tracker_domain::TrackingState::Running { worklog } => worklog.id(),
                _ => panic!("tracking must be running"),
            },
            started.id()
        );
        assert!(matches!(
            backend.set_active_task(first.id(), at(11)).await.unwrap(),
            SetActiveTaskOutcome::AlreadyActive { .. }
        ));
        let SetActiveTaskOutcome::Switched {
            started: second_active,
            stopped,
        } = backend
            .set_active_task_with_expected_active(second.id(), Some(started.id()), at(20))
            .await
            .unwrap()
        else {
            panic!("tracking should switch");
        };
        assert_eq!(stopped.id(), started.id());
        assert_eq!(stopped.end(), Some(at(20)));
        let ClearActiveTaskOutcome::Stopped { worklog: completed } = backend
            .clear_active_task(second_active.id(), at(30))
            .await
            .unwrap()
        else {
            panic!("tracking should stop");
        };
        assert_eq!(completed.end(), Some(at(30)));

        let page = backend.worklogs_for_task(first.id(), None).await.unwrap();
        assert_eq!(page.worklogs, vec![stopped.clone()]);
        assert_eq!(backend.all_worklogs(None).await.unwrap().worklogs.len(), 2);
        let corrected = backend
            .correct_worklog(
                stopped.id(),
                stopped.times(),
                WorklogTimes::new(at(12), Some(at(18))),
                at(31),
            )
            .await
            .unwrap();
        assert_eq!(corrected.times(), WorklogTimes::new(at(12), Some(at(18))));
        let moved = backend
            .move_worklog(corrected.id(), first.id(), corrected.times(), second.id())
            .await
            .unwrap();
        assert_eq!(moved.task_id(), second.id());
        assert!(
            backend
                .worklogs_for_task(first.id(), None)
                .await
                .unwrap()
                .worklogs
                .is_empty()
        );
        let report = backend.report(at(0), at(40), at(35)).await.unwrap();
        assert_eq!(report.total, TimeDelta::seconds(16));
        assert_eq!(report.now, at(35));
        assert_eq!(report.rows.len(), 1);
        assert_eq!(report.rows[0].task_id, second.id());
        assert_eq!(report.rows[0].duration, TimeDelta::seconds(16));
        assert_eq!(report.revision.is_some(), remote);
        let totals = backend.report_totals(at(0), at(40), at(35)).await.unwrap();
        assert_eq!(totals.rows[0].task.id(), second.id());
        assert_eq!(totals.rows[0].duration, report.rows[0].duration);
        assert_eq!(totals.total, report.total);
        for include_tasks in [false, true] {
            assert_eq!(
                backend
                    .refresh_totals(include_tasks, at(0), at(40), at(35))
                    .await
                    .unwrap()
                    .total,
                report.total
            );
        }
        backend
            .delete_completed_worklog(moved.id(), second.id(), moved.times())
            .await
            .unwrap();
        assert_eq!(
            backend.all_worklogs(None).await.unwrap().worklogs,
            vec![completed]
        );

        let archived = backend.archive_task(first.id(), at(41)).await.unwrap();
        assert!(archived.is_archived());
        assert!(
            !backend
                .unarchive_task(first.id(), at(42))
                .await
                .unwrap()
                .is_archived()
        );
        assert!(
            backend
                .archive_task_with_expected_name(first.id(), &name("Reviewed project"), at(43))
                .await
                .unwrap()
                .is_archived()
        );
        backend
            .refresh_resources(ResourceSelection::Tasks)
            .await
            .unwrap();
        backend
            .refresh_resources(ResourceSelection::Tracking)
            .await
            .unwrap();
        assert_eq!(backend.tasks(TaskOrdering::RecentlyCreated).len(), 2);
        let (items, revision) = backend.task_observation().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(revision.is_some(), remote);
        let (tracking, revision) = backend.tracking_observation().unwrap();
        assert!(matches!(tracking, tracker_domain::TrackingState::Idle));
        assert_eq!(revision.is_some(), remote);
    }
}

#[tokio::test]
async fn inactivity_confirmation_keeps_the_reviewed_period_and_candidates() {
    let server = Server::start();
    let now = DateTime::from_timestamp(Utc::now().timestamp(), 0).unwrap();
    for mut backend in [local(), TrackerBackend::remote(&server.endpoint).unwrap()] {
        backend.refresh_task_list().await.unwrap();
        let old = backend
            .create_task(name("Old project"), now - TimeDelta::days(20))
            .await
            .unwrap();
        let recent = backend
            .create_task(name("Recent project"), now - TimeDelta::days(1))
            .await
            .unwrap();
        let period = InactivityPeriod::new(7).unwrap();
        let preview = backend.preview_inactive_tasks(now, period).await.unwrap();
        assert_eq!(preview.as_of(), now);
        assert_eq!(preview.days(), 7);
        assert_eq!(backend.archive_inactive_tasks(preview).await.unwrap(), 1);
        assert!(backend.task(old.id()).unwrap().is_archived());
        assert!(!backend.task(recent.id()).unwrap().is_archived());
    }
}

#[tokio::test]
async fn incompatible_server_protocol_is_reported_without_adopting_state() {
    let router = axum::Router::new().route(
        "/v1/health",
        axum::routing::get(|| async {
            axum::Json(tracker_protocol::HealthDto {
                status: "ok".to_owned(),
                protocol_version: tracker_protocol::VERSION + 1,
            })
        }),
    );
    let server = Server::with_router(TempDir::new().unwrap(), router);
    let mut backend = TrackerBackend::remote(&server.endpoint).unwrap();
    let error = backend
        .refresh_task_list()
        .await
        .unwrap_err()
        .into_application_error();
    assert!(matches!(
        error,
        tracker_application::ApplicationError::RemoteProtocol(_)
    ));
    assert!(backend.is_remote());
    assert!(backend.task_observation().is_none());
    assert!(backend.tracking_observation().is_none());
    assert!(backend.tasks(TaskOrdering::default()).is_empty());
}

#[tokio::test]
async fn remote_disconnect_keeps_the_selected_backend_and_last_confirmed_state() {
    let mut server = Server::start();
    let mut backend = TrackerBackend::remote(&server.endpoint).unwrap();
    assert!(backend.task_observation().is_none());
    assert!(backend.tracking_observation().is_none());
    backend.refresh_task_list().await.unwrap();
    let task = backend
        .create_task(name("Server project"), at(0))
        .await
        .unwrap();
    let confirmed = backend.task_observation().unwrap();
    server.stop();
    let error = backend
        .refresh_task_list()
        .await
        .unwrap_err()
        .into_application_error();
    assert!(matches!(
        error,
        tracker_application::ApplicationError::RemoteUnavailable(_)
    ));
    assert!(backend.is_remote());
    assert_eq!(backend.task_observation().unwrap(), confirmed);
    assert!(
        backend
            .create_task(name("Offline project"), at(1))
            .await
            .is_err()
    );
    assert_eq!(backend.task(task.id()).unwrap(), &task);
    assert_eq!(backend.tasks(TaskOrdering::default()).len(), 1);
}
