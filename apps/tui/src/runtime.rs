//! One event loop owns terminal input, presentation state, and drawing.
//! Application requests are polled alongside input, so HTTP never holds up a key.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::time::{Duration, Instant};

use crossterm::event::{DisableFocusChange, EnableFocusChange, Event, EventStream};
use crossterm::execute;
use futures_util::StreamExt;
use ratatui::Frame;
use tracker_application::TrackerApplication;
use tracker_remote::{RemoteApplication, RemoteError, RemoteFailureKind};
use tracker_storage::SqliteRepository;

use crate::app::{AppEffect, AppState, Status};
use crate::application_request::{
    ApplicationRequest, CompletedRequest, execute_local, execute_remote,
};
use crate::terminal::TerminalGuard;

const TICK: Duration = Duration::from_millis(250);
const REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const BUSY_DELAY: Duration = Duration::from_millis(100);

pub(crate) enum Backend {
    Local(TrackerApplication<SqliteRepository>),
    Remote(Box<RemoteApplication>),
}

enum ResultKind {
    Request(Box<CompletedRequest>),
    Refresh(Result<(), RemoteError>),
}

type Pending = Pin<Box<dyn Future<Output = (Backend, ResultKind)>>>;

struct FocusGuard;

impl FocusGuard {
    fn enable() -> io::Result<Self> {
        execute!(io::stdout(), EnableFocusChange)?;
        Ok(Self)
    }
}

impl Drop for FocusGuard {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), DisableFocusChange);
    }
}

pub(crate) fn run(guard: &mut TerminalGuard, backend: Backend, state: AppState) -> io::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let _focus = FocusGuard::enable()?;
    runtime.block_on(event_loop(guard, backend, state))
}

