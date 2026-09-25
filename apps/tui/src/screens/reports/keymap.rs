use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::screens::KeymapCommand;

use super::{DateShift, ReportCommand, ReportFocus, ReportMode, ReportState};

pub(crate) fn map(state: &ReportState, key: KeyEvent) -> Option<KeymapCommand<ReportCommand>> {
    if matches!(state.mode, ReportMode::Custom { .. }) {
        return custom(key);
    }
    match state.focus {
        ReportFocus::TopTabs => top_tabs(key),
        ReportFocus::Presets => presets(key),
        ReportFocus::Rows => rows(state, key),
    }
}

fn top_tabs(key: KeyEvent) -> Option<KeymapCommand<ReportCommand>> {
    if key.modifiers == KeyModifiers::SHIFT && matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
        return Some(KeymapCommand::Local(ReportCommand::ShowArchived));
    }
    if key.modifiers != KeyModifiers::NONE {
        return None;
    }
    let command = match key.code {
        KeyCode::Tab => ReportCommand::ShowActive,
        KeyCode::BackTab => ReportCommand::ShowArchived,
        KeyCode::Enter | KeyCode::Char('j') | KeyCode::Down => ReportCommand::FocusPresets,
        KeyCode::Char('q') | KeyCode::Esc => return Some(KeymapCommand::Quit),
        _ => return None,
    };
    Some(KeymapCommand::Local(command))
}

fn presets(key: KeyEvent) -> Option<KeymapCommand<ReportCommand>> {
    if key.modifiers == KeyModifiers::SHIFT && matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
        return Some(KeymapCommand::Local(ReportCommand::PresetPrevious));
    }
    if key.modifiers != KeyModifiers::NONE {
        return None;
    }
    let command = match key.code {
        KeyCode::Tab | KeyCode::Char('l') | KeyCode::Right => ReportCommand::PresetNext,
        KeyCode::BackTab | KeyCode::Char('h') | KeyCode::Left => ReportCommand::PresetPrevious,
        KeyCode::Char('j') | KeyCode::Down => ReportCommand::FocusRows,
        KeyCode::Enter => ReportCommand::ChoosePreset,
        KeyCode::Esc | KeyCode::Char('k') | KeyCode::Up => ReportCommand::FocusTabs,
        _ => return None,
    };
    Some(KeymapCommand::Local(command))
}

fn rows(state: &ReportState, key: KeyEvent) -> Option<KeymapCommand<ReportCommand>> {
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
        return Some(KeymapCommand::Local(ReportCommand::FocusPresets));
    }
    if key.modifiers != KeyModifiers::NONE {
        return None;
    }
    rows_plain(state, key.code)
}

fn rows_plain(state: &ReportState, code: KeyCode) -> Option<KeymapCommand<ReportCommand>> {
    let command = match code {
        KeyCode::Tab | KeyCode::BackTab => ReportCommand::FocusPresets,
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
        KeyCode::Char('r') => ReportCommand::Refresh,
        KeyCode::Char('c') => ReportCommand::CopyName,
        KeyCode::Char('t') => ReportCommand::CopyExact,
        KeyCode::Char('s') => ReportCommand::CopyRounded,
        KeyCode::Enter => ReportCommand::OpenHistory,
        KeyCode::Esc => ReportCommand::FocusTabs,
        KeyCode::Char('q') => return Some(KeymapCommand::Quit),
        _ => return None,
    };
    Some(KeymapCommand::Local(command))
}

