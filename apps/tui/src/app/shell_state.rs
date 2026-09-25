use chrono_tz::Tz;

use crate::screens::{
    InputState, ReportState, Screen, ScreenState, TaskListState, TaskView, WorklogHistoryState,
};

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
    report_return: Option<ReportState>,
    task_list_return: Option<TaskListState>,
}

impl ShellState {
    pub(crate) fn new(status: Status, task_list: TaskListState, timezone: Tz) -> Self {
        Self {
            screen: ScreenState::TaskList(task_list),
            status,
            lifecycle: Lifecycle::Running,
            timezone,
            report_return: None,
            task_list_return: None,
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
            ScreenState::Reports(_) => self.task_list_return.as_ref().expect("task list is saved"),
        }
    }

    pub(crate) fn task_list_mut(&mut self) -> &mut TaskListState {
        match &mut self.screen {
            ScreenState::TaskList(state) => state,
            ScreenState::WorklogHistory(state) => state.task_list_mut(),
            ScreenState::Reports(_) => self.task_list_return.as_mut().expect("task list is saved"),
        }
    }

    pub(crate) fn input_state(&self) -> InputState<'_> {
        match &self.screen {
            ScreenState::TaskList(state) => InputState::TaskList(state),
            ScreenState::WorklogHistory(state) => InputState::WorklogHistory(state),
            ScreenState::Reports(state) => InputState::Reports(state),
        }
    }

    pub(crate) fn history(&self) -> Option<&WorklogHistoryState> {
        match &self.screen {
            ScreenState::TaskList(_) => None,
            ScreenState::WorklogHistory(state) => Some(state),
            ScreenState::Reports(_) => None,
        }
    }

    pub(crate) fn history_mut(&mut self) -> Option<&mut WorklogHistoryState> {
        match &mut self.screen {
            ScreenState::TaskList(_) => None,
            ScreenState::WorklogHistory(state) => Some(state),
            ScreenState::Reports(_) => None,
        }
    }

    pub(crate) fn timezone(&self) -> Tz {
        self.timezone
    }

    pub(crate) fn report(&self) -> Option<&ReportState> {
        match &self.screen {
            ScreenState::Reports(state) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn report_mut(&mut self) -> Option<&mut ReportState> {
        match &mut self.screen {
            ScreenState::Reports(state) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn open_reports(&mut self, now: chrono::DateTime<chrono::Utc>) {
        let previous = std::mem::replace(
            &mut self.screen,
            ScreenState::Reports(Box::new(ReportState::new(now, self.timezone))),
        );
        let ScreenState::TaskList(list) = previous else {
            unreachable!("task list is open")
        };
        self.task_list_return = Some(list);
    }

    pub(crate) fn leave_reports(&mut self, view: TaskView, first: Option<tracker_domain::TaskId>) {
        let mut list = self
            .task_list_return
            .take()
            .unwrap_or_else(|| TaskListState::new(None));
        list.show(view, first);
        self.screen = ScreenState::TaskList(list);
    }

    pub(crate) fn open_report_history(&mut self, history: crate::screens::History) {
        let old = std::mem::replace(
            &mut self.screen,
            ScreenState::TaskList(TaskListState::new(None)),
        );
        let ScreenState::Reports(report) = old else {
            unreachable!("report is open")
        };
        self.report_return = Some(*report);
        self.screen = ScreenState::WorklogHistory(Box::new(WorklogHistoryState::from_reports(
            TaskListState::new(None),
            history,
        )));
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
        if let Some(report) = self.report_return.take() {
            self.screen = ScreenState::Reports(Box::new(report));
            return;
        }
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