async fn event_loop(
    guard: &mut TerminalGuard,
    backend: Backend,
    mut state: AppState,
) -> io::Result<()> {
    let mut backend = Some(backend);
    let mut pending: Option<Pending> = None;
    let mut active_effect: Option<AppEffect> = None;
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let remote_mode = matches!(backend, Some(Backend::Remote(_)));
    let mut refresh_requested = remote_mode;
    let mut last_refresh = Instant::now();
    let mut busy: Option<(&'static str, Instant)> = None;
    let mut busy_visible = false;

    draw(guard, &state, None)?;
    loop {
        if should_exit(&state) {
            break;
        }
        if pending.is_none() {
            if let Some(effect) = state.take_effect() {
                let request = effect.request.clone();
                busy = Some((busy_label(&request), Instant::now()));
                busy_visible = false;
                active_effect = Some(effect);
                pending = Some(start_request(
                    backend.take().expect("backend is idle"),
                    request,
                ));
            } else if state.is_running()
                && refresh_requested
                && let Some(Backend::Remote(remote)) = backend.take()
            {
                busy = Some(("Loading...", Instant::now()));
                busy_visible = false;
                pending = Some(Box::pin(async move {
                    let mut remote = remote;
                    let result = remote.refresh_task_list().await;
                    (Backend::Remote(remote), ResultKind::Refresh(result))
                }));
                refresh_requested = false;
                last_refresh = Instant::now();
            }
        }

        let busy_deadline = pending_busy_deadline(busy, busy_visible);
        tokio::select! {
            event = events.next(), if state.is_running() => {
                let event = event.ok_or_else(|| io::Error::other("terminal event stream stopped"))??;
                match event {
                    Event::Key(key) => {
                        if let Some(command) = state.command_for(key)
                            && super::command_is_allowed(command, crossterm::terminal::size()?.0)
                        {
                            state.handle_command(command);
                            draw(guard, &state, visible_busy_label(busy, Instant::now()))?;
                        }
                    }
                    Event::Resize(_, _) => {
                        draw(guard, &state, visible_busy_label(busy, Instant::now()))?;
                    }
                    Event::FocusGained if remote_mode => refresh_requested = true,
                    _ => {}
                }
            }
            result = async { pending.as_mut().expect("pending request").await }, if pending.is_some() => {
                let (returned_backend, result) = result;
                backend = Some(returned_backend);
                pending = None;
                busy = None;
                busy_visible = false;
                match result {
                    ResultKind::Request(completed) => {
                        let effect = active_effect.take().expect("request has a completion");
                        publish_request_resources(&mut state, backend.as_ref().expect("request returned backend"), &completed);
                        state.complete_effect(effect, *completed);
                        if matches!(backend, Some(Backend::Remote(ref remote)) if remote.last_failure() == Some(RemoteFailureKind::Unavailable)) {
                            state.shell_mut().error("Server unavailable");
                        }
                    }
                    ResultKind::Refresh(result) => {
                        let remote = match backend.as_ref().expect("refresh returned backend") {
                            Backend::Remote(remote) => remote,
                            Backend::Local(_) => unreachable!("only remote backends refresh"),
                        };
                        apply_refresh(&mut state, remote, result);
                    }
                }
                draw(guard, &state, None)?;
            }
            _ = tick.tick() => {
                state.expire_copy_confirmation();
                state.refresh_reports();
                if remote_mode && last_refresh.elapsed() >= REFRESH_INTERVAL {
                    refresh_requested = true;
                }
                draw(guard, &state, visible_busy_label(busy, Instant::now()))?;
            }
            _ = wait_until(busy_deadline), if busy_deadline.is_some() => {
                busy_visible = true;
                draw(guard, &state, visible_busy_label(busy, Instant::now()))?;
            }
        }
    }
    Ok(())
}

fn should_exit(state: &AppState) -> bool {
    !state.is_running() && !state.active_request_is_write() && !state.has_queued_write()
}

fn start_request(backend: Backend, request: ApplicationRequest) -> Pending {
    Box::pin(async move {
        let mut backend = backend;
        let completed = match &mut backend {
            Backend::Local(application) => execute_local(application, request),
            Backend::Remote(application) => execute_remote(application, request).await,
        };
        if completed.request.needs_task_list()
            && completed.request != ApplicationRequest::RefreshTaskList
        {
            match &mut backend {
                Backend::Local(application) => {
                    let _ = tracker_application::TaskQueries::refresh_task_list(application);
                }
                Backend::Remote(application) => {
                    let _ = application.refresh_task_list().await;
                }
            }
        }
        (backend, ResultKind::Request(Box::new(completed)))
    })
}

fn publish_request_resources(
    state: &mut AppState,
    backend: &Backend,
    completed: &CompletedRequest,
) {
    use tracker_application::{TaskOrdering, TaskQueries, TrackingOperations};
    match backend {
        Backend::Local(application) => state.publish_request_resources(
            completed,
            application.tasks(TaskOrdering::default()),
            Some(application.current_tracking().clone()),
        ),
        Backend::Remote(application) => {
            if completed.request.needs_task_list() {
                if let (Some(tasks), Some(tracking)) = (
                    application.task_observation(),
                    application.tracking_observation(),
                ) && tasks.revision == tracking.revision
                {
                    state.publish_request_resources(
                        completed,
                        tasks.value.clone(),
                        Some(tracking.value.clone()),
                    );
                }
            } else {
                let items = selected_remote_task_items(application, completed);
                state.publish_request_resources(
                    completed,
                    items,
                    application
                        .tracking_observation()
                        .map(|tracking| tracking.value.clone()),
                );
            }
        }
    }
}

fn selected_remote_task_items(
    application: &RemoteApplication,
    completed: &CompletedRequest,
) -> Vec<tracker_application::TaskListItem> {
    let ids = completed.metadata_task_ids(None);
    ids.into_iter()
        .filter_map(|id| application.task_item(id).cloned())
        .collect()
}

fn apply_refresh(
    state: &mut AppState,
    remote: &RemoteApplication,
    result: Result<(), RemoteError>,
) {
    match result {
        Ok(()) => {
            if let (Some(tasks), Some(tracking)) =
                (remote.task_observation(), remote.tracking_observation())
                && tasks.revision == tracking.revision
            {
                state.sync_task_list(tasks.value.clone(), tracking.value.clone(), false);
            }
            if matches!(state.shell().status(), Status::Info(message) if message == "Connecting to server...")
                || matches!(state.shell().status(), Status::Error(message) if message == "Server unavailable")
            {
                state.shell_mut().clear_status();
            }
        }
        Err(error) if error.is_unavailable() => state.shell_mut().error("Server unavailable"),
        Err(_) => state.shell_mut().error("Server protocol error"),
    }
}

async fn wait_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
        None => std::future::pending().await,
    }
}

