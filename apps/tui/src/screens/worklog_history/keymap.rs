//! Key mappings and footer help for worklog history.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::command::Command;

use super::{WorklogHistoryMode, WorklogHistoryState};

pub(crate) fn map(state: &WorklogHistoryState, key: KeyEvent) -> Option<Command> {
    match state.mode() {
        WorklogHistoryMode::Normal => map_normal(state, key),
        WorklogHistoryMode::ConfirmDeletion { .. } => map_confirm_deletion(key),
        WorklogHistoryMode::Correction(_) => map_correction(key),
    }
}

fn map_normal(state: &WorklogHistoryState, key: KeyEvent) -> Option<Command> {
    if !state.history().is_available() || key.modifiers != KeyModifiers::NONE {
        if !state.history().is_available() {
            return match (key.code, key.modifiers) {
                (KeyCode::Char('r'), KeyModifiers::NONE) => Some(Command::RefreshWorklogs),
                (KeyCode::Esc, KeyModifiers::NONE) => Some(Command::BackToTaskList),
                (KeyCode::Char('q'), KeyModifiers::NONE) => Some(Command::Quit),
                _ => None,
            };
        }
        return None;
    }
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => Some(Command::MoveDown),
        KeyCode::Char('k') | KeyCode::Up => Some(Command::MoveUp),
        KeyCode::Char('e') => Some(Command::OpenCorrection),
        KeyCode::Char('d') => Some(Command::OpenDeletion),
        KeyCode::Char('o') => Some(Command::LoadOlderWorklogs),
        KeyCode::Char('r') => Some(Command::RefreshWorklogs),
        KeyCode::Esc => Some(Command::BackToTaskList),
        KeyCode::Char('q') => Some(Command::Quit),
        _ => None,
    }
}

fn map_confirm_deletion(key: KeyEvent) -> Option<Command> {
    if key.modifiers != KeyModifiers::NONE {
        return None;
    }
    match key.code {
        KeyCode::Enter | KeyCode::Char('d') | KeyCode::Char('y') => Some(Command::Confirm),
        KeyCode::Esc | KeyCode::Char('n') => Some(Command::Cancel),
        _ => None,
    }
}

fn map_correction(key: KeyEvent) -> Option<Command> {
    match (key.code, key.modifiers) {
        (KeyCode::Enter, KeyModifiers::NONE) => Some(Command::Confirm),
        (KeyCode::Esc, KeyModifiers::NONE) => Some(Command::Cancel),
        (KeyCode::Tab, KeyModifiers::NONE)
        | (KeyCode::BackTab, KeyModifiers::SHIFT)
        | (KeyCode::BackTab, KeyModifiers::NONE) => Some(Command::SwitchCorrectionField),
        (KeyCode::Left, KeyModifiers::NONE) => Some(Command::MoveCursorLeft),
        (KeyCode::Right, KeyModifiers::NONE) => Some(Command::MoveCursorRight),
        (KeyCode::Backspace, KeyModifiers::NONE) => Some(Command::Backspace),
        (KeyCode::Delete, KeyModifiers::NONE) => Some(Command::Delete),
        (KeyCode::Char('j'), KeyModifiers::NONE) => Some(Command::AdjustForwardFiveMinutes),
        (KeyCode::Char('k'), KeyModifiers::NONE) => Some(Command::AdjustBackwardFiveMinutes),
        (KeyCode::Char('J'), modifiers)
            if modifiers == KeyModifiers::NONE || modifiers == KeyModifiers::SHIFT =>
        {
            Some(Command::AdjustForwardOneHour)
        }
        (KeyCode::Char('K'), modifiers)
            if modifiers == KeyModifiers::NONE || modifiers == KeyModifiers::SHIFT =>
        {
            Some(Command::AdjustBackwardOneHour)
        }
        (KeyCode::Char(character), modifiers)
            if modifiers - KeyModifiers::SHIFT == KeyModifiers::NONE
                && crate::app::is_timestamp_character(character) =>
        {
            Some(Command::Insert(character))
        }
        _ => None,
    }
}

