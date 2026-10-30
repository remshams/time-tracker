use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tracker_application::{ActiveTrackingRead, GlobalWorklogPage};
use tracker_domain::{TaskId, Worklog, WorklogId, WorklogTimes};

use crate::command::Command;
use crate::screens::all_worklogs::AllWorklogsFocus;
use crate::screens::reports::{
    ReportFocus, ReportMode, ReportPresentation, ReportPresentationRow, ReportState,
};
use crate::screens::task_list::{InputPurpose, TaskListMode};
use crate::screens::{
    AllWorklogsState, CorrectionDraft, InputState, MoveDraft, TaskListState, TaskView,
    WorklogHistoryMode, WorklogHistoryState, map_key,
};
use crate::test_support::at;

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

fn inactive_preview() -> crate::screens::task_list::InactiveTaskPreview {
    crate::screens::task_list::InactiveTaskPreview::local(
        at(0),
        vec![TaskId::generate()],
        vec!["old task".to_owned()],
    )
}

/// Builds a pending archive fixture for keymap and footer assertions.
pub(crate) fn archiving_mode() -> TaskListMode {
    TaskListMode::ArchivingInactiveTasks {
        preview: inactive_preview(),
    }
}

pub(crate) fn deletion() -> WorklogHistoryMode {
    WorklogHistoryMode::ConfirmDeletion {
        worklog: Worklog::new(
            WorklogId::generate(),
            TaskId::generate(),
            at(0),
            Some(at(60)),
        )
        .unwrap(),
    }
}

pub(crate) fn correction() -> WorklogHistoryMode {
    let start = at(0);
    WorklogHistoryMode::Correction(CorrectionDraft::new(
        WorklogId::generate(),
        WorklogTimes::new(start, Some(start)),
        "1970-01-01 00:00".to_owned(),
        Some("1970-01-01 00:00".to_owned()),
    ))
}

