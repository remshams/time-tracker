use tracker_domain::{TaskId, TaskName, Worklog};

/// Which part of the move dialog receives keyboard input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveFocus {
    Search,
    Results,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MoveCandidate {
    id: TaskId,
    name: String,
}

impl MoveCandidate {
    pub(crate) fn new(id: TaskId, name: String) -> Self {
        Self { id, name }
    }

    pub(crate) fn id(&self) -> TaskId {
        self.id
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }
}

/// The selected worklog and filtered destination candidates for a move dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveDraft {
    worklog: Worklog,
    candidates: Vec<MoveCandidate>,
    query: String,
    results: Vec<usize>,
    selected: Option<usize>,
    focus: MoveFocus,
}

impl MoveDraft {
    pub(crate) fn new(worklog: Worklog, candidates: Vec<MoveCandidate>) -> Self {
        let mut draft = Self {
            worklog,
            candidates,
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
        self.results.iter().map(|index| &self.candidates[*index])
    }

    pub(crate) fn result_count(&self) -> usize {
        self.results.len()
    }

    pub(crate) fn selected_task_id(&self) -> Option<TaskId> {
        self.selected
            .and_then(|index| self.results.get(index))
            .map(|index| self.candidates[*index].id())
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
        self.selected = match self.selected {
            Some(index) => Some(index.saturating_sub(1)),
            None => self.results.len().checked_sub(1),
        };
    }

    pub(crate) fn move_down(&mut self) {
        self.selected = match self.selected {
            Some(index) => Some((index + 1).min(self.results.len().saturating_sub(1))),
            None if !self.results.is_empty() => Some(0),
            None => None,
        };
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
        let mut results = self
            .candidates
            .iter()
            .enumerate()
            .filter_map(|(index, candidate)| {
                fuzzy_score(&candidate.name, &self.query).map(|score| (index, score))
            })
            .collect::<Vec<_>>();
        results.sort_by(|(_, left), (_, right)| right.cmp(left));
        self.results = results.into_iter().map(|(index, _)| index).collect();
        self.selected = (!self.results.is_empty()).then_some(0);
    }
}

/// Scores a non-contiguous case-insensitive match. The tuple sorts matches by
/// adjacent characters first, then by character positions at word starts.
fn fuzzy_score(candidate: &str, query: &str) -> Option<(usize, usize)> {
    let query = query
        .chars()
        .flat_map(char::to_lowercase)
        .collect::<Vec<_>>();
    if query.is_empty() {
        return Some((0, 0));
    }
    let candidate = candidate
        .chars()
        .flat_map(char::to_lowercase)
        .collect::<Vec<_>>();
    if query.len() > candidate.len() {
        return None;
    }
    let word_start = |index: usize| index == 0 || !candidate[index - 1].is_alphanumeric();

    let mut previous = vec![None; candidate.len()];
    for (index, character) in candidate.iter().enumerate() {
        if *character == query[0] {
            previous[index] = Some((0, usize::from(word_start(index))));
        }
    }
    for wanted in query.into_iter().skip(1) {
        let mut next = vec![None; candidate.len()];
        let mut best_before = None;
        for (index, character) in candidate.iter().enumerate() {
            if *character == wanted {
                let word_start_bonus = usize::from(word_start(index));
                let non_adjacent =
                    best_before.map(|(adjacent, starts)| (adjacent, starts + word_start_bonus));
                let adjacent = index.checked_sub(1).and_then(|previous_index| {
                    previous[previous_index]
                        .map(|(adjacent, starts)| (adjacent + 1, starts + word_start_bonus))
                });
                next[index] = non_adjacent.max(adjacent);
            }
            if let Some(score) = previous[index] {
                best_before = best_before.max(Some(score));
            }
        }
        previous = next;
    }
    previous.into_iter().flatten().max()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use tracker_domain::{WorklogId, WorklogTimes};

    fn worklog() -> Worklog {
        Worklog::new(
            WorklogId::generate(),
            TaskId::generate(),
            DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
            Some(DateTime::<Utc>::from_timestamp(60, 0).unwrap()),
        )
        .unwrap()
    }

    fn candidate(name: &str) -> MoveCandidate {
        MoveCandidate::new(TaskId::generate(), name.to_owned())
    }

    #[test]
    fn fuzzy_matching_is_case_insensitive_non_contiguous_and_prefers_better_matches() {
        assert!(fuzzy_score("Build release", "BR").is_some());
        assert!(fuzzy_score("Build release", "BSE").is_some());
        assert!(fuzzy_score("Build release", "BX").is_none());
        assert!(fuzzy_score("release build", "bu") > fuzzy_score("blue sky", "bu"));
    }

    #[test]
    fn filtering_uses_catalog_order_for_ties_and_resets_selection() {
        let alpha = candidate("alpha");
        let alpine = candidate("alpine");
        let beta = candidate("beta");
        let mut draft = MoveDraft::new(worklog(), vec![alpha.clone(), alpine.clone(), beta]);
        assert_eq!(draft.selected_task_id(), Some(alpha.id()));
        draft.move_down();
        draft.insert('a');
        assert_eq!(draft.selected_task_id(), Some(alpha.id()));
        draft.insert('l');
        assert_eq!(draft.selected_task_id(), Some(alpha.id()));
        draft.insert('z');
        assert_eq!(draft.selected_task_id(), None);
        draft.backspace();
        assert_eq!(draft.selected_task_id(), Some(alpha.id()));
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
        assert_eq!(
            draft.worklog().times(),
            WorklogTimes::new(
                DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
                Some(DateTime::<Utc>::from_timestamp(60, 0).unwrap()),
            )
        );
    }

    #[test]
    fn search_input_is_bounded_to_the_task_name_limit() {
        let mut draft = MoveDraft::new(worklog(), vec![candidate("alpha")]);
        for _ in 0..=TaskName::MAX_LEN {
            draft.insert('a');
        }
        assert_eq!(draft.query().chars().count(), TaskName::MAX_LEN);
        assert_eq!(draft.selected_task_id(), None);
    }

    #[test]
    fn fuzzy_matching_handles_the_maximum_query_in_linear_space() {
        let text = "a".repeat(TaskName::MAX_LEN);
        assert_eq!(fuzzy_score(&text, &text), Some((TaskName::MAX_LEN - 1, 1)));
        assert_eq!(fuzzy_score(&text, &format!("{text}a")), None);
    }
}
