//! Key mappings and footer help for the task-list screen.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::command::Command;

use super::{TaskListMode, TaskListState, TaskView};

pub(crate) fn map(state: &TaskListState, key: KeyEvent) -> Option<Command> {
    match state.mode() {
        TaskListMode::Normal => map_normal(state.view(), key),
        TaskListMode::Input { .. } => map_input(key),
        TaskListMode::ConfirmArchive { .. } => map_confirm(key),
    }
}

fn map_normal(view: TaskView, key: KeyEvent) -> Option<Command> {
    if key.modifiers != KeyModifiers::NONE {
        return None;
    }
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => Some(Command::MoveDown),
        KeyCode::Char('k') | KeyCode::Up => Some(Command::MoveUp),
        KeyCode::Char('h') => Some(Command::ShowActiveTasks),
        KeyCode::Char('l') => Some(Command::ShowArchivedTasks),
        KeyCode::Char('s') => Some(Command::CycleOrdering),
        KeyCode::Char(' ') if view == TaskView::Active => Some(Command::ToggleTracking),
        KeyCode::Char('a') if view == TaskView::Active => Some(Command::OpenAdd),
        KeyCode::Char('e') if view == TaskView::Active => Some(Command::OpenRename),
        KeyCode::Char('d') if view == TaskView::Active => Some(Command::OpenArchiveConfirm),
        KeyCode::Char('u') if view == TaskView::Archived => Some(Command::UnarchiveSelected),
        KeyCode::Enter => Some(Command::OpenHistory),
        KeyCode::Char('q') | KeyCode::Esc => Some(Command::Quit),
        _ => None,
    }
}

fn map_input(key: KeyEvent) -> Option<Command> {
    match key.code {
        KeyCode::Enter => Some(Command::Confirm),
        KeyCode::Esc => Some(Command::Cancel),
        KeyCode::Backspace => Some(Command::Backspace),
        KeyCode::Char(character) if key.modifiers - KeyModifiers::SHIFT == KeyModifiers::NONE => {
            Some(Command::Insert(character))
        }
        _ => None,
    }
}

fn map_confirm(key: KeyEvent) -> Option<Command> {
    if key.modifiers != KeyModifiers::NONE {
        return None;
    }
    match key.code {
        KeyCode::Enter | KeyCode::Char('y') => Some(Command::Confirm),
        KeyCode::Esc | KeyCode::Char('n') => Some(Command::Cancel),
        _ => None,
    }
}

pub(crate) fn footer_hints(state: &TaskListState, width: u16) -> &'static str {
    if width < 80 {
        return match state.mode() {
            TaskListMode::Normal => match state.view() {
                TaskView::Active => "j/k/↑/↓ h/l spc enter history a/e/d s sort q/esc/ctrl+c quit",
                TaskView::Archived => {
                    "j/k/↑/↓ h/l enter history s sort u restore q/esc/ctrl+c quit"
                }
            },
            TaskListMode::Input { .. } => {
                "type · backspace · enter save · esc cancel · ctrl+c quit"
            }
            TaskListMode::ConfirmArchive { .. } => "y/enter · n/esc · ctrl+c quit",
        };
    }
    match state.mode() {
        TaskListMode::Normal => match state.view() {
            TaskView::Active => {
                "j/k/↑/↓ h/l view space track enter history s sort a/e/d edit q/esc/ctrl+c quit"
            }
            TaskView::Archived => {
                "j/k/↑/↓ move h/l view enter history s sort u unarchive q/esc/ctrl+c quit"
            }
        },
        TaskListMode::Input { .. } => {
            "type · backspace delete · enter save · esc cancel · ctrl+c quit"
        }
        TaskListMode::ConfirmArchive { .. } => "y/enter confirm · n/esc cancel · ctrl+c quit",
    }
}

