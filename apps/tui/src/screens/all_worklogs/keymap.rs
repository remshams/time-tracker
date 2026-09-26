use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::screens::KeymapCommand;
use crate::screens::worklog_history::{WorklogHistoryCommand, keymap as history_keymap};

use super::{AllWorklogsCommand as C, AllWorklogsFocus, AllWorklogsState};

pub(crate) fn map(state: &AllWorklogsState, key: KeyEvent) -> Option<KeymapCommand<C>> {
    if let Some(draft) = &state.move_draft {
        return history_keymap::map_move(draft.focus(), key).and_then(|mapped| match mapped {
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
        });
    }
    if state.focus == AllWorklogsFocus::Tabs {
        if key.modifiers == KeyModifiers::SHIFT
            && matches!(key.code, KeyCode::Tab | KeyCode::BackTab)
        {
            return Some(KeymapCommand::Local(C::ShowReports));
        }
        if key.modifiers != KeyModifiers::NONE {
            return None;
        }
        return match key.code {
            KeyCode::Tab => Some(KeymapCommand::Local(C::ShowActive)),
            KeyCode::BackTab => Some(KeymapCommand::Local(C::ShowReports)),
            KeyCode::Enter | KeyCode::Char('j') | KeyCode::Down => {
                Some(KeymapCommand::Local(C::FocusRows))
            }
            KeyCode::Char('q') | KeyCode::Esc => Some(KeymapCommand::Quit),
            _ => None,
        };
    }
    if !state.available {
        return match (key.code, key.modifiers) {
            (KeyCode::Char('r'), KeyModifiers::NONE) => Some(KeymapCommand::Local(C::Refresh)),
            (KeyCode::Esc, KeyModifiers::NONE) => Some(KeymapCommand::Local(C::FocusTabs)),
            (KeyCode::Char('q'), KeyModifiers::NONE) => Some(KeymapCommand::Quit),
            _ => None,
        };
    }
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
    let command = match key.code {
        KeyCode::Char('j') | KeyCode::Down => C::MoveDown,
        KeyCode::Char('k') | KeyCode::Up => C::MoveUp,
        KeyCode::Char('g') => {
            if state.g_prefix {
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
