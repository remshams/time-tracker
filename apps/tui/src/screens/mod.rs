pub mod task_list;
pub mod worklog_history;

use tracker_domain::{TaskId, Worklog};

pub use task_list::{InputPurpose, TaskListMode, TaskListState, TaskView};
pub use worklog_history::{CorrectionDraft, History, WorklogHistoryMode, WorklogHistoryState};

/// Which screen the interface currently shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    TaskList,
    WorklogHistory,
}

/// The only valid stored screen states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenState {
    TaskList(TaskListState),
    WorklogHistory(Box<WorklogHistoryState>),
}

impl ScreenState {
    pub fn screen(&self) -> Screen {
        match self {
            Self::TaskList(_) => Screen::TaskList,
            Self::WorklogHistory(_) => Screen::WorklogHistory,
        }
    }

    pub(crate) fn task_list(&self) -> &TaskListState {
        match self {
            Self::TaskList(state) => state,
            Self::WorklogHistory(state) => state.task_list(),
        }
    }

    pub(crate) fn task_list_mut(&mut self) -> &mut TaskListState {
        match self {
            Self::TaskList(state) => state,
            Self::WorklogHistory(state) => state.task_list_mut(),
        }
    }

    pub(crate) fn mode(&self) -> Mode {
        match self {
            Self::TaskList(state) => state.mode().into(),
            Self::WorklogHistory(state) => state.mode().into(),
        }
    }
}

/// A read-only mode projection used by the current keymap and renderer.
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
    ConfirmDeletion {
        worklog: Worklog,
    },
    Correction(CorrectionDraft),
}

impl From<&TaskListMode> for Mode {
    fn from(mode: &TaskListMode) -> Self {
        match mode {
            TaskListMode::Normal => Self::Normal,
            TaskListMode::Input { purpose, buffer } => Self::Input {
                purpose: *purpose,
                buffer: buffer.clone(),
            },
            TaskListMode::ConfirmArchive { task_id, name } => Self::ConfirmArchive {
                task_id: *task_id,
                name: name.clone(),
            },
        }
    }
}

impl From<&WorklogHistoryMode> for Mode {
    fn from(mode: &WorklogHistoryMode) -> Self {
        match mode {
            WorklogHistoryMode::Normal => Self::Normal,
            WorklogHistoryMode::ConfirmDeletion { worklog } => Self::ConfirmDeletion {
                worklog: worklog.clone(),
            },
            WorklogHistoryMode::Correction(draft) => Self::Correction(draft.clone()),
        }
    }
}
