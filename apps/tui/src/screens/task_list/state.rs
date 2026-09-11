use tracker_domain::TaskId;

/// What a confirmed task-name input does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputPurpose {
    Add,
    Rename { task_id: TaskId },
}

/// The modes valid while the task list is visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskListMode {
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

/// Which task list the interface currently shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskView {
    Active,
    Archived,
}

/// Task-list navigation and modal state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskListState {
    pub(super) view: TaskView,
    pub(super) active_selection: Option<TaskId>,
    pub(super) archived_selection: Option<TaskId>,
    pub(super) mode: TaskListMode,
}

impl TaskListState {
    pub(crate) fn new(active_selection: Option<TaskId>) -> Self {
        Self {
            view: TaskView::Active,
            active_selection,
            archived_selection: None,
            mode: TaskListMode::Normal,
        }
    }

    pub fn view(&self) -> TaskView {
        self.view
    }

    #[cfg(test)]
    pub(crate) fn set_view_for_test(&mut self, view: TaskView) {
        self.view = view;
    }

    pub fn mode(&self) -> &TaskListMode {
        &self.mode
    }

    #[cfg(test)]
    pub(crate) fn set_mode_for_test(&mut self, mode: TaskListMode) {
        self.mode = mode;
    }

    pub(super) fn selection(&self) -> Option<TaskId> {
        match self.view {
            TaskView::Active => self.active_selection,
            TaskView::Archived => self.archived_selection,
        }
    }

    pub(super) fn set_selection(&mut self, selected: Option<TaskId>) {
        match self.view {
            TaskView::Active => self.active_selection = selected,
            TaskView::Archived => self.archived_selection = selected,
        }
    }
}
