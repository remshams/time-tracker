use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::screens::KeymapCommand;

use super::{ReportCommand, ReportMode, ReportState};

pub(crate) fn map(state: &ReportState, key: KeyEvent) -> Option<KeymapCommand<ReportCommand>> {
    match &state.mode {
        ReportMode::Normal => normal(state, key),
        ReportMode::Presets { .. } => presets(key),
        ReportMode::Custom { .. } => custom(key),
    }
}

fn normal(state: &ReportState, key: KeyEvent) -> Option<KeymapCommand<ReportCommand>> {
    if key.code == KeyCode::Char('G') && key.modifiers == KeyModifiers::SHIFT {
        return Some(KeymapCommand::Local(ReportCommand::Last));
    }
    if key.modifiers == KeyModifiers::CONTROL {
        return match key.code {
            KeyCode::Char('d') => Some(KeymapCommand::Local(ReportCommand::PageDown)),
            KeyCode::Char('u') => Some(KeymapCommand::Local(ReportCommand::PageUp)),
            _ => None,
        };
    }
    if key.modifiers == KeyModifiers::SHIFT && matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
        return Some(KeymapCommand::Local(ReportCommand::ShowArchived));
    }
    if key.modifiers != KeyModifiers::NONE {
        return None;
    }
    let command = match key.code {
        KeyCode::Tab => ReportCommand::ShowActive,
        KeyCode::BackTab => ReportCommand::ShowArchived,
        KeyCode::Char('j') | KeyCode::Down => ReportCommand::MoveDown,
        KeyCode::Char('k') | KeyCode::Up => ReportCommand::MoveUp,
        KeyCode::Char('G') => ReportCommand::Last,
        KeyCode::Char('g') => {
            if state.g_prefix {
                ReportCommand::First
            } else {
                ReportCommand::GPrefix
            }
        }
        KeyCode::Char('h') | KeyCode::Left => ReportCommand::PreviousPeriod,
        KeyCode::Char('l') | KeyCode::Right => ReportCommand::NextPeriod,
        KeyCode::Char('p') => ReportCommand::OpenPresets,
        KeyCode::Char('r') => ReportCommand::Refresh,
        KeyCode::Char('c') => ReportCommand::CopyName,
        KeyCode::Char('t') => ReportCommand::CopyExact,
        KeyCode::Char('s') => ReportCommand::CopyRounded,
        KeyCode::Enter => ReportCommand::OpenHistory,
        KeyCode::Char('q') | KeyCode::Esc => return Some(KeymapCommand::Quit),
        _ => return None,
    };
    Some(KeymapCommand::Local(command))
}

fn presets(key: KeyEvent) -> Option<KeymapCommand<ReportCommand>> {
    if key.modifiers != KeyModifiers::NONE {
        return None;
    }
    let command = match key.code {
        KeyCode::Char('j') | KeyCode::Down => ReportCommand::PresetDown,
        KeyCode::Char('k') | KeyCode::Up => ReportCommand::PresetUp,
        KeyCode::Enter => ReportCommand::ChoosePreset,
        KeyCode::Esc => ReportCommand::Cancel,
        _ => return None,
    };
    Some(KeymapCommand::Local(command))
}

fn custom(key: KeyEvent) -> Option<KeymapCommand<ReportCommand>> {
    let command = match key.code {
        KeyCode::Tab | KeyCode::BackTab
            if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
        {
            ReportCommand::SwitchField
        }
        KeyCode::Enter if key.modifiers == KeyModifiers::NONE => ReportCommand::ConfirmCustom,
        KeyCode::Esc if key.modifiers == KeyModifiers::NONE => ReportCommand::Cancel,
        KeyCode::Backspace if key.modifiers == KeyModifiers::NONE => ReportCommand::Backspace,
        KeyCode::Char(character)
            if key.modifiers - KeyModifiers::SHIFT == KeyModifiers::NONE
                && (character.is_ascii_digit() || character == '-') =>
        {
            ReportCommand::Insert(character)
        }
        _ => return None,
    };
    Some(KeymapCommand::Local(command))
}

