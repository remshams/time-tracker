//! Key mappings and footer help for the task-list screen.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::screens::KeymapCommand;

use super::{TaskListCommand, TaskListMode, TaskListState, TaskView};

pub(crate) fn map(state: &TaskListState, key: KeyEvent) -> Option<KeymapCommand<TaskListCommand>> {
    match state.mode() {
        TaskListMode::Normal => map_normal(state, key),
        TaskListMode::Search => map_search(key),
        TaskListMode::Input { .. } => map_input(key),
        TaskListMode::ConfirmArchive { .. } => map_confirm(key),
    }
}

fn map_normal(state: &TaskListState, key: KeyEvent) -> Option<KeymapCommand<TaskListCommand>> {
    let view = state.view();
    if key.modifiers != KeyModifiers::NONE {
        return None;
    }
    let command = match key.code {
        KeyCode::Char('j') | KeyCode::Down => TaskListCommand::MoveDown,
        KeyCode::Char('k') | KeyCode::Up => TaskListCommand::MoveUp,
        KeyCode::Char('h') => TaskListCommand::ShowActiveTasks,
        KeyCode::Char('l') => TaskListCommand::ShowArchivedTasks,
        KeyCode::Char('s') => TaskListCommand::CycleOrdering,
        KeyCode::Char('/') => TaskListCommand::OpenSearch,
        KeyCode::Char(' ') if view == TaskView::Active => TaskListCommand::ToggleTracking,
        KeyCode::Char('a') if view == TaskView::Active => TaskListCommand::OpenAdd,
        KeyCode::Char('e') if view == TaskView::Active => TaskListCommand::OpenRename,
        KeyCode::Char('d') if view == TaskView::Active => TaskListCommand::OpenArchiveConfirm,
        KeyCode::Char('u') if view == TaskView::Archived => TaskListCommand::UnarchiveSelected,
        KeyCode::Enter => TaskListCommand::OpenHistory,
        KeyCode::Esc if state.search_query().is_some() => TaskListCommand::ClearSearch,
        KeyCode::Char('q') | KeyCode::Esc => return Some(KeymapCommand::Quit),
        _ => return None,
    };
    Some(KeymapCommand::Local(command))
}

fn map_search(key: KeyEvent) -> Option<KeymapCommand<TaskListCommand>> {
    if key.modifiers != KeyModifiers::NONE
        && !(matches!(key.code, KeyCode::Char(_)) && key.modifiers == KeyModifiers::SHIFT)
    {
        return None;
    }
    let command = match key.code {
        KeyCode::Enter => TaskListCommand::CommitSearch,
        KeyCode::Esc => TaskListCommand::CancelSearch,
        KeyCode::Backspace => TaskListCommand::BackspaceSearch,
        KeyCode::Up => TaskListCommand::MoveUp,
        KeyCode::Down => TaskListCommand::MoveDown,
        KeyCode::Char(character) => TaskListCommand::InsertSearch(character),
        _ => return None,
    };
    Some(KeymapCommand::Local(command))
}

fn map_input(key: KeyEvent) -> Option<KeymapCommand<TaskListCommand>> {
    let command = match key.code {
        KeyCode::Enter => TaskListCommand::Confirm,
        KeyCode::Esc => TaskListCommand::Cancel,
        KeyCode::Backspace => TaskListCommand::Backspace,
        KeyCode::Char(character) if key.modifiers - KeyModifiers::SHIFT == KeyModifiers::NONE => {
            TaskListCommand::Insert(character)
        }
        _ => return None,
    };
    Some(KeymapCommand::Local(command))
}

fn map_confirm(key: KeyEvent) -> Option<KeymapCommand<TaskListCommand>> {
    if key.modifiers != KeyModifiers::NONE {
        return None;
    }
    let command = match key.code {
        KeyCode::Enter | KeyCode::Char('y') => TaskListCommand::Confirm,
        KeyCode::Esc | KeyCode::Char('n') => TaskListCommand::Cancel,
        _ => return None,
    };
    Some(KeymapCommand::Local(command))
}

