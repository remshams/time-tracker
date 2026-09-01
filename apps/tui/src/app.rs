//! TUI state and the commands that change it.
//!
//! [`App`] owns the visible task list, the tracker, the selection, the modal
//! mode, and the status line. Commands arrive already translated from raw
//! keys. Every state change that persists applies to a cloned candidate
//! first: the in-memory state is replaced only after the repository write
//! succeeds, so a storage failure can never desynchronize memory and SQLite.

use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use tracker_core::{
    Task, TaskId, TaskName, Tracker, TrackerRepository, TrackingError, TrackingOutcome,
};
use tracker_storage::{SqliteRepository, StorageError};

use crate::command::Command;

/// Why the TUI state could not be loaded from storage.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// Reading tasks or entries failed.
    #[error(transparent)]
    Storage(#[from] StorageError),
    /// The recovered active entry cannot resume tracking.
    #[error(transparent)]
    Tracking(#[from] TrackingError),
}

/// What the TUI is currently asking of the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// The plain task list.
    Normal,
    /// A one-line text input is open.
    Input {
        /// What the confirmed text will do.
        purpose: InputPurpose,
        /// The text typed so far.
        buffer: String,
    },
    /// An archive confirmation is open.
    ConfirmArchive {
        /// The task that will be archived on confirmation.
        task_id: TaskId,
        /// The task name, kept for the dialog text and status message.
        name: String,
    },
}

/// What a confirmed text input does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputPurpose {
    /// Create a new task with the typed name.
    Add,
    /// Rename the task with the given identifier.
    Rename { task_id: TaskId },
}

/// The most recent message shown in the status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// A normal outcome, shown in the default color.
    Info(String),
    /// A failure, shown in the error color.
    Error(String),
}

/// The event loop's two lifecycle states.
///
/// Only a quit command moves the state to [`Lifecycle::Quitting`], and no
/// command ever moves it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    /// The event loop keeps running.
    Running,
    /// The event loop should exit.
    Quitting,
}

/// Derives the displayed elapsed time from a monotonic clock.
///
/// The visible timer is anchored to the elapsed duration measured in UTC when
/// the process builds its running state. Afterwards it advances with
/// [`Instant`], so wall-clock adjustments never make the timer jump.
#[derive(Debug, Clone)]
pub(crate) struct ElapsedClock {
    anchor: Instant,
    base: Duration,
}

impl ElapsedClock {
    /// Anchors the clock to an entry that started at `start` in UTC.
    pub(crate) fn since(start: DateTime<Utc>) -> Self {
        Self::anchored(Self::base_since(start, Utc::now()))
    }

    /// Builds a clock whose elapsed time is already `base` when created.
    pub(crate) fn anchored(base: Duration) -> Self {
        Self {
            anchor: Instant::now(),
            base,
        }
    }

    /// The elapsed duration the anchor represents at `now`, clamped at zero
    /// for a start that lies in the future.
    fn base_since(start: DateTime<Utc>, now: DateTime<Utc>) -> Duration {
        (now - start).to_std().unwrap_or(Duration::ZERO)
    }

    /// The elapsed duration `since_anchor` after the anchor instant.
    pub(crate) fn at(&self, since_anchor: Duration) -> Duration {
        self.base + since_anchor
    }

    /// The current elapsed duration.
    pub(crate) fn elapsed(&self) -> Duration {
        self.at(self.anchor.elapsed())
    }
}

/// The task-list interface state.
pub struct App {
    repository: SqliteRepository,
    /// The visible tasks: every non-archived task, ordered by identifier.
    tasks: Vec<Task>,
    tracker: Tracker,
    /// The selected row, absent for an empty list. Always either absent or an
    /// existing index into `tasks`.
    selected: Option<usize>,
    mode: Mode,
    status: Status,
    /// The monotonic clock for the active entry, present while tracking.
    clock: Option<ElapsedClock>,
    lifecycle: Lifecycle,
}

impl App {
    /// Loads the interface state from the repository.
    ///
    /// Archived tasks stay hidden. An active entry recovered from storage
    /// resumes running and keeps running when the application exits.
    pub fn load(repository: SqliteRepository) -> Result<Self, LoadError> {
        let stored = repository.list_tasks()?;
        let tasks: Vec<Task> = stored.into_iter().filter(|task| !task.archived).collect();
        let (tracker, clock) = match repository.active_entry()? {
            Some(entry) => {
                let clock = ElapsedClock::since(entry.start);
                (Tracker::resume(entry)?, Some(clock))
            }
            None => (Tracker::idle(), None),
        };
        let status = match tracker.active() {
            Some(_) => Status::Info("Recovered the previous active timer".to_owned()),
            None => Status::Info("Ready".to_owned()),
        };
        let selected = (!tasks.is_empty()).then_some(0);
        Ok(Self {
            repository,
            tasks,
            tracker,
            selected,
            mode: Mode::Normal,
            status,
            clock,
            lifecycle: Lifecycle::Running,
        })
    }

