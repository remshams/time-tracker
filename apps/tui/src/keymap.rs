//! Translation from raw key events to semantic commands.
//!
//! This is the only place where key codes meet commands. Every key a mode
//! accepts must appear in that mode's footer text, so the help strings live
//! here next to the mappings they document.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::app::{Mode, Screen, TaskView};
use crate::command::Command;

/// The quit chord, accepted in every mode.
const QUIT_MODIFIER: KeyModifiers = KeyModifiers::CONTROL;
const QUIT_KEY: KeyCode = KeyCode::Char('c');

/// Maps a key event to a command for the given screen, mode, and task view.
///
/// Returns `None` for keys the state does not handle. Ctrl+C quits in every
/// screen and mode, including while typing. In normal and confirmation
/// modes only unmodified keys act; in input mode Shift still produces
/// capital letters.
pub fn map(mode: &Mode, view: TaskView, screen: Screen, key: KeyEvent) -> Option<Command> {
    if key.kind != KeyEventKind::Press {
        return None;
    }
    if key.modifiers.contains(QUIT_MODIFIER) && key.code == QUIT_KEY {
        return Some(Command::Quit);
    }
    match screen {
        Screen::WorklogHistory => map_history(key),
        Screen::TaskList => match mode {
            Mode::Normal => map_normal(view, key),
            Mode::Input { .. } => map_input(key),
            Mode::ConfirmArchive { .. } => map_confirm(key),
        },
    }
}

/// Keys for the task list of one view.
///
/// Every action needs an unmodified key; chords were either handled as the
/// quit shortcut above or do nothing here. `h` and `l` switch between the
/// active and archived views in both directions; each is a no-op when the
/// target view is already shown. Enter opens the selected task's worklog
/// history in both views; an empty list makes it a no-op in command
/// handling.
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

/// Keys for the read-only worklog history.
///
/// Every action needs an unmodified key. The history answers to no
/// task-list command; Escape returns to the task list and quit works as
/// everywhere.
fn map_history(key: KeyEvent) -> Option<Command> {
    if key.modifiers != KeyModifiers::NONE {
        return None;
    }
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => Some(Command::MoveDown),
        KeyCode::Char('k') | KeyCode::Up => Some(Command::MoveUp),
        KeyCode::Char('o') => Some(Command::LoadOlderWorklogs),
        KeyCode::Char('r') => Some(Command::RefreshWorklogs),
        KeyCode::Esc => Some(Command::BackToTaskList),
        KeyCode::Char('q') => Some(Command::Quit),
        _ => None,
    }
}

/// Keys for the one-line text input.
///
/// Printable characters and Backspace edit, Enter confirms, and Escape
/// cancels. Space is ordinary input.
fn map_input(key: KeyEvent) -> Option<Command> {
    match key.code {
        KeyCode::Enter => Some(Command::Confirm),
        KeyCode::Esc => Some(Command::Cancel),
        KeyCode::Backspace => Some(Command::Backspace),
        KeyCode::Char(character) if !is_chord(key) => Some(Command::Insert(character)),
        _ => None,
    }
}

/// Whether the key carries a command modifier rather than plain text.
///
/// Shift is not a command modifier: capitals are ordinary input. Every other
/// modifier, including Ctrl, Alt, Super, Hyper, and Meta, makes the key a
/// chord that the input does not type.
fn is_chord(key: KeyEvent) -> bool {
    key.modifiers - KeyModifiers::SHIFT != KeyModifiers::NONE
}

/// Keys for the archive confirmation.
///
/// Every action needs an unmodified key.
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

