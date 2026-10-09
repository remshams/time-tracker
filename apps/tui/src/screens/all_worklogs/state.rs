use std::sync::atomic::{AtomicU64, Ordering};
use tracker_application::{GlobalWorklogCursor, GlobalWorklogPage};
use tracker_domain::{Worklog, WorklogId};

use crate::screens::worklog_history::MoveDraft;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AllWorklogsFocus {
    Tabs,
    Rows,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AllWorklogsState {
    session_id: u64,
    worklogs: Vec<Worklog>,
    next_cursor: Option<GlobalWorklogCursor>,
    selected: Option<WorklogId>,
    pub(crate) focus: AllWorklogsFocus,
    pub(crate) move_draft: Option<MoveDraft>,
    pub(crate) g_prefix: bool,
    pub(crate) available: bool,
    pub(crate) pagination_invalidated: bool,
    pub(crate) page_generation: u64,
    pub(crate) loading_cursor: Option<GlobalWorklogCursor>,
    pub(crate) move_draft_generation: u64,
}

impl AllWorklogsState {
    pub(crate) fn new(page: GlobalWorklogPage) -> Self {
        static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);
        Self {
            session_id: NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed),
            selected: page.worklogs.first().map(Worklog::id),
            worklogs: page.worklogs,
            next_cursor: page.next_cursor,
            focus: AllWorklogsFocus::Tabs,
            move_draft: None,
            g_prefix: false,
            available: true,
            pagination_invalidated: false,
            page_generation: 0,
            loading_cursor: None,
            move_draft_generation: 0,
        }
    }

    pub(crate) fn session_id(&self) -> u64 {
        self.session_id
    }

    pub(crate) fn worklogs(&self) -> &[Worklog] {
        &self.worklogs
    }

    pub(crate) fn selected_index(&self) -> Option<usize> {
        let id = self.selected?;
        self.available
            .then(|| self.worklogs.iter().position(|row| row.id() == id))
            .flatten()
    }

    pub(crate) fn selected_id(&self) -> Option<WorklogId> {
        self.selected
    }

    pub(crate) fn selected_worklog(&self) -> Option<&Worklog> {
        self.selected_index().map(|index| &self.worklogs[index])
    }

    pub(crate) fn next_cursor(&self) -> Option<GlobalWorklogCursor> {
        self.next_cursor
    }

    pub(crate) fn select_index(&mut self, index: usize) {
        self.selected = self.worklogs.get(index).map(Worklog::id);
    }

    pub(crate) fn move_selection(&mut self, down: bool) {
        if self.worklogs.is_empty() {
            return;
        }
        let last = self.worklogs.len() - 1;
        let index = match (self.selected_index(), down) {
            (Some(index), true) => (index + 1).min(last),
            (Some(index), false) => index.saturating_sub(1),
            (None, true) => 0,
            (None, false) => last,
        };
        self.select_index(index);
    }

    pub(crate) fn append(&mut self, page: GlobalWorklogPage) {
        self.page_generation = self.page_generation.wrapping_add(1);
        let empty = self.worklogs.is_empty();
        self.worklogs.extend(page.worklogs);
        self.next_cursor = page.next_cursor;
        self.pagination_invalidated = false;
        if empty {
            self.selected = self.worklogs.first().map(Worklog::id);
        }
    }

    pub(crate) fn replace(&mut self, page: GlobalWorklogPage, preferred: Option<WorklogId>) {
        self.replace_rows(page.worklogs, page.next_cursor, preferred);
    }

    pub(crate) fn replace_rows(
        &mut self,
        worklogs: Vec<Worklog>,
        next_cursor: Option<GlobalWorklogCursor>,
        preferred: Option<WorklogId>,
    ) {
        self.page_generation = self.page_generation.wrapping_add(1);
        self.loading_cursor = None;
        self.worklogs = worklogs;
        self.next_cursor = next_cursor;
        self.selected = preferred
            .filter(|id| self.worklogs.iter().any(|row| row.id() == *id))
            .or_else(|| self.worklogs.first().map(Worklog::id));
        self.available = true;
        self.pagination_invalidated = false;
    }

    /// Keeps the loaded position after a move and discards its stale cursor.
    pub(crate) fn apply_move(&mut self, moved: Worklog) {
        self.page_generation = self.page_generation.wrapping_add(1);
        if let Some(row) = self.worklogs.iter_mut().find(|row| row.id() == moved.id()) {
            *row = moved;
        }
        self.pagination_invalidated = self.next_cursor.is_some();
        self.next_cursor = None;
    }

    pub(crate) fn mark_unavailable(&mut self) {
        self.page_generation = self.page_generation.wrapping_add(1);
        self.loading_cursor = None;
        self.available = false;
        self.worklogs.clear();
        self.next_cursor = None;
        self.selected = None;
        self.pagination_invalidated = false;
    }
}

#[cfg(test)]
mod tests {
    use tracker_application::ActiveTrackingRead;
    use tracker_domain::{TaskId, WorklogId};

    use super::*;

    fn worklog(start: i64) -> Worklog {
        Worklog::begin(
            WorklogId::generate(),
            TaskId::generate(),
            chrono::DateTime::from_timestamp(start, 0).unwrap(),
        )
    }

    fn page(worklogs: Vec<Worklog>) -> GlobalWorklogPage {
        GlobalWorklogPage {
            worklogs,
            task_items: Vec::new(),
            tracking: Some(ActiveTrackingRead {
                active_worklog: None,
                active_task_item: None,
            }),
            next_cursor: None,
        }
    }

    #[test]
    fn selection_moves_one_row_and_stops_at_each_end() {
        let rows = vec![worklog(1), worklog(2), worklog(3)];
        let mut state = AllWorklogsState::new(page(rows));
        assert_eq!(state.selected_index(), Some(0));
        state.move_selection(true);
        assert_eq!(state.selected_index(), Some(1));
        state.move_selection(false);
        assert_eq!(state.selected_index(), Some(0));
        state.move_selection(false);
        assert_eq!(state.selected_index(), Some(0));
        state.move_selection(true);
        state.move_selection(true);
        state.move_selection(true);
        assert_eq!(state.selected_index(), Some(2));
    }

    #[test]
    fn refresh_preserves_an_existing_selection_and_falls_back_when_it_is_gone() {
        let first = worklog(1);
        let second = worklog(2);
        let mut state = AllWorklogsState::new(page(vec![first.clone(), second.clone()]));
        state.select_index(1);
        state.replace(
            page(vec![first.clone(), second.clone()]),
            state.selected_id(),
        );
        assert_eq!(state.selected_id(), Some(second.id()));
        state.replace(page(vec![first.clone()]), Some(second.id()));
        assert_eq!(state.selected_id(), Some(first.id()));
    }

    #[test]
    fn unavailable_state_clears_rows_and_selection() {
        let mut state = AllWorklogsState::new(page(vec![worklog(1)]));
        state.mark_unavailable();
        assert!(!state.available);
        assert!(state.worklogs().is_empty());
        assert_eq!(state.selected_id(), None);
        assert_eq!(state.selected_index(), None);
    }
}