    /// Applies one semantic command.
    pub fn handle(&mut self, command: Command) {
        match command {
            Command::MoveUp => self.move_up(),
            Command::MoveDown => self.move_down(),
            Command::ToggleTracking => self.toggle_tracking(),
            Command::OpenAdd => self.open_add(),
            Command::OpenRename => self.open_rename(),
            Command::OpenArchiveConfirm => self.open_archive_confirm(),
            Command::Confirm => self.confirm(),
            Command::Cancel => self.mode = Mode::Normal,
            Command::Insert(character) => {
                if let Mode::Input { buffer, .. } = &mut self.mode {
                    buffer.push(character);
                }
            }
            Command::Backspace => {
                if let Mode::Input { buffer, .. } = &mut self.mode {
                    buffer.pop();
                }
            }
            Command::Quit => self.lifecycle = Lifecycle::Quitting,
            Command::Reserved => {}
        }
    }

    /// Whether the event loop should keep running.
    pub fn is_running(&self) -> bool {
        self.lifecycle == Lifecycle::Running
    }

    /// The visible (non-archived) tasks, ordered by identifier.
    pub fn tasks(&self) -> &[Task] {
        &self.tasks
    }

    /// The selected row index, absent for an empty list.
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// The current mode.
    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    /// The most recent status message.
    pub fn status(&self) -> &Status {
        &self.status
    }

    /// The identifier of the active entry's task, if tracking runs.
    pub fn active_task_id(&self) -> Option<TaskId> {
        Some(self.tracker.active()?.task_id)
    }

    /// The name of the active task, if it is visible in the list.
    pub fn active_task_name(&self) -> Option<&str> {
        let task_id = self.active_task_id()?;
        Some(
            self.tasks
                .iter()
                .find(|task| task.id == task_id)?
                .name
                .as_str(),
        )
    }

    /// The live elapsed duration of the active entry, if tracking runs.
    pub fn elapsed(&self) -> Option<Duration> {
        self.clock.as_ref().map(ElapsedClock::elapsed)
    }

    /// The selected task, if any.
    fn selected_task(&self) -> Option<&Task> {
        self.tasks.get(self.selected?)
    }

    /// Moves the selection up, stopping at the first row.
    ///
    /// With no selection, selects the last row. An empty list has no
    /// selection to move.
    fn move_up(&mut self) {
        self.selected = match self.selected {
            None => self.tasks.len().checked_sub(1),
            Some(0) => Some(0),
            Some(index) => Some(index - 1),
        };
    }

    /// Moves the selection down, stopping at the last row.
    ///
    /// With no selection, selects the first row.
    fn move_down(&mut self) {
        if self.tasks.is_empty() {
            self.selected = None;
            return;
        }
        let last = self.tasks.len() - 1;
        self.selected = Some(match self.selected {
            None => 0,
            Some(index) => index.saturating_add(1).min(last),
        });
    }

    /// Starts, stops, or switches tracking for the selected task.
    ///
    /// The command applies to a cloned tracker first. The clone becomes the
    /// in-memory state only after the repository write succeeds; on failure
    /// the status line reports the error and nothing changes.
    fn toggle_tracking(&mut self) {
        let Some(task) = self.selected_task().cloned() else {
            return;
        };
        let mut candidate = self.tracker.clone();
        let outcome = match candidate.toggle(&task, Utc::now()) {
            Ok(outcome) => outcome,
            Err(error) => {
                self.status = Status::Error(error.to_string());
                return;
            }
        };
        if let Err(error) = self.persist_outcome(&outcome) {
            self.status = Status::Error(format!("Storage error: {error}"));
            return;
        }
        self.tracker = candidate;
        self.clock = self
            .tracker
            .active()
            .map(|active| ElapsedClock::since(active.start));
        self.status = match outcome {
            TrackingOutcome::Started { .. } => Status::Info(format!("Started \"{}\"", task.name)),
            TrackingOutcome::Stopped { .. } => Status::Info(format!("Stopped \"{}\"", task.name)),
            TrackingOutcome::Switched { .. } => {
                Status::Info(format!("Switched to \"{}\"", task.name))
            }
        };
    }

