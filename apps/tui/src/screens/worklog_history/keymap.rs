//! Key mappings and footer help for worklog history.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::screens::KeymapCommand;
use crate::support::timestamps::is_timestamp_character;

use super::{MoveFocus, WorklogHistoryCommand, WorklogHistoryMode, WorklogHistoryState};

pub(crate) fn map(
    state: &WorklogHistoryState,
    key: KeyEvent,
) -> Option<KeymapCommand<WorklogHistoryCommand>> {
    match state.mode() {
        WorklogHistoryMode::Normal => map_normal(state, key),
        WorklogHistoryMode::ConfirmDeletion { .. } => map_confirm_deletion(key),
        WorklogHistoryMode::Correction(_) => map_correction(key),
        WorklogHistoryMode::Move(draft) => map_move(draft.focus(), key),
    }
}

fn map_normal(
    state: &WorklogHistoryState,
    key: KeyEvent,
) -> Option<KeymapCommand<WorklogHistoryCommand>> {
    if state.history().is_available() && key.modifiers == KeyModifiers::CONTROL {
        return match key.code {
            KeyCode::Char('d') => Some(KeymapCommand::Local(WorklogHistoryCommand::PageDown)),
            KeyCode::Char('u') => Some(KeymapCommand::Local(WorklogHistoryCommand::PageUp)),
            _ => None,
        };
    }
    if state.history().is_available()
        && key.code == KeyCode::Char('G')
        && key.modifiers == KeyModifiers::SHIFT
    {
        return Some(KeymapCommand::Local(WorklogHistoryCommand::Last));
    }
    if !state.history().is_available() || key.modifiers != KeyModifiers::NONE {
        if !state.history().is_available() {
            return match (key.code, key.modifiers) {
                (KeyCode::Char('r'), KeyModifiers::NONE) => {
                    Some(KeymapCommand::Local(WorklogHistoryCommand::RefreshWorklogs))
                }
                (KeyCode::Esc, KeyModifiers::NONE) => {
                    Some(KeymapCommand::Local(WorklogHistoryCommand::BackToTaskList))
                }
                (KeyCode::Char('q'), KeyModifiers::NONE) => Some(KeymapCommand::Quit),
                _ => None,
            };
        }
        return None;
    }
    let command = match key.code {
        KeyCode::Char('j') | KeyCode::Down => WorklogHistoryCommand::MoveDown,
        KeyCode::Char('k') | KeyCode::Up => WorklogHistoryCommand::MoveUp,
        KeyCode::Char('g') => {
            if state.g_prefix() {
                WorklogHistoryCommand::First
            } else {
                WorklogHistoryCommand::GPrefix
            }
        }
        KeyCode::Char('G') => WorklogHistoryCommand::Last,
        KeyCode::Char('e') => WorklogHistoryCommand::OpenCorrection,
        KeyCode::Char('d') => WorklogHistoryCommand::OpenDeletion,
        KeyCode::Char('m') => WorklogHistoryCommand::OpenMove,
        KeyCode::Char('o') => WorklogHistoryCommand::LoadOlderWorklogs,
        KeyCode::Char('r') => WorklogHistoryCommand::RefreshWorklogs,
        KeyCode::Esc => WorklogHistoryCommand::BackToTaskList,
        KeyCode::Char('q') => return Some(KeymapCommand::Quit),
        _ => return None,
    };
    Some(KeymapCommand::Local(command))
}

