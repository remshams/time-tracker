//! The `tt` binary: wires the app, terminal, and UI together.
//!
//! Startup failures print one concise line to stderr and exit nonzero.
//! Runtime storage failures stay in the status line; terminal failures
//! propagate after the terminal guard has restored the screen.

mod app;
mod application_request;
mod cli;
mod command;
mod components;
mod runtime;
mod screens;
mod styles;
mod support;
mod terminal;
#[cfg(test)]
mod test_support;
mod ui;
#[cfg(test)]
mod ui_tests;

use std::error::Error;
use std::io;
use std::process::ExitCode;

use tracker_application::TrackerApplication;
use tracker_storage::{SqliteRepository, default_database_path, ensure_app_data_dir};

use crate::app::{App, AppState};
use crate::command::Command;
use crate::runtime::Backend;
#[cfg(test)]
use crate::screens::{TaskListCommand, WorklogHistoryCommand};
use crate::terminal::{Restoration, TerminalGuard};

fn main() -> ExitCode {
    report(run_app())
}

/// Runs the application, mapping any failure to one process exit code.
fn report(result: Result<(), Box<dyn Error>>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("tt: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_app() -> Result<(), Box<dyn Error>> {
    match cli::parse(std::env::args_os().skip(1))? {
        cli::Mode::Local => run_local_tui(),
        cli::Mode::Remote { server } => run_remote_tui(&server),
        cli::Mode::Serve { bind, database } => run_server(bind, database),
        cli::Mode::Help => {
            print!("{}", cli::HELP);
            Ok(())
        }
    }
}

fn run_server(
    bind: std::net::SocketAddr,
    database: Option<std::path::PathBuf>,
) -> Result<(), Box<dyn Error>> {
    let database = match database {
        Some(path) => path,
        None => {
            ensure_app_data_dir()?;
            default_database_path()?.with_file_name("tt-server.db")
        }
    };
    tracker_server::run(bind, database).map_err(|error| error as Box<dyn Error>)
}

fn run_local_tui() -> Result<(), Box<dyn Error>> {
    ensure_app_data_dir()?;
    let repository = SqliteRepository::open(default_database_path()?)?;
    let application = TrackerApplication::load(repository)?;
    let app = App::load(application);
    let (application, state) = app.into_parts();
    let mut guard = terminal_guard()?;
    runtime::run(&mut guard, Backend::Local(application), state).map_err(Into::into)
}

fn run_remote_tui(server: &str) -> Result<(), Box<dyn Error>> {
    let application = tracker_remote::RemoteApplication::disconnected(server)?;
    let mut state = AppState::load_from_snapshot(
        application.tasks(tracker_application::TaskOrdering::default()),
        application.current_tracking().clone(),
    );
    state.shell_mut().info("Connecting to server...");
    let mut guard = terminal_guard()?;
    runtime::run(&mut guard, Backend::Remote(application), state).map_err(Into::into)
}

fn terminal_guard() -> io::Result<TerminalGuard> {
    // The guard and panic hook share restoration state, so only one restores
    // the terminal after a failure.
    let restoration = Restoration::new();
    terminal::install_panic_hook({
        let restoration = restoration.clone();
        move || restoration.restore()
    });
    TerminalGuard::new(restoration)
}

/// Below the supported width only quitting may change application state.
fn command_is_allowed(command: Command, terminal_width: u16) -> bool {
    terminal_width >= ui::MIN_TERMINAL_WIDTH || command == Command::Quit
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_too_narrow_terminal_allows_only_quitting() {
        assert!(command_is_allowed(Command::Quit, 1));
        assert!(!command_is_allowed(
            Command::TaskList(TaskListCommand::OpenAdd),
            59
        ));
        assert!(!command_is_allowed(
            Command::TaskList(TaskListCommand::CycleOrdering),
            59
        ));
        assert!(!command_is_allowed(
            Command::TaskList(TaskListCommand::OpenHistory),
            59
        ));
        assert!(!command_is_allowed(
            Command::WorklogHistory(WorklogHistoryCommand::LoadOlderWorklogs),
            59
        ));
        assert!(!command_is_allowed(
            Command::WorklogHistory(WorklogHistoryCommand::RefreshWorklogs),
            59
        ));
        assert!(command_is_allowed(
            Command::TaskList(TaskListCommand::OpenAdd),
            60
        ));
        assert!(command_is_allowed(
            Command::TaskList(TaskListCommand::OpenHistory),
            60
        ));
    }

    #[test]
    fn a_success_reports_success() {
        assert_eq!(report(Ok(())), ExitCode::SUCCESS);
    }

    #[test]
    fn a_failure_prints_one_line_and_fails() {
        let error: Box<dyn Error> = "something broke".into();
        assert_eq!(report(Err(error)), ExitCode::FAILURE);
    }

    #[test]
    fn server_startup_reports_an_invalid_database_path() {
        let directory = tempfile::tempdir().unwrap();
        let bind = "127.0.0.1:0".parse().unwrap();
        let error = run_server(bind, Some(directory.path().to_owned())).unwrap_err();
        assert!(!error.to_string().is_empty());
    }
}
