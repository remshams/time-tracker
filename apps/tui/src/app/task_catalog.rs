use std::collections::HashMap;

use tracker_application::{TaskListItem, TaskOrdering};
use tracker_domain::{Task, TaskId};

use crate::screens::TaskView;
use crate::support::task_search::{SearchRank, fuzzy_match};

/// The authoritative in-memory task collections and their ordering.
pub(crate) struct TaskCatalog {
    active_tasks: Vec<Task>,
    archived_tasks: Vec<Task>,
    search_ranks: HashMap<TaskId, SearchRank>,
    ordering: TaskOrdering,
}

impl TaskCatalog {
    pub(crate) fn new(items: Vec<TaskListItem>) -> Self {
        let search_ranks = search_ranks(&items);
        let (active_tasks, archived_tasks) = split_tasks(items);
        Self {
            active_tasks,
            archived_tasks,
            search_ranks,
            ordering: TaskOrdering::default(),
        }
    }

    pub(crate) fn tasks(&self, view: TaskView) -> &[Task] {
        match view {
            TaskView::Active => &self.active_tasks,
            TaskView::Archived => &self.archived_tasks,
        }
    }

    pub(crate) fn visible_tasks(&self, view: TaskView, query: Option<&str>) -> Vec<&Task> {
        let Some(query) = query else {
            return self.tasks(view).iter().collect();
        };
        let mut matches = self
            .tasks(view)
            .iter()
            .filter(|task| fuzzy_match(task.name().as_str(), query))
            .collect::<Vec<_>>();
        matches.sort_by_key(|task| self.search_rank(task.id()));
        matches
    }

    pub(crate) fn search_rank(&self, id: TaskId) -> SearchRank {
        self.search_ranks[&id]
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
        self.search_ranks = search_ranks(&items);
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

fn search_ranks(items: &[TaskListItem]) -> HashMap<TaskId, SearchRank> {
    items
        .iter()
        .map(|item| {
            let updated = item.task.updated_at();
            let rank = SearchRank::new(
                item.task.id(),
                item.task.created_at(),
                item.latest_work_start
                    .map_or(updated, |worked| worked.max(updated)),
            );
            (item.task.id(), rank)
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use tracker_domain::TaskName;

    use super::*;

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(second, 0).unwrap()
    }

    fn item(name: &str, updated_at: i64, worked_at: Option<i64>) -> TaskListItem {
        TaskListItem {
            task: Task::rehydrate(
                TaskId::generate(),
                TaskName::new(name).unwrap(),
                false,
                at(100),
                at(updated_at),
            )
            .unwrap(),
            latest_work_start: worked_at.map(at),
        }
    }

    #[test]
    fn search_filters_fuzzily_and_ranks_by_newer_update_or_work() {
        let catalog = TaskCatalog::new(vec![
            item("Book travel", 800, None),
            item("Backlog triage", 300, Some(700)),
            item("Build testing", 200, Some(900)),
            item("Update docs", 950, None),
        ]);
        let names = |query| {
            catalog
                .visible_tasks(TaskView::Active, query)
                .iter()
                .map(|task| task.name().as_str().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(Some("bT")),
            ["Build testing", "Book travel", "Backlog triage"]
        );
        assert_eq!(
            names(None),
            [
                "Book travel",
                "Backlog triage",
                "Build testing",
                "Update docs"
            ]
        );
    }

    #[test]
    fn reload_updates_search_activity_without_changing_task_identity() {
        let initial = item("Book travel", 300, None);
        let book_id = initial.task.id();
        let other = item("Build testing", 200, Some(900));
        let mut catalog = TaskCatalog::new(vec![initial, other.clone()]);
        assert_eq!(
            catalog.visible_tasks(TaskView::Active, Some("bt"))[0].id(),
            other.task.id()
        );
        let renamed = TaskListItem {
            task: Task::rehydrate(
                book_id,
                TaskName::new("Book travel").unwrap(),
                false,
                at(100),
                at(1_000),
            )
            .unwrap(),
            latest_work_start: None,
        };
        catalog.reload(vec![other, renamed], TaskView::Active, Some(book_id));
        assert_eq!(
            catalog.visible_tasks(TaskView::Active, Some("bt"))[0].id(),
            book_id
        );
    }

    #[test]
    fn reload_selects_the_preceding_row_when_the_last_task_disappears() {
        let first = item("First task", 100, None);
        let second = item("Second task", 200, None);
        let last = item("Last task", 300, None);
        let selected = last.task.id();
        let expected = second.task.id();
        let mut catalog = TaskCatalog::new(vec![first.clone(), second.clone(), last]);

        let resolved = catalog.reload(vec![first, second], TaskView::Active, Some(selected));
        assert_eq!(resolved, Some(expected));
    }
}
