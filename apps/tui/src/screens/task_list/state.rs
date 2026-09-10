use tracker_application::TaskOrdering;
use tracker_domain::{Task, TaskId};

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

/// Task-list data, navigation, and modal state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskListState {
    pub(super) tasks: Vec<Task>,
    pub(super) archived_tasks: Vec<Task>,
    pub(super) ordering: TaskOrdering,
    pub(super) view: TaskView,
    pub(super) active_selection: Option<TaskId>,
    pub(super) archived_selection: Option<TaskId>,
    pub(super) mode: TaskListMode,
}

impl TaskListState {
    pub(super) fn new(tasks: Vec<Task>, archived_tasks: Vec<Task>, ordering: TaskOrdering) -> Self {
        let active_selection = tasks.first().map(Task::id);
        Self {
            tasks,
            archived_tasks,
            ordering,
            view: TaskView::Active,
            active_selection,
            archived_selection: None,
            mode: TaskListMode::Normal,
        }
    }

    pub fn view(&self) -> TaskView {
        self.view
    }

    pub fn mode(&self) -> &TaskListMode {
        &self.mode
    }

    pub(super) fn tasks_in(&self, view: TaskView) -> &[Task] {
        match view {
            TaskView::Active => &self.tasks,
            TaskView::Archived => &self.archived_tasks,
        }
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
