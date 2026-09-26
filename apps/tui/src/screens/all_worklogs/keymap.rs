use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::screens::KeymapCommand;
use crate::screens::worklog_history::{WorklogHistoryCommand, keymap as history_keymap};

use super::{AllWorklogsCommand as C, AllWorklogsFocus, AllWorklogsState};

pub(crate) fn map(state: &AllWorklogsState, key: KeyEvent) -> Option<KeymapCommand<C>> {
    if let Some(draft) = &state.move_draft {
        return map_move(draft.focus(), key);
    }
    if state.focus == AllWorklogsFocus::Tabs {
        return map_tabs(key);
    }
    if !state.available {
        return map_unavailable(key);
    }
    map_rows(state.g_prefix, key)
}

fn map_move(
    focus: crate::screens::worklog_history::MoveFocus,
    key: KeyEvent,
) -> Option<KeymapCommand<C>> {
    history_keymap::map_move(focus, key).and_then(|mapped| match mapped {
        KeymapCommand::Quit => Some(KeymapCommand::Quit),
        KeymapCommand::Local(command) => Some(KeymapCommand::Local(match command {
            WorklogHistoryCommand::ToggleMoveFocus => C::ToggleMoveFocus,
            WorklogHistoryCommand::MoveDestinationUp => C::MoveDestinationUp,
            WorklogHistoryCommand::MoveDestinationDown => C::MoveDestinationDown,
            WorklogHistoryCommand::InsertMoveQuery(character) => C::InsertMoveQuery(character),
            WorklogHistoryCommand::BackspaceMoveQuery => C::BackspaceMoveQuery,
            WorklogHistoryCommand::Confirm => C::ConfirmMove,
            WorklogHistoryCommand::Cancel => C::CancelMove,
            _ => return None,
        })),
    })
}

fn map_tabs(key: KeyEvent) -> Option<KeymapCommand<C>> {
    if key.modifiers == KeyModifiers::SHIFT && matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
        return Some(KeymapCommand::Local(C::ShowReports));
    }
    if key.modifiers != KeyModifiers::NONE {
        return None;
    }
    match key.code {
        KeyCode::Tab => Some(KeymapCommand::Local(C::ShowActive)),
        KeyCode::BackTab => Some(KeymapCommand::Local(C::ShowReports)),
        KeyCode::Enter | KeyCode::Char('j') | KeyCode::Down => {
            Some(KeymapCommand::Local(C::FocusRows))
        }
        KeyCode::Char('q') | KeyCode::Esc => Some(KeymapCommand::Quit),
        _ => None,
    }
}

fn map_unavailable(key: KeyEvent) -> Option<KeymapCommand<C>> {
    match (key.code, key.modifiers) {
        (KeyCode::Char('r'), KeyModifiers::NONE) => Some(KeymapCommand::Local(C::Refresh)),
        (KeyCode::Esc, KeyModifiers::NONE) => Some(KeymapCommand::Local(C::FocusTabs)),
        (KeyCode::Char('q'), KeyModifiers::NONE) => Some(KeymapCommand::Quit),
        _ => None,
    }
}

fn map_rows(g_prefix: bool, key: KeyEvent) -> Option<KeymapCommand<C>> {
    if key.modifiers == KeyModifiers::CONTROL {
        return match key.code {
            KeyCode::Char('d') => Some(KeymapCommand::Local(C::PageDown)),
            KeyCode::Char('u') => Some(KeymapCommand::Local(C::PageUp)),
            _ => None,
        };
    }
    if key.code == KeyCode::Char('G') && key.modifiers == KeyModifiers::SHIFT {
        return Some(KeymapCommand::Local(C::Last));
    }
    if key.modifiers == KeyModifiers::SHIFT && matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
        return Some(KeymapCommand::Local(C::FocusTabs));
    }
    if key.modifiers != KeyModifiers::NONE {
        return None;
    }
    map_rows_plain(g_prefix, key.code)
}

fn map_rows_plain(g_prefix: bool, code: KeyCode) -> Option<KeymapCommand<C>> {
    let command = match code {
        KeyCode::Char('j') | KeyCode::Down => C::MoveDown,
        KeyCode::Char('k') | KeyCode::Up => C::MoveUp,
        KeyCode::Char('g') => {
            if g_prefix {
                C::First
            } else {
                C::GPrefix
            }
        }
        KeyCode::Char('G') => C::Last,
        KeyCode::Char('m') => C::OpenMove,
        KeyCode::Char('o') => C::LoadOlder,
        KeyCode::Char('r') => C::Refresh,
        KeyCode::Tab | KeyCode::BackTab | KeyCode::Esc => C::FocusTabs,
        KeyCode::Char('q') => return Some(KeymapCommand::Quit),
        _ => return None,
    };
    Some(KeymapCommand::Local(command))
}