    /// Writes one tracking outcome to the repository.
    ///
    /// A switch lands in a single transaction, so the old entry cannot end up
    /// stopped without the new one existing.
    fn persist_outcome(&self, outcome: &TrackingOutcome) -> Result<(), StorageError> {
        match outcome {
            TrackingOutcome::Started { entry } => self.repository.insert_entry(entry),
            TrackingOutcome::Stopped { entry } => self
                .repository
                .stop_entry(entry.id, entry.end.unwrap_or(entry.start))
                .map(|_| ()),
            TrackingOutcome::Switched { stopped, started } => self.repository.switch_entry(
                stopped.id,
                stopped.end.unwrap_or(stopped.start),
                started,
            ),
        }
    }

    /// Opens an empty text input for a new task.
    fn open_add(&mut self) {
        self.mode = Mode::Input {
            purpose: InputPurpose::Add,
            buffer: String::new(),
        };
    }

    /// Opens a text input pre-filled with the selected task's name.
    fn open_rename(&mut self) {
        let (task_id, name) = match self.selected_task() {
            Some(task) => (task.id, task.name.to_string()),
            None => return,
        };
        self.mode = Mode::Input {
            purpose: InputPurpose::Rename { task_id },
            buffer: name,
        };
    }

    /// Opens the archive confirmation for the selected task.
    fn open_archive_confirm(&mut self) {
        let (task_id, name) = match self.selected_task() {
            Some(task) => (task.id, task.name.to_string()),
            None => return,
        };
        self.mode = Mode::ConfirmArchive { task_id, name };
    }

    /// Confirms the pending dialog.
    fn confirm(&mut self) {
        match &self.mode {
            Mode::Input { .. } => self.confirm_input(),
            Mode::ConfirmArchive { .. } => self.confirm_archive(),
            Mode::Normal => {}
        }
    }

    /// Confirms the open text input.
    ///
    /// An empty or whitespace-only name is rejected with a short error and
    /// keeps the input open. A storage failure reports the error and also
    /// keeps the input open, so the typed text is never lost.
    fn confirm_input(&mut self) {
        let Mode::Input { purpose, buffer } = self.mode.clone() else {
            return;
        };
        let name = match TaskName::new(&buffer) {
            Ok(name) => name,
            Err(_) => {
                self.status = Status::Error("The task name must not be empty".to_owned());
                return;
            }
        };
        let result = match purpose {
            InputPurpose::Add => self.add_task(name),
            InputPurpose::Rename { task_id } => self.rename_task(task_id, name),
        };
        match result {
            Ok(status) => {
                self.mode = Mode::Normal;
                self.status = status;
            }
            Err(error) => self.status = Status::Error(format!("Storage error: {error}")),
        }
    }

    /// Confirms the open archive dialog.
    ///
    /// The active task cannot be archived. A storage failure keeps the
    /// dialog open; anything else closes it.
    fn confirm_archive(&mut self) {
        let Mode::ConfirmArchive { task_id, name } = self.mode.clone() else {
            return;
        };
        if self.tracker.ensure_archivable(task_id).is_err() {
            self.mode = Mode::Normal;
            self.status = Status::Error("The active task cannot be archived".to_owned());
            return;
        }
        match self.repository.archive_task(task_id) {
            Ok(_) => {
                self.mode = Mode::Normal;
                self.status = Status::Info(format!("Archived \"{name}\""));
                if let Err(error) = self.reload(None) {
                    self.status = Status::Error(format!("Storage error: {error}"));
                }
            }
            Err(error) => self.status = Status::Error(format!("Storage error: {error}")),
        }
    }

    /// Creates the task and selects it once the write is visible.
    fn add_task(&mut self, name: TaskName) -> Result<Status, StorageError> {
        let task = Task::new(TaskId::generate(), name);
        self.repository.create_task(task.clone())?;
        self.reload(Some(task.id))?;
        Ok(Status::Info(format!("Added \"{}\"", task.name)))
    }

    /// Renames the task and keeps it selected.
    fn rename_task(&mut self, task_id: TaskId, name: TaskName) -> Result<Status, StorageError> {
        self.repository.rename_task(task_id, name.clone())?;
        self.reload(Some(task_id))?;
        Ok(Status::Info(format!("Renamed to \"{}\"", name)))
    }