/// One-line key help for the given screen, mode, task view, and available
/// width.
///
/// Every accepted key appears in both the standard and compact variants.
/// The compact text fits the narrowest geometry covered by the TUI.
pub fn footer_hints(mode: &Mode, view: TaskView, screen: Screen, width: u16) -> &'static str {
    if screen == Screen::WorklogHistory {
        return if width < 80 {
            "j/k/↑/↓ · o older · r refresh · esc back · q/ctrl+c quit"
        } else {
            "j/k/↑/↓ move · o older · r refresh · esc back · q/ctrl+c quit"
        };
    }
    if width < 80 {
        return match mode {
            Mode::Normal => match view {
                TaskView::Active => "j/k/↑/↓ · h/l · space · a/e/d · s sort · q/esc/ctrl+c quit",
                TaskView::Archived => "j/k/↑/↓ · h/l · s sort · u restore · q/esc/ctrl+c quit",
            },
            Mode::Input { .. } => "type · backspace · enter save · esc cancel · ctrl+c quit",
            Mode::ConfirmArchive { .. } => "y/enter · n/esc · ctrl+c quit",
        };
    }

    match mode {
        Mode::Normal => match view {
            TaskView::Active => {
                "j/k/↑/↓ move · h/l view · space track · s sort · a/e/d edit · q/esc/ctrl+c quit"
            }
            TaskView::Archived => {
                "j/k/↑/↓ move · h/l view · s sort · u unarchive · q/esc/ctrl+c quit"
            }
        },
        Mode::Input { .. } => "type · backspace delete · enter save · esc cancel · ctrl+c quit",
        Mode::ConfirmArchive { .. } => "y/enter confirm · n/esc cancel · ctrl+c quit",
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    use super::*;
    use crate::app::InputPurpose;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(character: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL)
    }

    fn with_modifier(character: char, modifier: KeyModifiers) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(character), modifier)
    }

    fn input() -> Mode {
        Mode::Input {
            purpose: InputPurpose::Add,
            buffer: String::new(),
        }
    }

    fn confirm() -> Mode {
        Mode::ConfirmArchive {
            task_id: tracker_domain::TaskId::generate(),
            name: "task".to_owned(),
        }
    }

    fn normal_modes() -> [(TaskView, Mode); 2] {
        [
            (TaskView::Active, Mode::Normal),
            (TaskView::Archived, Mode::Normal),
        ]
    }

    #[test]
    fn key_releases_produce_no_command() {
        let release = KeyEvent::new_with_kind(
            KeyCode::Char('j'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );
        for (view, mode) in normal_modes() {
            assert_eq!(map(&mode, view, Screen::TaskList, release), None);
        }
        for mode in [input(), confirm()] {
            assert_eq!(
                map(&mode, TaskView::Active, Screen::TaskList, release),
                None
            );
        }
    }

    #[test]
    fn active_mode_maps_movement_actions_and_view_switching() {
        let view = TaskView::Active;
        assert_eq!(
            map(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('j'))
            ),
            Some(Command::MoveDown)
        );
        assert_eq!(
            map(&Mode::Normal, view, Screen::TaskList, key(KeyCode::Down)),
            Some(Command::MoveDown)
        );
        assert_eq!(
            map(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('k'))
            ),
            Some(Command::MoveUp)
        );
        assert_eq!(
            map(&Mode::Normal, view, Screen::TaskList, key(KeyCode::Up)),
            Some(Command::MoveUp)
        );
        assert_eq!(
            map(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('h'))
            ),
            Some(Command::ShowActiveTasks)
        );
        assert_eq!(
            map(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('l'))
            ),
            Some(Command::ShowArchivedTasks)
        );
        assert_eq!(
            map(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('s'))
            ),
            Some(Command::CycleOrdering)
        );
        assert_eq!(
            map(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char(' '))
            ),
            Some(Command::ToggleTracking)
        );
        assert_eq!(
            map(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('a'))
            ),
            Some(Command::OpenAdd)
        );
        assert_eq!(
            map(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('e'))
            ),
            Some(Command::OpenRename)
        );
        assert_eq!(
            map(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('d'))
            ),
            Some(Command::OpenArchiveConfirm)
        );
        assert_eq!(
            map(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('q'))
            ),
            Some(Command::Quit)
        );
        assert_eq!(
            map(&Mode::Normal, view, Screen::TaskList, key(KeyCode::Esc)),
            Some(Command::Quit)
        );
    }

    #[test]
    fn archived_mode_maps_movement_unarchive_and_view_switching() {
        let view = TaskView::Archived;
        assert_eq!(
            map(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('j'))
            ),
            Some(Command::MoveDown)
        );
        assert_eq!(
            map(&Mode::Normal, view, Screen::TaskList, key(KeyCode::Down)),
            Some(Command::MoveDown)
        );
        assert_eq!(
            map(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('k'))
            ),
            Some(Command::MoveUp)
        );
        assert_eq!(
            map(&Mode::Normal, view, Screen::TaskList, key(KeyCode::Up)),
            Some(Command::MoveUp)
        );
        assert_eq!(
            map(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('h'))
            ),
            Some(Command::ShowActiveTasks)
        );
        assert_eq!(
            map(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('l'))
            ),
            Some(Command::ShowArchivedTasks)
        );
        assert_eq!(
            map(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('s'))
            ),
            Some(Command::CycleOrdering)
        );
        assert_eq!(
            map(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('u'))
            ),
            Some(Command::UnarchiveSelected)
        );
        assert_eq!(
            map(
                &Mode::Normal,
                view,
                Screen::TaskList,
                key(KeyCode::Char('q'))
            ),
            Some(Command::Quit)
        );
        assert_eq!(
            map(&Mode::Normal, view, Screen::TaskList, key(KeyCode::Esc)),
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
                map(&Mode::Normal, view, Screen::TaskList, key(code)),
                None,
                "archived mode must not map {code:?}"
            );
        }
        // Enter is the one key both task views share: it opens the read-only
        // history instead of acting on the task.
        assert_eq!(
            map(&Mode::Normal, view, Screen::TaskList, key(KeyCode::Enter)),
            Some(Command::OpenHistory)
        );
    }

    #[test]
    fn normal_mode_ignores_unmapped_keys() {
        for view in [TaskView::Active, TaskView::Archived] {
            assert_eq!(
                map(
                    &Mode::Normal,
                    view,
                    Screen::TaskList,
                    key(KeyCode::Char('x'))
                ),
                None
            );
            assert_eq!(
                map(&Mode::Normal, view, Screen::TaskList, key(KeyCode::Left)),
                None
            );
        }
        assert_eq!(
            map(
                &Mode::Normal,
                TaskView::Archived,
                Screen::TaskList,
                key(KeyCode::Backspace)
            ),
            None
        );
        assert_eq!(
            map(
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
                map(
                    &Mode::Normal,
                    view,
                    Screen::TaskList,
                    key(KeyCode::Char('s'))
                ),
                Some(Command::CycleOrdering)
            );
        }
        assert_eq!(
            map(
                &input(),
                TaskView::Active,
                Screen::TaskList,
                key(KeyCode::Char('s'))
            ),
            Some(Command::Insert('s'))
        );
        assert_eq!(
            map(
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
                    map(&Mode::Normal, view, Screen::TaskList, key),
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
                map(&confirm(), TaskView::Active, Screen::TaskList, key),
                None,
                "modified {key:?} must not act"
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
            assert_eq!(map(&mode, view, Screen::TaskList, release), None);
        }
        for mode in [input(), confirm()] {
            assert_eq!(
                map(&mode, TaskView::Active, Screen::TaskList, release),
                None
            );
        }
    }

    #[test]
    fn ctrl_c_quits_in_every_mode_and_view() {
        for (view, mode) in normal_modes() {
            assert_eq!(
                map(&mode, view, Screen::TaskList, ctrl('c')),
                Some(Command::Quit)
            );
        }
        for mode in [input(), confirm()] {
            assert_eq!(
                map(&mode, TaskView::Active, Screen::TaskList, ctrl('c')),
                Some(Command::Quit)
            );
        }
    }

    #[test]
    fn input_mode_edits_confirms_and_cancels() {
        let view = TaskView::Active;
        assert_eq!(
            map(&input(), view, Screen::TaskList, key(KeyCode::Char('x'))),
            Some(Command::Insert('x'))
        );
        // Space is ordinary input while typing.
        assert_eq!(
            map(&input(), view, Screen::TaskList, key(KeyCode::Char(' '))),
            Some(Command::Insert(' '))
        );
        assert_eq!(
            map(&input(), view, Screen::TaskList, key(KeyCode::Backspace)),
            Some(Command::Backspace)
        );
        assert_eq!(
            map(&input(), view, Screen::TaskList, key(KeyCode::Enter)),
            Some(Command::Confirm)
        );
        assert_eq!(
            map(&input(), view, Screen::TaskList, key(KeyCode::Esc)),
            Some(Command::Cancel)
        );
    }

    #[test]
    fn input_mode_ignores_modifier_chords_and_other_keys() {
        assert_eq!(
            map(&input(), TaskView::Active, Screen::TaskList, ctrl('a')),
            None
        );
        let alt = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT);
        assert_eq!(map(&input(), TaskView::Active, Screen::TaskList, alt), None);
        let hyper = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::HYPER);
        assert_eq!(
            map(&input(), TaskView::Active, Screen::TaskList, hyper),
            None
        );
        assert_eq!(
            map(
                &input(),
                TaskView::Active,
                Screen::TaskList,
                key(KeyCode::Up)
            ),
            None
        );
        assert_eq!(
            map(
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
            map(&input(), TaskView::Active, Screen::TaskList, capital),
            Some(Command::Insert('A'))
        );
    }

    #[test]
    fn confirm_mode_accepts_and_cancels() {
        let view = TaskView::Active;
        assert_eq!(
            map(&confirm(), view, Screen::TaskList, key(KeyCode::Enter)),
            Some(Command::Confirm)
        );
        assert_eq!(
            map(&confirm(), view, Screen::TaskList, key(KeyCode::Char('y'))),
            Some(Command::Confirm)
        );
        assert_eq!(
            map(&confirm(), view, Screen::TaskList, key(KeyCode::Char('n'))),
            Some(Command::Cancel)
        );
        assert_eq!(
            map(&confirm(), view, Screen::TaskList, key(KeyCode::Esc)),
            Some(Command::Cancel)
        );
        assert_eq!(
            map(&confirm(), view, Screen::TaskList, key(KeyCode::Char('x'))),
            None
        );
    }

    #[test]
    fn enter_opens_the_history_in_both_task_views() {
        for view in [TaskView::Active, TaskView::Archived] {
            assert_eq!(
                map(&Mode::Normal, view, Screen::TaskList, key(KeyCode::Enter)),
                Some(Command::OpenHistory)
            );
        }
        // The modal modes keep their Enter meaning.
        assert_eq!(
            map(
                &input(),
                TaskView::Active,
                Screen::TaskList,
                key(KeyCode::Enter)
            ),
            Some(Command::Confirm)
        );
        assert_eq!(
            map(
                &confirm(),
                TaskView::Active,
                Screen::TaskList,
                key(KeyCode::Enter)
            ),
            Some(Command::Confirm)
        );
    }

    #[test]
    fn the_history_maps_movement_paging_refresh_and_back() {
        let history = Screen::WorklogHistory;
        assert_eq!(
            map(
                &Mode::Normal,
                TaskView::Active,
                history,
                key(KeyCode::Char('j'))
            ),
            Some(Command::MoveDown)
        );
        assert_eq!(
            map(&Mode::Normal, TaskView::Active, history, key(KeyCode::Down)),
            Some(Command::MoveDown)
        );
        assert_eq!(
            map(
                &Mode::Normal,
                TaskView::Active,
                history,
                key(KeyCode::Char('k'))
            ),
            Some(Command::MoveUp)
        );
        assert_eq!(
            map(&Mode::Normal, TaskView::Active, history, key(KeyCode::Up)),
            Some(Command::MoveUp)
        );
        assert_eq!(
            map(
                &Mode::Normal,
                TaskView::Active,
                history,
                key(KeyCode::Char('o'))
            ),
            Some(Command::LoadOlderWorklogs)
        );
        assert_eq!(
            map(
                &Mode::Normal,
                TaskView::Active,
                history,
                key(KeyCode::Char('r'))
            ),
            Some(Command::RefreshWorklogs)
        );
        assert_eq!(
            map(&Mode::Normal, TaskView::Active, history, key(KeyCode::Esc)),
            Some(Command::BackToTaskList)
        );
        assert_eq!(
            map(
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
            KeyCode::Char('e'),
            KeyCode::Char('d'),
            KeyCode::Char('u'),
            KeyCode::Char('s'),
            KeyCode::Char('h'),
            KeyCode::Char('l'),
            KeyCode::Enter,
            KeyCode::Backspace,
            KeyCode::Char('x'),
        ] {
            assert_eq!(
                map(
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
                map(
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
            map(
                &Mode::Normal,
                TaskView::Active,
                Screen::WorklogHistory,
                ctrl('c')
            ),
            Some(Command::Quit)
        );
    }

    #[test]
    fn every_accepted_key_is_listed_in_the_footer() {
        let active_keys = footer_hints(&Mode::Normal, TaskView::Active, Screen::TaskList, 80);
        for hint in [
            "j/k/↑/↓",
            "h/l",
            "space",
            "s sort",
            "a/e/d",
            "edit",
            "q/esc",
            "ctrl+c",
        ] {
            assert!(active_keys.contains(hint), "active footer misses {hint:?}");
        }
        assert!(active_keys.contains("s sort"));
        let archived_keys = footer_hints(&Mode::Normal, TaskView::Archived, Screen::TaskList, 80);
        for hint in ["j/k/↑/↓", "h/l", "s sort", "u ", "q/esc", "ctrl+c"] {
            assert!(
                archived_keys.contains(hint),
                "archived footer misses {hint:?}"
            );
        }
        let input_keys = footer_hints(&input(), TaskView::Active, Screen::TaskList, 80);
        for hint in ["type", "backspace", "enter", "esc", "ctrl+c"] {
            assert!(input_keys.contains(hint), "input footer misses {hint:?}");
        }
        let confirm_keys = footer_hints(&confirm(), TaskView::Active, Screen::TaskList, 80);
        for hint in ["y/enter", "n/esc", "ctrl+c"] {
            assert!(
                confirm_keys.contains(hint),
                "confirm footer misses {hint:?}"
            );
        }
        let history_keys =
            footer_hints(&Mode::Normal, TaskView::Active, Screen::WorklogHistory, 80);
        for hint in [
            "j/k/↑/↓ move",
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
    fn compact_footers_keep_every_key_visible_at_sixty_columns() {
        let footers = [
            footer_hints(&Mode::Normal, TaskView::Active, Screen::TaskList, 60),
            footer_hints(&Mode::Normal, TaskView::Archived, Screen::TaskList, 60),
            footer_hints(&input(), TaskView::Active, Screen::TaskList, 60),
            footer_hints(&confirm(), TaskView::Active, Screen::TaskList, 60),
            footer_hints(&Mode::Normal, TaskView::Active, Screen::WorklogHistory, 60),
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
        assert!(footers[0].contains("space"));
        assert!(footers[0].contains("a/e/d"));
        assert!(footers[0].contains("s sort"));
        assert!(footers[1].contains("u restore"));
        assert!(footers[4].contains("o older"));
        assert!(footers[4].contains("r refresh"));
        assert!(footers[4].contains("esc back"));
    }
}
