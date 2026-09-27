//! Remote TUI runtime. HTTP calls stay on a worker while terminal input and
//! drawing stay on the main thread.

use std::io;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use crossterm::event::{DisableFocusChange, EnableFocusChange, Event, EventStream, KeyEvent};
use crossterm::execute;
use futures_util::StreamExt;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Paragraph;
use tokio::sync::mpsc as async_mpsc;
use tracker_remote::{RemoteApplication, RemoteFailureKind};

use crate::app::{App, Status};
use crate::command::Command;
use crate::screens::{AllWorklogsCommand, TaskListCommand, WorklogHistoryCommand};
use crate::terminal::TerminalGuard;

const TICK: Duration = Duration::from_millis(250);
const REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const BUSY_DELAY: Duration = Duration::from_millis(100);
// Bound frames if terminal drawing stalls; the UI drains queued frames before drawing.
const WORKER_OUTPUT_CAPACITY: usize = 8;

enum WorkerInput {
    Key(KeyEvent, u16),
    Resize(u16, u16),
    Refresh,
    Quit,
}

enum WorkerOutput {
    Busy(&'static str),
    Frame { buffer: Buffer, running: bool },
}

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

/// Runs the TUI with a remote application service. A stalled request cannot
/// block terminal reads or redraws, and the last frame remains visible.
pub(crate) fn run(guard: &mut TerminalGuard, application: RemoteApplication) -> io::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()?;
    let _focus = FocusGuard::enable()?;
    let (width, height) = crossterm::terminal::size()?;
    let (input_tx, input_rx) = mpsc::channel();
    let (output_tx, output_rx) = async_mpsc::channel(WORKER_OUTPUT_CAPACITY);
    let worker = thread::spawn(move || worker(application, input_rx, output_tx, width, height));

    let result = runtime.block_on(input_loop(guard, &input_tx, output_rx, (width, height)));
    let _ = input_tx.send(WorkerInput::Quit);
    drop(input_tx);
    worker
        .join()
        .map_err(|_| io::Error::other("remote TUI worker stopped unexpectedly"))?;
    result
}

async fn input_loop(
    guard: &mut TerminalGuard,
    input: &Sender<WorkerInput>,
    mut output: async_mpsc::Receiver<WorkerOutput>,
    mut size: (u16, u16),
) -> io::Result<()> {
    let mut terminal_events = EventStream::new();
    let mut last_frame = None;
    let mut busy = None;
    let mut busy_visible = false;
    let mut running = true;

    draw(guard, last_frame.as_ref(), None)?;
    while running {
        let busy_deadline = pending_busy_deadline(busy, busy_visible);
        tokio::select! {
            event = terminal_events.next() => {
                let event = event.ok_or_else(|| io::Error::other("terminal event stream stopped"))??;
                if let Event::Resize(width, height) = &event {
                    size = (*width, *height);
                    draw(guard, last_frame.as_ref(), visible_busy_label(busy, Instant::now()))?;
                }
                if let Some(event) = event_to_worker_input(event, size.0) {
                    input
                        .send(event)
                        .map_err(|_| io::Error::other("remote TUI worker stopped"))?;
                }
            }
            event = output.recv() => {
                let event = event.ok_or_else(|| io::Error::other("remote TUI worker stopped"))?;
                apply_output(event, &mut last_frame, &mut busy, &mut busy_visible, &mut running);
                while let Ok(event) = output.try_recv() {
                    apply_output(event, &mut last_frame, &mut busy, &mut busy_visible, &mut running);
                }
                let current_size = crossterm::terminal::size()?;
                if running && size != current_size {
                    size = current_size;
                    input
                        .send(WorkerInput::Resize(size.0, size.1))
                        .map_err(|_| io::Error::other("remote TUI worker stopped"))?;
                }
                draw(guard, last_frame.as_ref(), visible_busy_label(busy, Instant::now()))?;
            }
            _ = wait_for_busy(busy_deadline) => {
                busy_visible = true;
                draw(guard, last_frame.as_ref(), visible_busy_label(busy, Instant::now()))?;
            }
        }
    }
    Ok(())
}

