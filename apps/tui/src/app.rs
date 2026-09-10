//! Global TUI state, semantic command routing, and screen transitions.

use chrono::FixedOffset;
use chrono_tz::Tz;
use tracker_application::TrackerApplicationService;
use tracker_domain::{TaskId, TrackingState};

use crate::command::Command;
use crate::screens::task_list::load_state;
use crate::screens::worklog_history::active_worklog_for_task;
use crate::support::clock::ElapsedClock;
use crate::support::errors::application_error_text;
use crate::support::timestamps::startup_timezone;

#[cfg(test)]
pub use crate::screens::worklog_history::{CorrectionField, HistoryAvailability};
#[cfg(test)]
pub use crate::screens::{CorrectionDraft, InputPurpose, TaskView};
pub use crate::screens::{History, Screen, ScreenState, TaskListState, WorklogHistoryState};
#[cfg(test)]
pub use crate::screens::{TaskListMode, WorklogHistoryMode};
pub use crate::support::timestamps::TimestampInput;
pub(crate) use crate::support::timestamps::is_timestamp_character;

/// The most recent message shown in the status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Info(String),
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lifecycle {
    Running,
    Quitting,
}

/// Shared application state around one exclusively owned screen state.
pub struct App<S: TrackerApplicationService> {
    pub(crate) application: S,
    pub(crate) tracking: TrackingState,
    pub(crate) clock: Option<ElapsedClock>,
    pub(crate) timezone: Tz,
    pub(crate) frozen_offset: Option<FixedOffset>,
    pub(crate) status: Status,
    lifecycle: Lifecycle,
    pub(crate) screen: ScreenState,
}

impl<S: TrackerApplicationService> App<S> {
    /// Builds presentation state from an already loaded application service.
    pub fn load(application: S) -> Self {
        let task_list = load_state(&application);
        let tracking = application.current_tracking().clone();
        let clock = match &tracking {
            TrackingState::Idle => None,
            TrackingState::Running { worklog } => Some(ElapsedClock::since(worklog.start())),
        };
        let (timezone, timezone_status) = startup_timezone();
        let status = match timezone_status {
            Some(message) => Status::Error(message.to_owned()),
            None if clock.is_some() => {
                Status::Info("Recovered the previous active timer".to_owned())
            }
            None => Status::Info("Ready".to_owned()),
        };
        Self {
            application,
            tracking,
            clock,
            timezone,
            frozen_offset: None,
            status,
            lifecycle: Lifecycle::Running,
            screen: ScreenState::TaskList(task_list),
        }
    }

    pub fn handle(&mut self, command: Command) {
        match command {
            Command::Quit => self.lifecycle = Lifecycle::Quitting,
            Command::OpenHistory => self.open_history(),
            Command::BackToTaskList => self.back_to_task_list(),
            command => match self.screen() {
                Screen::TaskList => self.handle_task_list_command(command),
                Screen::WorklogHistory => self.handle_worklog_history_command(command),
            },
        }
    }

    pub fn is_running(&self) -> bool {
        self.lifecycle == Lifecycle::Running
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn screen(&self) -> Screen {
        self.screen.screen()
    }

    pub(crate) fn task_list(&self) -> &TaskListState {
        self.screen.task_list()
    }

    pub(crate) fn task_list_mut(&mut self) -> &mut TaskListState {
        self.screen.task_list_mut()
    }

    pub(crate) fn task_name_for(&self, task_id: TaskId) -> Option<&str> {
        Some(self.application.task(task_id)?.name().as_str())
    }

    fn open_history(&mut self) {
        if !matches!(&self.screen, ScreenState::TaskList(state) if matches!(state.mode(), crate::screens::TaskListMode::Normal))
        {
            return;
        }
        let Some(task) = self.selected_task().cloned() else {
            return;
        };
        let result = self.application.worklogs_for_task(task.id(), None);
        self.sync_from_application(false);
        match result {
            Ok(page) => {
                let baseline = active_worklog_for_task(&page.snapshot.active_worklog, task.id());
                let history = History::new(task.id(), page.worklogs, page.next_cursor, baseline);
                let task_list = self.task_list().clone();
                self.screen = ScreenState::WorklogHistory(Box::new(WorklogHistoryState::new(
                    task_list, history,
                )));
                self.status = Status::Info(format!("History of \"{}\"", task.name()));
            }
            Err(error) => self.status = Status::Error(application_error_text(&error)),
        }
    }

    fn back_to_task_list(&mut self) {
        if !matches!(&self.screen, ScreenState::WorklogHistory(state) if matches!(state.mode(), crate::screens::WorklogHistoryMode::Normal))
        {
            return;
        }
        let ScreenState::WorklogHistory(state) = &self.screen else {
            return;
        };
        self.screen = ScreenState::TaskList(state.task_list_clone());
    }

    #[cfg(test)]
    pub(crate) fn set_history_next_cursor_for_tests(
        &mut self,
        cursor: tracker_application::WorklogCursor,
    ) {
        if let ScreenState::WorklogHistory(state) = &mut self.screen {
            state.set_next_cursor(cursor);
        }
    }
}
