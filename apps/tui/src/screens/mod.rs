pub mod all_worklogs;
pub mod reports;
pub mod task_list;
pub mod worklog_history;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::command::Command;
pub(crate) use reports::ReportCommand;
pub use reports::ReportState;
pub(crate) use task_list::TaskListCommand;
pub use task_list::{TaskListState, TaskView};
#[cfg(test)]
pub use worklog_history::CorrectionDraft;
#[cfg(test)]
pub use worklog_history::MoveDraft;
pub(crate) use worklog_history::WorklogHistoryCommand;
pub use worklog_history::{History, WorklogHistoryMode, WorklogHistoryState};

/// Which screen the interface currently shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    TaskList,
    WorklogHistory,
    Reports,
    AllWorklogs,
}

/// The only valid stored screen states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenState {
    TaskList(TaskListState),
    WorklogHistory(Box<WorklogHistoryState>),
    Reports(Box<ReportState>),
    AllWorklogs(Box<AllWorklogsState>),
}

impl ScreenState {
    pub fn screen(&self) -> Screen {
        match self {
            Self::TaskList(_) => Screen::TaskList,
            Self::WorklogHistory(_) => Screen::WorklogHistory,
            Self::Reports(_) => Screen::Reports,
            Self::AllWorklogs(_) => Screen::AllWorklogs,
        }
    }
}

/// Immutable input context for the active screen.
#[derive(Clone, Copy)]
pub(crate) enum InputState<'a> {
    TaskList(&'a TaskListState),
    WorklogHistory(&'a WorklogHistoryState),
    Reports(&'a ReportState),
    AllWorklogs(&'a AllWorklogsState),
}

impl InputState<'_> {
    pub(crate) fn footer_hints(self, width: u16) -> &'static str {
        match self {
            Self::TaskList(state) => task_list::footer_hints(state, width),
            Self::WorklogHistory(state) => worklog_history::footer_hints(state, width),
            Self::Reports(state) => reports::footer_hints(state, width),
            Self::AllWorklogs(state) => all_worklogs::footer_hints(state, width),
        }
    }
}

/// A screen keymap result before the event-loop command is tagged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeymapCommand<C> {
    /// A command owned by the active screen.
    Local(C),
    /// A mode-sensitive request for the global quit command.
    Quit,
}

/// Applies global event filtering and tags commands from the active screen.
pub(crate) fn map_key(state: InputState<'_>, key: KeyEvent) -> Option<Command> {
    if key.kind != KeyEventKind::Press {
        return None;
    }
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(Command::Quit);
    }
    match state {
        InputState::TaskList(state) => task_list::map(state, key).map(|command| match command {
            KeymapCommand::Local(command) => Command::TaskList(command),
            KeymapCommand::Quit => Command::Quit,
        }),
        InputState::WorklogHistory(state) => {
            worklog_history::map(state, key).map(|command| match command {
                KeymapCommand::Local(command) => Command::WorklogHistory(command),
                KeymapCommand::Quit => Command::Quit,
            })
        }
        InputState::Reports(state) => reports::map(state, key).map(|command| match command {
            KeymapCommand::Local(command) => Command::Reports(command),
            KeymapCommand::Quit => Command::Quit,
        }),
        InputState::AllWorklogs(state) => {
            all_worklogs::map(state, key).map(|command| match command {
                KeymapCommand::Local(command) => Command::AllWorklogs(command),
                KeymapCommand::Quit => Command::Quit,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::keymap::{ctrl, valid_input_states};
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    #[test]
    fn key_releases_produce_no_command() {
        let release = KeyEvent::new_with_kind(
            KeyCode::Char('j'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );
        for state in valid_input_states() {
            assert_eq!(map_key(state.input(), release), None, "state was {state:?}");
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
        for state in valid_input_states() {
            for release in releases {
                assert_eq!(map_key(state.input(), release), None, "state was {state:?}");
            }
        }
    }

    #[test]
    fn ctrl_c_quits_in_every_mode_and_view() {
        for state in valid_input_states() {
            assert_eq!(
                map_key(state.input(), ctrl('c')),
                Some(Command::Quit),
                "state was {state:?}"
            );
        }
    }
}
pub(crate) use all_worklogs::{AllWorklogsCommand, AllWorklogsState};
