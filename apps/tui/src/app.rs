//! TUI presentation state and semantic command handling.

use std::time::{Duration, Instant};

use chrono::{DateTime, TimeDelta, Utc};
use tracker_application::{
    ApplicationError, ClearActiveTaskOutcome, SetActiveTaskOutcome, TaskOperations, TaskOutcome,
    TaskQueries, TrackerApplication, TrackerRepository, TrackingOperations,
};
use tracker_domain::{Task, TaskId, TaskName, TaskNameError, TrackingError, TrackingState};

use crate::command::Command;

/// What the TUI is currently asking of the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Input {
        purpose: InputPurpose,
        buffer: String,
    },
    ConfirmArchive {
        task_id: TaskId,
        name: String,
    },
}

/// What a confirmed text input does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputPurpose {
    Add,
    Rename { task_id: TaskId },
}

/// The most recent message shown in the status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Info(String),
    Error(String),
}

/// The event loop's two lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    Running,
    Quitting,
}

/// Derives displayed elapsed time from a monotonic clock.
#[derive(Debug, Clone)]
pub(crate) struct ElapsedClock {
    anchor: Instant,
    base: Duration,
}

impl ElapsedClock {
    pub(crate) fn since(start: DateTime<Utc>) -> Self {
        Self::anchored(Self::base_since(start, Utc::now()))
    }

    pub(crate) fn anchored(base: Duration) -> Self {
        Self {
            anchor: Instant::now(),
            base,
        }
    }

    fn base_since(start: DateTime<Utc>, now: DateTime<Utc>) -> Duration {
        (now - start).to_std().unwrap_or(Duration::ZERO)
    }

    pub(crate) fn at(&self, since_anchor: Duration) -> Duration {
        self.base + since_anchor
    }

    pub(crate) fn elapsed(&self) -> Duration {
        self.at(self.anchor.elapsed())
    }
}

/// Converts displayed monotonic elapsed time to a client-created UTC instant.
fn tracking_timestamp(start: DateTime<Utc>, elapsed: Duration) -> DateTime<Utc> {
    let delta = TimeDelta::from_std(elapsed).unwrap_or(TimeDelta::MAX);
    start
        .checked_add_signed(delta)
        .unwrap_or(DateTime::<Utc>::MAX_UTC)
}

fn task_name_error_text(error: TaskNameError) -> String {
    match error {
        TaskNameError::Empty => "The task name must not be empty".to_owned(),
        TaskNameError::Control => "The task name must not contain control characters".to_owned(),
        TaskNameError::TooLong => format!(
            "The task name must be at most {} characters",
            TaskName::MAX_LEN
        ),
    }
}

fn application_error_text(error: &ApplicationError) -> String {
    match error {
        ApplicationError::Domain(error) => error.to_string(),
        ApplicationError::Repository(error) | ApplicationError::TrackingWrite(error) => {
            format!("Storage error: {error}")
        }
        ApplicationError::TrackingRecovery(error) => format!("Storage error: {error}"),
    }
}

/// Task-list presentation state.
pub struct App<R: TrackerRepository> {
    application: TrackerApplication<R>,
    tasks: Vec<Task>,
    tracking: TrackingState,
    selected: Option<usize>,
    mode: Mode,
    status: Status,
    clock: Option<ElapsedClock>,
    lifecycle: Lifecycle,
}