fn apply_output(
    event: WorkerOutput,
    last_frame: &mut Option<Buffer>,
    busy: &mut Option<(&'static str, Instant)>,
    busy_visible: &mut bool,
    running: &mut bool,
) {
    match event {
        WorkerOutput::Busy(label) => {
            *busy = Some((label, Instant::now()));
            *busy_visible = false;
        }
        WorkerOutput::Frame {
            buffer,
            running: next,
        } => {
            *last_frame = Some(buffer);
            *busy = None;
            *busy_visible = false;
            *running = next;
        }
    }
}

async fn wait_for_busy(deadline: Option<Instant>) {
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

fn draw(
    guard: &mut TerminalGuard,
    last_frame: Option<&Buffer>,
    busy_label: Option<&str>,
) -> io::Result<()> {
    guard.draw(|frame| {
        if let Some(buffer) = last_frame
            && buffer.area == frame.area()
        {
            frame.buffer_mut().clone_from(buffer);
        } else {
            frame.render_widget(Paragraph::new("Connecting to server..."), frame.area());
        }
        if let Some(label) = busy_label {
            let area = frame.area();
            let status = Rect::new(0, area.height.saturating_sub(2), area.width, 1);
            frame.render_widget(Paragraph::new(label), status);
        }
    })?;
    Ok(())
}

fn event_to_worker_input(event: Event, width: u16) -> Option<WorkerInput> {
    match event {
        Event::Key(key) => Some(WorkerInput::Key(key, width)),
        Event::FocusGained => Some(WorkerInput::Refresh),
        Event::Resize(width, height) => Some(WorkerInput::Resize(width, height)),
        _ => None,
    }
}

fn visible_busy_label(busy: Option<(&'static str, Instant)>, now: Instant) -> Option<&'static str> {
    busy.and_then(|(label, started)| {
        (now.saturating_duration_since(started) >= BUSY_DELAY).then_some(label)
    })
}

fn worker(
    application: RemoteApplication,
    input: Receiver<WorkerInput>,
    output: async_mpsc::Sender<WorkerOutput>,
    mut width: u16,
    mut height: u16,
) {
    let mut app = App::load(application);
    app.shell_mut().info("Connecting to server...");
    send_frame(&app, &output, width, height);
    let _ = output.blocking_send(WorkerOutput::Busy("Loading..."));
    refresh(&mut app);
    send_frame(&app, &output, width, height);
    let mut last_refresh = Instant::now();

    while app.is_running() {
        match input.recv_timeout(TICK) {
            Ok(WorkerInput::Key(key, terminal_width)) => {
                if let Some(command) = app.command_for(key)
                    && super::command_is_allowed(command, terminal_width)
                {
                    let _ = output.blocking_send(WorkerOutput::Busy(command_busy_label(command)));
                    app.handle(command);
                    app.sync_from_application(false);
                    if app.application_mut().last_failure() == Some(RemoteFailureKind::Unavailable)
                    {
                        app.shell_mut().error("Server unavailable");
                    }
                }
            }
            Ok(WorkerInput::Resize(next_width, next_height)) => {
                width = next_width;
                height = next_height;
            }
            Ok(WorkerInput::Refresh) => {
                let _ = output.blocking_send(WorkerOutput::Busy("Loading..."));
                refresh(&mut app);
                last_refresh = Instant::now();
            }
            Ok(WorkerInput::Quit) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if refresh_due(last_refresh, Instant::now()) {
            let _ = output.blocking_send(WorkerOutput::Busy("Loading..."));
            refresh(&mut app);
            last_refresh = Instant::now();
        }
        app.expire_copy_confirmation();
        app.refresh_reports();
        send_frame(&app, &output, width, height);
    }
}

fn refresh_due(last_refresh: Instant, now: Instant) -> bool {
    now.saturating_duration_since(last_refresh) >= REFRESH_INTERVAL
}

fn refresh(app: &mut App<RemoteApplication>) {
    match app.application_mut().refresh() {
        Ok(()) => {
            app.sync_from_application(false);
            if matches!(
                app.shell().status(),
                Status::Info(message) if message == "Connecting to server..."
            ) || matches!(
                app.shell().status(),
                Status::Error(message) if message == "Server unavailable"
            ) {
                app.shell_mut().clear_status();
            }
        }
        Err(error) if error.is_unavailable() => app.shell_mut().error("Server unavailable"),
        Err(_) => app.shell_mut().error("Server protocol error"),
    }
}

fn send_frame(
    app: &App<RemoteApplication>,
    output: &async_mpsc::Sender<WorkerOutput>,
    width: u16,
    height: u16,
) {
    let mut terminal = Terminal::new(TestBackend::new(width.max(1), height.max(1)))
        .expect("test backend must be available");
    terminal
        .draw(|frame| crate::ui::render(frame, app.app_view()))
        .expect("test backend draw must succeed");
    let _ = output.blocking_send(WorkerOutput::Frame {
        buffer: terminal.backend().buffer().clone(),
        running: app.is_running(),
    });
}

fn command_busy_label(command: Command) -> &'static str {
    match command {
        Command::TaskList(
            TaskListCommand::ToggleTracking
            | TaskListCommand::UnarchiveSelected
            | TaskListCommand::Confirm,
        )
        | Command::WorklogHistory(WorklogHistoryCommand::Confirm)
        | Command::AllWorklogs(AllWorklogsCommand::ConfirmMove) => "Saving...",
        _ => "Loading...",
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::{Duration, Instant};

    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use tracker_remote::RemoteApplication;

    use super::{
        WorkerInput, command_busy_label, event_to_worker_input, pending_busy_deadline, refresh,
        refresh_due, visible_busy_label,
    };
    use crate::app::{App, Status};
    use crate::command::Command;
    use crate::screens::{AllWorklogsCommand, TaskListCommand, WorklogHistoryCommand};

    #[test]
    fn write_commands_show_saving_while_navigation_shows_loading() {
        for command in [
            Command::TaskList(TaskListCommand::ToggleTracking),
            Command::TaskList(TaskListCommand::UnarchiveSelected),
            Command::TaskList(TaskListCommand::Confirm),
            Command::WorklogHistory(WorklogHistoryCommand::Confirm),
            Command::AllWorklogs(AllWorklogsCommand::ConfirmMove),
        ] {
            assert_eq!(command_busy_label(command), "Saving...");
        }
        for command in [
            Command::TaskList(TaskListCommand::MoveDown),
            Command::WorklogHistory(WorklogHistoryCommand::RefreshWorklogs),
            Command::AllWorklogs(AllWorklogsCommand::Refresh),
            Command::Quit,
        ] {
            assert_eq!(command_busy_label(command), "Loading...");
        }
    }

    #[test]
    fn failed_initial_refresh_shows_server_unavailable() {
        let application = RemoteApplication::disconnected("http://127.0.0.1:0/").unwrap();
        let mut app = App::load(application);
        app.shell_mut().info("Connecting to server...");

        refresh(&mut app);

        assert!(
            matches!(app.shell().status(), Status::Error(message) if message == "Server unavailable")
        );
        assert!(app.application_mut().snapshot().task_items.is_empty());
    }

    #[test]
    fn incompatible_server_refresh_shows_protocol_error() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "refresh did not contact the server"
                        );
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("server accept failed: {error}"),
                }
            };
            let mut request = [0_u8; 1024];
            let read = stream.read(&mut request).unwrap();
            assert!(request[..read].starts_with(b"GET /v1/health "));
            let body = br#"{"status":"ok","protocol_version":2}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
        });
        let application = RemoteApplication::disconnected(&endpoint).unwrap();
        let mut app = App::load(application);
        refresh(&mut app);
        server.join().unwrap();

        assert!(
            matches!(app.shell().status(), Status::Error(message) if message == "Server protocol error")
        );
    }

    #[test]
    fn busy_label_appears_only_after_the_short_delay() {
        let started = Instant::now();
        assert_eq!(visible_busy_label(None, started), None);
        assert_eq!(
            visible_busy_label(Some(("Saving...", started)), started),
            None
        );
        assert_eq!(
            visible_busy_label(
                Some(("Saving...", started)),
                started + Duration::from_millis(99)
            ),
            None
        );
        assert_eq!(
            visible_busy_label(
                Some(("Saving...", started)),
                started + Duration::from_millis(100)
            ),
            Some("Saving...")
        );
        assert_eq!(
            visible_busy_label(
                Some(("Saving...", started)),
                started + Duration::from_millis(101)
            ),
            Some("Saving...")
        );
    }

    #[test]
    fn busy_deadline_is_armed_once_and_cancelled_when_visible() {
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
    }

    #[test]
    fn terminal_events_forward_the_right_worker_messages() {
        let key = KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE);
        assert!(matches!(
            event_to_worker_input(Event::Key(key), 83),
            Some(WorkerInput::Key(received, 83)) if received == key
        ));
        assert!(matches!(
            event_to_worker_input(Event::FocusGained, 83),
            Some(WorkerInput::Refresh)
        ));
        assert!(matches!(
            event_to_worker_input(Event::Resize(104, 32), 83),
            Some(WorkerInput::Resize(104, 32))
        ));
        assert!(event_to_worker_input(Event::FocusLost, 83).is_none());
    }

    #[test]
    fn periodic_refresh_becomes_due_at_one_second() {
        let last = Instant::now();
        assert!(!refresh_due(last, last));
        assert!(!refresh_due(last, last + Duration::from_millis(999)));
        assert!(refresh_due(last, last + Duration::from_secs(1)));
        assert!(refresh_due(last, last + Duration::from_millis(1001)));
        assert!(!refresh_due(last + Duration::from_secs(1), last));
    }
}