    /// Refreshes the visible task list from the repository.
    ///
    /// The selection keeps the preferred task while it exists, then falls
    /// back to the previous index clamped into bounds, then to nothing.
    fn reload(&mut self, preferred: Option<TaskId>) -> Result<(), StorageError> {
        let keep = preferred.or_else(|| {
            self.selected
                .and_then(|index| self.tasks.get(index))
                .map(|task| task.id)
        });
        self.tasks = self
            .repository
            .list_tasks()?
            .into_iter()
            .filter(|task| !task.archived)
            .collect();
        self.selected = keep
            .and_then(|id| self.tasks.iter().position(|task| task.id == id))
            .or_else(|| {
                self.selected
                    .filter(|_| !self.tasks.is_empty())
                    .map(|index| index.min(self.tasks.len() - 1))
            });
        Ok(())
    }

    /// Whether a monotonic clock is tracking the active entry.
    #[cfg(test)]
    fn clock_active(&self) -> bool {
        self.clock.is_some()
    }

    /// Replaces the clock with one that reads `base` from its creation on.
    #[cfg(test)]
    pub(crate) fn freeze_elapsed_for_tests(&mut self, base: Duration) {
        self.clock = Some(ElapsedClock::anchored(base));
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use chrono::TimeDelta;
    use tempfile::TempDir;
    use tracker_core::{EntryId, TimeEntry};
    use tracker_storage::SqliteRepository;

    use super::*;

    fn repository() -> SqliteRepository {
        SqliteRepository::open_in_memory().unwrap()
    }

    fn named_task(name: &str) -> Task {
        Task::new(TaskId::generate(), TaskName::new(name).unwrap())
    }

    fn app_with(names: &[&str]) -> App {
        let repository = repository();
        for name in names {
            repository.create_task(named_task(name)).unwrap();
        }
        App::load(repository).unwrap()
    }

    fn assert_error_status(app: &App) {
        assert!(
            matches!(app.status(), Status::Error(_)),
            "expected an error status, got {:?}",
            app.status()
        );
    }

    /// Renders the status text of either status flavor.
    fn status_text(app: &App) -> &str {
        match app.status() {
            Status::Info(text) | Status::Error(text) => text,
        }
    }

    #[test]
    fn a_fresh_app_shows_ready_and_selects_the_first_task() {
        let app = app_with(&["one", "two"]);
        assert_eq!(app.status(), &Status::Info("Ready".to_owned()));
        assert_eq!(app.selected(), Some(0));
        assert_eq!(app.tasks().len(), 2);
        assert!(app.is_running());
        assert!(!app.clock_active());
    }

    #[test]
    fn an_empty_repository_loads_without_tasks_or_selection() {
        let app = App::load(repository()).unwrap();
        assert!(app.tasks().is_empty());
        assert_eq!(app.selected(), None);
        assert_eq!(app.status(), &Status::Info("Ready".to_owned()));
    }

    #[test]
    fn archived_tasks_stay_hidden() {
        let mut app = app_with(&["alpha", "beta"]);
        let beta = app.tasks()[1].id;
        app.repository.archive_task(beta).unwrap();
        app.reload(None).unwrap();
        let names: Vec<&str> = app.tasks().iter().map(|task| task.name.as_str()).collect();
        assert_eq!(names, ["alpha"]);
    }

    #[test]
    fn loading_recovers_a_running_timer_with_an_anchored_clock() {
        let repository = repository();
        let task = named_task("long job");
        repository.create_task(task.clone()).unwrap();
        let started = Utc::now() - TimeDelta::seconds(60);
        let entry = TimeEntry::begin(
            EntryId::from_uuid(TaskId::generate().as_uuid()),
            task.id,
            started,
        );
        repository.insert_entry(&entry).unwrap();

        let app = App::load(repository).unwrap();
        let active = app.tracker.active().expect("timer recovered");
        assert_eq!(active.id, entry.id);
        assert_eq!(active.task_id, task.id);
        assert_eq!(
            app.status(),
            &Status::Info("Recovered the previous active timer".to_owned())
        );
        // The visible elapsed time is anchored to the entry's start, not to
        // the moment the process came up.
        assert!(app.elapsed().unwrap_or_default() >= Duration::from_secs(59));
        assert!(app.clock_active());
    }

    #[test]
    fn selection_moves_with_safe_bounds() {
        let mut app = app_with(&["one", "two", "three"]);
        app.handle(Command::MoveDown);
        assert_eq!(app.selected(), Some(1));
        app.handle(Command::MoveDown);
        app.handle(Command::MoveDown);
        assert_eq!(app.selected(), Some(2), "stops at the last row");
        app.handle(Command::MoveUp);
        assert_eq!(app.selected(), Some(1));
        app.handle(Command::MoveUp);
        app.handle(Command::MoveUp);
        assert_eq!(app.selected(), Some(0));
        app.handle(Command::MoveUp);
        assert_eq!(app.selected(), Some(0), "the first row is a bound");
        app.handle(Command::MoveDown);
        app.handle(Command::MoveDown);
        app.handle(Command::MoveDown);
        assert_eq!(app.selected(), Some(2), "the last row is a bound");
    }

    #[test]
    fn a_single_task_stays_selected_in_both_directions() {
        let mut app = app_with(&["only"]);
        app.handle(Command::MoveDown);
        assert_eq!(app.selected(), Some(0));
        app.handle(Command::MoveUp);
        assert_eq!(app.selected(), Some(0));
    }

    #[test]
    fn navigation_on_an_empty_list_stays_unselected() {
        let mut app = App::load(repository()).unwrap();
        app.handle(Command::MoveDown);
        app.handle(Command::MoveUp);
        assert_eq!(app.selected(), None);
    }

    #[test]
    fn moving_from_no_selection_picks_the_ends() {
        let mut app = app_with(&["one", "two", "three"]);
        app.selected = None;
        app.handle(Command::MoveDown);
        assert_eq!(app.selected(), Some(0));
        app.selected = None;
        app.handle(Command::MoveUp);
        assert_eq!(
            app.selected(),
            Some(2),
            "up from nothing picks the last row"
        );
    }

    #[test]
    fn reserved_keys_change_nothing() {
        let mut app = app_with(&["one", "two"]);
        app.handle(Command::MoveDown);
        app.handle(Command::Reserved);
        assert_eq!(app.selected(), Some(1));
        assert_eq!(app.mode(), &Mode::Normal);
        assert!(app.is_running());
    }

    #[test]
    fn space_starts_tracking_the_selected_task() {
        let mut app = app_with(&["write docs"]);
        app.handle(Command::ToggleTracking);
        let task_id = app.tasks()[0].id;
        let active = app.tracker.active().expect("tracking started");
        assert_eq!(active.task_id, task_id);
        assert!(app.clock_active());
        assert_eq!(
            app.status(),
            &Status::Info("Started \"write docs\"".to_owned())
        );

        let stored = app.repository.active_entry().unwrap().expect("persisted");
        assert_eq!(stored.id, active.id);
        assert_eq!(stored.task_id, task_id);
        assert_eq!(stored.end, None);
    }

    #[test]
    fn space_on_the_active_task_stops_it() {
        let mut app = app_with(&["write docs"]);
        app.handle(Command::ToggleTracking);
        let entry_id = app.tracker.active().unwrap().id;
        app.handle(Command::ToggleTracking);

        assert_eq!(app.tracker.active(), None);
        assert!(!app.clock_active());
        assert_eq!(
            app.status(),
            &Status::Info("Stopped \"write docs\"".to_owned())
        );
        assert_eq!(app.repository.active_entry().unwrap(), None);
        let entries = app.repository.list_entries(app.tasks()[0].id).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, entry_id);
        assert!(entries[0].end.is_some(), "the entry is stopped");
    }

