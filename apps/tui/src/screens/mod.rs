pub mod task_list;
#[cfg(test)]
pub(crate) mod test_support;
pub mod worklog_history;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::command::Command;
#[cfg(test)]
pub use task_list::{InputPurpose, TaskView};
pub use task_list::{TaskListMode, TaskListState};
#[cfg(test)]
pub use worklog_history::CorrectionDraft;
pub use worklog_history::{History, WorklogHistoryMode, WorklogHistoryState};

/// Which screen the interface currently shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    TaskList,
    WorklogHistory,
}

/// The only valid stored screen states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenState {
    TaskList(TaskListState),
    WorklogHistory(Box<WorklogHistoryState>),
}

impl ScreenState {
    pub fn screen(&self) -> Screen {
        match self {
            Self::TaskList(_) => Screen::TaskList,
            Self::WorklogHistory(_) => Screen::WorklogHistory,
        }
    }

    pub(crate) fn task_list(&self) -> &TaskListState {
        match self {
            Self::TaskList(state) => state,
            Self::WorklogHistory(state) => state.task_list(),
        }
    }

    pub(crate) fn task_list_mut(&mut self) -> &mut TaskListState {
        match self {
            Self::TaskList(state) => state,
            Self::WorklogHistory(state) => state.task_list_mut(),
        }
    }

    pub(crate) fn footer_hints(&self, width: u16) -> &'static str {
        match self {
            Self::TaskList(state) => task_list::footer_hints(state, width),
            Self::WorklogHistory(state) => worklog_history::footer_hints(state, width),
        }
    }
}

#[cfg(test)]
pub(crate) mod keymap_test_support {
    use chrono::{DateTime, Utc};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use tracker_domain::{TaskId, Worklog, WorklogId, WorklogTimes};

    use super::{ScreenState, map_key};
    use crate::app::CorrectionDraft;
    use crate::command::Command;
    use crate::screens::task_list::{InputPurpose, TaskListMode, TaskListState, TaskView};
    use crate::screens::worklog_history::{History, WorklogHistoryMode, WorklogHistoryState};

    pub fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    pub fn ctrl(character: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL)
    }

    pub fn with_modifier(character: char, modifier: KeyModifiers) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(character), modifier)
    }

    pub fn input() -> TaskListMode {
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

    pub fn confirm() -> TaskListMode {
        TaskListMode::ConfirmArchive {
            task_id: TaskId::generate(),
            name: "task".to_owned(),
        }
    }

    pub fn deletion() -> WorklogHistoryMode {
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

    pub fn correction() -> WorklogHistoryMode {
        let at = DateTime::<Utc>::from_timestamp(0, 0).unwrap();
        WorklogHistoryMode::Correction(CorrectionDraft::new(
            WorklogId::generate(),
            WorklogTimes::new(at, Some(at)),
            "1970-01-01 00:00".to_owned(),
            Some("1970-01-01 00:00".to_owned()),
        ))
    }

    pub fn task_list_state(mode: TaskListMode, view: TaskView) -> ScreenState {
        let mut state = TaskListState::new(None);
        state.set_view_for_test(view);
        state.set_mode_for_test(mode);
        ScreenState::TaskList(state)
    }

    pub fn history_state(mode: WorklogHistoryMode, available: bool) -> ScreenState {
        let tasks = TaskListState::new(None);
        let history = History::new(TaskId::generate(), Vec::new(), None, None);
        let mut state = WorklogHistoryState::new(tasks, history);
        state.set_mode_for_test(mode);
        if !available {
            state.set_history_unavailable_for_test();
        }
        ScreenState::WorklogHistory(Box::new(state))
    }

    pub fn valid_screen_states() -> Vec<ScreenState> {
        vec![
            task_list_state(TaskListMode::Normal, TaskView::Active),
            task_list_state(TaskListMode::Normal, TaskView::Archived),
            task_list_state(input(), TaskView::Active),
            task_list_state(rename_input(), TaskView::Active),
            task_list_state(confirm(), TaskView::Active),
            history_state(WorklogHistoryMode::Normal, true),
            history_state(WorklogHistoryMode::Normal, false),
            history_state(deletion(), true),
            history_state(correction(), true),
        ]
    }

    pub fn map_task_list(mode: TaskListMode, view: TaskView, key: KeyEvent) -> Option<Command> {
        map_key(&task_list_state(mode, view), key)
    }

    pub fn map_history(mode: &WorklogHistoryMode, key: KeyEvent) -> Option<Command> {
        map_key(&history_state(mode.clone(), true), key)
    }

    pub fn map_unavailable(key: KeyEvent) -> Option<Command> {
        map_key(&history_state(WorklogHistoryMode::Normal, false), key)
    }

    pub fn task_list_footer(mode: TaskListMode, view: TaskView, width: u16) -> &'static str {
        task_list_state(mode, view).footer_hints(width)
    }

    pub fn history_footer(mode: WorklogHistoryMode, available: bool, width: u16) -> &'static str {
        history_state(mode, available).footer_hints(width)
    }
}

/// Applies global event filtering and dispatches the event to the active screen.
pub(crate) fn map_key(state: &ScreenState, key: KeyEvent) -> Option<Command> {
    if key.kind != KeyEventKind::Press {
        return None;
    }
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(Command::Quit);
    }
    match state {
        ScreenState::TaskList(state) => task_list::map(state, key),
        ScreenState::WorklogHistory(state) => worklog_history::map(state, key),
    }
}

#[cfg(test)]
mod restored_keymap_tests {
    use super::*;
    use crate::screens::keymap_test_support::{ctrl, valid_screen_states};
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    #[test]
    fn key_releases_produce_no_command() {
        let release = KeyEvent::new_with_kind(
            KeyCode::Char('j'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );
        for state in valid_screen_states() {
            assert_eq!(map_key(&state, release), None, "state was {state:?}");
        }
    }

    #[test]
    fn a_modified_release_does_nothing_even_for_ctrl_c() {
        let releases = [
            KeyEvent::new_with_kind(KeyCode::Char('j'), KeyModifiers::ALT, KeyEventKind::Release),
            KeyEvent::new_with_kind(
                KeyCode::Char('c'),
                KeyModifiers::CONTROL,
                KeyEventKind::Release,
            ),
        ];
        for state in valid_screen_states() {
            for release in releases {
                assert_eq!(map_key(&state, release), None, "state was {state:?}");
            }
        }
    }

    #[test]
    fn ctrl_c_quits_in_every_mode_and_view() {
        for state in valid_screen_states() {
            assert_eq!(
                map_key(&state, ctrl('c')),
                Some(Command::Quit),
                "state was {state:?}"
            );
        }
    }
}