pub(crate) fn footer_hints(state: &TaskListState, width: u16) -> &'static str {
    let filtered = state.search_query().is_some();
    if width < 80 {
        return match state.mode() {
            TaskListMode::Normal => match (state.view(), filtered) {
                (TaskView::Active, false) => {
                    "j/k/↑/↓ h/l / ␣ enter history a/e/d s sort q/esc/ctrl+c quit"
                }
                (TaskView::Archived, false) => {
                    "j/k h/l / enter history s sort u restore q/esc/ctrl+c quit"
                }
                (TaskView::Active, true) => {
                    "j/k h/l / spc enter history a/e/d s sort esc clear q/ctrl+c"
                }
                (TaskView::Archived, true) => {
                    "j/k h/l / enter history s sort u restore esc clear q/ctrl+c"
                }
            },
            TaskListMode::Search => "type · ↑/↓ select · enter keep · esc cancel · ctrl+c quit",
            TaskListMode::Input { .. } => {
                "type · backspace · enter save · esc cancel · ctrl+c quit"
            }
            TaskListMode::ConfirmArchive { .. } => "y/enter · n/esc · ctrl+c quit",
        };
    }
    match state.mode() {
        TaskListMode::Normal => match (state.view(), filtered) {
            (TaskView::Active, false) => {
                "j/k/↑/↓ h/l view / space track enter history s sort a/e/d edit q/esc/ctrl+c quit"
            }
            (TaskView::Archived, false) => {
                "j/k/↑/↓ h/l view / enter history s sort u unarchive q/esc ctrl+c quit"
            }
            (TaskView::Active, true) => {
                "j/k/↑/↓ h/l /find spc track enter history s sort a/e/d edit esc clear q/ctrl+c"
            }
            (TaskView::Archived, true) => {
                "j/k/↑/↓ h/l /find enter history s sort u restore esc clear q/ctrl+c"
            }
        },
        TaskListMode::Search => {
            "type · backspace · ↑/↓ select · enter keep · esc cancel · ctrl+c quit"
        }
        TaskListMode::Input { .. } => {
            "type · backspace delete · enter save · esc cancel · ctrl+c quit"
        }
        TaskListMode::ConfirmArchive { .. } => "y/enter confirm · n/esc cancel · ctrl+c quit",
    }
}