    #[test]
    fn space_on_another_task_switches_in_one_transaction() {
        let mut app = app_with(&["alpha", "beta"]);
        app.handle(Command::ToggleTracking);
        let first_entry = app.tracker.active().unwrap().id;
        app.handle(Command::MoveDown);
        app.handle(Command::ToggleTracking);

        let beta_id = app.tasks()[1].id;
        let active = app.tracker.active().expect("switched");
        assert_eq!(active.task_id, beta_id);
        assert_ne!(active.id, first_entry, "a switch starts a new entry");
        assert_eq!(
            app.status(),
            &Status::Info("Switched to \"beta\"".to_owned())
        );

        // Exactly one active entry, and the old one is stopped with an end
        // time: both halves of the switch became visible together.
        let stored = app.repository.active_entry().unwrap().unwrap();
        assert_eq!(stored.id, active.id);
        let old = &app.repository.list_entries(app.tasks()[0].id).unwrap()[0];
        assert_eq!(old.id, first_entry);
        assert!(old.end.is_some());
    }

    #[test]
    fn every_stop_and_restart_creates_a_new_entry() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::ToggleTracking);
        let first = app.tracker.active().unwrap().id;
        app.handle(Command::ToggleTracking);
        app.handle(Command::ToggleTracking);
        let second = app.tracker.active().unwrap().id;
        assert_ne!(first, second);
        let entries = app.repository.list_entries(app.tasks()[0].id).unwrap();
        assert_eq!(entries.len(), 2, "each restart is its own entry");
    }

    #[test]
    fn a_failed_switch_leaves_memory_and_storage_consistent() {
        let mut app = app_with(&["alpha", "beta"]);
        app.handle(Command::ToggleTracking);
        let entry_id = app.tracker.active().unwrap().id;

        // Tamper with the database so the switch's stop half matches no
        // active row: the repository write fails as a whole.
        let beta_id = app.tasks()[1].id;
        let now_us = Utc::now().timestamp_micros();
        let connection = app.repository.connection();
        connection
            .execute(
                &format!("UPDATE time_entries SET end_us = {now_us} WHERE id = '{entry_id}'"),
                [],
            )
            .unwrap();
        connection
            .execute(
                &format!(
                    "INSERT INTO time_entries (id, task_id, start_us, end_us)
                     VALUES ('00000000-0000-7000-8000-000000000009', '{beta_id}', {now_us}, NULL)"
                ),
                [],
            )
            .unwrap();

        app.handle(Command::MoveDown);
        app.handle(Command::ToggleTracking);

        assert_error_status(&app);
        // The candidate was discarded: memory still shows the original entry.
        assert_eq!(app.tracker.active().unwrap().id, entry_id);
        assert!(app.is_running());
        // The database is untouched by the failed switch.
        let active = app.repository.active_entry().unwrap().unwrap();
        assert_eq!(
            active.id.to_string(),
            "00000000-0000-7000-8000-000000000009"
        );
    }

    #[test]
    fn quitting_from_any_mode_keeps_the_active_entry_running() {
        let setups: [fn(&mut App); 3] = [
            |app| app.handle(Command::OpenAdd),
            |app| app.handle(Command::OpenArchiveConfirm),
            |_| {},
        ];
        for setup in setups {
            let mut app = app_with(&["alpha"]);
            app.handle(Command::ToggleTracking);
            let entry_id = app.tracker.active().unwrap().id;
            setup(&mut app);
            app.handle(Command::Quit);
            assert!(!app.is_running());
            assert!(app.tracker.active().is_some());
            assert_eq!(app.repository.active_entry().unwrap().unwrap().id, entry_id);
        }
    }

    #[test]
    fn adding_a_task_persists_reloads_and_selects_it() {
        let mut app = app_with(&["one"]);
        app.handle(Command::OpenAdd);
        for character in "new task".chars() {
            app.handle(Command::Insert(character));
        }
        app.handle(Command::Confirm);

        assert_eq!(app.mode(), &Mode::Normal);
        assert_eq!(app.status(), &Status::Info("Added \"new task\"".to_owned()));
        let names: Vec<&str> = app.tasks().iter().map(|task| task.name.as_str()).collect();
        assert_eq!(names, ["one", "new task"]);
        assert_eq!(app.selected(), Some(1), "the added task is selected");
        assert_eq!(app.repository.list_tasks().unwrap().len(), 2);
    }

    #[test]
    fn an_empty_name_shows_a_short_error_and_keeps_the_input_open() {
        for typed in ["", "   "] {
            let mut app = app_with(&["one"]);
            app.handle(Command::OpenAdd);
            for character in typed.chars() {
                app.handle(Command::Insert(character));
            }
            app.handle(Command::Confirm);

            assert_eq!(
                app.status(),
                &Status::Error("The task name must not be empty".to_owned())
            );
            assert_eq!(
                app.mode(),
                &Mode::Input {
                    purpose: InputPurpose::Add,
                    buffer: typed.to_owned()
                }
            );
            assert_eq!(app.tasks().len(), 1, "nothing was created");
        }
    }

    #[test]
    fn a_failed_add_keeps_the_input_open_and_memory_intact() {
        let mut app = app_with(&["one"]);
        app.handle(Command::OpenAdd);
        for character in "doomed".chars() {
            app.handle(Command::Insert(character));
        }
        app.repository
            .connection()
            .execute("DROP TABLE tasks", [])
            .unwrap();
        app.handle(Command::Confirm);

        assert_error_status(&app);
        assert_eq!(
            app.mode(),
            &Mode::Input {
                purpose: InputPurpose::Add,
                buffer: "doomed".to_owned()
            }
        );
        assert_eq!(app.tasks().len(), 1, "memory is unchanged");
    }

    #[test]
    fn cancelling_the_add_input_discards_the_buffer() {
        let mut app = app_with(&["one"]);
        app.handle(Command::OpenAdd);
        app.handle(Command::Insert('x'));
        app.handle(Command::Cancel);
        assert_eq!(app.mode(), &Mode::Normal);
        assert_eq!(app.tasks().len(), 1);
    }

    #[test]
    fn renaming_a_task_updates_the_list_and_persists() {
        let mut app = app_with(&["old name"]);
        app.handle(Command::OpenRename);
        assert_eq!(
            app.mode(),
            &Mode::Input {
                purpose: InputPurpose::Rename {
                    task_id: app.tasks()[0].id
                },
                buffer: "old name".to_owned()
            },
            "the input starts pre-filled"
        );
        // Clear the pre-filled text and type the new name.
        for _ in 0..8 {
            app.handle(Command::Backspace);
        }
        for character in "new name".chars() {
            app.handle(Command::Insert(character));
        }
        app.handle(Command::Confirm);

        assert_eq!(app.mode(), &Mode::Normal);
        assert_eq!(
            app.status(),
            &Status::Info("Renamed to \"new name\"".to_owned())
        );
        assert_eq!(app.tasks()[0].name.as_str(), "new name");
        let stored = app
            .repository
            .find_task(app.tasks()[0].id)
            .unwrap()
            .unwrap();
        assert_eq!(stored.name.as_str(), "new name");
        assert_eq!(app.selected(), Some(0));
    }

    #[test]
    fn an_empty_rename_shows_an_error_and_keeps_the_input_open() {
        let mut app = app_with(&["name"]);
        app.handle(Command::OpenRename);
        for _ in 0..4 {
            app.handle(Command::Backspace);
        }
        app.handle(Command::Confirm);
        assert_error_status(&app);
        assert_eq!(app.tasks()[0].name.as_str(), "name", "unchanged");
        assert!(matches!(app.mode(), Mode::Input { .. }));
    }

    #[test]
    fn a_failed_rename_keeps_the_input_open() {
        let mut app = app_with(&["name"]);
        app.handle(Command::OpenRename);
        app.repository
            .connection()
            .execute("DROP TABLE tasks", [])
            .unwrap();
        app.handle(Command::Confirm);
        assert_error_status(&app);
        assert!(matches!(app.mode(), Mode::Input { .. }));
        assert_eq!(app.tasks()[0].name.as_str(), "name");
    }

    #[test]
    fn opening_dialogs_without_a_selection_does_nothing() {
        let mut app = App::load(repository()).unwrap();
        app.handle(Command::OpenRename);
        app.handle(Command::OpenArchiveConfirm);
        assert_eq!(app.mode(), &Mode::Normal);
    }

    #[test]
    fn archiving_after_confirmation_hides_the_task() {
        let mut app = app_with(&["alpha", "beta", "gamma"]);
        app.handle(Command::MoveDown);
        let beta = app.tasks()[1].id;
        app.handle(Command::OpenArchiveConfirm);
        assert_eq!(
            app.mode(),
            &Mode::ConfirmArchive {
                task_id: beta,
                name: "beta".to_owned()
            }
        );
        app.handle(Command::Confirm);

        assert_eq!(app.mode(), &Mode::Normal);
        assert_eq!(app.status(), &Status::Info("Archived \"beta\"".to_owned()));
        let names: Vec<&str> = app.tasks().iter().map(|task| task.name.as_str()).collect();
        assert_eq!(names, ["alpha", "gamma"]);
        assert!(app.repository.find_task(beta).unwrap().unwrap().archived);
        assert_eq!(app.selected(), Some(1), "selection lands on the next task");
    }

    #[test]
    fn archiving_the_last_task_empties_the_list_safely() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::OpenArchiveConfirm);
        app.handle(Command::Confirm);
        assert!(app.tasks().is_empty());
        assert_eq!(app.selected(), None);
        // The empty list stays navigable.
        app.handle(Command::MoveDown);
        app.handle(Command::MoveUp);
        assert_eq!(app.selected(), None);
    }

    #[test]
    fn archiving_the_active_task_is_rejected() {
        let mut app = app_with(&["alpha", "beta"]);
        app.handle(Command::ToggleTracking);
        let entry_id = app.tracker.active().unwrap().id;
        app.handle(Command::OpenArchiveConfirm);
        app.handle(Command::Confirm);

        assert_eq!(
            app.status(),
            &Status::Error("The active task cannot be archived".to_owned())
        );
        assert_eq!(app.mode(), &Mode::Normal);
        assert!(!app.tasks()[0].archived, "the task is untouched");
        assert_eq!(app.tracker.active().unwrap().id, entry_id);
        assert_eq!(status_text(&app), "The active task cannot be archived");
        assert!(
            !app.repository
                .find_task(app.tasks()[0].id)
                .unwrap()
                .unwrap()
                .archived
        );
    }

    #[test]
    fn a_failed_archive_keeps_the_dialog_open() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::OpenArchiveConfirm);
        app.repository
            .connection()
            .execute("DROP TABLE tasks", [])
            .unwrap();
        app.handle(Command::Confirm);
        assert_error_status(&app);
        assert!(matches!(app.mode(), Mode::ConfirmArchive { .. }));
        assert!(!app.tasks().is_empty(), "memory is unchanged");
    }

    #[test]
    fn cancelling_the_archive_dialog_keeps_the_task() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::OpenArchiveConfirm);
        app.handle(Command::Cancel);
        assert_eq!(app.mode(), &Mode::Normal);
        assert!(!app.tasks()[0].archived);
    }

    #[test]
    fn reloading_clamps_an_out_of_bounds_selection() {
        let mut app = app_with(&["alpha", "beta"]);
        app.selected = Some(5);
        app.reload(None).unwrap();
        assert_eq!(app.selected(), Some(1), "clamped to the last row");
        app.selected = None;
        app.reload(None).unwrap();
        assert_eq!(app.selected(), None);
    }

    #[test]
    fn reload_keeps_the_preferred_task_selected() {
        let mut app = app_with(&["alpha", "beta", "gamma"]);
        let gamma = app.tasks()[2].id;
        app.reload(Some(gamma)).unwrap();
        assert_eq!(app.selected(), Some(2));
        // A preferred task that no longer exists falls back to the current
        // index.
        let archived = named_task("gone");
        app.repository.create_task(archived.clone()).unwrap();
        app.repository.archive_task(archived.id).unwrap();
        app.reload(Some(archived.id)).unwrap();
        assert_eq!(app.selected(), Some(2));
    }

    #[test]
    fn the_clock_adds_the_anchor_to_the_time_since_it_was_taken() {
        let clock = ElapsedClock::anchored(Duration::from_secs(100));
        assert_eq!(clock.at(Duration::from_secs(5)), Duration::from_secs(105));
        assert_eq!(clock.at(Duration::ZERO), Duration::from_secs(100));
    }

    #[test]
    fn the_clock_anchor_clamps_future_starts_to_zero() {
        let now = Utc::now();
        let future = now + TimeDelta::seconds(30);
        let past = now - TimeDelta::seconds(5);
        assert_eq!(ElapsedClock::base_since(future, now), Duration::ZERO);
        assert_eq!(ElapsedClock::base_since(past, now), Duration::from_secs(5));
    }

    #[test]
    fn the_clock_is_monotonic_and_anchored_to_the_base() {
        let clock = ElapsedClock::anchored(Duration::from_secs(100));
        assert!(
            clock.elapsed() >= Duration::from_secs(100),
            "the base must anchor the elapsed time"
        );
        std::thread::sleep(Duration::from_millis(15));
        let later = clock.elapsed();
        assert!(
            later >= Duration::from_millis(100_015),
            "elapsed must advance monotonically past the base, got {later:?}"
        );
        assert!(
            clock.elapsed() >= later,
            "repeated reads never go backwards"
        );
    }

    #[test]
    fn the_frozen_clock_reads_the_given_base() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::ToggleTracking);
        assert!(app.clock_active());
        app.freeze_elapsed_for_tests(Duration::from_secs(125));
        assert_eq!(app.elapsed().unwrap().as_secs(), 125);
    }

    #[test]
    fn a_running_timer_survives_reopening_the_database() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("tracker.db");
        let entry_id;
        {
            let repository = SqliteRepository::open(&path).unwrap();
            repository.create_task(named_task("long job")).unwrap();
            let mut app = App::load(repository).unwrap();
            app.handle(Command::ToggleTracking);
            entry_id = app.tracker.active().unwrap().id;
            // Dropping the app stops nothing: exiting must not discard the
            // active timer.
        }
        assert_database_recovers_running(&path, entry_id);
    }

    fn assert_database_recovers_running(path: &Path, entry_id: tracker_core::EntryId) {
        let repository = SqliteRepository::open(path).unwrap();
        let recovered = repository.active_entry().unwrap().expect("still running");
        assert_eq!(recovered.id, entry_id);
        assert_eq!(recovered.end, None);
        let app = App::load(repository).unwrap();
        assert_eq!(app.tracker.active().unwrap().id, entry_id);
        assert_eq!(
            app.status(),
            &Status::Info("Recovered the previous active timer".to_owned())
        );
        assert!(app.clock_active());
    }
}