pub(crate) fn footer_hints(state: &WorklogHistoryState, width: u16) -> &'static str {
    match state.mode() {
        WorklogHistoryMode::Correction(_) => {
            if width < 80 {
                "type ←/→ bs/del tab/S-tab j/k ±5m J/K ±1h enter esc ctrl+c"
            } else {
                "type · ←/→ · bs/del · tab/S-tab · j/k ±5m · J/K ±1h · enter · esc · ctrl+c"
            }
        }
        WorklogHistoryMode::ConfirmDeletion { .. } => {
            if width < 80 {
                "d/y/enter delete n/esc cancel ctrl+c quit"
            } else {
                "d/y/enter delete · n/esc cancel · ctrl+c quit"
            }
        }
        WorklogHistoryMode::Normal if !state.history().is_available() => {
            if width < 80 {
                "r retry esc back q/ctrl+c quit"
            } else {
                "r retry · esc back · q/ctrl+c quit"
            }
        }
        WorklogHistoryMode::Normal => {
            if width < 80 {
                "j/k/↑/↓ e edit d delete o older r refresh esc back q ctrl+c"
            } else {
                "j/k/↑/↓ move e edit d delete o older r refresh esc back q/ctrl+c quit"
            }
        }
    }
}

#[cfg(test)]
mod restored_keymap_tests {
    use crate::command::Command;
    use crate::screens::keymap_test_support::*;
    use crate::screens::{Screen, TaskView};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn deletion_mode_accepts_and_cancels_without_other_commands() {
        let mode = deletion();
        for code in [KeyCode::Enter, KeyCode::Char('d'), KeyCode::Char('y')] {
            assert_eq!(
                map_legacy(&mode, TaskView::Active, Screen::WorklogHistory, key(code)),
                Some(Command::Confirm)
            );
        }
        for code in [KeyCode::Esc, KeyCode::Char('n')] {
            assert_eq!(
                map_legacy(&mode, TaskView::Active, Screen::WorklogHistory, key(code)),
                Some(Command::Cancel)
            );
        }
        assert_eq!(
            map_legacy(
                &mode,
                TaskView::Active,
                Screen::WorklogHistory,
                with_modifier('d', KeyModifiers::SHIFT),
            ),
            None
        );
        for code in [
            KeyCode::Char('e'),
            KeyCode::Char('o'),
            KeyCode::Char('r'),
            KeyCode::Char('j'),
        ] {
            assert_eq!(
                map_legacy(&mode, TaskView::Active, Screen::WorklogHistory, key(code)),
                None
            );
        }
    }