pub(crate) fn footer_hints(state: &AllWorklogsState, width: u16) -> &'static str {
    if let Some(draft) = &state.move_draft {
        return history_keymap::move_footer_hints(draft.focus(), width);
    }
    match state.focus {
        AllWorklogsFocus::Tabs => "Tab/⇧Tab switch tabs · Enter rows · q quit",
        AllWorklogsFocus::Rows if !state.available => "r retry · Esc tabs · q quit",
        AllWorklogsFocus::Rows if state.pagination_invalidated => {
            "j/k rows · m move · r refresh for older · Esc tabs · q quit"
        }
        AllWorklogsFocus::Rows if width < 80 => {
            "j/k rows · m move · o older · r refresh · Esc tabs · q quit"
        }
        AllWorklogsFocus::Rows => {
            "j/k · gg/G · Ctrl+d/u · m move · o older · r refresh · Esc tabs · q quit"
        }
    }
}

#[cfg(test)]
mod tests {
    use tracker_application::{GlobalWorklogPage, TrackerSnapshot};
    use tracker_domain::{TaskId, Worklog, WorklogId};

    use crate::screens::worklog_history::MoveDraft;

    use super::*;

    fn state() -> AllWorklogsState {
        AllWorklogsState::new(GlobalWorklogPage {
            worklogs: Vec::new(),
            snapshot: TrackerSnapshot {
                task_items: Vec::new(),
                active_worklog: None,
            },
            next_cursor: None,
        })
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn tabs_wrap_and_only_enter_descends_into_rows() {
        let state = state();
        for (code, modifiers, expected) in [
            (
                KeyCode::Tab,
                KeyModifiers::NONE,
                Some(KeymapCommand::Local(C::ShowActive)),
            ),
            (
                KeyCode::BackTab,
                KeyModifiers::NONE,
                Some(KeymapCommand::Local(C::ShowReports)),
            ),
            (
                KeyCode::Tab,
                KeyModifiers::SHIFT,
                Some(KeymapCommand::Local(C::ShowReports)),
            ),
            (
                KeyCode::BackTab,
                KeyModifiers::SHIFT,
                Some(KeymapCommand::Local(C::ShowReports)),
            ),
            (
                KeyCode::Enter,
                KeyModifiers::NONE,
                Some(KeymapCommand::Local(C::FocusRows)),
            ),
            (
                KeyCode::Char('j'),
                KeyModifiers::NONE,
                Some(KeymapCommand::Local(C::FocusRows)),
            ),
            (
                KeyCode::Down,
                KeyModifiers::NONE,
                Some(KeymapCommand::Local(C::FocusRows)),
            ),
            (KeyCode::Esc, KeyModifiers::NONE, Some(KeymapCommand::Quit)),
            (
                KeyCode::Char('q'),
                KeyModifiers::NONE,
                Some(KeymapCommand::Quit),
            ),
            (KeyCode::Char('m'), KeyModifiers::NONE, None),
            (KeyCode::Tab, KeyModifiers::ALT, None),
        ] {
            assert_eq!(map(&state, key(code, modifiers)), expected);
        }
    }

    #[test]
    fn row_motions_and_actions_respect_modifiers_and_g_prefix() {
        let mut state = state();
        state.focus = AllWorklogsFocus::Rows;
        for (code, modifiers, expected) in [
            (
                KeyCode::Char('j'),
                KeyModifiers::NONE,
                Some(KeymapCommand::Local(C::MoveDown)),
            ),
            (
                KeyCode::Down,
                KeyModifiers::NONE,
                Some(KeymapCommand::Local(C::MoveDown)),
            ),
            (
                KeyCode::Char('k'),
                KeyModifiers::NONE,
                Some(KeymapCommand::Local(C::MoveUp)),
            ),
            (
                KeyCode::Up,
                KeyModifiers::NONE,
                Some(KeymapCommand::Local(C::MoveUp)),
            ),
            (
                KeyCode::Char('g'),
                KeyModifiers::NONE,
                Some(KeymapCommand::Local(C::GPrefix)),
            ),
            (
                KeyCode::Char('G'),
                KeyModifiers::SHIFT,
                Some(KeymapCommand::Local(C::Last)),
            ),
            (
                KeyCode::Char('d'),
                KeyModifiers::CONTROL,
                Some(KeymapCommand::Local(C::PageDown)),
            ),
            (
                KeyCode::Char('u'),
                KeyModifiers::CONTROL,
                Some(KeymapCommand::Local(C::PageUp)),
            ),
            (
                KeyCode::Char('m'),
                KeyModifiers::NONE,
                Some(KeymapCommand::Local(C::OpenMove)),
            ),
            (
                KeyCode::Char('o'),
                KeyModifiers::NONE,
                Some(KeymapCommand::Local(C::LoadOlder)),
            ),
            (
                KeyCode::Char('r'),
                KeyModifiers::NONE,
                Some(KeymapCommand::Local(C::Refresh)),
            ),
            (
                KeyCode::Tab,
                KeyModifiers::NONE,
                Some(KeymapCommand::Local(C::FocusTabs)),
            ),
            (
                KeyCode::BackTab,
                KeyModifiers::NONE,
                Some(KeymapCommand::Local(C::FocusTabs)),
            ),
            (
                KeyCode::Esc,
                KeyModifiers::NONE,
                Some(KeymapCommand::Local(C::FocusTabs)),
            ),
            (
                KeyCode::Char('q'),
                KeyModifiers::NONE,
                Some(KeymapCommand::Quit),
            ),
            (KeyCode::Char('m'), KeyModifiers::SHIFT, None),
            (KeyCode::Tab, KeyModifiers::ALT, None),
            (KeyCode::Char('x'), KeyModifiers::CONTROL, None),
        ] {
            assert_eq!(map(&state, key(code, modifiers)), expected);
        }
        state.g_prefix = true;
        assert_eq!(
            map(&state, key(KeyCode::Char('g'), KeyModifiers::NONE)),
            Some(KeymapCommand::Local(C::First))
        );
        assert_eq!(
            map(&state, key(KeyCode::Tab, KeyModifiers::SHIFT)),
            Some(KeymapCommand::Local(C::FocusTabs))
        );
    }

    #[test]
    fn unavailable_rows_only_allow_retry_return_and_quit() {
        let mut state = state();
        state.focus = AllWorklogsFocus::Rows;
        state.available = false;
        for (code, modifiers, expected) in [
            (
                KeyCode::Char('r'),
                KeyModifiers::NONE,
                Some(KeymapCommand::Local(C::Refresh)),
            ),
            (
                KeyCode::Esc,
                KeyModifiers::NONE,
                Some(KeymapCommand::Local(C::FocusTabs)),
            ),
            (
                KeyCode::Char('q'),
                KeyModifiers::NONE,
                Some(KeymapCommand::Quit),
            ),
            (KeyCode::Char('m'), KeyModifiers::NONE, None),
            (KeyCode::Char('r'), KeyModifiers::ALT, None),
        ] {
            assert_eq!(map(&state, key(code, modifiers)), expected);
        }
    }

    #[test]
    fn move_dialog_uses_the_history_keymap_before_row_actions() {
        let mut state = state();
        state.focus = AllWorklogsFocus::Rows;
        let source = Worklog::begin(
            WorklogId::generate(),
            TaskId::generate(),
            chrono::Utc::now(),
        );
        state.move_draft = Some(MoveDraft::new(source, Vec::new()));
        for (code, modifiers, expected) in [
            (
                KeyCode::Char('a'),
                KeyModifiers::NONE,
                Some(KeymapCommand::Local(C::InsertMoveQuery('a'))),
            ),
            (
                KeyCode::Backspace,
                KeyModifiers::NONE,
                Some(KeymapCommand::Local(C::BackspaceMoveQuery)),
            ),
            (
                KeyCode::Tab,
                KeyModifiers::NONE,
                Some(KeymapCommand::Local(C::ToggleMoveFocus)),
            ),
            (
                KeyCode::Down,
                KeyModifiers::NONE,
                Some(KeymapCommand::Local(C::MoveDestinationDown)),
            ),
            (
                KeyCode::Up,
                KeyModifiers::NONE,
                Some(KeymapCommand::Local(C::MoveDestinationUp)),
            ),
            (
                KeyCode::Enter,
                KeyModifiers::NONE,
                Some(KeymapCommand::Local(C::ConfirmMove)),
            ),
            (
                KeyCode::Esc,
                KeyModifiers::NONE,
                Some(KeymapCommand::Local(C::CancelMove)),
            ),
            (KeyCode::Char('r'), KeyModifiers::ALT, None),
        ] {
            assert_eq!(map(&state, key(code, modifiers)), expected);
        }
    }
}
