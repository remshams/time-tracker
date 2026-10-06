use tracker_domain::{TaskId, TaskName, Worklog};

use tracker_application::{MoveCandidate, TaskListItem, move_candidates_for_tasks};

/// Which part of the move dialog receives keyboard input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveFocus {
    Search,
    Results,
}

/// The selected worklog and filtered destination candidates for a move dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveDraft {
    worklog: Worklog,
    tasks: Vec<TaskListItem>,
    query: String,
    results: Vec<MoveCandidate>,
    selected: Option<usize>,
    focus: MoveFocus,
}

impl MoveDraft {
    pub(crate) fn new(worklog: Worklog, tasks: Vec<TaskListItem>) -> Self {
        let mut draft = Self {
            worklog,
            tasks,
            query: String::new(),
            results: Vec::new(),
            selected: None,
            focus: MoveFocus::Search,
        };
        draft.refilter();
        draft
    }

    pub(crate) fn worklog(&self) -> &Worklog {
        &self.worklog
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn focus(&self) -> MoveFocus {
        self.focus
    }

    pub(crate) fn results(&self) -> impl Iterator<Item = &MoveCandidate> {
        self.results.iter()
    }

    pub(crate) fn result_count(&self) -> usize {
        self.results.len()
    }

    pub(crate) fn selected_task_id(&self) -> Option<TaskId> {
        self.selected
            .and_then(|index| self.results.get(index))
            .map(|candidate| candidate.id)
    }

    pub(crate) fn selected_result_index(&self) -> Option<usize> {
        self.selected
    }

    pub(crate) fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            MoveFocus::Search => MoveFocus::Results,
            MoveFocus::Results => MoveFocus::Search,
        };
    }

    pub(crate) fn move_up(&mut self) {
        if let Some(index) = self.selected {
            self.selected = Some(index.saturating_sub(1));
        }
    }

    pub(crate) fn move_down(&mut self) {
        if let Some(index) = self.selected {
            self.selected = Some((index + 1).min(self.results.len() - 1));
        }
    }

    pub(crate) fn insert(&mut self, character: char) {
        if self.query.chars().count() >= TaskName::MAX_LEN {
            return;
        }
        self.query.push(character);
        self.refilter();
    }

    pub(crate) fn backspace(&mut self) {
        self.query.pop();
        self.refilter();
    }

    fn refilter(&mut self) {
        self.results = move_candidates_for_tasks(&self.tasks, self.worklog.task_id(), &self.query);
        self.selected = (!self.results.is_empty()).then_some(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use tracker_application::task_search::fuzzy_match;
    use tracker_domain::{Task, WorklogId, WorklogTimes};

    fn worklog() -> Worklog {
        Worklog::new(
            WorklogId::generate(),
            TaskId::generate(),
            DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
            Some(DateTime::<Utc>::from_timestamp(60, 0).unwrap()),
        )
        .unwrap()
    }

    fn candidate(name: &str, created_seconds: i64, activity_seconds: i64) -> TaskListItem {
        TaskListItem {
            task: Task::rehydrate(
                TaskId::generate(),
                TaskName::new(name).unwrap(),
                false,
                DateTime::<Utc>::from_timestamp(created_seconds, 0).unwrap(),
                DateTime::<Utc>::from_timestamp(activity_seconds, 0).unwrap(),
            )
            .unwrap(),
            latest_work_start: None,
        }
    }

    #[test]
    fn fuzzy_search_keeps_non_contiguous_case_insensitive_matches() {
        let build = candidate("Build release", 0, 1);
        let blue = candidate("blue sky", 0, 2);
        let mut draft = MoveDraft::new(worklog(), vec![build.clone(), blue.clone()]);
        draft.insert('b');
        draft.insert('u');
        assert_eq!(
            draft
                .results()
                .map(|candidate| candidate.id)
                .collect::<Vec<_>>(),
            vec![blue.task.id(), build.task.id()]
        );
        assert!(fuzzy_match("Build release", "BSE"));
        assert!(!fuzzy_match("Build release", "BX"));
    }

    #[test]
    fn filtering_ranks_by_recent_activity_then_creation() {
        let alpha = candidate("alpha", 2, 4);
        let alpine = candidate("alpine", 3, 4);
        let beta = candidate("beta", 1, 1);
        let mut draft = MoveDraft::new(worklog(), vec![alpha.clone(), alpine.clone(), beta]);
        assert_eq!(draft.selected_task_id(), Some(alpine.task.id()));
        draft.move_down();
        draft.insert('a');
        assert_eq!(draft.selected_task_id(), Some(alpine.task.id()));
        draft.insert('l');
        assert_eq!(draft.selected_task_id(), Some(alpine.task.id()));
        draft.insert('z');
        assert_eq!(draft.selected_task_id(), None);
        draft.backspace();
        assert_eq!(draft.selected_task_id(), Some(alpine.task.id()));
    }

    #[test]
    fn focus_and_result_navigation_handle_empty_results() {
        let mut draft = MoveDraft::new(worklog(), Vec::new());
        assert_eq!(draft.focus(), MoveFocus::Search);
        draft.toggle_focus();
        assert_eq!(draft.focus(), MoveFocus::Results);
        draft.move_up();
        draft.move_down();
        assert_eq!(draft.selected_task_id(), None);
        assert_eq!(draft.selected_result_index(), None);
        assert_eq!(draft.result_count(), 0);
        assert_eq!(
            draft.worklog().times(),
            WorklogTimes::new(
                DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
                Some(DateTime::<Utc>::from_timestamp(60, 0).unwrap()),
            )
        );
    }

    #[test]
    fn result_navigation_moves_and_clamps_the_selected_index() {
        let mut draft = MoveDraft::new(
            worklog(),
            vec![
                candidate("alpha", 0, 1),
                candidate("beta", 0, 2),
                candidate("gamma", 0, 3),
            ],
        );
        assert_eq!(draft.result_count(), 3);
        assert_eq!(draft.selected_result_index(), Some(0));
        draft.move_down();
        assert_eq!(draft.selected_result_index(), Some(1));
        draft.move_down();
        draft.move_down();
        assert_eq!(draft.selected_result_index(), Some(2));
        draft.move_up();
        assert_eq!(draft.selected_result_index(), Some(1));
        draft.move_up();
        draft.move_up();
        assert_eq!(draft.selected_result_index(), Some(0));
    }

    #[test]
    fn search_input_is_bounded_to_the_task_name_limit() {
        let mut draft = MoveDraft::new(worklog(), vec![candidate("alpha", 0, 1)]);
        for _ in 0..=TaskName::MAX_LEN {
            draft.insert('a');
        }
        assert_eq!(draft.query().chars().count(), TaskName::MAX_LEN);
        assert_eq!(draft.selected_task_id(), None);
    }

    #[test]
    fn fuzzy_matching_handles_the_maximum_query() {
        let text = "a".repeat(TaskName::MAX_LEN);
        assert!(fuzzy_match(&text, &text));
        assert!(!fuzzy_match(&text, &format!("{text}a")));
    }
}
