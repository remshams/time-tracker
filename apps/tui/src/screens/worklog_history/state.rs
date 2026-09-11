use chrono::{DateTime, Utc};
use tracker_application::WorklogCursor;
use tracker_domain::{TaskId, Worklog, WorklogId};

use crate::screens::task_list::TaskListState;

use super::correction::CorrectionDraft;

/// Whether the open history holds a valid page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryAvailability {
    Available,
    Unavailable,
}

/// Worklog pages loaded for one task, newest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct History {
    pub task_id: TaskId,
    pub availability: HistoryAvailability,
    pub worklogs: Vec<Worklog>,
    pub next_cursor: Option<WorklogCursor>,
    pub(super) active_worklog_baseline: Option<(WorklogId, DateTime<Utc>)>,
    pub(super) selected: Option<WorklogId>,
}

impl History {
    pub(crate) fn new(
        task_id: TaskId,
        worklogs: Vec<Worklog>,
        next_cursor: Option<WorklogCursor>,
        active_worklog_baseline: Option<(WorklogId, DateTime<Utc>)>,
    ) -> Self {
        let selected = worklogs.first().map(Worklog::id);
        Self {
            task_id,
            availability: HistoryAvailability::Available,
            worklogs,
            next_cursor,
            active_worklog_baseline,
            selected,
        }
    }

    pub fn is_available(&self) -> bool {
        self.availability == HistoryAvailability::Available
    }

    #[cfg(test)]
    pub(crate) fn set_unavailable_for_test(&mut self) {
        self.availability = HistoryAvailability::Unavailable;
    }

    pub(super) fn selected_index(&self) -> Option<usize> {
        let id = self.selected?;
        self.is_available()
            .then(|| self.worklogs.iter().position(|worklog| worklog.id() == id))
            .flatten()
    }
}

/// The modes valid while worklog history is visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorklogHistoryMode {
    Normal,
    ConfirmDeletion { worklog: Worklog },
    Correction(CorrectionDraft),
}

/// History state and the exact task-list navigation restored on return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorklogHistoryState {
    pub(super) task_list: TaskListState,
    pub(super) history: History,
    pub(super) mode: WorklogHistoryMode,
}

impl WorklogHistoryState {
    pub(crate) fn new(task_list: TaskListState, history: History) -> Self {
        Self {
            task_list,
            history,
            mode: WorklogHistoryMode::Normal,
        }
    }

    pub fn task_list(&self) -> &TaskListState {
        &self.task_list
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    pub fn mode(&self) -> &WorklogHistoryMode {
        &self.mode
    }

    #[cfg(test)]
    pub(crate) fn set_mode_for_test(&mut self, mode: WorklogHistoryMode) {
        self.mode = mode;
    }

    pub(crate) fn task_list_mut(&mut self) -> &mut TaskListState {
        &mut self.task_list
    }

    #[cfg(test)]
    pub(crate) fn set_history_unavailable_for_test(&mut self) {
        self.history.set_unavailable_for_test();
    }

    #[cfg(test)]
    pub(crate) fn set_next_cursor(&mut self, cursor: WorklogCursor) {
        self.history.next_cursor = Some(cursor);
    }
}

pub(crate) fn active_worklog_for_task(
    active_worklog: &Option<Worklog>,
    task_id: TaskId,
) -> Option<(WorklogId, DateTime<Utc>)> {
    active_worklog
        .as_ref()
        .filter(|worklog| worklog.task_id() == task_id)
        .map(|worklog| (worklog.id(), worklog.start()))
}
