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
    view: TaskView,
    active_selection: Option<TaskId>,
    archived_selection: Option<TaskId>,
    mode: TaskListMode,
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

    pub fn mode(&self) -> &TaskListMode {
        &self.mode
    }

    pub(crate) fn accepts_unarchiving(&self) -> bool {
        self.view == TaskView::Archived && matches!(self.mode, TaskListMode::Normal)
    }

    pub(crate) fn selection(&self) -> Option<TaskId> {
        match self.view {
            TaskView::Active => self.active_selection,
            TaskView::Archived => self.archived_selection,
        }
    }

    pub(crate) fn set_selection(&mut self, selected: Option<TaskId>) {
        self.remember(self.view, selected);
    }

    pub(crate) fn remember(&mut self, view: TaskView, selected: Option<TaskId>) {
        match view {
            TaskView::Active => self.active_selection = selected,
            TaskView::Archived => self.archived_selection = selected,
        }
    }

    pub(crate) fn show(&mut self, view: TaskView, initial_selection: Option<TaskId>) {
        if self.view == view {
            return;
        }
        self.view = view;
        if self.selection().is_none() {
            self.set_selection(initial_selection);
        }
    }

    pub(crate) fn open_input(&mut self, purpose: InputPurpose, buffer: String) {
        self.mode = TaskListMode::Input { purpose, buffer };
    }

    pub(crate) fn open_archive_confirmation(&mut self, task_id: TaskId, name: String) {
        self.mode = TaskListMode::ConfirmArchive { task_id, name };
    }

    pub(crate) fn insert_name(&mut self, character: char, limit: usize) {
        if let TaskListMode::Input { buffer, .. } = &mut self.mode
            && buffer.chars().count() < limit
        {
            buffer.push(character);
        }
    }

    pub(crate) fn backspace_name(&mut self) {
        if let TaskListMode::Input { buffer, .. } = &mut self.mode {
            buffer.pop();
        }
    }

    pub(crate) fn close_mode(&mut self) {
        self.mode = TaskListMode::Normal;
    }
}
