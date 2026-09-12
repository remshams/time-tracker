use std::time::Duration;

use chrono::{DateTime, Utc};
#[cfg(test)]
use tracker_application::TaskOrdering;
#[cfg(test)]
use tracker_domain::WorklogId;
use tracker_domain::{Task, TaskId, Worklog};

#[cfg(test)]
use crate::screens::{CorrectionDraft, Screen, TaskView, WorklogHistoryMode};
use crate::screens::{History, InputState, ScreenState, TaskListState, WorklogHistoryState};
use crate::support::timestamps::local_time;

use super::Status;
use super::shell_state::ShellState;
use super::task_catalog::TaskCatalog;
use super::tracking_session::TrackingSession;

/// A read-only projection of the presentation state used by rendering and input mapping.
#[derive(Clone, Copy)]
pub(crate) struct AppView<'a> {
    catalog: &'a TaskCatalog,
    tracking: &'a TrackingSession,
    shell: &'a ShellState,
}

impl<'a> AppView<'a> {
    pub(super) fn new(
        catalog: &'a TaskCatalog,
        tracking: &'a TrackingSession,
        shell: &'a ShellState,
    ) -> Self {
        Self {
            catalog,
            tracking,
            shell,
        }
    }

    pub(crate) fn status(self) -> &'a Status {
        self.shell.status()
    }

    #[cfg(test)]
    pub(crate) fn screen(self) -> Screen {
        self.shell.screen()
    }

    pub(crate) fn screen_state(self) -> &'a ScreenState {
        self.shell.screen_state()
    }

    pub(crate) fn task_list(self) -> &'a TaskListState {
        self.shell.task_list()
    }

    pub(crate) fn input_state(self) -> InputState<'a> {
        self.shell.input_state()
    }

    pub(crate) fn footer_hints(self, width: u16) -> &'static str {
        self.input_state().footer_hints(width)
    }

    pub(crate) fn tasks(self) -> &'a [Task] {
        self.catalog.tasks(self.task_list().view())
    }

    #[cfg(test)]
    pub(crate) fn view(self) -> TaskView {
        self.task_list().view()
    }

    #[cfg(test)]
    pub(crate) fn selected(self) -> Option<usize> {
        let selected = self.task_list().selection()?;
        self.tasks().iter().position(|task| task.id() == selected)
    }

    #[cfg(test)]
    pub(crate) fn ordering(self) -> TaskOrdering {
        self.catalog.ordering()
    }

    pub(crate) fn ordering_label(self) -> &'static str {
        self.catalog.ordering_label()
    }

    pub(crate) fn active_task_id(self) -> Option<TaskId> {
        self.tracking.active_task_id()
    }

    #[cfg(test)]
    pub(crate) fn active_worklog_id(self) -> Option<WorklogId> {
        self.tracking.active_worklog_id()
    }

    pub(crate) fn active_task_name(self) -> Option<&'a str> {
        self.catalog
            .task(self.active_task_id()?)
            .map(|task| task.name().as_str())
    }

    pub(crate) fn elapsed(self) -> Option<Duration> {
        self.tracking.elapsed()
    }

    pub(crate) fn history_state(self) -> Option<&'a WorklogHistoryState> {
        self.shell.history()
    }

    pub(crate) fn history(self) -> Option<&'a History> {
        self.history_state().map(WorklogHistoryState::history)
    }

    #[cfg(test)]
    pub(crate) fn history_selected_index(self) -> Option<usize> {
        self.history()?.selected_index()
    }

    #[cfg(test)]
    pub(crate) fn correction(self) -> Option<&'a CorrectionDraft> {
        self.history_state()
            .and_then(WorklogHistoryState::correction)
    }

    #[cfg(test)]
    pub(crate) fn deletion(self) -> Option<&'a Worklog> {
        match self.history_state()?.mode() {
            WorklogHistoryMode::ConfirmDeletion { worklog } => Some(worklog),
            _ => None,
        }
    }

    pub(crate) fn history_task_name(self) -> Option<&'a str> {
        self.catalog
            .task(self.history()?.task_id())
            .map(|task| task.name().as_str())
    }

    pub(crate) fn history_row_duration(self, worklog: &Worklog) -> Duration {
        self.tracking.row_duration(worklog)
    }

    pub(crate) fn local_time(self, at: DateTime<Utc>) -> String {
        local_time(at, &self.shell.timezone())
    }
}
