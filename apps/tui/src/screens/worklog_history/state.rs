use chrono::{DateTime, Utc};
use std::sync::atomic::{AtomicU64, Ordering};
use tracker_application::WorklogCursor;
use tracker_domain::{TaskId, Worklog, WorklogId};

use crate::screens::task_list::TaskListState;

use super::correction::CorrectionDraft;
use super::move_worklog::MoveDraft;

/// Whether the open history holds a valid page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryAvailability {
    Available,
    Unavailable,
}

/// One task's loaded worklog history and stable-ID selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct History {
    task_id: TaskId,
    availability: HistoryAvailability,
    worklogs: Vec<Worklog>,
    next_cursor: Option<WorklogCursor>,
    active_worklog_baseline: Option<(WorklogId, DateTime<Utc>)>,
    selected: Option<WorklogId>,
}

impl History {
    pub(crate) fn new(
        task_id: TaskId,
        worklogs: Vec<Worklog>,
        next_cursor: Option<WorklogCursor>,
        active_worklog_baseline: Option<(WorklogId, DateTime<Utc>)>,
    ) -> Self {
        let selected = worklogs.first().map(Worklog::id);
        Self {
            task_id,
            availability: HistoryAvailability::Available,
            worklogs,
            next_cursor,
            active_worklog_baseline,
            selected,
        }
    }

    pub fn task_id(&self) -> TaskId {
        self.task_id
    }

    pub fn availability(&self) -> HistoryAvailability {
        self.availability
    }

    pub fn worklogs(&self) -> &[Worklog] {
        &self.worklogs
    }

    pub fn next_cursor(&self) -> Option<WorklogCursor> {
        self.next_cursor
    }

    pub fn is_available(&self) -> bool {
        self.availability == HistoryAvailability::Available
    }

    pub(crate) fn selected_index(&self) -> Option<usize> {
        let id = self.selected?;
        self.is_available()
            .then(|| self.worklogs.iter().position(|worklog| worklog.id() == id))
            .flatten()
    }

    pub(crate) fn selected_id(&self) -> Option<WorklogId> {
        self.selected
    }

    pub(crate) fn active_worklog_baseline(&self) -> Option<(WorklogId, DateTime<Utc>)> {
        self.active_worklog_baseline
    }

    pub(crate) fn selected_worklog(&self) -> Option<&Worklog> {
        self.selected_index().map(|index| &self.worklogs[index])
    }

    pub(crate) fn move_up(&mut self) {
        if !self.is_available() {
            return;
        }
        let index = match self.selected_index() {
            None => self.worklogs.len().checked_sub(1),
            Some(0) => Some(0),
            Some(index) => Some(index - 1),
        };
        self.selected = index
            .and_then(|index| self.worklogs.get(index))
            .map(Worklog::id);
    }

    pub(crate) fn move_down(&mut self) {
        if !self.is_available() {
            return;
        }
        if self.worklogs.is_empty() {
            self.selected = None;
            return;
        }
        let last = self.worklogs.len() - 1;
        let index = match self.selected_index() {
            None => 0,
            Some(index) => index.saturating_add(1).min(last),
        };
        self.selected = Some(self.worklogs[index].id());
    }

    pub(crate) fn select_index(&mut self, index: usize) {
        if self.is_available() {
            self.selected = self.worklogs.get(index).map(Worklog::id);
        }
    }

    pub(crate) fn append(&mut self, worklogs: Vec<Worklog>, next_cursor: Option<WorklogCursor>) {
        let select_first = self.worklogs.is_empty();
        self.worklogs.extend(worklogs);
        self.next_cursor = next_cursor;
        if select_first {
            self.selected = self.worklogs.first().map(Worklog::id);
        }
    }

    pub(crate) fn replace(
        &mut self,
        task_id: TaskId,
        worklogs: Vec<Worklog>,
        next_cursor: Option<WorklogCursor>,
        active_worklog_baseline: Option<(WorklogId, DateTime<Utc>)>,
        preferred: Option<WorklogId>,
    ) {
        let selected = preferred
            .filter(|id| worklogs.iter().any(|worklog| worklog.id() == *id))
            .or_else(|| worklogs.first().map(Worklog::id));
        *self = Self {
            task_id,
            availability: HistoryAvailability::Available,
            worklogs,
            next_cursor,
            active_worklog_baseline,
            selected,
        };
    }

