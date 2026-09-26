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
    Search,
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
    search_query: Option<String>,
    search_snapshot: Option<(Option<String>, Option<TaskId>)>,
    before_filter_selection: Option<TaskId>,
    g_prefix: bool,
}

impl TaskListState {
    pub(crate) fn new(active_selection: Option<TaskId>) -> Self {
        Self {
            view: TaskView::Active,
            active_selection,
            archived_selection: None,
            mode: TaskListMode::Normal,
            search_query: None,
            search_snapshot: None,
            before_filter_selection: None,
            g_prefix: false,
        }
    }

    pub fn view(&self) -> TaskView {
        self.view
    }

    pub fn mode(&self) -> &TaskListMode {
        &self.mode
    }

    pub(crate) fn search_query(&self) -> Option<&str> {
        self.search_query.as_deref()
    }

    pub(crate) fn open_search(&mut self) {
        self.search_snapshot = Some((self.search_query.clone(), self.selection()));
        if self.search_query.is_none() {
            self.before_filter_selection = self.selection();
            self.search_query = Some(String::new());
        }
        self.mode = TaskListMode::Search;
    }

    pub(crate) fn insert_search(&mut self, character: char, limit: usize) {
        if let Some(query) = &mut self.search_query
            && query.chars().count() < limit
        {
            query.push(character);
        }
    }

    pub(crate) fn backspace_search(&mut self) {
        if let Some(query) = &mut self.search_query {
            query.pop();
        }
    }

    pub(crate) fn commit_search(&mut self) {
        if self.search_query.as_deref() == Some("") {
            self.search_query = None;
            if let Some((_, selection)) = &self.search_snapshot {
                self.set_selection(*selection);
            }
            self.before_filter_selection = None;
        }
        self.search_snapshot = None;
        self.mode = TaskListMode::Normal;
    }

    pub(crate) fn cancel_search(&mut self) {
        if let Some((query, selection)) = self.search_snapshot.take() {
            self.search_query = query;
            self.set_selection(selection);
        }
        if self.search_query.is_none() {
            self.before_filter_selection = None;
        }
        self.mode = TaskListMode::Normal;
    }

    pub(crate) fn clear_search(&mut self) {
        self.search_query = None;
        if self.selection().is_none() {
            self.set_selection(self.before_filter_selection);
        }
        self.before_filter_selection = None;
    }

    pub(crate) fn accepts_unarchiving(&self) -> bool {
        self.view == TaskView::Archived && matches!(self.mode, TaskListMode::Normal)
    }

    pub(crate) fn selection(&self) -> Option<TaskId> {
        self.selection_for(self.view)
    }

    pub(crate) fn selection_for(&self, view: TaskView) -> Option<TaskId> {
        match view {
            TaskView::Active => self.active_selection,
            TaskView::Archived => self.archived_selection,
        }
    }

    pub(crate) fn g_prefix(&self) -> bool {
        self.g_prefix
    }

    pub(crate) fn set_g_prefix(&mut self, value: bool) {
        self.g_prefix = value;
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
        self.clear_search();
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
