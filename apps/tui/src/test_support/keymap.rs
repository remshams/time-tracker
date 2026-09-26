use chrono::{DateTime, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tracker_application::{GlobalWorklogPage, TrackerSnapshot};
use tracker_domain::{TaskId, Worklog, WorklogId, WorklogTimes};

use crate::command::Command;
use crate::screens::task_list::{InputPurpose, TaskListMode};
use crate::screens::{
    AllWorklogsState, CorrectionDraft, InputState, MoveDraft, TaskListState, TaskView,
    WorklogHistoryMode, WorklogHistoryState, map_key,
};

pub(crate) fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

pub(crate) fn ctrl(character: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL)
}

pub(crate) fn with_modifier(character: char, modifier: KeyModifiers) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(character), modifier)
}

pub(crate) fn input() -> TaskListMode {
    TaskListMode::Input {
        purpose: InputPurpose::Add,
        buffer: String::new(),
    }
}

fn rename_input() -> TaskListMode {
    TaskListMode::Input {
        purpose: InputPurpose::Rename {
            task_id: TaskId::generate(),
        },
        buffer: "task".to_owned(),
    }
}

pub(crate) fn confirm() -> TaskListMode {
    TaskListMode::ConfirmArchive {
        task_id: TaskId::generate(),
        name: "task".to_owned(),
    }
}

pub(crate) fn deletion() -> WorklogHistoryMode {
    WorklogHistoryMode::ConfirmDeletion {
        worklog: Worklog::new(
            WorklogId::generate(),
            TaskId::generate(),
            DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
            Some(DateTime::<Utc>::from_timestamp(60, 0).unwrap()),
        )
        .unwrap(),
    }
}

pub(crate) fn correction() -> WorklogHistoryMode {
    let at = DateTime::<Utc>::from_timestamp(0, 0).unwrap();
    WorklogHistoryMode::Correction(CorrectionDraft::new(
        WorklogId::generate(),
        WorklogTimes::new(at, Some(at)),
        "1970-01-01 00:00".to_owned(),
        Some("1970-01-01 00:00".to_owned()),
    ))
}

pub(crate) fn move_dialog() -> WorklogHistoryMode {
    let at = DateTime::<Utc>::from_timestamp(0, 0).unwrap();
    WorklogHistoryMode::Move(MoveDraft::new(
        Worklog::new(
            WorklogId::generate(),
            TaskId::generate(),
            at,
            Some(DateTime::<Utc>::from_timestamp(60, 0).unwrap()),
        )
        .unwrap(),
        Vec::new(),
    ))
}

fn task_list_state(mode: TaskListMode, view: TaskView) -> TaskListState {
    let mut state = TaskListState::new(None);
    state.show(view, None);
    match mode {
        TaskListMode::Normal => {}
        TaskListMode::Search => state.open_search(),
        TaskListMode::Input { purpose, buffer } => state.open_input(purpose, buffer),
        TaskListMode::ConfirmArchive { task_id, name } => {
            state.open_archive_confirmation(task_id, name);
        }
    }
    state
}

fn history_state(mode: WorklogHistoryMode, available: bool) -> WorklogHistoryState {
    let history = crate::screens::History::new(TaskId::generate(), Vec::new(), None, None);
    let mut state = WorklogHistoryState::new(TaskListState::new(None), history);
    match mode {
        WorklogHistoryMode::Normal => {}
        WorklogHistoryMode::ConfirmDeletion { worklog } => state.open_deletion(worklog),
        WorklogHistoryMode::Correction(draft) => state.open_correction(draft),
        WorklogHistoryMode::Move(draft) => state.open_move(draft),
    }
    if !available {
        state.history_mut().mark_unavailable();
    }
    state
}

#[derive(Debug)]
pub(crate) enum TestInputState {
    TaskList(TaskListState),
    History(Box<WorklogHistoryState>),
    Reports(crate::screens::ReportState),
    AllWorklogs(Box<AllWorklogsState>),
}

impl TestInputState {
    pub(crate) fn input(&self) -> InputState<'_> {
        match self {
            Self::TaskList(state) => InputState::TaskList(state),
            Self::History(state) => InputState::WorklogHistory(state),
            Self::Reports(state) => InputState::Reports(state),
            Self::AllWorklogs(state) => InputState::AllWorklogs(state),
        }
    }
}

pub(crate) fn valid_input_states() -> Vec<TestInputState> {
    vec![
        TestInputState::TaskList(task_list_state(TaskListMode::Normal, TaskView::Active)),
        TestInputState::TaskList(task_list_state(TaskListMode::Normal, TaskView::Archived)),
        TestInputState::TaskList(task_list_state(input(), TaskView::Active)),
        TestInputState::TaskList(task_list_state(rename_input(), TaskView::Active)),
        TestInputState::TaskList(task_list_state(confirm(), TaskView::Active)),
        TestInputState::History(Box::new(history_state(WorklogHistoryMode::Normal, true))),
        TestInputState::History(Box::new(history_state(WorklogHistoryMode::Normal, false))),
        TestInputState::History(Box::new(history_state(deletion(), true))),
        TestInputState::History(Box::new(history_state(correction(), true))),
        TestInputState::History(Box::new(history_state(move_dialog(), true))),
        TestInputState::Reports(crate::screens::ReportState::new(Utc::now(), chrono_tz::UTC)),
        TestInputState::AllWorklogs(Box::new(AllWorklogsState::new(GlobalWorklogPage {
            worklogs: Vec::new(),
            snapshot: TrackerSnapshot {
                task_items: Vec::new(),
                active_worklog: None,
            },
            next_cursor: None,
        }))),
    ]
}

pub(crate) fn map_task_list(mode: TaskListMode, view: TaskView, key: KeyEvent) -> Option<Command> {
    let state = task_list_state(mode, view);
    map_key(InputState::TaskList(&state), key)
}

pub(crate) fn map_history(mode: &WorklogHistoryMode, key: KeyEvent) -> Option<Command> {
    let state = history_state(mode.clone(), true);
    map_key(InputState::WorklogHistory(&state), key)
}

pub(crate) fn map_unavailable(key: KeyEvent) -> Option<Command> {
    let state = history_state(WorklogHistoryMode::Normal, false);
    map_key(InputState::WorklogHistory(&state), key)
}

pub(crate) fn task_list_footer(mode: TaskListMode, view: TaskView, width: u16) -> &'static str {
    crate::screens::task_list::footer_hints(&task_list_state(mode, view), width)
}

pub(crate) fn history_footer(
    mode: WorklogHistoryMode,
    available: bool,
    width: u16,
) -> &'static str {
    crate::screens::worklog_history::footer_hints(&history_state(mode, available), width)
}