#[cfg(test)]
mod tests {
    use crate::command::Command;
    use crate::screens::WorklogHistoryMode;
    use crate::screens::task_list::{TaskListCommand, TaskListMode, TaskView};
    use crate::test_support::keymap::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn active_mode_maps_movement_actions_and_view_switching() {
        let view = TaskView::Active;
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Char('j'))),
            Some(Command::TaskList(TaskListCommand::MoveDown))
        );
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Down)),
            Some(Command::TaskList(TaskListCommand::MoveDown))
        );
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Char('k'))),
            Some(Command::TaskList(TaskListCommand::MoveUp))
        );
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Up)),
            Some(Command::TaskList(TaskListCommand::MoveUp))
        );
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Char('h'))),
            Some(Command::TaskList(TaskListCommand::ShowActiveTasks))
        );
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Char('l'))),
            Some(Command::TaskList(TaskListCommand::ShowArchivedTasks))
        );
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Char('s'))),
            Some(Command::TaskList(TaskListCommand::CycleOrdering))
        );
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Char(' '))),
            Some(Command::TaskList(TaskListCommand::ToggleTracking))
        );
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Char('a'))),
            Some(Command::TaskList(TaskListCommand::OpenAdd))
        );
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Char('e'))),
            Some(Command::TaskList(TaskListCommand::OpenRename))
        );
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Char('d'))),
            Some(Command::TaskList(TaskListCommand::OpenArchiveConfirm))
        );
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Char('q'))),
            Some(Command::Quit)
        );
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Esc)),
            Some(Command::Quit)
        );
    }

    #[test]
    fn archived_mode_maps_movement_unarchive_and_view_switching() {
        let view = TaskView::Archived;
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Char('j'))),
            Some(Command::TaskList(TaskListCommand::MoveDown))
        );
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Down)),
            Some(Command::TaskList(TaskListCommand::MoveDown))
        );
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Char('k'))),
            Some(Command::TaskList(TaskListCommand::MoveUp))
        );
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Up)),
            Some(Command::TaskList(TaskListCommand::MoveUp))
        );
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Char('h'))),
            Some(Command::TaskList(TaskListCommand::ShowActiveTasks))
        );
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Char('l'))),
            Some(Command::TaskList(TaskListCommand::ShowArchivedTasks))
        );
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Char('s'))),
            Some(Command::TaskList(TaskListCommand::CycleOrdering))
        );
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Char('u'))),
            Some(Command::TaskList(TaskListCommand::UnarchiveSelected))
        );
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Char('q'))),
            Some(Command::Quit)
        );
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Esc)),
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
                map_task_list(TaskListMode::Normal, view, key(code)),
                None,
                "archived mode must not map {code:?}"
            );
        }
        // Enter is the one key both task views share. It opens history
        // instead of acting on the task.
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Enter)),
            Some(Command::TaskList(TaskListCommand::OpenHistory))
        );
    }

    #[test]
    fn normal_mode_ignores_unmapped_keys() {
        for view in [TaskView::Active, TaskView::Archived] {
            assert_eq!(
                map_task_list(TaskListMode::Normal, view, key(KeyCode::Char('x'))),
                None
            );
            assert_eq!(
                map_task_list(TaskListMode::Normal, view, key(KeyCode::Left)),
                None
            );
        }
        assert_eq!(
            map_task_list(
                TaskListMode::Normal,
                TaskView::Archived,
                key(KeyCode::Backspace)
            ),
            None
        );
        assert_eq!(
            map_task_list(
                TaskListMode::Normal,
                TaskView::Active,
                key(KeyCode::Char('u'))
            ),
            None
        );
    }

    #[test]
    fn sort_is_normal_mode_only() {
        for view in [TaskView::Active, TaskView::Archived] {
            assert_eq!(
                map_task_list(TaskListMode::Normal, view, key(KeyCode::Char('s'))),
                Some(Command::TaskList(TaskListCommand::CycleOrdering))
            );
        }
        assert_eq!(
            map_task_list(input(), TaskView::Active, key(KeyCode::Char('s'))),
            Some(Command::TaskList(TaskListCommand::Insert('s')))
        );
        assert_eq!(
            map_task_list(confirm(), TaskView::Active, key(KeyCode::Char('s'))),
            None
        );
    }

    #[test]
    fn modified_action_keys_do_nothing_in_normal_mode() {
        for view in [TaskView::Active, TaskView::Archived] {
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
                    map_task_list(TaskListMode::Normal, view, key),
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
                map_task_list(confirm(), TaskView::Active, key),
                None,
                "modified {key:?} must not act"
            );
        }
    }

    #[test]
    fn input_mode_edits_confirms_and_cancels() {
        let view = TaskView::Active;
        assert_eq!(
            map_task_list(input(), view, key(KeyCode::Char('x'))),
            Some(Command::TaskList(TaskListCommand::Insert('x')))
        );
        // Space is ordinary input while typing.
        assert_eq!(
            map_task_list(input(), view, key(KeyCode::Char(' '))),
            Some(Command::TaskList(TaskListCommand::Insert(' ')))
        );
        assert_eq!(
            map_task_list(input(), view, key(KeyCode::Backspace)),
            Some(Command::TaskList(TaskListCommand::Backspace))
        );
        assert_eq!(
            map_task_list(input(), view, key(KeyCode::Enter)),
            Some(Command::TaskList(TaskListCommand::Confirm))
        );
        assert_eq!(
            map_task_list(input(), view, key(KeyCode::Esc)),
            Some(Command::TaskList(TaskListCommand::Cancel))
        );
    }

    #[test]
    fn input_mode_ignores_modifier_chords_and_other_keys() {
        assert_eq!(map_task_list(input(), TaskView::Active, ctrl('a')), None);
        let alt = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT);
        assert_eq!(map_task_list(input(), TaskView::Active, alt), None);
        let hyper = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::HYPER);
        assert_eq!(map_task_list(input(), TaskView::Active, hyper), None);
        assert_eq!(
            map_task_list(input(), TaskView::Active, key(KeyCode::Up)),
            None
        );
        assert_eq!(
            map_task_list(input(), TaskView::Active, key(KeyCode::Tab)),
            None
        );
    }

    #[test]
    fn shift_is_not_a_chord_so_capitals_are_typed() {
        let capital = KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT);
        assert_eq!(
            map_task_list(input(), TaskView::Active, capital),
            Some(Command::TaskList(TaskListCommand::Insert('A')))
        );
    }

    #[test]
    fn confirm_mode_accepts_and_cancels() {
        let view = TaskView::Active;
        assert_eq!(
            map_task_list(confirm(), view, key(KeyCode::Enter)),
            Some(Command::TaskList(TaskListCommand::Confirm))
        );
        assert_eq!(
            map_task_list(confirm(), view, key(KeyCode::Char('y'))),
            Some(Command::TaskList(TaskListCommand::Confirm))
        );
        assert_eq!(
            map_task_list(confirm(), view, key(KeyCode::Char('n'))),
            Some(Command::TaskList(TaskListCommand::Cancel))
        );
        assert_eq!(
            map_task_list(confirm(), view, key(KeyCode::Esc)),
            Some(Command::TaskList(TaskListCommand::Cancel))
        );
        assert_eq!(
            map_task_list(confirm(), view, key(KeyCode::Char('x'))),
            None
        );
    }

    #[test]
    fn enter_opens_the_history_in_both_task_views() {
        for view in [TaskView::Active, TaskView::Archived] {
            assert_eq!(
                map_task_list(TaskListMode::Normal, view, key(KeyCode::Enter)),
                Some(Command::TaskList(TaskListCommand::OpenHistory))
            );
        }
        // The modal modes keep their Enter meaning.
        assert_eq!(
            map_task_list(input(), TaskView::Active, key(KeyCode::Enter)),
            Some(Command::TaskList(TaskListCommand::Confirm))
        );
        assert_eq!(
            map_task_list(confirm(), TaskView::Active, key(KeyCode::Enter)),
            Some(Command::TaskList(TaskListCommand::Confirm))
        );
    }

    #[test]
    fn search_mode_types_and_navigates_without_triggering_task_actions() {
        let view = TaskView::Active;
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Char('/'))),
            Some(Command::TaskList(TaskListCommand::OpenSearch))
        );
        for (key_code, expected) in [
            (KeyCode::Char('j'), TaskListCommand::InsertSearch('j')),
            (KeyCode::Down, TaskListCommand::MoveDown),
            (KeyCode::Up, TaskListCommand::MoveUp),
            (KeyCode::Backspace, TaskListCommand::BackspaceSearch),
            (KeyCode::Enter, TaskListCommand::CommitSearch),
            (KeyCode::Esc, TaskListCommand::CancelSearch),
        ] {
            assert_eq!(
                map_task_list(TaskListMode::Search, view, key(key_code)),
                Some(Command::TaskList(expected))
            );
        }
        assert!(task_list_footer(TaskListMode::Search, view, 60).contains("enter keep"));
        for code in [
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Backspace,
            KeyCode::Down,
        ] {
            assert_eq!(
                map_task_list(
                    TaskListMode::Search,
                    view,
                    KeyEvent::new(code, KeyModifiers::ALT)
                ),
                None
            );
        }
        assert_eq!(
            map_task_list(
                TaskListMode::Search,
                view,
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT)
            ),
            None
        );
    }

    #[test]
    fn every_accepted_key_is_listed_in_the_footer() {
        let active_keys = task_list_footer(TaskListMode::Normal, TaskView::Active, 80);
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
        let archived_keys = task_list_footer(TaskListMode::Normal, TaskView::Archived, 80);
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
        let input_keys = task_list_footer(input(), TaskView::Active, 80);
        for hint in ["type", "backspace", "enter", "esc", "ctrl+c"] {
            assert!(input_keys.contains(hint), "input footer misses {hint:?}");
        }
        let confirm_keys = task_list_footer(confirm(), TaskView::Active, 80);
        for hint in ["y/enter", "n/esc", "ctrl+c"] {
            assert!(
                confirm_keys.contains(hint),
                "confirm footer misses {hint:?}"
            );
        }
        let history_keys = history_footer(WorklogHistoryMode::Normal, true, 80);
        for hint in [
            "j/k/↑/↓ move",
            "e edit",
            "m move",
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
                let footer = task_list_footer(TaskListMode::Normal, view, width);
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
            task_list_footer(TaskListMode::Normal, TaskView::Active, 60),
            task_list_footer(TaskListMode::Normal, TaskView::Archived, 60),
            task_list_footer(input(), TaskView::Active, 60),
            task_list_footer(confirm(), TaskView::Active, 60),
            history_footer(WorklogHistoryMode::Normal, true, 60),
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
        assert!(footers[0].contains("␣"));
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