pub(crate) fn map_move(
    focus: MoveFocus,
    key: KeyEvent,
) -> Option<KeymapCommand<WorklogHistoryCommand>> {
    let command = match (key.code, key.modifiers) {
        (KeyCode::Enter, KeyModifiers::NONE) => WorklogHistoryCommand::Confirm,
        (KeyCode::Esc, KeyModifiers::NONE) => WorklogHistoryCommand::Cancel,
        (KeyCode::Tab, KeyModifiers::NONE | KeyModifiers::SHIFT)
        | (KeyCode::BackTab, KeyModifiers::NONE)
        | (KeyCode::BackTab, KeyModifiers::SHIFT) => WorklogHistoryCommand::ToggleMoveFocus,
        (KeyCode::Up, KeyModifiers::NONE) => WorklogHistoryCommand::MoveDestinationUp,
        (KeyCode::Down, KeyModifiers::NONE) => WorklogHistoryCommand::MoveDestinationDown,
        (KeyCode::Char('j'), KeyModifiers::NONE) if focus == MoveFocus::Results => {
            WorklogHistoryCommand::MoveDestinationDown
        }
        (KeyCode::Char('k'), KeyModifiers::NONE) if focus == MoveFocus::Results => {
            WorklogHistoryCommand::MoveDestinationUp
        }
        (KeyCode::Backspace, KeyModifiers::NONE) if focus == MoveFocus::Search => {
            WorklogHistoryCommand::BackspaceMoveQuery
        }
        (KeyCode::Char(character), modifiers)
            if focus == MoveFocus::Search
                && modifiers - KeyModifiers::SHIFT == KeyModifiers::NONE
                && !character.is_control() =>
        {
            WorklogHistoryCommand::InsertMoveQuery(character)
        }
        _ => return None,
    };
    Some(KeymapCommand::Local(command))
}

fn map_confirm_deletion(key: KeyEvent) -> Option<KeymapCommand<WorklogHistoryCommand>> {
    if key.modifiers != KeyModifiers::NONE {
        return None;
    }
    let command = match key.code {
        KeyCode::Enter | KeyCode::Char('d') | KeyCode::Char('y') => WorklogHistoryCommand::Confirm,
        KeyCode::Esc | KeyCode::Char('n') => WorklogHistoryCommand::Cancel,
        _ => return None,
    };
    Some(KeymapCommand::Local(command))
}

fn map_correction(key: KeyEvent) -> Option<KeymapCommand<WorklogHistoryCommand>> {
    let command = match (key.code, key.modifiers) {
        (KeyCode::Enter, KeyModifiers::NONE) => WorklogHistoryCommand::Confirm,
        (KeyCode::Esc, KeyModifiers::NONE) => WorklogHistoryCommand::Cancel,
        (KeyCode::Tab, KeyModifiers::NONE)
        | (KeyCode::BackTab, KeyModifiers::SHIFT)
        | (KeyCode::BackTab, KeyModifiers::NONE) => WorklogHistoryCommand::SwitchCorrectionField,
        (KeyCode::Left, KeyModifiers::NONE) => WorklogHistoryCommand::MoveCursorLeft,
        (KeyCode::Right, KeyModifiers::NONE) => WorklogHistoryCommand::MoveCursorRight,
        (KeyCode::Backspace, KeyModifiers::NONE) => WorklogHistoryCommand::Backspace,
        (KeyCode::Delete, KeyModifiers::NONE) => WorklogHistoryCommand::Delete,
        (KeyCode::Char('j'), KeyModifiers::NONE) => WorklogHistoryCommand::AdjustForwardFiveMinutes,
        (KeyCode::Char('k'), KeyModifiers::NONE) => {
            WorklogHistoryCommand::AdjustBackwardFiveMinutes
        }
        (KeyCode::Char('J'), modifiers)
            if modifiers == KeyModifiers::NONE || modifiers == KeyModifiers::SHIFT =>
        {
            WorklogHistoryCommand::AdjustForwardOneHour
        }
        (KeyCode::Char('K'), modifiers)
            if modifiers == KeyModifiers::NONE || modifiers == KeyModifiers::SHIFT =>
        {
            WorklogHistoryCommand::AdjustBackwardOneHour
        }
        (KeyCode::Char(character), modifiers)
            if modifiers - KeyModifiers::SHIFT == KeyModifiers::NONE
                && is_timestamp_character(character) =>
        {
            WorklogHistoryCommand::Insert(character)
        }
        _ => return None,
    };
    Some(KeymapCommand::Local(command))
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
        WorklogHistoryMode::Move(draft) => move_footer_hints(draft.focus(), width),
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
                "j/k/↑/↓ e m move d del o older r refresh esc back q ctrl+c"
            } else {
                "j/k/↑/↓ move e edit m move d delete o older r refresh esc back q/ctrl+c quit"
            }
        }
    }
}