impl<R: TrackerRepository> App<R> {
    /// Builds presentation state from an already loaded application service.
    pub fn load(application: TrackerApplication<R>) -> Self {
        let tasks = application
            .tasks()
            .iter()
            .filter(|task| !task.archived)
            .cloned()
            .collect::<Vec<_>>();
        let tracking = application.current_tracking().clone();
        let clock = match &tracking {
            TrackingState::Idle => None,
            TrackingState::Running { worklog } => Some(ElapsedClock::since(worklog.start)),
        };
        let status = if clock.is_some() {
            Status::Info("Recovered the previous active timer".to_owned())
        } else {
            Status::Info("Ready".to_owned())
        };
        let selected = (!tasks.is_empty()).then_some(0);
        Self {
            application,
            tasks,
            tracking,
            selected,
            mode: Mode::Normal,
            status,
            clock,
            lifecycle: Lifecycle::Running,
        }
    }

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
                if let Mode::Input { buffer, .. } = &mut self.mode
                    && buffer.chars().count() < TaskName::MAX_LEN
                {
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

    pub fn is_running(&self) -> bool {
        self.lifecycle == Lifecycle::Running
    }

    pub fn tasks(&self) -> &[Task] {
        &self.tasks
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn active_task_id(&self) -> Option<TaskId> {
        match &self.tracking {
            TrackingState::Idle => None,
            TrackingState::Running { worklog } => Some(worklog.task_id),
        }
    }

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

    pub fn elapsed(&self) -> Option<Duration> {
        self.clock.as_ref().map(ElapsedClock::elapsed)
    }

    fn active_start(&self) -> Option<DateTime<Utc>> {
        match &self.tracking {
            TrackingState::Idle => None,
            TrackingState::Running { worklog } => Some(worklog.start),
        }
    }

    fn selected_task(&self) -> Option<&Task> {
        self.tasks.get(self.selected?)
    }

    fn move_up(&mut self) {
        self.selected = match self.selected {
            None => self.tasks.len().checked_sub(1),
            Some(0) => Some(0),
            Some(index) => Some(index - 1),
        };
    }

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

    /// Translates Space into desired tracking state with an explicit client
    /// timestamp. The application service owns persistence and recovery.
    fn toggle_tracking(&mut self) {
        let Some(task) = self.selected_task().cloned() else {
            return;
        };
        let was_active = self.active_task_id();
        let occurred_at = self.active_start().map_or_else(Utc::now, |start| {
            let elapsed = self
                .clock
                .as_ref()
                .map_or(Duration::ZERO, ElapsedClock::elapsed);
            tracking_timestamp(start, elapsed)
        });

        let result = if was_active == Some(task.id) {
            self.application
                .clear_active_task(occurred_at)
                .map(|outcome| match outcome {
                    ClearActiveTaskOutcome::Stopped { .. }
                    | ClearActiveTaskOutcome::AlreadyIdle => ("stopped", false),
                })
        } else {
            self.application
                .set_active_task(task.id, occurred_at)
                .map(|outcome| match outcome {
                    SetActiveTaskOutcome::Started { .. } => ("started", true),
                    SetActiveTaskOutcome::Switched { .. } => ("switched", true),
                    SetActiveTaskOutcome::AlreadyActive { .. } if was_active.is_some() => {
                        ("switched", false)
                    }
                    SetActiveTaskOutcome::AlreadyActive { .. } => ("started", false),
                })
        };

        match result {
            Ok((action, fresh_active)) => {
                self.sync_from_application(fresh_active);
                self.status = match action {
                    "started" => Status::Info(format!("Started \"{}\"", task.name)),
                    "switched" => Status::Info(format!("Switched to \"{}\"", task.name)),
                    _ => Status::Info(format!("Stopped \"{}\"", task.name)),
                };
            }
            Err(error) => {
                self.sync_from_application(false);
                self.status = Status::Error(application_error_text(&error));
            }
        }
    }

    /// Copies backend-neutral query state after an application operation.
    fn sync_from_application(&mut self, fresh_active: bool) {
        self.sync_tasks_from_application();
        self.tracking = self.application.current_tracking().clone();
        self.clock = match &self.tracking {
            TrackingState::Idle => None,
            TrackingState::Running { .. } if fresh_active => {
                Some(ElapsedClock::anchored(Duration::ZERO))
            }
            TrackingState::Running { worklog } => Some(ElapsedClock::since(worklog.start)),
        };
    }

    fn sync_tasks_from_application(&mut self) {
        let previous_index = self.selected;
        let preferred = self.selected_task().map(|task| task.id);
        self.tasks = self
            .application
            .tasks()
            .iter()
            .filter(|task| !task.archived)
            .cloned()
            .collect();
        self.selected = preferred
            .and_then(|id| self.tasks.iter().position(|task| task.id == id))
            .or_else(|| {
                previous_index
                    .filter(|_| !self.tasks.is_empty())
                    .map(|index| index.min(self.tasks.len() - 1))
            });
    }

    fn open_add(&mut self) {
        self.mode = Mode::Input {
            purpose: InputPurpose::Add,
            buffer: String::new(),
        };
    }

    fn open_rename(&mut self) {
        let Some(task) = self.selected_task() else {
            return;
        };
        self.mode = Mode::Input {
            purpose: InputPurpose::Rename { task_id: task.id },
            buffer: task.name.to_string(),
        };
    }

    fn open_archive_confirm(&mut self) {
        let Some(task) = self.selected_task() else {
            return;
        };
        self.mode = Mode::ConfirmArchive {
            task_id: task.id,
            name: task.name.to_string(),
        };
    }

    fn confirm(&mut self) {
        match &self.mode {
            Mode::Input { .. } => self.confirm_input(),
            Mode::ConfirmArchive { .. } => self.confirm_archive(),
            Mode::Normal => {}
        }
    }

    fn confirm_input(&mut self) {
        let Mode::Input { purpose, buffer } = self.mode.clone() else {
            return;
        };
        let name = match TaskName::new(&buffer) {
            Ok(name) => name,
            Err(error) => {
                self.status = Status::Error(task_name_error_text(error));
                return;
            }
        };
        let result = match purpose {
            InputPurpose::Add => self.application.create_task(name),
            InputPurpose::Rename { task_id } => self.application.rename_task(task_id, name),
        };
        match result {
            Ok(TaskOutcome::Created(task)) => {
                self.sync_tasks_from_application();
                self.selected = self.tasks.iter().position(|item| item.id == task.id);
                self.mode = Mode::Normal;
                self.status = Status::Info(format!("Added \"{}\"", task.name));
            }
            Ok(TaskOutcome::Renamed(task)) => {
                self.sync_tasks_from_application();
                self.mode = Mode::Normal;
                self.status = Status::Info(format!("Renamed to \"{}\"", task.name));
            }
            Ok(TaskOutcome::Archived(_)) => unreachable!("input cannot archive a task"),
            Err(error) => self.status = Status::Error(application_error_text(&error)),
        }
    }

    fn confirm_archive(&mut self) {
        let Mode::ConfirmArchive { task_id, name } = self.mode.clone() else {
            return;
        };
        match self.application.archive_task(task_id) {
            Ok(TaskOutcome::Archived(_)) => {
                self.sync_tasks_from_application();
                self.mode = Mode::Normal;
                self.status = Status::Info(format!("Archived \"{name}\""));
            }
            Err(ApplicationError::Domain(TrackingError::TaskIsActive { .. })) => {
                self.sync_from_application(false);
                self.mode = Mode::Normal;
                self.status = Status::Error("The active task cannot be archived".to_owned());
            }
            Err(error) => self.status = Status::Error(application_error_text(&error)),
            Ok(_) => unreachable!("archive returned another task outcome"),
        }
    }

    #[cfg(test)]
    pub(crate) fn freeze_elapsed_for_tests(&mut self, base: Duration) {
        self.clock = Some(ElapsedClock::anchored(base));
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use tracker_application::{TrackerApplication, WorklogQueries};
    use tracker_domain::{Task, TaskId, TaskName};
    use tracker_storage::SqliteRepository;

    use super::*;

    fn app_with(names: &[&str]) -> App<SqliteRepository> {
        let repository = SqliteRepository::open_in_memory().unwrap();
        for name in names {
            repository
                .create_task(Task::new(TaskId::generate(), TaskName::new(name).unwrap()))
                .unwrap();
        }
        App::load(TrackerApplication::load(repository).unwrap())
    }

    fn text(status: &Status) -> &str {
        match status {
            Status::Info(text) | Status::Error(text) => text,
        }
    }

    #[test]
    fn fresh_and_empty_apps_have_safe_selection() {
        let app = app_with(&["one", "two"]);
        assert_eq!(app.selected(), Some(0));
        assert_eq!(app.status(), &Status::Info("Ready".to_owned()));
        let mut empty = app_with(&[]);
        empty.handle(Command::MoveDown);
        empty.handle(Command::MoveUp);
        assert_eq!(empty.selected(), None);
    }

    #[test]
    fn selection_movement_stays_inside_the_task_list() {
        let mut app = app_with(&["one", "two", "three"]);
        app.handle(Command::MoveUp);
        assert_eq!(app.selected(), Some(0));
        for _ in 0..5 {
            app.handle(Command::MoveDown);
        }
        assert_eq!(app.selected(), Some(2));
        app.selected = None;
        app.handle(Command::MoveUp);
        assert_eq!(app.selected(), Some(2));
    }

    #[test]
    fn adding_a_task_updates_the_list_and_selection() {
        let mut app = app_with(&["one"]);
        app.handle(Command::OpenAdd);
        for character in "new task".chars() {
            app.handle(Command::Insert(character));
        }
        app.handle(Command::Confirm);
        assert_eq!(app.mode(), &Mode::Normal);
        assert_eq!(app.tasks()[1].name.as_str(), "new task");
        assert_eq!(app.selected(), Some(1));
        assert_eq!(app.status(), &Status::Info("Added \"new task\"".to_owned()));
    }

    #[test]
    fn invalid_input_keeps_the_dialog_and_reports_the_same_text() {
        let mut app = app_with(&["one"]);
        app.handle(Command::OpenAdd);
        app.handle(Command::Confirm);
        assert!(matches!(app.mode(), Mode::Input { .. }));
        assert_eq!(
            app.status(),
            &Status::Error("The task name must not be empty".to_owned())
        );
    }

    #[test]
    fn renaming_a_task_keeps_its_row_selected() {
        let mut app = app_with(&["old"]);
        app.handle(Command::OpenRename);
        for _ in 0..3 {
            app.handle(Command::Backspace);
        }
        for character in "new".chars() {
            app.handle(Command::Insert(character));
        }
        app.handle(Command::Confirm);
        assert_eq!(app.tasks()[0].name.as_str(), "new");
        assert_eq!(app.selected(), Some(0));
        assert_eq!(app.status(), &Status::Info("Renamed to \"new\"".to_owned()));
    }

    #[test]
    fn archiving_hides_the_task_and_clamps_selection() {
        let mut app = app_with(&["alpha", "beta"]);
        app.handle(Command::MoveDown);
        app.handle(Command::OpenArchiveConfirm);
        app.handle(Command::Confirm);
        assert_eq!(app.tasks().len(), 1);
        assert_eq!(app.tasks()[0].name.as_str(), "alpha");
        assert_eq!(app.selected(), Some(0));
        assert_eq!(app.status(), &Status::Info("Archived \"beta\"".to_owned()));
    }

    #[test]
    fn archiving_the_active_task_keeps_the_timer() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::ToggleTracking);
        let active = app.active_task_id();
        app.handle(Command::OpenArchiveConfirm);
        app.handle(Command::Confirm);
        assert_eq!(app.active_task_id(), active);
        assert_eq!(app.tasks().len(), 1);
        assert_eq!(
            app.status(),
            &Status::Error("The active task cannot be archived".to_owned())
        );
    }

    #[test]
    fn space_starts_stops_and_restarts_with_separate_worklogs() {
        let mut app = app_with(&["alpha"]);
        let task_id = app.tasks()[0].id;
        app.handle(Command::ToggleTracking);
        assert_eq!(app.active_task_id(), Some(task_id));
        assert_eq!(text(app.status()), "Started \"alpha\"");
        app.handle(Command::ToggleTracking);
        assert_eq!(app.active_task_id(), None);
        assert_eq!(text(app.status()), "Stopped \"alpha\"");
        app.handle(Command::ToggleTracking);
        let worklogs = app.application.worklogs_for_task(task_id).unwrap();
        assert_eq!(worklogs.len(), 2);
        assert_ne!(worklogs[0].id, worklogs[1].id);
    }

    #[test]
    fn space_switches_to_another_task() {
        let mut app = app_with(&["alpha", "beta"]);
        app.handle(Command::ToggleTracking);
        app.handle(Command::MoveDown);
        let beta = app.tasks()[1].id;
        app.handle(Command::ToggleTracking);
        assert_eq!(app.active_task_id(), Some(beta));
        assert_eq!(
            app.status(),
            &Status::Info("Switched to \"beta\"".to_owned())
        );
    }

    #[test]
    fn quitting_in_a_dialog_does_not_stop_tracking() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::ToggleTracking);
        app.handle(Command::OpenAdd);
        app.handle(Command::Quit);
        assert!(!app.is_running());
        assert!(app.active_task_id().is_some());
    }

    #[test]
    fn input_is_bounded_to_the_domain_limit() {
        let mut app = app_with(&[]);
        app.handle(Command::OpenAdd);
        for character in "x".repeat(TaskName::MAX_LEN + 10).chars() {
            app.handle(Command::Insert(character));
        }
        app.handle(Command::Confirm);
        assert_eq!(
            app.tasks()[0].name.as_str().chars().count(),
            TaskName::MAX_LEN
        );
    }

    #[test]
    fn elapsed_clock_and_client_timestamp_share_one_duration() {
        let clock = ElapsedClock::anchored(Duration::from_secs(100));
        assert_eq!(clock.at(Duration::from_secs(5)), Duration::from_secs(105));
        let start = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            tracking_timestamp(start, Duration::from_secs(5)),
            start + TimeDelta::seconds(5)
        );
    }

    #[test]
    fn future_clock_anchors_at_zero_and_timestamp_overflow_saturates() {
        let now = Utc::now();
        assert_eq!(
            ElapsedClock::base_since(now + TimeDelta::seconds(1), now),
            Duration::ZERO
        );
        assert_eq!(
            tracking_timestamp(DateTime::<Utc>::MAX_UTC, Duration::MAX),
            DateTime::<Utc>::MAX_UTC
        );
    }

    #[test]
    fn reserved_and_cancel_commands_preserve_expected_state() {
        let mut app = app_with(&["one"]);
        app.handle(Command::Reserved);
        assert_eq!(app.selected(), Some(0));
        app.handle(Command::OpenAdd);
        app.handle(Command::Insert('x'));
        app.handle(Command::Cancel);
        assert_eq!(app.mode(), &Mode::Normal);
        assert_eq!(app.tasks().len(), 1);
    }
}
