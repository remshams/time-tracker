//! The `tt` binary: wires the app, terminal, and UI together.
//!
//! Startup failures print one concise line to stderr and exit nonzero.
//! Runtime storage failures stay in the status line; terminal failures
//! propagate after the terminal guard has restored the screen.

mod app;
mod command;
mod components;
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
use std::time::Duration;

use chrono::{DateTime, Utc};
use crossterm::event::Event;
use tracker_application::{TrackerApplication, TrackerApplicationService};
use tracker_domain::TaskName;
use tracker_storage::{SqliteRepository, StorageError, default_database_path, ensure_app_data_dir};

use crate::app::App;
use crate::command::Command;
#[cfg(test)]
use crate::screens::{TaskListCommand, WorklogHistoryCommand};
use crate::terminal::{Restoration, TerminalGuard};

/// The tasks a brand-new database is seeded with, in order.
const SEED_TASK_NAMES: [&str; 3] = [
    "Write release notes",
    "Fix the coffee machine",
    "Plan Friday's demo",
];

/// How long to wait for input before redrawing, so the elapsed timer stays
/// fresh without burning CPU.
const TICK: Duration = Duration::from_millis(250);

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
    // The guard and the panic hook share one restoration state, so a panic
    // cleanup and the guard's drop never both write to the terminal.
    let restoration = Restoration::new();
    terminal::install_panic_hook({
        let restoration = restoration.clone();
        move || restoration.restore()
    });
    ensure_app_data_dir()?;
    let repository = SqliteRepository::open(default_database_path()?)?;
    seed_default_tasks(&repository, Utc::now())?;
    let application = TrackerApplication::load(repository)?;
    let mut app = App::load(application);
    let mut guard = TerminalGuard::new(restoration)?;
    run(&mut guard, &mut app).map_err(Into::into)
}

/// The synchronous event loop: redraw, then handle at most one key per tick.
///
/// The guard restores the terminal on drop, for normal quits and for errors
/// alike.
fn run<S: TrackerApplicationService>(
    guard: &mut TerminalGuard,
    app: &mut App<S>,
) -> io::Result<()> {
    while app.is_running() {
        app.refresh_reports();
        guard.draw(|frame| ui::render(frame, app.app_view()))?;
        if crossterm::event::poll(TICK)?
            && let Event::Key(key) = crossterm::event::read()?
            && let Some(command) = app.command_for(key)
            && command_is_allowed(command, crossterm::terminal::size()?.0)
        {
            app.handle(command);
        }
    }
    Ok(())
}

/// Below the supported width only quitting may change application state.
fn command_is_allowed(command: Command, terminal_width: u16) -> bool {
    terminal_width >= ui::MIN_TERMINAL_WIDTH || command == Command::Quit
}

/// Seeds a brand-new empty database with the default task list, once.
///
/// The repository checks emptiness and inserts inside one immediate
/// transaction, so a database that already has tasks, archived or not, is
/// left untouched, concurrent starts cannot seed twice, and a failure leaves
/// no partial seed.
fn seed_default_tasks(
    repository: &SqliteRepository,
    created_at: DateTime<Utc>,
) -> Result<(), StorageError> {
    let names: Vec<TaskName> = SEED_TASK_NAMES
        .iter()
        .map(|name| TaskName::new(name).expect("seed task names are valid"))
        .collect();
    repository.seed_default_tasks(&names, created_at)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracker_domain::{Task, TaskId};

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).unwrap()
    }

    fn repository() -> SqliteRepository {
        SqliteRepository::open_in_memory().unwrap()
    }

    #[test]
    fn a_new_database_is_seeded_with_exactly_the_default_tasks() {
        let repository = repository();
        seed_default_tasks(&repository, at(100)).unwrap();
        let tasks = repository.list_tasks().unwrap();
        let names: Vec<String> = tasks.iter().map(|task| task.name().to_string()).collect();
        assert_eq!(
            names,
            [
                "Write release notes".to_owned(),
                "Fix the coffee machine".to_owned(),
                "Plan Friday's demo".to_owned()
            ]
        );
        assert!(
            tasks
                .iter()
                .all(|task| task.created_at() == at(100) && task.updated_at() == at(100))
        );

        // Seeding again adds nothing: it happens exactly once.
        seed_default_tasks(&repository, at(200)).unwrap();
        assert_eq!(repository.list_tasks().unwrap().len(), 3);
    }

    #[test]
    fn an_existing_database_is_never_reseeded() {
        let repository = repository();
        let task = Task::create(TaskId::generate(), TaskName::new("mine").unwrap(), at(50));
        repository.create_task(task).unwrap();
        seed_default_tasks(&repository, at(100)).unwrap();
        let names: Vec<String> = repository
            .list_tasks()
            .unwrap()
            .into_iter()
            .map(|task| task.name().to_string())
            .collect();
        assert_eq!(names, ["mine".to_owned()]);
    }

    #[test]
    fn archived_tasks_also_prevent_reseeding() {
        let repository = repository();
        let mut task = Task::create(TaskId::generate(), TaskName::new("mine").unwrap(), at(50));
        assert!(task.archive(at(50)));
        repository.create_task(task).unwrap();
        seed_default_tasks(&repository, at(100)).unwrap();
        assert_eq!(repository.list_tasks().unwrap().len(), 1);
    }

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
}