pub(crate) fn move_dialog() -> WorklogHistoryMode {
    let start = at(0);
    WorklogHistoryMode::Move(MoveDraft::new(
        Worklog::new(
            WorklogId::generate(),
            TaskId::generate(),
            start,
            Some(at(60)),
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
        TaskListMode::PreviewingInactiveTasks { as_of } => {
            state.open_inactive_archive_preview(as_of);
        }
        TaskListMode::ConfirmInactiveArchive { preview } => {
            state.confirm_inactive_archive(preview);
        }
        TaskListMode::ArchivingInactiveTasks { preview } => {
            state.begin_inactive_archive(preview);
        }
    }
    state
}

/// Builds a normal-mode task list with a committed search filter.
fn filtered_task_list_state(view: TaskView) -> TaskListState {
    let mut state = task_list_state(TaskListMode::Search, view);
    state.insert_search('x', 1);
    state.commit_search();
    state
}

fn history_state(mode: WorklogHistoryMode, available: bool) -> WorklogHistoryState {
    let history = crate::screens::History::new(TaskId::generate(), Vec::new(), None);
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

/// Builds a move dialog with the results list focused.
fn move_dialog_results() -> MoveDraft {
    let WorklogHistoryMode::Move(mut draft) = move_dialog() else {
        unreachable!("move fixture opens the move dialog");
    };
    draft.toggle_focus();
    draft
}

fn report_state(mode: ReportMode, focus: ReportFocus) -> ReportState {
    let mut state = ReportState::new(Utc::now(), chrono_tz::UTC);
    state.set_totals(ReportPresentation {
        rows: vec![ReportPresentationRow {
            task_id: TaskId::generate(),
            task_name: Some("task".to_owned()),
            duration: chrono::Duration::minutes(1),
        }],
        total: chrono::Duration::minutes(1),
    });
    state.mode = mode;
    state.focus = focus;
    state
}

fn custom_report_mode(focus_to: bool) -> ReportMode {
    ReportMode::Custom {
        from: "2026-01-01".to_owned(),
        to: "2026-01-31".to_owned(),
        focus_to,
    }
}

fn all_worklogs_state(
    focus: AllWorklogsFocus,
    available: bool,
    move_draft: Option<MoveDraft>,
) -> AllWorklogsState {
    let mut state = AllWorklogsState::new(GlobalWorklogPage {
        worklogs: Vec::new(),
        task_items: Vec::new(),
        tracking: Some(ActiveTrackingRead {
            active_worklog: None,
            active_task_item: None,
        }),
        next_cursor: None,
    });
    state.focus = focus;
    state.available = available;
    state.move_draft = move_draft;
    state
}

pub(crate) fn valid_input_states() -> Vec<TestInputState> {
    let WorklogHistoryMode::Move(move_draft) = move_dialog() else {
        unreachable!("move fixture opens the move dialog");
    };
    let mut states = Vec::new();
    for view in [TaskView::Active, TaskView::Archived] {
        states.extend([
            TestInputState::TaskList(task_list_state(TaskListMode::Normal, view)),
            TestInputState::TaskList(task_list_state(TaskListMode::Search, view)),
            TestInputState::TaskList(filtered_task_list_state(view)),
        ]);
    }
    states.extend([
        TestInputState::TaskList(task_list_state(input(), TaskView::Active)),
        TestInputState::TaskList(task_list_state(rename_input(), TaskView::Active)),
        TestInputState::TaskList(task_list_state(confirm(), TaskView::Active)),
        TestInputState::TaskList(task_list_state(
            TaskListMode::PreviewingInactiveTasks { as_of: at(0) },
            TaskView::Active,
        )),
        TestInputState::TaskList(task_list_state(
            TaskListMode::ConfirmInactiveArchive {
                preview: inactive_preview(),
            },
            TaskView::Active,
        )),
        TestInputState::TaskList(task_list_state(archiving_mode(), TaskView::Active)),
        TestInputState::History(Box::new(history_state(WorklogHistoryMode::Normal, true))),
        TestInputState::History(Box::new(history_state(WorklogHistoryMode::Normal, false))),
        TestInputState::History(Box::new(history_state(deletion(), true))),
        TestInputState::History(Box::new(history_state(correction(), true))),
        TestInputState::History(Box::new(history_state(move_dialog(), true))),
        TestInputState::History(Box::new(history_state(
            WorklogHistoryMode::Move(move_dialog_results()),
            true,
        ))),
        TestInputState::Reports(ReportState::new(Utc::now(), chrono_tz::UTC)),
        TestInputState::Reports(report_state(ReportMode::Normal, ReportFocus::Presets)),
        TestInputState::Reports(report_state(ReportMode::Normal, ReportFocus::Rows)),
        TestInputState::Reports(report_state(
            custom_report_mode(false),
            ReportFocus::Presets,
        )),
        TestInputState::Reports(report_state(custom_report_mode(true), ReportFocus::Presets)),
        TestInputState::AllWorklogs(Box::new(all_worklogs_state(
            AllWorklogsFocus::Tabs,
            true,
            None,
        ))),
        TestInputState::AllWorklogs(Box::new(all_worklogs_state(
            AllWorklogsFocus::Rows,
            true,
            None,
        ))),
        TestInputState::AllWorklogs(Box::new(all_worklogs_state(
            AllWorklogsFocus::Rows,
            false,
            None,
        ))),
        TestInputState::AllWorklogs(Box::new(all_worklogs_state(
            AllWorklogsFocus::Rows,
            true,
            Some(move_draft),
        ))),
        TestInputState::AllWorklogs(Box::new(all_worklogs_state(
            AllWorklogsFocus::Rows,
            true,
            Some(move_dialog_results()),
        ))),
    ]);
    states
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

pub(crate) fn filtered_task_list_footer(view: TaskView, width: u16) -> &'static str {
    crate::screens::task_list::footer_hints(&filtered_task_list_state(view), width)
}

pub(crate) fn history_footer(
    mode: WorklogHistoryMode,
    available: bool,
    width: u16,
) -> &'static str {
    crate::screens::worklog_history::footer_hints(&history_state(mode, available), width)
}