pub(crate) fn footer_hints(state: &ReportState, width: u16) -> &'static str {
    match state.mode {
        ReportMode::Normal if width < 80 => {
            "j/k h/l p preset r refresh enter log c/t/s copy tab/S-tab q"
        }
        ReportMode::Normal => {
            "j/k gg/G Ctrl+d/u h/l p preset r refresh enter history c/t/s copy tab/S-tab q"
        }
        ReportMode::Presets { .. } => "j/k choose · enter select · esc cancel · ctrl+c quit",
        ReportMode::Custom { .. } => {
            "type date · tab/S-tab field · enter apply · esc cancel · ctrl+c quit"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;
    use crate::screens::InputState;
    use crate::screens::map_key;

    #[test]
    fn dialogs_take_letters_and_tab_without_switching_screen() {
        let mut state = ReportState::new(chrono::Utc::now(), chrono_tz::UTC);
        state.mode = ReportMode::Presets { selected: 0 };
        assert_eq!(
            map_key(
                InputState::Reports(&state),
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)
            ),
            Some(Command::Reports(ReportCommand::PresetDown))
        );
        state.mode = ReportMode::Custom {
            from: String::new(),
            to: String::new(),
            focus_to: false,
        };
        assert_eq!(
            map_key(
                InputState::Reports(&state),
                KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)
            ),
            Some(Command::Reports(ReportCommand::SwitchField))
        );
        assert_eq!(
            map_key(
                InputState::Reports(&state),
                KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE)
            ),
            Some(Command::Reports(ReportCommand::Insert('2')))
        );
    }

    #[test]
    fn normal_keys_select_rows_periods_and_copy_values() {
        let state = ReportState::new(chrono::Utc::now(), chrono_tz::UTC);
        let cases = [
            (KeyCode::Tab, ReportCommand::ShowActive),
            (KeyCode::BackTab, ReportCommand::ShowArchived),
            (KeyCode::Char('j'), ReportCommand::MoveDown),
            (KeyCode::Char('k'), ReportCommand::MoveUp),
            (KeyCode::Char('h'), ReportCommand::PreviousPeriod),
            (KeyCode::Char('l'), ReportCommand::NextPeriod),
            (KeyCode::Char('p'), ReportCommand::OpenPresets),
            (KeyCode::Char('r'), ReportCommand::Refresh),
            (KeyCode::Char('g'), ReportCommand::GPrefix),
            (KeyCode::Char('G'), ReportCommand::Last),
            (KeyCode::Char('c'), ReportCommand::CopyName),
            (KeyCode::Char('t'), ReportCommand::CopyExact),
            (KeyCode::Char('s'), ReportCommand::CopyRounded),
            (KeyCode::Enter, ReportCommand::OpenHistory),
        ];
        for (code, command) in cases {
            assert_eq!(
                map_key(
                    InputState::Reports(&state),
                    KeyEvent::new(code, KeyModifiers::NONE)
                ),
                Some(Command::Reports(command))
            );
        }
        assert_eq!(
            map_key(
                InputState::Reports(&state),
                KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)
            ),
            Some(Command::Reports(ReportCommand::PageDown))
        );
        assert_eq!(
            map_key(
                InputState::Reports(&state),
                KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)
            ),
            Some(Command::Reports(ReportCommand::PageUp))
        );
        assert_eq!(
            map_key(
                InputState::Reports(&state),
                KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT)
            ),
            Some(Command::Reports(ReportCommand::Last))
        );
        assert_eq!(
            map_key(
                InputState::Reports(&state),
                KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT)
            ),
            Some(Command::Reports(ReportCommand::ShowArchived))
        );
        assert_eq!(
            map_key(
                InputState::Reports(&state),
                KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)
            ),
            Some(Command::Quit)
        );
        assert_eq!(
            map_key(
                InputState::Reports(&state),
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)
            ),
            None
        );
        assert_eq!(
            map_key(
                InputState::Reports(&state),
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::ALT)
            ),
            None
        );
        assert_eq!(
            map_key(
                InputState::Reports(&state),
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)
            ),
            None
        );
        let mut pending = state;
        pending.g_prefix = true;
        assert_eq!(
            map_key(
                InputState::Reports(&pending),
                KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE)
            ),
            Some(Command::Reports(ReportCommand::First))
        );
    }
}