    pub(crate) fn mark_unavailable(&mut self) {
        self.availability = HistoryAvailability::Unavailable;
        self.worklogs.clear();
        self.next_cursor = None;
    }

    pub(crate) fn remove(&mut self, deleted_id: WorklogId) {
        let Some(index) = self
            .worklogs
            .iter()
            .position(|worklog| worklog.id() == deleted_id)
        else {
            self.selected = None;
            return;
        };
        self.worklogs.remove(index);
        self.selected = self
            .worklogs
            .get(index.min(self.worklogs.len().saturating_sub(1)))
            .map(Worklog::id);
    }
}

/// The modes valid while worklog history is visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorklogHistoryMode {
    Normal,
    ConfirmDeletion { worklog: Worklog },
    Correction(CorrectionDraft),
    Move(MoveDraft),
}

/// History state and the exact task-list navigation restored on return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorklogHistoryState {
    session_id: u64,
    task_list: TaskListState,
    history: History,
    mode: WorklogHistoryMode,
    from_reports: bool,
    g_prefix: bool,
}

pub(crate) fn active_worklog_for_task(
    active_worklog: Option<&Worklog>,
    task_id: TaskId,
) -> Option<(WorklogId, DateTime<Utc>)> {
    active_worklog
        .filter(|worklog| worklog.task_id() == task_id)
        .map(|worklog| (worklog.id(), worklog.start()))
}

impl WorklogHistoryState {
    pub(crate) fn new(task_list: TaskListState, history: History) -> Self {
        static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);
        Self {
            session_id: NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed),
            task_list,
            history,
            mode: WorklogHistoryMode::Normal,
            from_reports: false,
            g_prefix: false,
        }
    }

    pub(crate) fn from_reports(task_list: TaskListState, history: History) -> Self {
        let mut state = Self::new(task_list, history);
        state.from_reports = true;
        state
    }

    pub(crate) fn has_report_source(&self) -> bool {
        self.from_reports
    }

    pub(crate) fn session_id(&self) -> u64 {
        self.session_id
    }

    pub(crate) fn g_prefix(&self) -> bool {
        self.g_prefix
    }

    pub(crate) fn set_g_prefix(&mut self, value: bool) {
        self.g_prefix = value;
    }

    pub fn task_list(&self) -> &TaskListState {
        &self.task_list
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    pub fn mode(&self) -> &WorklogHistoryMode {
        &self.mode
    }

    pub(crate) fn task_list_mut(&mut self) -> &mut TaskListState {
        &mut self.task_list
    }

    pub(crate) fn history_mut(&mut self) -> &mut History {
        &mut self.history
    }

    pub(crate) fn is_normal(&self) -> bool {
        matches!(self.mode, WorklogHistoryMode::Normal)
    }

    pub(crate) fn open_deletion(&mut self, worklog: Worklog) {
        self.mode = WorklogHistoryMode::ConfirmDeletion { worklog };
    }

    pub(crate) fn open_correction(&mut self, draft: CorrectionDraft) {
        self.mode = WorklogHistoryMode::Correction(draft);
    }

    pub(crate) fn open_move(&mut self, draft: MoveDraft) {
        self.mode = WorklogHistoryMode::Move(draft);
    }

    pub(crate) fn correction(&self) -> Option<&CorrectionDraft> {
        match &self.mode {
            WorklogHistoryMode::Correction(draft) => Some(draft),
            _ => None,
        }
    }

    pub(crate) fn correction_mut(&mut self) -> Option<&mut CorrectionDraft> {
        match &mut self.mode {
            WorklogHistoryMode::Correction(draft) => Some(draft),
            _ => None,
        }
    }

    pub(crate) fn move_draft_mut(&mut self) -> Option<&mut MoveDraft> {
        match &mut self.mode {
            WorklogHistoryMode::Move(draft) => Some(draft),
            _ => None,
        }
    }

    pub(crate) fn close_mode(&mut self) {
        self.mode = WorklogHistoryMode::Normal;
    }

    pub(crate) fn into_task_list(self) -> TaskListState {
        self.task_list
    }
}