#[cfg(test)]
mod restored_keymap_tests {
    use crate::command::Command;
    use crate::screens::keymap_test_support::*;
    use crate::screens::{Screen, TaskView};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn active_mode_maps_movement_actions_and_view_switching() {
        let view = TaskView::Active;
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('j'))
            ),
            Some(Command::MoveDown)
        );
        assert_eq!(
            map_legacy(&Mode::Normal, view, Screen::TaskList, key(KeyCode::Down)),
            Some(Command::MoveDown)
        );
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('k'))
            ),
            Some(Command::MoveUp)
        );
        assert_eq!(
            map_legacy(&Mode::Normal, view, Screen::TaskList, key(KeyCode::Up)),
            Some(Command::MoveUp)
        );
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('h'))
            ),
            Some(Command::ShowActiveTasks)
        );
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('l'))
            ),
            Some(Command::ShowArchivedTasks)
        );
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('s'))
            ),
            Some(Command::CycleOrdering)
        );
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char(' '))
            ),
            Some(Command::ToggleTracking)
        );
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('a'))
            ),
            Some(Command::OpenAdd)
        );
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('e'))
            ),
            Some(Command::OpenRename)
        );
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('d'))
            ),
            Some(Command::OpenArchiveConfirm)
        );
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('q'))
            ),
            Some(Command::Quit)
        );
        assert_eq!(
            map_legacy(&Mode::Normal, view, Screen::TaskList, key(KeyCode::Esc)),
            Some(Command::Quit)
        );
    }

    #[test]
    fn archived_mode_maps_movement_unarchive_and_view_switching() {
        let view = TaskView::Archived;
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('j'))
            ),
            Some(Command::MoveDown)
        );
        assert_eq!(
            map_legacy(&Mode::Normal, view, Screen::TaskList, key(KeyCode::Down)),
            Some(Command::MoveDown)
        );
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('k'))
            ),
            Some(Command::MoveUp)
        );
        assert_eq!(
            map_legacy(&Mode::Normal, view, Screen::TaskList, key(KeyCode::Up)),
            Some(Command::MoveUp)
        );
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('h'))
            ),
            Some(Command::ShowActiveTasks)
        );
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('l'))
            ),
            Some(Command::ShowArchivedTasks)
        );
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('s'))
            ),
            Some(Command::CycleOrdering)
        );
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('u'))
            ),
            Some(Command::UnarchiveSelected)
        );
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('q'))
            ),
            Some(Command::Quit)
        );
        assert_eq!(
            map_legacy(&Mode::Normal, view, Screen::TaskList, key(KeyCode::Esc)),
            Some(Command::Quit)
        );
    }

    #[test]
    fn archived_mode_never_maps_active_view_actions() {
        let view = TaskView::Archived;
        for code in [
            KeyCode::Char(' '),
            KeyCode::Char('a'),
            KeyCode::Char('e'),
            KeyCode::Char('d'),
            KeyCode::Char('x'),
            KeyCode::Backspace,
        ] {
            assert_eq!(
                map_legacy(&Mode::Normal, view, Screen::TaskList, key(code)),
                None,
                "archived mode must not map {code:?}"
            );
        }
        // Enter is the one key both task views share. It opens history
        // instead of acting on the task.
        assert_eq!(
            map_legacy(&Mode::Normal, view, Screen::TaskList, key(KeyCode::Enter)),
            Some(Command::OpenHistory)
        );
    }

    #[test]
    fn normal_mode_ignores_unmapped_keys() {
        for view in [TaskView::Active, TaskView::Archived] {
            assert_eq!(
                map_legacy(
                    &Mode::Normal,
                    view,
                    Screen::TaskList,
                    key(KeyCode::Char('x'))
                ),
                None
            );
            assert_eq!(
                map_legacy(&Mode::Normal, view, Screen::TaskList, key(KeyCode::Left)),
                None
            );
        }
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                TaskView::Archived,
                Screen::TaskList,
                key(KeyCode::Backspace)
            ),
            None
        );
        assert_eq!(
            map_legacy(
                &Mode::Normal,
                TaskView::Active,
                Screen::TaskList,
                key(KeyCode::Char('u'))
            ),
            None
        );
    }

    #[test]
    fn sort_is_normal_mode_only() {
        for view in [TaskView::Active, TaskView::Archived] {
            assert_eq!(
                map_legacy(
                    &Mode::Normal,
                    view,
                    Screen::TaskList,
                    key(KeyCode::Char('s'))
                ),
                Some(Command::CycleOrdering)
            );
        }
        assert_eq!(
            map_legacy(
                &input(),
                TaskView::Active,
                Screen::TaskList,
                key(KeyCode::Char('s'))
            ),
            Some(Command::Insert('s'))
        );
        assert_eq!(
            map_legacy(
                &confirm(),
                TaskView::Active,
                Screen::TaskList,
                key(KeyCode::Char('s'))
            ),
            None
        );
    }

    #[test]
    fn modified_action_keys_do_nothing_in_normal_mode() {
        for (view, _) in normal_modes() {
            for key in [
                ctrl('a'),
                ctrl('d'),
                ctrl('u'),
                ctrl('y'),
                ctrl('j'),
                ctrl('q'),
                with_modifier('j', KeyModifiers::ALT),
                with_modifier('j', KeyModifiers::SUPER),
                with_modifier('j', KeyModifiers::META),
                with_modifier('j', KeyModifiers::HYPER),
                with_modifier('j', KeyModifiers::SHIFT),
                with_modifier('d', KeyModifiers::CONTROL | KeyModifiers::ALT),
            ] {
                assert_eq!(
                    map_legacy(&Mode::Normal, view, Screen::TaskList, key),
                    None,
                    "modified {key:?} must not act"
                );
            }
        }
    }

    #[test]
    fn modified_keys_do_nothing_in_confirmation_mode() {
        for key in [
            ctrl('y'),
            ctrl('n'),
            with_modifier('y', KeyModifiers::ALT),
            with_modifier('y', KeyModifiers::SUPER),
            with_modifier('y', KeyModifiers::META),
            with_modifier('n', KeyModifiers::SHIFT),
        ] {
            assert_eq!(
                map_legacy(&confirm(), TaskView::Active, Screen::TaskList, key),
                None,
                "modified {key:?} must not act"
            );
        }
    }

    #[test]
    fn input_mode_edits_confirms_and_cancels() {
        let view = TaskView::Active;
        assert_eq!(
            map_legacy(&input(), view, Screen::TaskList, key(KeyCode::Char('x'))),
            Some(Command::Insert('x'))
        );
        // Space is ordinary input while typing.
        assert_eq!(
            map_legacy(&input(), view, Screen::TaskList, key(KeyCode::Char(' '))),
            Some(Command::Insert(' '))
        );
        assert_eq!(
            map_legacy(&input(), view, Screen::TaskList, key(KeyCode::Backspace)),
            Some(Command::Backspace)
        );
        assert_eq!(
            map_legacy(&input(), view, Screen::TaskList, key(KeyCode::Enter)),
            Some(Command::Confirm)
        );
        assert_eq!(
            map_legacy(&input(), view, Screen::TaskList, key(KeyCode::Esc)),
            Some(Command::Cancel)
        );
    }

    #[test]
    fn input_mode_ignores_modifier_chords_and_other_keys() {
        assert_eq!(
            map_legacy(&input(), TaskView::Active, Screen::TaskList, ctrl('a')),
            None
        );
        let alt = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT);
        assert_eq!(
            map_legacy(&input(), TaskView::Active, Screen::TaskList, alt),
            None
        );
        let hyper = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::HYPER);
        assert_eq!(
            map_legacy(&input(), TaskView::Active, Screen::TaskList, hyper),
            None
        );
        assert_eq!(
            map_legacy(
                &input(),
                TaskView::Active,
                Screen::TaskList,
                key(KeyCode::Up)
            ),
            None
        );
        assert_eq!(
            map_legacy(
                &input(),
                TaskView::Active,
                Screen::TaskList,
                key(KeyCode::Tab)
            ),
            None
        );
    }

    #[test]
    fn shift_is_not_a_chord_so_capitals_are_typed() {
        let capital = KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT);
        assert_eq!(
            map_legacy(&input(), TaskView::Active, Screen::TaskList, capital),
            Some(Command::Insert('A'))
        );
    }

    #[test]
    fn confirm_mode_accepts_and_cancels() {
        let view = TaskView::Active;
        assert_eq!(
            map_legacy(&confirm(), view, Screen::TaskList, key(KeyCode::Enter)),
            Some(Command::Confirm)
        );
        assert_eq!(
            map_legacy(&confirm(), view, Screen::TaskList, key(KeyCode::Char('y'))),
            Some(Command::Confirm)
        );
        assert_eq!(
            map_legacy(&confirm(), view, Screen::TaskList, key(KeyCode::Char('n'))),
            Some(Command::Cancel)
        );
        assert_eq!(
            map_legacy(&confirm(), view, Screen::TaskList, key(KeyCode::Esc)),
            Some(Command::Cancel)
        );
        assert_eq!(
            map_legacy(&confirm(), view, Screen::TaskList, key(KeyCode::Char('x'))),
            None
        );
    }

    #[test]
    fn enter_opens_the_history_in_both_task_views() {
        for view in [TaskView::Active, TaskView::Archived] {
            assert_eq!(
                map_legacy(&Mode::Normal, view, Screen::TaskList, key(KeyCode::Enter)),
                Some(Command::OpenHistory)
            );
        }
        // The modal modes keep their Enter meaning.
        assert_eq!(
            map_legacy(
                &input(),
                TaskView::Active,
                Screen::TaskList,
                key(KeyCode::Enter)
            ),
            Some(Command::Confirm)
        );
        assert_eq!(
            map_legacy(
                &confirm(),
                TaskView::Active,
                Screen::TaskList,
                key(KeyCode::Enter)
            ),
            Some(Command::Confirm)
        );
    }

    #[test]
    fn every_accepted_key_is_listed_in_the_footer() {
        let active_keys =
            footer_hints_legacy(&Mode::Normal, TaskView::Active, Screen::TaskList, true, 80);
        for hint in [
            "j/k/↑/↓",
            "h/l",
            "space",
            "enter history",
            "s sort",
            "a/e/d",
            "edit",
            "q/esc",
            "ctrl+c",
        ] {
            assert!(active_keys.contains(hint), "active footer misses {hint:?}");
        }
        assert!(active_keys.contains("s sort"));
        let archived_keys = footer_hints_legacy(
            &Mode::Normal,
            TaskView::Archived,
            Screen::TaskList,
            true,
            80,
        );
        for hint in [
            "j/k/↑/↓",
            "h/l",
            "enter history",
            "s sort",
            "u ",
            "q/esc",
            "ctrl+c",
        ] {
            assert!(
                archived_keys.contains(hint),
                "archived footer misses {hint:?}"
            );
        }
        let input_keys =
            footer_hints_legacy(&input(), TaskView::Active, Screen::TaskList, true, 80);
        for hint in ["type", "backspace", "enter", "esc", "ctrl+c"] {
            assert!(input_keys.contains(hint), "input footer misses {hint:?}");
        }
        let confirm_keys =
            footer_hints_legacy(&confirm(), TaskView::Active, Screen::TaskList, true, 80);
        for hint in ["y/enter", "n/esc", "ctrl+c"] {
            assert!(
                confirm_keys.contains(hint),
                "confirm footer misses {hint:?}"
            );
        }
        let history_keys = footer_hints_legacy(
            &Mode::Normal,
            TaskView::Active,
            Screen::WorklogHistory,
            true,
            80,
        );
        for hint in [
            "j/k/↑/↓ move",
            "e edit",
            "o older",
            "r refresh",
            "esc back",
            "q",
            "ctrl+c",
        ] {
            assert!(
                history_keys.contains(hint),
                "history footer misses {hint:?}"
            );
        }
        // Every footer fits a standard 80-column terminal.
        for footer in [
            active_keys,
            archived_keys,
            input_keys,
            confirm_keys,
            history_keys,
        ] {
            assert!(footer.chars().count() <= 80, "footer too wide: {footer:?}");
        }
    }

    #[test]
    fn normal_task_list_footers_explain_how_to_open_worklogs() {
        for width in [60, 80] {
            for view in [TaskView::Active, TaskView::Archived] {
                let footer =
                    footer_hints_legacy(&Mode::Normal, view, Screen::TaskList, true, width);
                assert!(
                    footer.contains("enter history"),
                    "task-list footer misses the history hint at width {width}: {footer:?}"
                );
            }
        }
    }

    #[test]
    fn compact_footers_keep_every_key_visible_at_sixty_columns() {
        let footers = [
            footer_hints_legacy(&Mode::Normal, TaskView::Active, Screen::TaskList, true, 60),
            footer_hints_legacy(
                &Mode::Normal,
                TaskView::Archived,
                Screen::TaskList,
                true,
                60,
            ),
            footer_hints_legacy(&input(), TaskView::Active, Screen::TaskList, true, 60),
            footer_hints_legacy(&confirm(), TaskView::Active, Screen::TaskList, true, 60),
            footer_hints_legacy(
                &Mode::Normal,
                TaskView::Active,
                Screen::WorklogHistory,
                true,
                60,
            ),
        ];
        for footer in footers {
            assert!(footer.chars().count() <= 60, "footer too wide: {footer:?}");
            assert!(
                footer.contains("ctrl+c"),
                "footer misses ctrl+c: {footer:?}"
            );
        }
        assert!(footers[0].contains("j/k/↑/↓"));
        assert!(footers[0].contains("h/l"));
        assert!(footers[0].contains("spc"));
        assert!(footers[0].contains("a/e/d"));
        assert!(footers[0].contains("enter history"));
        assert!(footers[0].contains("s sort"));
        assert!(footers[1].contains("enter history"));
        assert!(footers[1].contains("u restore"));
        assert!(footers[4].contains("o older"));
        assert!(footers[4].contains("r refresh"));
        assert!(footers[4].contains("esc back"));
    }
}