pub(crate) fn move_footer_hints(focus: MoveFocus, width: u16) -> &'static str {
    match (focus, width < 80) {
        (MoveFocus::Search, true) => "type/bs tab/S-tab ↑/↓ choose enter move esc cancel ctrl+c",
        (MoveFocus::Search, false) => {
            "type/bs · tab/S-tab · ↑/↓ choose · enter move · esc cancel · ctrl+c quit"
        }
        (MoveFocus::Results, true) => "j/k/↑/↓ choose tab/S-tab enter move esc cancel ctrl+c",
        (MoveFocus::Results, false) => {
            "j/k/↑/↓ choose · tab/S-tab · enter move · esc cancel · ctrl+c quit"
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::command::Command;
    use crate::screens::{WorklogHistoryCommand, WorklogHistoryMode};
    use crate::test_support::keymap::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn vim_motions_apply_only_to_normal_available_history() {
        for (key, command) in [
            (key(KeyCode::Char('g')), WorklogHistoryCommand::GPrefix),
            (key(KeyCode::Char('G')), WorklogHistoryCommand::Last),
            (ctrl('d'), WorklogHistoryCommand::PageDown),
            (ctrl('u'), WorklogHistoryCommand::PageUp),
        ] {
            assert_eq!(
                map_history(&WorklogHistoryMode::Normal, key),
                Some(Command::WorklogHistory(command))
            );
            assert_eq!(map_unavailable(key), None);
        }
        assert_eq!(map_history(&correction(), ctrl('d')), None);
    }

    #[test]
    fn deletion_mode_accepts_and_cancels_without_other_commands() {
        let mode = deletion();
        for code in [KeyCode::Enter, KeyCode::Char('d'), KeyCode::Char('y')] {
            assert_eq!(
                map_history(&mode, key(code)),
                Some(Command::WorklogHistory(WorklogHistoryCommand::Confirm))
            );
        }
        for code in [KeyCode::Esc, KeyCode::Char('n')] {
            assert_eq!(
                map_history(&mode, key(code)),
                Some(Command::WorklogHistory(WorklogHistoryCommand::Cancel))
            );
        }
        assert_eq!(
            map_history(&mode, with_modifier('d', KeyModifiers::SHIFT)),
            None
        );
        for code in [
            KeyCode::Char('e'),
            KeyCode::Char('o'),
            KeyCode::Char('r'),
            KeyCode::Char('j'),
        ] {
            assert_eq!(map_history(&mode, key(code)), None);
        }
    }

    #[test]
    fn the_history_maps_movement_paging_refresh_and_back() {
        assert_eq!(
            map_history(&WorklogHistoryMode::Normal, key(KeyCode::Char('j'))),
            Some(Command::WorklogHistory(WorklogHistoryCommand::MoveDown))
        );
        assert_eq!(
            map_history(&WorklogHistoryMode::Normal, key(KeyCode::Down)),
            Some(Command::WorklogHistory(WorklogHistoryCommand::MoveDown))
        );
        assert_eq!(
            map_history(&WorklogHistoryMode::Normal, key(KeyCode::Char('k'))),
            Some(Command::WorklogHistory(WorklogHistoryCommand::MoveUp))
        );
        assert_eq!(
            map_history(&WorklogHistoryMode::Normal, key(KeyCode::Up)),
            Some(Command::WorklogHistory(WorklogHistoryCommand::MoveUp))
        );
        assert_eq!(
            map_history(&WorklogHistoryMode::Normal, key(KeyCode::Char('e'))),
            Some(Command::WorklogHistory(
                WorklogHistoryCommand::OpenCorrection
            ))
        );
        assert_eq!(
            map_history(&WorklogHistoryMode::Normal, key(KeyCode::Char('d'))),
            Some(Command::WorklogHistory(WorklogHistoryCommand::OpenDeletion))
        );
        assert_eq!(
            map_history(&WorklogHistoryMode::Normal, key(KeyCode::Char('m'))),
            Some(Command::WorklogHistory(WorklogHistoryCommand::OpenMove))
        );
        assert_eq!(
            map_history(&WorklogHistoryMode::Normal, key(KeyCode::Char('o'))),
            Some(Command::WorklogHistory(
                WorklogHistoryCommand::LoadOlderWorklogs
            ))
        );
        assert_eq!(
            map_history(&WorklogHistoryMode::Normal, key(KeyCode::Char('r'))),
            Some(Command::WorklogHistory(
                WorklogHistoryCommand::RefreshWorklogs
            ))
        );
        assert_eq!(
            map_history(&WorklogHistoryMode::Normal, key(KeyCode::Esc)),
            Some(Command::WorklogHistory(
                WorklogHistoryCommand::BackToTaskList
            ))
        );
        assert_eq!(
            map_history(&WorklogHistoryMode::Normal, key(KeyCode::Char('q'))),
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
                map_history(&WorklogHistoryMode::Normal, key(code)),
                None,
                "the history must not map {code:?}"
            );
        }
        for modified in [
            ctrl('o'),
            with_modifier('r', KeyModifiers::ALT),
            with_modifier('X', KeyModifiers::SHIFT),
        ] {
            assert_eq!(
                map_history(&WorklogHistoryMode::Normal, modified),
                None,
                "modified {modified:?} must not act"
            );
        }
    }

    #[test]
    fn ctrl_c_quits_from_the_history() {
        assert_eq!(
            map_history(&WorklogHistoryMode::Normal, ctrl('c')),
            Some(Command::Quit)
        );
    }

    #[test]
    fn correction_maps_editing_switching_adjustment_and_exit_commands() {
        let mode = correction();
        let cases = [
            (
                key(KeyCode::Tab),
                Command::WorklogHistory(WorklogHistoryCommand::SwitchCorrectionField),
            ),
            (
                key(KeyCode::BackTab),
                Command::WorklogHistory(WorklogHistoryCommand::SwitchCorrectionField),
            ),
            (
                key(KeyCode::Left),
                Command::WorklogHistory(WorklogHistoryCommand::MoveCursorLeft),
            ),
            (
                key(KeyCode::Right),
                Command::WorklogHistory(WorklogHistoryCommand::MoveCursorRight),
            ),
            (
                key(KeyCode::Backspace),
                Command::WorklogHistory(WorklogHistoryCommand::Backspace),
            ),
            (
                key(KeyCode::Delete),
                Command::WorklogHistory(WorklogHistoryCommand::Delete),
            ),
            (
                key(KeyCode::Char('j')),
                Command::WorklogHistory(WorklogHistoryCommand::AdjustForwardFiveMinutes),
            ),
            (
                key(KeyCode::Char('k')),
                Command::WorklogHistory(WorklogHistoryCommand::AdjustBackwardFiveMinutes),
            ),
            (
                key(KeyCode::Char('J')),
                Command::WorklogHistory(WorklogHistoryCommand::AdjustForwardOneHour),
            ),
            (
                key(KeyCode::Char('K')),
                Command::WorklogHistory(WorklogHistoryCommand::AdjustBackwardOneHour),
            ),
            (
                key(KeyCode::Char('2')),
                Command::WorklogHistory(WorklogHistoryCommand::Insert('2')),
            ),
            (
                key(KeyCode::Enter),
                Command::WorklogHistory(WorklogHistoryCommand::Confirm),
            ),
            (
                key(KeyCode::Esc),
                Command::WorklogHistory(WorklogHistoryCommand::Cancel),
            ),
            (ctrl('c'), Command::Quit),
        ];
        for (key, expected) in cases {
            assert_eq!(
                map_history(&mode, key),
                Some(expected),
                "wrong command for {key:?}"
            );
        }
        assert_eq!(
            map_history(
                &mode,
                KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT)
            ),
            Some(Command::WorklogHistory(
                WorklogHistoryCommand::AdjustForwardOneHour
            ))
        );
        assert_eq!(
            map_history(
                &mode,
                KeyEvent::new(KeyCode::Char('K'), KeyModifiers::SHIFT)
            ),
            Some(Command::WorklogHistory(
                WorklogHistoryCommand::AdjustBackwardOneHour
            ))
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
                map_history(&mode, key),
                None,
                "modified or invalid correction key must not act: {key:?}"
            );
        }
        assert_eq!(map_history(&mode, key(KeyCode::Up)), None);
    }

    #[test]
    fn correction_footers_fit_and_advertise_every_accepted_key() {
        for width in [60, 80] {
            let footer = history_footer(correction(), true, width);
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
    fn move_dialog_maps_search_and_results_keys_without_leaking_history_navigation() {
        let search = move_dialog();
        for (key, expected) in [
            (
                key(KeyCode::Char('j')),
                WorklogHistoryCommand::InsertMoveQuery('j'),
            ),
            (
                key(KeyCode::Char('k')),
                WorklogHistoryCommand::InsertMoveQuery('k'),
            ),
            (
                key(KeyCode::Backspace),
                WorklogHistoryCommand::BackspaceMoveQuery,
            ),
            (key(KeyCode::Up), WorklogHistoryCommand::MoveDestinationUp),
            (
                key(KeyCode::Down),
                WorklogHistoryCommand::MoveDestinationDown,
            ),
            (key(KeyCode::Tab), WorklogHistoryCommand::ToggleMoveFocus),
            (
                KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT),
                WorklogHistoryCommand::ToggleMoveFocus,
            ),
            (
                key(KeyCode::BackTab),
                WorklogHistoryCommand::ToggleMoveFocus,
            ),
            (key(KeyCode::Enter), WorklogHistoryCommand::Confirm),
            (key(KeyCode::Esc), WorklogHistoryCommand::Cancel),
        ] {
            assert_eq!(
                map_history(&search, key),
                Some(Command::WorklogHistory(expected)),
                "wrong search command for {key:?}"
            );
        }

        let mut results = move_dialog();
        let WorklogHistoryMode::Move(draft) = &mut results else {
            unreachable!("move fixture opens the move dialog");
        };
        draft.toggle_focus();
        for (key, expected) in [
            (
                key(KeyCode::Char('j')),
                WorklogHistoryCommand::MoveDestinationDown,
            ),
            (
                key(KeyCode::Char('k')),
                WorklogHistoryCommand::MoveDestinationUp,
            ),
            (key(KeyCode::Up), WorklogHistoryCommand::MoveDestinationUp),
            (
                key(KeyCode::Down),
                WorklogHistoryCommand::MoveDestinationDown,
            ),
        ] {
            assert_eq!(
                map_history(&results, key),
                Some(Command::WorklogHistory(expected)),
                "wrong results command for {key:?}"
            );
        }
        assert_eq!(map_history(&results, key(KeyCode::Backspace)), None);
        assert_eq!(map_history(&results, key(KeyCode::Char('x'))), None);
        assert_eq!(map_history(&results, ctrl('c')), Some(Command::Quit));
    }

    #[test]
    fn move_footer_fits_and_names_only_the_focused_controls() {
        for width in [60, 80] {
            let search_footer = history_footer(move_dialog(), true, width);
            assert!(
                search_footer.chars().count() <= width as usize,
                "{search_footer:?}"
            );
            for hint in ["type", "bs", "tab", "↑/↓", "enter", "esc", "ctrl+c"] {
                assert!(
                    search_footer.contains(hint),
                    "search footer misses {hint:?}: {search_footer:?}"
                );
            }
            assert!(!search_footer.contains("j/k"));

            let mut results = move_dialog();
            let WorklogHistoryMode::Move(draft) = &mut results else {
                unreachable!("move fixture opens the move dialog");
            };
            draft.toggle_focus();
            let results_footer = history_footer(results, true, width);
            assert!(
                results_footer.chars().count() <= width as usize,
                "{results_footer:?}"
            );
            for hint in ["j/k", "tab", "↑/↓", "enter", "esc", "ctrl+c"] {
                assert!(
                    results_footer.contains(hint),
                    "results footer misses {hint:?}: {results_footer:?}"
                );
            }
            assert!(!results_footer.contains("type"));
            assert!(!results_footer.contains("bs"));
        }
        assert_eq!(
            history_footer(move_dialog(), true, 79),
            "type/bs tab/S-tab ↑/↓ choose enter move esc cancel ctrl+c"
        );
        assert_eq!(
            history_footer(move_dialog(), true, 80),
            "type/bs · tab/S-tab · ↑/↓ choose · enter move · esc cancel · ctrl+c quit"
        );
        assert!(history_footer(WorklogHistoryMode::Normal, true, 80).contains("m move"));
    }

    #[test]
    fn unavailable_history_footers_advertise_only_working_commands() {
        for (width, expected) in [
            (60, "r retry esc back q/ctrl+c quit"),
            (80, "r retry · esc back · q/ctrl+c quit"),
        ] {
            let footer = history_footer(WorklogHistoryMode::Normal, false, width);
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
            (60, "d del", "d/y/enter delete"),
            (80, "d delete", "d/y/enter delete"),
        ] {
            let history = history_footer(WorklogHistoryMode::Normal, true, width);
            assert!(history.contains(delete_hint), "{history:?}");
            let confirmation = history_footer(deletion(), true, width);
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
        let compact = history_footer(WorklogHistoryMode::Normal, true, 79);
        let full = history_footer(WorklogHistoryMode::Normal, true, 80);
        assert_eq!(
            compact,
            "j/k/↑/↓ e m move d del o older r refresh esc back q ctrl+c"
        );
        assert_eq!(
            full,
            "j/k/↑/↓ move e edit m move d delete o older r refresh esc back q/ctrl+c quit"
        );
        assert_eq!(
            history_footer(deletion(), true, 79),
            "d/y/enter delete n/esc cancel ctrl+c quit"
        );
        assert_eq!(
            history_footer(deletion(), true, 80),
            "d/y/enter delete · n/esc cancel · ctrl+c quit"
        );
    }

    #[test]
    fn correction_footer_switches_to_the_full_variant_at_eighty_columns() {
        assert_eq!(
            history_footer(correction(), true, 79),
            "type ←/→ bs/del tab/S-tab j/k ±5m J/K ±1h enter esc ctrl+c"
        );
        assert_eq!(
            history_footer(correction(), true, 80),
            "type · ←/→ · bs/del · tab/S-tab · j/k ±5m · J/K ±1h · enter · esc · ctrl+c"
        );
    }

    #[test]
    fn unavailable_history_maps_only_retry_back_and_quit() {
        for (code, expected) in [
            (
                KeyCode::Char('r'),
                Command::WorklogHistory(WorklogHistoryCommand::RefreshWorklogs),
            ),
            (
                KeyCode::Esc,
                Command::WorklogHistory(WorklogHistoryCommand::BackToTaskList),
            ),
            (KeyCode::Char('q'), Command::Quit),
        ] {
            assert_eq!(
                map_unavailable(KeyEvent::new(code, KeyModifiers::NONE)),
                Some(expected)
            );
        }
        assert_eq!(
            map_unavailable(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)),
            None
        );
    }
}
