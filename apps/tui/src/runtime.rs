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
    ApplicationRequest, ApplicationSnapshot, CompletedRequest, execute_local, execute_remote,
};
use crate::terminal::TerminalGuard;

const TICK: Duration = Duration::from_millis(250);
const REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const BUSY_DELAY: Duration = Duration::from_millis(100);

pub(crate) enum Backend {
    Local(TrackerApplication<SqliteRepository>),
    Remote(RemoteApplication),
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
        if !state.is_running() && !state.active_request_is_write() && !state.has_queued_write() {
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
                    let result = remote.refresh().await;
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

fn start_request(backend: Backend, request: ApplicationRequest) -> Pending {
    Box::pin(async move {
        let mut backend = backend;
        let completed = match &mut backend {
            Backend::Local(application) => execute_local(application, request),
            Backend::Remote(application) => execute_remote(application, request).await,
        };
        (backend, ResultKind::Request(Box::new(completed)))
    })
}

fn apply_refresh(
    state: &mut AppState,
    remote: &RemoteApplication,
    result: Result<(), RemoteError>,
) {
    match result {
        Ok(()) => {
            let snapshot = ApplicationSnapshot::from_remote(remote);
            state.sync_from_snapshot(snapshot.items, snapshot.tracking, false);
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
    use tracker_domain::TrackingState;
    use tracker_remote::{RemoteApplication, RemoteError};

    use super::{
        apply_refresh, busy_label, pending_busy_deadline, render_frame, visible_busy_label,
        wait_until,
    };
    use crate::app::{AppState, Status};
    use crate::application_request::ApplicationRequest;

    #[test]
    fn a_busy_refresh_does_not_overlap_an_existing_error_status() {
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
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
        let state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
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
        let state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
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
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
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
