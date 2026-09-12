use tracker_application::{TaskListItem, TaskOrdering};
use tracker_domain::{Task, TaskId};

use crate::screens::TaskView;

/// The authoritative in-memory task collections and their ordering.
pub(crate) struct TaskCatalog {
    active_tasks: Vec<Task>,
    archived_tasks: Vec<Task>,
    ordering: TaskOrdering,
}

impl TaskCatalog {
    pub(crate) fn new(items: Vec<TaskListItem>) -> Self {
        let (active_tasks, archived_tasks) = split_tasks(items);
        Self {
            active_tasks,
            archived_tasks,
            ordering: TaskOrdering::default(),
        }
    }

    pub(crate) fn tasks(&self, view: TaskView) -> &[Task] {
        match view {
            TaskView::Active => &self.active_tasks,
            TaskView::Archived => &self.archived_tasks,
        }
    }

    pub(crate) fn task(&self, id: TaskId) -> Option<&Task> {
        self.active_tasks
            .iter()
            .chain(&self.archived_tasks)
            .find(|task| task.id() == id)
    }

    pub(crate) fn ordering(&self) -> TaskOrdering {
        self.ordering
    }

    pub(crate) fn ordering_label(&self) -> &'static str {
        match self.ordering {
            TaskOrdering::RecentlyWorked => "recently worked",
            TaskOrdering::RecentlyUpdated => "recently updated",
            TaskOrdering::RecentlyCreated => "recently created",
        }
    }

    pub(crate) fn cycle_ordering(&mut self) {
        self.ordering = match self.ordering {
            TaskOrdering::RecentlyWorked => TaskOrdering::RecentlyUpdated,
            TaskOrdering::RecentlyUpdated => TaskOrdering::RecentlyCreated,
            TaskOrdering::RecentlyCreated => TaskOrdering::RecentlyWorked,
        };
    }

    /// Reloads task data and resolves the visible stable-ID selection.
    pub(crate) fn reload(
        &mut self,
        items: Vec<TaskListItem>,
        view: TaskView,
        selected: Option<TaskId>,
    ) -> Option<TaskId> {
        let previous_index = selected_index(self.tasks(view), selected);
        (self.active_tasks, self.archived_tasks) = split_tasks(items);
        let visible = self.tasks(view);
        selected
            .filter(|id| visible.iter().any(|task| task.id() == *id))
            .or_else(|| {
                previous_index
                    .filter(|_| !visible.is_empty())
                    .and_then(|index| visible.get(index.min(visible.len() - 1)))
                    .map(Task::id)
            })
    }
}

fn selected_index(tasks: &[Task], selected: Option<TaskId>) -> Option<usize> {
    selected.and_then(|id| tasks.iter().position(|task| task.id() == id))
}

fn split_tasks(items: Vec<TaskListItem>) -> (Vec<Task>, Vec<Task>) {
    items
        .into_iter()
        .map(|item| item.task)
        .partition(|task| !task.is_archived())
}