fn pending_busy_deadline(
    busy: Option<(&'static str, Instant)>,
    busy_visible: bool,
) -> Option<Instant> {
    busy.filter(|_| !busy_visible)
        .map(|(_, started)| started + BUSY_DELAY)
}

fn visible_busy_label(busy: Option<(&'static str, Instant)>, now: Instant) -> Option<&'static str> {
    busy.and_then(|(label, started)| {
        (now.saturating_duration_since(started) >= BUSY_DELAY).then_some(label)
    })
}

fn draw(guard: &mut TerminalGuard, state: &AppState, busy_label: Option<&str>) -> io::Result<()> {
    guard.draw(|frame| render_frame(frame, state, busy_label))?;
    Ok(())
}

fn render_frame(frame: &mut Frame<'_>, state: &AppState, busy_label: Option<&str>) {
    crate::ui::render_with_busy(frame, state.app_view(), busy_label);
}

fn busy_label(request: &ApplicationRequest) -> &'static str {
    if request.is_write() {
        "Saving..."
    } else {
        "Loading..."
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use ratatui::{Terminal, backend::TestBackend};
    use tracker_application::TaskListItem;
    use tracker_domain::{ActiveWorklog, Task, TrackingState};
    use tracker_remote::{RemoteApplication, RemoteError};

    use super::{
        Backend, ResultKind, apply_refresh, busy_label, pending_busy_deadline,
        publish_request_resources, render_frame, should_exit, start_request, visible_busy_label,
        wait_until,
    };
    use crate::app::{AppState, Status};
    use crate::application_request::remote_resource_tests::stoppable_resource_server;
    use crate::application_request::{ApplicationOutcome, ApplicationRequest, CompletedRequest};
    use crate::command::Command;
    use crate::test_support::{at, task, worklog_id};

    fn confirmed_state() -> (AppState, Task, ActiveWorklog) {
        let cached = task(2, "Previously confirmed task");
        let running = ActiveWorklog::begin(worklog_id(2), cached.id(), at(100));
        let state = AppState::load_task_list(
            vec![TaskListItem {
                task: cached.clone(),
                latest_work_start: Some(at(100)),
            }],
            TrackingState::Running {
                worklog: running.clone(),
            },
        );
        (state, cached, running)
    }

    fn task_list_responses(task_revision: &str, tracking_revision: &str) -> Vec<(u16, String)> {
        vec![
            (
                200,
                format!(r#"{{"tasks":[],"revision":"{task_revision}"}}"#),
            ),
            (
                200,
                format!(r#"{{"active_worklog":null,"revision":"{tracking_revision}"}}"#),
            ),
        ]
    }

    #[test]
    fn exit_gate_waits_for_quit_and_accepted_writes_but_allows_pending_reads() {
        let mut idle = AppState::load_task_list(Vec::new(), TrackingState::Idle);
        assert!(!should_exit(&idle));
        idle.handle_command(Command::Quit);
        assert!(should_exit(&idle));

        let mut reading = AppState::load_task_list(Vec::new(), TrackingState::Idle);
        let read = ApplicationRequest::AllWorklogs { after: None };
        assert!(reading.enqueue(read.clone(), |_, _| {}));
        let _ = reading.take_effect().unwrap();
        reading.handle_command(Command::Quit);
        assert!(should_exit(&reading));

        let mut state = AppState::load_task_list(Vec::new(), TrackingState::Idle);
        assert!(state.enqueue(read.clone(), |_, _| {}));
        let active_read = state.take_effect().unwrap();
        let created = task(1, "Accepted task");
        let write = ApplicationRequest::CreateTask {
            name: created.name().clone(),
            occurred_at: at(100),
        };
        assert!(state.enqueue(write.clone(), |_, _| {}));
        state.handle_command(Command::Quit);
        assert!(!should_exit(&state));
        state.complete_effect(
            active_read,
            CompletedRequest {
                request: read,
                outcome: ApplicationOutcome::GlobalWorklogPage(Err(
                    tracker_application::ApplicationError::InvalidReportRange,
                )),
            },
        );
        assert!(!should_exit(&state));
        let active_write = state.take_effect().unwrap();
        assert!(!should_exit(&state));
        state.complete_effect(
            active_write,
            CompletedRequest {
                request: write,
                outcome: ApplicationOutcome::Task(Ok(created)),
            },
        );
        assert!(should_exit(&state));
    }

    #[tokio::test]
    async fn remote_history_publishes_labels_without_refreshing_unrelated_resources() {
        let history_task = task(1, "History task");
        let task_id = history_task.id();
        let log_id = worklog_id(1);
        let mut responses = vec![
            (200, r#"{"status":"ok","protocol_version":3}"#.to_owned()),
            (
                200,
                format!(
                    r#"{{"worklogs":[{{"id":"{log_id}","task_id":"{task_id}","start":"{}","end":"{}"}}],"next_cursor":null,"revision":"history-1"}}"#,
                    at(100).to_rfc3339(),
                    at(150).to_rfc3339(),
                ),
            ),
            (
                200,
                format!(
                    r#"{{"task":{{"id":"{task_id}","name":"History task","archived":false,"created_at":"{}","updated_at":"{}","latest_work_start":null}},"revision":"task-2"}}"#,
                    at(100).to_rfc3339(),
                    at(100).to_rfc3339(),
                ),
            ),
        ];
        responses.extend(task_list_responses("state-1", "state-1"));
        let (endpoint, worker, stop) = stoppable_resource_server(responses);
        let application = RemoteApplication::disconnected(&endpoint).unwrap();
        let (backend, result) = start_request(
            Backend::Remote(Box::new(application)),
            ApplicationRequest::AllWorklogs { after: None },
        )
        .await;
        let _ = stop.send(());
        let paths = worker.join().unwrap();
        let ResultKind::Request(completed) = result else {
            panic!("history returns a request completion");
        };
        assert!(matches!(
            completed.outcome,
            ApplicationOutcome::GlobalWorklogPage(Ok(_))
        ));
        let Backend::Remote(application) = &backend else {
            panic!("a remote history request retains its backend");
        };
        assert!(application.task_observation().is_none());
        assert!(application.tracking_observation().is_none());
        assert!(application.last_failure().is_none());
        let (mut state, cached, running) = confirmed_state();

        publish_request_resources(&mut state, &backend, &completed);

        assert_eq!(state.catalog().task(task_id), Some(&history_task));
        assert_eq!(state.catalog().task(cached.id()), Some(&cached));
        assert_eq!(state.tracking().active_worklog(), Some(&running));
        assert_eq!(
            paths,
            vec![
                "/v1/health".to_owned(),
                "/v1/worklogs".to_owned(),
                format!("/v1/tasks/{task_id}")
            ]
        );
    }

    #[tokio::test]
    async fn explicit_remote_refresh_reads_each_resource_once_and_publishes_a_coherent_pair() {
        let mut responses = vec![(200, r#"{"status":"ok","protocol_version":3}"#.to_owned())];
        responses.extend(task_list_responses("state-1", "state-1"));
        responses.extend(task_list_responses("state-1", "state-1"));
        let (endpoint, worker, stop) = stoppable_resource_server(responses);
        let application = RemoteApplication::disconnected(&endpoint).unwrap();
        let (backend, result) = start_request(
            Backend::Remote(Box::new(application)),
            ApplicationRequest::RefreshTaskList,
        )
        .await;
        let _ = stop.send(());
        let paths = worker.join().unwrap();
        let ResultKind::Request(completed) = result else {
            panic!("an explicit refresh returns its request completion");
        };
        assert!(matches!(
            completed.outcome,
            ApplicationOutcome::TaskListRefresh(Ok(()))
        ));
        let (mut state, cached, _) = confirmed_state();

        publish_request_resources(&mut state, &backend, &completed);

        assert!(state.catalog().task(cached.id()).is_none());
        assert!(state.tracking().active_worklog().is_none());
        assert_eq!(paths, vec!["/v1/health", "/v1/tasks", "/v1/tracking"]);
    }

    #[tokio::test]
    async fn failed_remote_refresh_does_not_publish_independently_loaded_mismatched_resources() {
        let mut responses = vec![(200, r#"{"status":"ok","protocol_version":3}"#.to_owned())];
        for _ in 0..3 {
            responses.extend(task_list_responses("tasks-1", "tracking-2"));
        }
        let (endpoint, worker, stop) = stoppable_resource_server(responses);
        let mut application = RemoteApplication::disconnected(&endpoint).unwrap();
        application.refresh_tasks().await.unwrap();
        application.refresh_tracking().await.unwrap();
        let (backend, result) = start_request(
            Backend::Remote(Box::new(application)),
            ApplicationRequest::RefreshTaskList,
        )
        .await;
        let _ = stop.send(());
        let paths = worker.join().unwrap();
        let ResultKind::Request(completed) = result else {
            panic!("a failed refresh returns its request completion");
        };
        assert!(matches!(
            completed.outcome,
            ApplicationOutcome::TaskListRefresh(Err(_))
        ));
        let Backend::Remote(application) = &backend else {
            panic!("a failed remote refresh retains its backend");
        };
        assert_ne!(application.task_revision(), application.tracking_revision());
        let (mut state, cached, running) = confirmed_state();

        publish_request_resources(&mut state, &backend, &completed);

        assert_eq!(state.catalog().task(cached.id()), Some(&cached));
        assert_eq!(state.tracking().active_worklog(), Some(&running));
        assert_eq!(
            paths,
            vec![
                "/v1/health",
                "/v1/tasks",
                "/v1/tracking",
                "/v1/tasks",
                "/v1/tracking",
                "/v1/tasks",
                "/v1/tracking"
            ]
        );
    }

    #[test]
    fn a_busy_refresh_does_not_overlap_an_existing_error_status() {
        let mut state = AppState::load_task_list(Vec::new(), TrackingState::Idle);
        state.shell_mut().error("Server unavailable");
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

        terminal
            .draw(|frame| render_frame(frame, &state, Some("Loading...")))
            .unwrap();

        let status: String = (0..80)
            .map(|x| terminal.backend().buffer()[(x, 22)].symbol())
            .collect();
        assert!(
            status.starts_with("Error: Server unavailable"),
            "{status:?}"
        );
        assert!(!status.contains("Loading..."), "{status:?}");
    }

    #[test]
    fn a_busy_refresh_uses_the_empty_status_row_and_leaves_the_footer_intact() {
        let state = AppState::load_task_list(Vec::new(), TrackingState::Idle);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

        terminal
            .draw(|frame| render_frame(frame, &state, Some("Loading...")))
            .unwrap();

        let status: String = (0..80)
            .map(|x| terminal.backend().buffer()[(x, 22)].symbol())
            .collect();
        let footer: String = (0..80)
            .map(|x| terminal.backend().buffer()[(x, 23)].symbol())
            .collect();
        assert!(status.starts_with("Loading..."), "{status:?}");
        assert!(footer.contains("quit"), "{footer:?}");
    }

    #[test]
    fn a_busy_refresh_does_not_cover_the_small_terminal_warning() {
        let state = AppState::load_task_list(Vec::new(), TrackingState::Idle);
        let mut terminal = Terminal::new(TestBackend::new(40, 2)).unwrap();

        terminal
            .draw(|frame| render_frame(frame, &state, Some("Loading...")))
            .unwrap();

        let warning: String = (0..40)
            .map(|x| terminal.backend().buffer()[(x, 0)].symbol())
            .collect();
        assert!(
            warning.starts_with("Time Tracker needs at least"),
            "{warning:?}"
        );
        assert!(!warning.contains("Loading..."), "{warning:?}");
    }

    #[test]
    fn busy_label_appears_after_the_short_delay() {
        let started = Instant::now();
        assert_eq!(pending_busy_deadline(None, false), None);
        assert_eq!(
            pending_busy_deadline(Some(("Loading...", started)), false),
            Some(started + Duration::from_millis(100))
        );
        assert_eq!(
            pending_busy_deadline(Some(("Loading...", started)), true),
            None
        );
        assert_eq!(
            visible_busy_label(Some(("Loading...", started)), started),
            None
        );
        assert_eq!(
            visible_busy_label(
                Some(("Loading...", started)),
                started + Duration::from_millis(100)
            ),
            Some("Loading...")
        );
    }

    #[tokio::test]
    async fn busy_deadline_waits_and_an_unarmed_deadline_never_fires() {
        let started = Instant::now();
        wait_until(Some(started + Duration::from_millis(25))).await;
        assert!(started.elapsed() >= Duration::from_millis(25));
        assert!(
            tokio::time::timeout(Duration::from_millis(20), wait_until(None))
                .await
                .is_err()
        );
    }

    #[test]
    fn writes_and_reads_use_distinct_busy_labels() {
        assert_eq!(
            busy_label(&ApplicationRequest::AllWorklogs { after: None }),
            "Loading..."
        );
        assert_eq!(
            busy_label(&ApplicationRequest::ArchiveTask {
                expected_name: tracker_domain::TaskName::new("Reviewed task").unwrap(),
                id: tracker_domain::TaskId::generate(),
                occurred_at: chrono::Utc::now(),
            }),
            "Saving..."
        );
    }

    #[test]
    fn refresh_clears_connection_status_and_reports_wire_failures() {
        let remote = RemoteApplication::disconnected("http://127.0.0.1:1/").unwrap();
        let mut state = AppState::load_task_list(Vec::new(), TrackingState::Idle);
        state.shell_mut().info("Connecting to server...");

        apply_refresh(&mut state, &remote, Ok(()));
        assert_eq!(state.shell().status(), &Status::Empty);

        apply_refresh(
            &mut state,
            &remote,
            Err(RemoteError::Unavailable("offline".into())),
        );
        assert_eq!(
            state.shell().status(),
            &Status::Error("Server unavailable".into())
        );

        apply_refresh(&mut state, &remote, Ok(()));
        assert_eq!(state.shell().status(), &Status::Empty);

        apply_refresh(
            &mut state,
            &remote,
            Err(RemoteError::Protocol("bad version".into())),
        );
        assert_eq!(
            state.shell().status(),
            &Status::Error("Server protocol error".into())
        );
    }
}