fn custom(key: KeyEvent) -> Option<KeymapCommand<ReportCommand>> {
    if let Some(shift) = custom_date_shift(key) {
        return Some(KeymapCommand::Local(ReportCommand::ShiftCustomDate(shift)));
    }
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

fn custom_date_shift(key: KeyEvent) -> Option<DateShift> {
    match (key.code, key.modifiers) {
        (KeyCode::Char('h') | KeyCode::Left, KeyModifiers::NONE) => Some(DateShift::PreviousDay),
        (KeyCode::Char('l') | KeyCode::Right, KeyModifiers::NONE) => Some(DateShift::NextDay),
        (KeyCode::Char('H'), KeyModifiers::NONE | KeyModifiers::SHIFT)
        | (KeyCode::Left, KeyModifiers::SHIFT) => Some(DateShift::PreviousMonth),
        (KeyCode::Char('L'), KeyModifiers::NONE | KeyModifiers::SHIFT)
        | (KeyCode::Right, KeyModifiers::SHIFT) => Some(DateShift::NextMonth),
        _ => None,
    }
}

pub(crate) fn footer_hints(state: &ReportState, width: u16) -> &'static str {
    if matches!(state.mode, ReportMode::Custom { .. }) {
        return if width < 80 {
            "h/l days · H/L months · Tab field · Enter apply · Esc cancel"
        } else {
            "YYYY-MM-DD · h/l day · H/L month · Tab/⇧Tab field · Enter apply · Esc cancel"
        };
    }
    match state.focus {
        ReportFocus::TopTabs if width < 80 => "Tab/⇧Tab tabs · Enter presets · q quit",
        ReportFocus::TopTabs => "Tab/⇧Tab switch tabs · Enter presets · q quit",
        ReportFocus::Presets if width < 80 => {
            "Tab/⇧Tab apply · Enter edit Custom · j rows · Esc tabs"
        }
        ReportFocus::Presets => {
            "Tab/⇧Tab or h/l apply period · Enter edit Custom · j rows · Esc tabs"
        }
        ReportFocus::Rows if width < 80 => {
            "j/k rows · h/l period · Enter logs · Tab presets · Esc tabs"
        }
        ReportFocus::Rows if width < 116 => {
            "j/k rows · h/l period · Enter logs · c/t/s copy · Tab presets · Esc tabs"
        }
        ReportFocus::Rows => {
            "j/k rows · gg/G ends · Ctrl+d/u page · h/l period · r refresh · Enter logs · c/t/s copy · Tab presets · Esc tabs"
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
    fn preset_keys_move_inline_cursor_without_switching_screen() {
        let mut state = ReportState::new(chrono::Utc::now(), chrono_tz::UTC);
        state.focus = ReportFocus::Presets;
        for (code, command) in [
            (KeyCode::Tab, ReportCommand::PresetNext),
            (KeyCode::BackTab, ReportCommand::PresetPrevious),
            (KeyCode::Char('h'), ReportCommand::PresetPrevious),
            (KeyCode::Char('l'), ReportCommand::PresetNext),
            (KeyCode::Char('j'), ReportCommand::FocusRows),
            (KeyCode::Enter, ReportCommand::ChoosePreset),
            (KeyCode::Esc, ReportCommand::FocusTabs),
        ] {
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
                KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)
            ),
            Some(Command::Reports(ReportCommand::PresetPrevious))
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
        for (code, command) in [
            (KeyCode::Enter, ReportCommand::ConfirmCustom),
            (KeyCode::Esc, ReportCommand::Cancel),
            (KeyCode::Backspace, ReportCommand::Backspace),
        ] {
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
                KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)
            ),
            Some(Command::Reports(ReportCommand::SwitchField))
        );
        assert_eq!(
            map_key(
                InputState::Reports(&state),
                KeyEvent::new(KeyCode::Char('-'), KeyModifiers::NONE)
            ),
            Some(Command::Reports(ReportCommand::Insert('-')))
        );
        for (code, modifiers, shift) in [
            (
                KeyCode::Char('h'),
                KeyModifiers::NONE,
                DateShift::PreviousDay,
            ),
            (KeyCode::Char('l'), KeyModifiers::NONE, DateShift::NextDay),
            (
                KeyCode::Char('H'),
                KeyModifiers::SHIFT,
                DateShift::PreviousMonth,
            ),
            (
                KeyCode::Char('L'),
                KeyModifiers::SHIFT,
                DateShift::NextMonth,
            ),
            (
                KeyCode::Char('H'),
                KeyModifiers::NONE,
                DateShift::PreviousMonth,
            ),
            (KeyCode::Char('L'), KeyModifiers::NONE, DateShift::NextMonth),
        ] {
            assert_eq!(
                map_key(InputState::Reports(&state), KeyEvent::new(code, modifiers)),
                Some(Command::Reports(ReportCommand::ShiftCustomDate(shift)))
            );
        }
        for code in [
            KeyCode::Tab,
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Backspace,
            KeyCode::Char('2'),
        ] {
            assert_eq!(
                map_key(
                    InputState::Reports(&state),
                    KeyEvent::new(code, KeyModifiers::CONTROL)
                ),
                None
            );
        }
        assert_eq!(
            map_key(
                InputState::Reports(&state),
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)
            ),
            None
        );
    }

    #[test]
    fn keymap_follows_the_focused_area() {
        let mut state = ReportState::new(chrono::Utc::now(), chrono_tz::UTC);
        let mapped = |state: &ReportState, code| {
            map_key(
                InputState::Reports(state),
                KeyEvent::new(code, KeyModifiers::NONE),
            )
        };
        assert_eq!(
            mapped(&state, KeyCode::Enter),
            Some(Command::Reports(ReportCommand::FocusPresets))
        );
        assert_eq!(
            mapped(&state, KeyCode::Tab),
            Some(Command::Reports(ReportCommand::ShowActive))
        );
        assert_eq!(
            mapped(&state, KeyCode::BackTab),
            Some(Command::Reports(ReportCommand::ShowArchived))
        );
        state.focus = ReportFocus::Rows;
        assert_eq!(
            mapped(&state, KeyCode::Tab),
            Some(Command::Reports(ReportCommand::FocusPresets))
        );
        assert_eq!(
            mapped(&state, KeyCode::Char('h')),
            Some(Command::Reports(ReportCommand::PreviousPeriod))
        );
        assert_eq!(
            mapped(&state, KeyCode::Char('l')),
            Some(Command::Reports(ReportCommand::NextPeriod))
        );
        assert_eq!(
            mapped(&state, KeyCode::Char('c')),
            Some(Command::Reports(ReportCommand::CopyName))
        );
        assert_eq!(
            mapped(&state, KeyCode::Enter),
            Some(Command::Reports(ReportCommand::OpenHistory))
        );
        assert_eq!(
            mapped(&state, KeyCode::Esc),
            Some(Command::Reports(ReportCommand::FocusTabs))
        );
        state.g_prefix = true;
        assert_eq!(
            mapped(&state, KeyCode::Char('g')),
            Some(Command::Reports(ReportCommand::First))
        );
    }

    #[test]
    fn tab_and_row_keys_respect_focus_and_modifiers() {
        let mut state = ReportState::new(chrono::Utc::now(), chrono_tz::UTC);
        let mapped = |state: &ReportState, code, modifiers| {
            map_key(InputState::Reports(state), KeyEvent::new(code, modifiers))
        };
        for code in [KeyCode::Char('q'), KeyCode::Esc] {
            assert_eq!(
                mapped(&state, code, KeyModifiers::NONE),
                Some(Command::Quit)
            );
        }
        state.focus = ReportFocus::Rows;
        for (code, expected) in [
            (
                KeyCode::Char('G'),
                Some(Command::Reports(ReportCommand::Last)),
            ),
            (
                KeyCode::Char('r'),
                Some(Command::Reports(ReportCommand::Refresh)),
            ),
            (KeyCode::Char('q'), Some(Command::Quit)),
        ] {
            assert_eq!(mapped(&state, code, KeyModifiers::NONE), expected);
        }
        assert_eq!(
            mapped(&state, KeyCode::Char('G'), KeyModifiers::SHIFT),
            Some(Command::Reports(ReportCommand::Last))
        );
        assert_eq!(
            mapped(&state, KeyCode::BackTab, KeyModifiers::SHIFT),
            Some(Command::Reports(ReportCommand::FocusPresets))
        );
        assert_eq!(
            mapped(&state, KeyCode::Tab, KeyModifiers::SHIFT),
            Some(Command::Reports(ReportCommand::FocusPresets))
        );
        for (code, modifiers) in [
            (KeyCode::Char('G'), KeyModifiers::ALT),
            (KeyCode::Char('r'), KeyModifiers::SHIFT),
            (KeyCode::Char('q'), KeyModifiers::CONTROL),
            (KeyCode::Char('x'), KeyModifiers::CONTROL),
            (KeyCode::Tab, KeyModifiers::ALT),
            (KeyCode::Char('d'), KeyModifiers::ALT),
        ] {
            assert_eq!(mapped(&state, code, modifiers), None);
        }
        assert_eq!(
            mapped(&state, KeyCode::Char('d'), KeyModifiers::CONTROL),
            Some(Command::Reports(ReportCommand::PageDown))
        );
        assert_eq!(
            mapped(&state, KeyCode::Char('u'), KeyModifiers::CONTROL),
            Some(Command::Reports(ReportCommand::PageUp))
        );
    }

    #[test]
    fn footer_hints_follow_focus_and_fit_each_width() {
        let mut state = ReportState::new(chrono::Utc::now(), chrono_tz::UTC);
        for (focus, narrow, medium, wide) in [
            (
                ReportFocus::TopTabs,
                "Tab/⇧Tab tabs · Enter presets · q quit",
                "Tab/⇧Tab switch tabs · Enter presets · q quit",
                "Tab/⇧Tab switch tabs · Enter presets · q quit",
            ),
            (
                ReportFocus::Presets,
                "Tab/⇧Tab apply · Enter edit Custom · j rows · Esc tabs",
                "Tab/⇧Tab or h/l apply period · Enter edit Custom · j rows · Esc tabs",
                "Tab/⇧Tab or h/l apply period · Enter edit Custom · j rows · Esc tabs",
            ),
            (
                ReportFocus::Rows,
                "j/k rows · h/l period · Enter logs · Tab presets · Esc tabs",
                "j/k rows · h/l period · Enter logs · c/t/s copy · Tab presets · Esc tabs",
                "j/k rows · gg/G ends · Ctrl+d/u page · h/l period · r refresh · Enter logs · c/t/s copy · Tab presets · Esc tabs",
            ),
        ] {
            state.focus = focus;
            assert!(footer_hints(&state, 60).chars().count() <= 60);
            for (width, expected) in [(79, narrow), (80, medium), (115, medium), (116, wide)] {
                let hints = footer_hints(&state, width);
                assert_eq!(hints, expected);
                assert!(hints.chars().count() <= width as usize);
            }
        }
        state.mode = ReportMode::Custom {
            from: String::new(),
            to: String::new(),
            focus_to: false,
        };
        assert_eq!(
            footer_hints(&state, 79),
            "h/l days · H/L months · Tab field · Enter apply · Esc cancel"
        );
        assert_eq!(
            footer_hints(&state, 80),
            "YYYY-MM-DD · h/l day · H/L month · Tab/⇧Tab field · Enter apply · Esc cancel"
        );
    }
}
