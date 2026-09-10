pub mod task_list;
pub mod worklog_history;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::command::Command;
pub use task_list::{InputPurpose, TaskListMode, TaskListState, TaskView};
pub use worklog_history::{CorrectionDraft, History, WorklogHistoryMode, WorklogHistoryState};

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
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use tracker_domain::{Worklog, WorklogId, WorklogTimes};

    use super::{Screen, ScreenState, map_key};
    use crate::app::CorrectionDraft;
    use crate::command::Command;
    use crate::screens::task_list::{InputPurpose, TaskListMode, TaskListState, TaskView};
    use crate::screens::worklog_history::{History, WorklogHistoryMode, WorklogHistoryState};

    pub enum Mode {
        Normal,
        Input {
            purpose: InputPurpose,
            buffer: String,
        },
        ConfirmArchive {
            task_id: tracker_domain::TaskId,
            name: String,
        },
        ConfirmDeletion {
            worklog: Worklog,
        },
        Correction(CorrectionDraft),
    }

    pub fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
    pub fn ctrl(character: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL)
    }
    pub fn with_modifier(character: char, modifier: KeyModifiers) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(character), modifier)
    }
    pub fn input() -> Mode {
        Mode::Input {
            purpose: InputPurpose::Add,
            buffer: String::new(),
        }
    }
    pub fn confirm() -> Mode {
        Mode::ConfirmArchive {
            task_id: tracker_domain::TaskId::generate(),
            name: "task".to_owned(),
        }
    }
    pub fn deletion() -> Mode {
        Mode::ConfirmDeletion {
            worklog: Worklog::new(
                WorklogId::generate(),
                tracker_domain::TaskId::generate(),
                DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
                Some(DateTime::<Utc>::from_timestamp(60, 0).unwrap()),
            )
            .unwrap(),
        }
    }
    pub fn correction() -> Mode {
        let at = DateTime::<Utc>::from_timestamp(0, 0).unwrap();
        Mode::Correction(CorrectionDraft::new(
            WorklogId::generate(),
            WorklogTimes::new(at, Some(at)),
            "1970-01-01 00:00".to_owned(),
            Some("1970-01-01 00:00".to_owned()),
        ))
    }
    pub fn normal_modes() -> [(TaskView, Mode); 2] {
        [
            (TaskView::Active, Mode::Normal),
            (TaskView::Archived, Mode::Normal),
        ]
    }

    fn state(mode: &Mode, view: TaskView, screen: Screen) -> ScreenState {
        let mut tasks = TaskListState::new(Vec::new(), Vec::new(), Default::default());
        tasks.set_view_for_test(view);
        match screen {
            Screen::TaskList => {
                let mode = match mode {
                    Mode::Normal => TaskListMode::Normal,
                    Mode::Input { purpose, buffer } => TaskListMode::Input {
                        purpose: *purpose,
                        buffer: buffer.clone(),
                    },
                    Mode::ConfirmArchive { task_id, name } => TaskListMode::ConfirmArchive {
                        task_id: *task_id,
                        name: name.clone(),
                    },
                    _ => unreachable!(),
                };
                tasks.set_mode_for_test(mode);
                ScreenState::TaskList(tasks)
            }
            Screen::WorklogHistory => {
                let history =
                    History::new(tracker_domain::TaskId::generate(), Vec::new(), None, None);
                let mut state = WorklogHistoryState::new(tasks, history);
                let mode = match mode {
                    Mode::Normal => WorklogHistoryMode::Normal,
                    Mode::ConfirmDeletion { worklog } => WorklogHistoryMode::ConfirmDeletion {
                        worklog: worklog.clone(),
                    },
                    Mode::Correction(draft) => WorklogHistoryMode::Correction(draft.clone()),
                    _ => unreachable!(),
                };
                state.set_mode_for_test(mode);
                ScreenState::WorklogHistory(Box::new(state))
            }
        }
    }

    pub fn map_legacy(
        mode: &Mode,
        view: TaskView,
        screen: Screen,
        key: KeyEvent,
    ) -> Option<Command> {
        if key.kind != KeyEventKind::Press {
            return None;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Some(Command::Quit);
        }
        map_key(&state(mode, view, screen), key)
    }
    pub fn map_unavailable(view: TaskView, key: KeyEvent) -> Option<Command> {
        let mut state = state(&Mode::Normal, view, Screen::WorklogHistory);
        if let ScreenState::WorklogHistory(history) = &mut state {
            history.set_history_unavailable_for_test();
        }
        map_key(&state, key)
    }

    pub fn footer_hints_legacy(
        mode: &Mode,
        view: TaskView,
        screen: Screen,
        available: bool,
        width: u16,
    ) -> &'static str {
        let mut state = state(mode, view, screen);
        if let ScreenState::WorklogHistory(history) = &mut state
            && !available
        {
            history.set_history_unavailable_for_test();
        }
        state.footer_hints(width)
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
    use crate::screens::keymap_test_support::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    #[test]
    fn key_releases_produce_no_command() {
        let release = KeyEvent::new_with_kind(
            KeyCode::Char('j'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );
        for (view, mode) in normal_modes() {
            assert_eq!(map_legacy(&mode, view, Screen::TaskList, release), None);
        }
        for mode in [input(), confirm(), deletion()] {
            assert_eq!(
                map_legacy(&mode, TaskView::Active, Screen::TaskList, release),
                None
            );
        }
    }

    #[test]
    fn a_modified_release_does_nothing_even_for_ctrl_c() {
        let release = KeyEvent::new_with_kind(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
            KeyEventKind::Release,
        );
        for (view, mode) in normal_modes() {
            assert_eq!(map_legacy(&mode, view, Screen::TaskList, release), None);
        }
        for mode in [input(), confirm()] {
            assert_eq!(
                map_legacy(&mode, TaskView::Active, Screen::TaskList, release),
                None
            );
        }
    }

    #[test]
    fn ctrl_c_quits_in_every_mode_and_view() {
        for (view, mode) in normal_modes() {
            assert_eq!(
                map_legacy(&mode, view, Screen::TaskList, ctrl('c')),
                Some(Command::Quit)
            );
        }
        for mode in [input(), confirm(), deletion()] {
            assert_eq!(
                map_legacy(&mode, TaskView::Active, Screen::TaskList, ctrl('c')),
                Some(Command::Quit)
            );
        }
    }
}
