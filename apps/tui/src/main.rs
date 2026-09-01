//! The `tt` binary: wires the app, keymap, terminal, and UI together.
//!
//! Startup failures print one concise line to stderr and exit nonzero.
//! Runtime storage failures stay in the status line; terminal failures
//! propagate after the terminal guard has restored the screen.

mod app;
mod command;
mod keymap;
mod terminal;
mod ui;

use std::error::Error;
use std::io;
use std::process::ExitCode;
use std::time::Duration;

use crossterm::event::Event;
use tracker_core::{Task, TaskId, TaskName, TrackerRepository};
use tracker_storage::{SqliteRepository, StorageError, default_database_path, ensure_app_data_dir};

use crate::app::App;
use crate::terminal::TerminalGuard;

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
    terminal::install_panic_hook(terminal::force_restore);
    ensure_app_data_dir()?;
    let repository = SqliteRepository::open(default_database_path()?)?;
    seed_if_empty(&repository)?;
    let mut app = App::load(repository)?;
    let mut guard = TerminalGuard::new()?;
    run(&mut guard, &mut app).map_err(Into::into)
}

/// The synchronous event loop: redraw, then handle at most one key per tick.
///
/// The guard restores the terminal on drop, for normal quits and for errors
/// alike.
fn run(guard: &mut TerminalGuard, app: &mut App) -> io::Result<()> {
    while app.is_running() {
        guard.draw(|frame| ui::render(frame, app))?;
        if crossterm::event::poll(TICK)?
            && let Event::Key(key) = crossterm::event::read()?
            && let Some(command) = keymap::map(app.mode(), key)
        {
            app.handle(command);
        }
    }
    Ok(())
}

/// Seeds a brand-new empty database with the default task list, once.
///
/// A database that already has tasks, archived or not, is left untouched.
fn seed_if_empty(repository: &SqliteRepository) -> Result<(), StorageError> {
    if !repository.list_tasks()?.is_empty() {
        return Ok(());
    }
    for name in SEED_TASK_NAMES {
        let task_name =
            TaskName::new(name).map_err(|_| StorageError::CorruptData("seed task name"))?;
        repository.create_task(Task::new(TaskId::generate(), task_name))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository() -> SqliteRepository {
        SqliteRepository::open_in_memory().unwrap()
    }

    #[test]
    fn a_new_database_is_seeded_with_exactly_the_default_tasks() {
        let repository = repository();
        seed_if_empty(&repository).unwrap();
        let names: Vec<String> = repository
            .list_tasks()
            .unwrap()
            .into_iter()
            .map(|task| task.name.to_string())
            .collect();
        assert_eq!(
            names,
            [
                "Write release notes".to_owned(),
                "Fix the coffee machine".to_owned(),
                "Plan Friday's demo".to_owned()
            ]
        );

        // Seeding again adds nothing: it happens exactly once.
        seed_if_empty(&repository).unwrap();
        assert_eq!(repository.list_tasks().unwrap().len(), 3);
    }

    #[test]
    fn an_existing_database_is_never_reseeded() {
        let repository = repository();
        let task = Task::new(TaskId::generate(), TaskName::new("mine").unwrap());
        repository.create_task(task).unwrap();
        seed_if_empty(&repository).unwrap();
        let names: Vec<String> = repository
            .list_tasks()
            .unwrap()
            .into_iter()
            .map(|task| task.name.to_string())
            .collect();
        assert_eq!(names, ["mine".to_owned()]);
    }

    #[test]
    fn archived_tasks_also_prevent_reseeding() {
        let repository = repository();
        let mut task = Task::new(TaskId::generate(), TaskName::new("mine").unwrap());
        task.archived = true;
        repository.create_task(task).unwrap();
        seed_if_empty(&repository).unwrap();
        assert_eq!(repository.list_tasks().unwrap().len(), 1);
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
