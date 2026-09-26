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
    worklogs: Vec<Worklog>,
    next_cursor: Option<GlobalWorklogCursor>,
    selected: Option<WorklogId>,
    pub(crate) focus: AllWorklogsFocus,
    pub(crate) move_draft: Option<MoveDraft>,
    pub(crate) g_prefix: bool,
    pub(crate) available: bool,
    pub(crate) pagination_invalidated: bool,
}

impl AllWorklogsState {
    pub(crate) fn new(page: GlobalWorklogPage) -> Self {
        Self {
            selected: page.worklogs.first().map(Worklog::id),
            worklogs: page.worklogs,
            next_cursor: page.next_cursor,
            focus: AllWorklogsFocus::Tabs,
            move_draft: None,
            g_prefix: false,
            available: true,
            pagination_invalidated: false,
        }
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
        if let Some(row) = self.worklogs.iter_mut().find(|row| row.id() == moved.id()) {
            *row = moved;
        }
        self.pagination_invalidated = self.next_cursor.is_some();
        self.next_cursor = None;
    }

    pub(crate) fn mark_unavailable(&mut self) {
        self.available = false;
        self.worklogs.clear();
        self.next_cursor = None;
        self.selected = None;
        self.pagination_invalidated = false;
    }
}
