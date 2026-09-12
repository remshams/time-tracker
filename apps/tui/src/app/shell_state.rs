use chrono_tz::Tz;

use crate::screens::{InputState, Screen, ScreenState, TaskListState, WorklogHistoryState};

use super::Status;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lifecycle {
    Running,
    Quitting,
}

/// Exclusive screen state, status reporting, process lifecycle, and timezone.
pub(crate) struct ShellState {
    screen: ScreenState,
    status: Status,
    lifecycle: Lifecycle,
    timezone: Tz,
}

impl ShellState {
    pub(crate) fn new(status: Status, task_list: TaskListState, timezone: Tz) -> Self {
        Self {
            screen: ScreenState::TaskList(task_list),
            status,
            lifecycle: Lifecycle::Running,
            timezone,
        }
    }

    pub(crate) fn is_running(&self) -> bool {
        self.lifecycle == Lifecycle::Running
    }

    pub(crate) fn quit(&mut self) {
        self.lifecycle = Lifecycle::Quitting;
    }

    pub(crate) fn status(&self) -> &Status {
        &self.status
    }

    pub(crate) fn info(&mut self, message: impl Into<String>) {
        self.status = Status::Info(message.into());
    }

    pub(crate) fn error(&mut self, message: impl Into<String>) {
        self.status = Status::Error(message.into());
    }

    pub(crate) fn screen(&self) -> Screen {
        self.screen.screen()
    }

    pub(crate) fn screen_state(&self) -> &ScreenState {
        &self.screen
    }

    pub(crate) fn task_list(&self) -> &TaskListState {
        match &self.screen {
            ScreenState::TaskList(state) => state,
            ScreenState::WorklogHistory(state) => state.task_list(),
        }
    }

    pub(crate) fn task_list_mut(&mut self) -> &mut TaskListState {
        match &mut self.screen {
            ScreenState::TaskList(state) => state,
            ScreenState::WorklogHistory(state) => state.task_list_mut(),
        }
    }

    pub(crate) fn input_state(&self) -> InputState<'_> {
        match &self.screen {
            ScreenState::TaskList(state) => InputState::TaskList(state),
            ScreenState::WorklogHistory(state) => InputState::WorklogHistory(state),
        }
    }

    pub(crate) fn history(&self) -> Option<&WorklogHistoryState> {
        match &self.screen {
            ScreenState::TaskList(_) => None,
            ScreenState::WorklogHistory(state) => Some(state),
        }
    }

    pub(crate) fn history_mut(&mut self) -> Option<&mut WorklogHistoryState> {
        match &mut self.screen {
            ScreenState::TaskList(_) => None,
            ScreenState::WorklogHistory(state) => Some(state),
        }
    }

    pub(crate) fn timezone(&self) -> Tz {
        self.timezone
    }

    pub(crate) fn open_history(&mut self, history: crate::screens::History) {
        let screen = std::mem::replace(
            &mut self.screen,
            ScreenState::TaskList(TaskListState::new(None)),
        );
        let ScreenState::TaskList(task_list) = screen else {
            unreachable!("history can open only from the task list");
        };
        self.screen =
            ScreenState::WorklogHistory(Box::new(WorklogHistoryState::new(task_list, history)));
    }

    pub(crate) fn back_to_task_list(&mut self) {
        let screen = std::mem::replace(
            &mut self.screen,
            ScreenState::TaskList(TaskListState::new(None)),
        );
        let ScreenState::WorklogHistory(history) = screen else {
            unreachable!("the task list is already open");
        };
        self.screen = ScreenState::TaskList(history.into_task_list());
    }
}