    #[test]
    fn the_history_maps_movement_paging_refresh_and_back() {
        let history = Screen::WorklogHistory;
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                TaskView::Active,
                history,
                key(KeyCode::Char('j'))
            ),
            Some(Command::MoveDown)
        );
        assert_eq!(
            map_legacy(&Mode::Normal, TaskView::Active, history, key(KeyCode::Down)),
            Some(Command::MoveDown)
        );
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                TaskView::Active,
                history,
                key(KeyCode::Char('k'))
            ),
            Some(Command::MoveUp)
        );
        assert_eq!(
            map_legacy(&Mode::Normal, TaskView::Active, history, key(KeyCode::Up)),
            Some(Command::MoveUp)
        );
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                TaskView::Active,
                history,
                key(KeyCode::Char('e'))
            ),
            Some(Command::OpenCorrection)
        );
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                TaskView::Active,
                history,
                key(KeyCode::Char('d'))
            ),
            Some(Command::OpenDeletion)
        );
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                TaskView::Active,
                history,
                key(KeyCode::Char('o'))
            ),
            Some(Command::LoadOlderWorklogs)
        );
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                TaskView::Active,
                history,
                key(KeyCode::Char('r'))
            ),
            Some(Command::RefreshWorklogs)
        );
        assert_eq!(
            map_legacy(&Mode::Normal, TaskView::Active, history, key(KeyCode::Esc)),
            Some(Command::BackToTaskList)
        );
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                TaskView::Active,
                history,
                key(KeyCode::Char('q'))
            ),
            Some(Command::Quit)
        );
    }

    #[test]
    fn the_history_maps_no_task_list_command() {
        for code in [
            KeyCode::Char(' '),
            KeyCode::Char('a'),
            KeyCode::Char('u'),
            KeyCode::Char('s'),
            KeyCode::Char('h'),
            KeyCode::Char('l'),
            KeyCode::Enter,
            KeyCode::Backspace,
            KeyCode::Char('x'),
        ] {
            assert_eq!(
                map_legacy(
                    &Mode::Normal,
                    TaskView::Active,
                    Screen::WorklogHistory,
                    key(code)
                ),
                None,
                "the history must not map {code:?}"
            );
        }
        for modified in [ctrl('o'), with_modifier('r', KeyModifiers::ALT)] {
            assert_eq!(
                map_legacy(
                    &Mode::Normal,
                    TaskView::Active,
                    Screen::WorklogHistory,
                    modified
                ),
                None,
                "modified {modified:?} must not act"
            );
        }
    }

    #[test]
    fn ctrl_c_quits_from_the_history() {
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                TaskView::Active,
                Screen::WorklogHistory,
                ctrl('c')
            ),
            Some(Command::Quit)
        );
    }

    #[test]
    fn correction_maps_editing_switching_adjustment_and_exit_commands() {
        let mode = correction();
        let screen = Screen::WorklogHistory;
        let cases = [
            (key(KeyCode::Tab), Command::SwitchCorrectionField),
            (key(KeyCode::BackTab), Command::SwitchCorrectionField),
            (key(KeyCode::Left), Command::MoveCursorLeft),
            (key(KeyCode::Right), Command::MoveCursorRight),
            (key(KeyCode::Backspace), Command::Backspace),
            (key(KeyCode::Delete), Command::Delete),
            (key(KeyCode::Char('j')), Command::AdjustForwardFiveMinutes),
            (key(KeyCode::Char('k')), Command::AdjustBackwardFiveMinutes),
            (key(KeyCode::Char('J')), Command::AdjustForwardOneHour),
            (key(KeyCode::Char('K')), Command::AdjustBackwardOneHour),
            (key(KeyCode::Char('2')), Command::Insert('2')),
            (key(KeyCode::Enter), Command::Confirm),
            (key(KeyCode::Esc), Command::Cancel),
            (ctrl('c'), Command::Quit),
        ];
        for (key, expected) in cases {
            assert_eq!(
                map_legacy(&mode, TaskView::Active, screen, key),
                Some(expected),
                "wrong command for {key:?}"
            );
        }
        assert_eq!(
            map_legacy(
                &mode,
                TaskView::Active,
                screen,
                KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT)
            ),
            Some(Command::AdjustForwardOneHour)
        );
        assert_eq!(
            map_legacy(
                &mode,
                TaskView::Active,
                screen,
                KeyEvent::new(KeyCode::Char('K'), KeyModifiers::SHIFT)
            ),
            Some(Command::AdjustBackwardOneHour)
        );
        for key in [
            ctrl('j'),
            ctrl('J'),
            ctrl('K'),
            with_modifier('J', KeyModifiers::ALT),
            with_modifier('K', KeyModifiers::ALT),
            ctrl('2'),
            key(KeyCode::Char('x')),
        ] {
            assert_eq!(
                map_legacy(&mode, TaskView::Active, screen, key),
                None,
                "modified or invalid correction key must not act: {key:?}"
            );
        }
        assert_eq!(
            map_legacy(&mode, TaskView::Active, screen, key(KeyCode::Up)),
            None
        );
    }

    #[test]
    fn correction_footers_fit_and_advertise_every_accepted_key() {
        for width in [60, 80] {
            let footer = footer_hints_legacy(
                &correction(),
                TaskView::Active,
                Screen::WorklogHistory,
                true,
                width,
            );
            assert!(footer.chars().count() <= width as usize, "{footer:?}");
            for hint in [
                "type",
                "←/→",
                "bs/del",
                "tab/S-tab",
                "j/k",
                "±5m",
                "J/K",
                "±1h",
                "enter",
                "esc",
                "ctrl+c",
            ] {
                assert!(footer.contains(hint), "correction footer misses {hint:?}");
            }
        }
    }

    #[test]
    fn unavailable_history_footers_advertise_only_working_commands() {
        for (width, expected) in [
            (60, "r retry esc back q/ctrl+c quit"),
            (80, "r retry · esc back · q/ctrl+c quit"),
        ] {
            let footer = footer_hints_legacy(
                &Mode::Normal,
                TaskView::Active,
                Screen::WorklogHistory,
                false,
                width,
            );
            assert_eq!(footer, expected);
            assert!(footer.chars().count() <= width as usize, "{footer:?}");
            for unavailable in ["j/k", "e edit", "o older"] {
                assert!(!footer.contains(unavailable), "{footer:?}");
            }
        }
    }

    #[test]
    fn deletion_footers_advertise_delete_and_confirmation_keys_at_both_widths() {
        for (width, delete_hint, confirm_hint) in [
            (60, "d delete", "d/y/enter delete"),
            (80, "d delete", "d/y/enter delete"),
        ] {
            let history = footer_hints_legacy(
                &Mode::Normal,
                TaskView::Active,
                Screen::WorklogHistory,
                true,
                width,
            );
            assert!(history.contains(delete_hint), "{history:?}");
            let confirmation = footer_hints_legacy(
                &deletion(),
                TaskView::Active,
                Screen::WorklogHistory,
                true,
                width,
            );
            assert!(history.chars().count() <= width as usize, "{history:?}");
            assert!(confirmation.contains(confirm_hint), "{confirmation:?}");
            assert!(
                confirmation.chars().count() <= width as usize,
                "{confirmation:?}"
            );
            assert!(confirmation.contains("ctrl+c"), "{confirmation:?}");
        }
    }

    #[test]
    fn deletion_footer_switches_to_the_full_variant_at_eighty_columns() {
        let compact = footer_hints_legacy(
            &Mode::Normal,
            TaskView::Active,
            Screen::WorklogHistory,
            true,
            79,
        );
        let full = footer_hints_legacy(
            &Mode::Normal,
            TaskView::Active,
            Screen::WorklogHistory,
            true,
            80,
        );
        assert_eq!(
            compact,
            "j/k/↑/↓ e edit d delete o older r refresh esc back q ctrl+c"
        );
        assert_eq!(
            full,
            "j/k/↑/↓ move e edit d delete o older r refresh esc back q/ctrl+c quit"
        );
        assert_eq!(
            footer_hints_legacy(
                &deletion(),
                TaskView::Active,
                Screen::WorklogHistory,
                true,
                79,
            ),
            "d/y/enter delete n/esc cancel ctrl+c quit"
        );
        assert_eq!(
            footer_hints_legacy(
                &deletion(),
                TaskView::Active,
                Screen::WorklogHistory,
                true,
                80,
            ),
            "d/y/enter delete · n/esc cancel · ctrl+c quit"
        );
    }

    #[test]
    fn correction_footer_switches_to_the_full_variant_at_eighty_columns() {
        assert_eq!(
            footer_hints_legacy(
                &correction(),
                TaskView::Active,
                Screen::WorklogHistory,
                true,
                79
            ),
            "type ←/→ bs/del tab/S-tab j/k ±5m J/K ±1h enter esc ctrl+c"
        );
        assert_eq!(
            footer_hints_legacy(
                &correction(),
                TaskView::Active,
                Screen::WorklogHistory,
                true,
                80
            ),
            "type · ←/→ · bs/del · tab/S-tab · j/k ±5m · J/K ±1h · enter · esc · ctrl+c"
        );
    }

    #[test]
    fn unavailable_history_maps_only_retry_back_and_quit() {
        for (code, expected) in [
            (KeyCode::Char('r'), Command::RefreshWorklogs),
            (KeyCode::Esc, Command::BackToTaskList),
            (KeyCode::Char('q'), Command::Quit),
        ] {
            assert_eq!(
                map_unavailable(TaskView::Active, KeyEvent::new(code, KeyModifiers::NONE)),
                Some(expected)
            );
        }
        assert_eq!(
            map_unavailable(
                TaskView::Active,
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)
            ),
            None
        );
    }
}
