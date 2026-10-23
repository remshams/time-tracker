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
        TaskListMode::PreviewingInactiveTasks { .. } => map_previewing(key),
        TaskListMode::ConfirmInactiveArchive { .. } => map_confirm(key),
        TaskListMode::ArchivingInactiveTasks { .. } => None,
    }
}

fn map_normal(state: &TaskListState, key: KeyEvent) -> Option<KeymapCommand<TaskListCommand>> {
    if key.modifiers == KeyModifiers::CONTROL {
        return match key.code {
            KeyCode::Char('d') => Some(KeymapCommand::Local(TaskListCommand::PageDown)),
            KeyCode::Char('u') => Some(KeymapCommand::Local(TaskListCommand::PageUp)),
            _ => None,
        };
    }
    if key.modifiers == KeyModifiers::SHIFT {
        return map_normal_shift(state, key.code);
    }
    if key.modifiers != KeyModifiers::NONE {
        return None;
    }
    map_normal_plain(state, key.code)
}

fn map_normal_shift(
    state: &TaskListState,
    code: KeyCode,
) -> Option<KeymapCommand<TaskListCommand>> {
    if code == KeyCode::Char('G') {
        return Some(KeymapCommand::Local(TaskListCommand::Last));
    }
    if state.view() == TaskView::Active && code == KeyCode::Char('D') {
        return Some(KeymapCommand::Local(
            TaskListCommand::OpenInactiveArchivePreview,
        ));
    }
    if matches!(code, KeyCode::Tab | KeyCode::BackTab) {
        return Some(KeymapCommand::Local(previous_tab(state.view())));
    }
    None
}

fn next_tab(view: TaskView) -> TaskListCommand {
    match view {
        TaskView::Active => TaskListCommand::ShowArchivedTasks,
        TaskView::Archived => TaskListCommand::ShowAllWorklogs,
    }
}

fn previous_tab(view: TaskView) -> TaskListCommand {
    match view {
        TaskView::Active => TaskListCommand::ShowReports,
        TaskView::Archived => TaskListCommand::ShowActiveTasks,
    }
}

fn map_normal_plain(
    state: &TaskListState,
    code: KeyCode,
) -> Option<KeymapCommand<TaskListCommand>> {
    let command = match code {
        KeyCode::Char('c') => TaskListCommand::CopySelectedName,
        KeyCode::Char('s') => TaskListCommand::CycleOrdering,
        KeyCode::Char('/') => TaskListCommand::OpenSearch,
        KeyCode::Enter => TaskListCommand::OpenHistory,
        KeyCode::Esc if state.search_query().is_some() => TaskListCommand::ClearSearch,
        KeyCode::Char('q') | KeyCode::Esc => return Some(KeymapCommand::Quit),
        _ => {
            return map_task_navigation(state, code)
                .or_else(|| map_task_action(state.view(), code))
                .map(KeymapCommand::Local);
        }
    };
    Some(KeymapCommand::Local(command))
}

fn map_task_navigation(state: &TaskListState, code: KeyCode) -> Option<TaskListCommand> {
    let command = match code {
        KeyCode::Char('j') | KeyCode::Down => TaskListCommand::MoveDown,
        KeyCode::Char('k') | KeyCode::Up => TaskListCommand::MoveUp,
        KeyCode::Char('g') => {
            if state.g_prefix() {
                TaskListCommand::First
            } else {
                TaskListCommand::GPrefix
            }
        }
        KeyCode::Char('G') => TaskListCommand::Last,
        KeyCode::Tab => next_tab(state.view()),
        KeyCode::BackTab => previous_tab(state.view()),
        _ => return None,
    };
    Some(command)
}

fn map_task_action(view: TaskView, code: KeyCode) -> Option<TaskListCommand> {
    let command = match (view, code) {
        (TaskView::Active, KeyCode::Char(' ')) => TaskListCommand::ToggleTracking,
        (TaskView::Active, KeyCode::Char('a')) => TaskListCommand::OpenAdd,
        (TaskView::Active, KeyCode::Char('e')) => TaskListCommand::OpenRename,
        (TaskView::Active, KeyCode::Char('d')) => TaskListCommand::OpenArchiveConfirm,
        (TaskView::Archived, KeyCode::Char('u')) => TaskListCommand::UnarchiveSelected,
        _ => return None,
    };
    Some(command)
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

fn map_previewing(key: KeyEvent) -> Option<KeymapCommand<TaskListCommand>> {
    if key.modifiers == KeyModifiers::NONE && key.code == KeyCode::Esc {
        Some(KeymapCommand::Local(TaskListCommand::Cancel))
    } else {
        None
    }
}

/// Returns the footer hints for the current mode.
///
/// Only normal mode depends on the width: its compact hints fit 60 columns,
/// the medium ones 80 and the wide ones 110. Every other mode has one text
/// that fits 60 columns.
pub(crate) fn footer_hints(state: &TaskListState, width: u16) -> &'static str {
    match state.mode() {
        TaskListMode::Normal => normal_footer_hints(state, width),
        TaskListMode::Search => "type/bs · ↑/↓ select · enter keep · esc cancel · ctrl+c quit",
        TaskListMode::Input { .. } => "type · backspace · enter save · esc cancel · ctrl+c quit",
        TaskListMode::ConfirmArchive { .. } => "y/enter confirm · n/esc cancel · ctrl+c quit",
        TaskListMode::PreviewingInactiveTasks { .. } => {
            "loading preview · esc cancel · ctrl+c quit"
        }
        TaskListMode::ConfirmInactiveArchive { .. } => {
            "enter/y confirm · n/esc cancel · ctrl+c quit"
        }
        TaskListMode::ArchivingInactiveTasks { .. } => "archive in progress · ctrl+c quit",
    }
}

/// While a search filter is active, Esc clears it instead of quitting.
fn normal_footer_hints(state: &TaskListState, width: u16) -> &'static str {
    let filtered = state.search_query().is_some();
    match state.view() {
        TaskView::Active => active_footer_hints(filtered, width),
        TaskView::Archived => archived_footer_hints(filtered, width),
    }
}

fn active_footer_hints(filtered: bool, width: u16) -> &'static str {
    match (filtered, width) {
        (false, ..80) => "j/k tab/⇧tab a/e/d/D space enter history s sort q/esc/ctrl+c",
        (false, ..110) => {
            "j/k/↑/↓ tab/⇧tab space track enter history s sort a/e/d edit D q/esc/ctrl+c quit"
        }
        (false, _) => {
            "j/k/↑/↓ tab/⇧tab / space track enter history s sort a/e/d edit D archive inactive q/esc/ctrl+c quit"
        }
        (true, ..80) => "j/k tab/⇧tab / a/e/d/D ␣ enter history s sort esc q/ctrl+c",
        (true, ..110) => {
            "j/k/↑/↓ tab/⇧tab / ␣ track enter history s sort a/e/d edit D esc clear q/ctrl+c"
        }
        (true, _) => {
            "j/k/↑/↓ tab/⇧tab /find space track enter history s sort a/e/d edit D archive inactive esc clear q/ctrl+c"
        }
    }
}

fn archived_footer_hints(filtered: bool, width: u16) -> &'static str {
    match (filtered, width) {
        (false, ..80) => "j/k tab/⇧tab / enter history s sort u restore q/esc/ctrl+c",
        (false, _) => "j/k/↑/↓ tab/⇧tab view / enter history s sort u unarchive q/esc/ctrl+c quit",
        (true, ..80) => "j/k tab/⇧tab / enter history s sort u restore esc q/ctrl+c",
        (true, _) => "j/k/↑/↓ tab/⇧tab /find enter history s sort u restore esc clear q/ctrl+c",
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
            map_task_list(TaskListMode::Normal, view, key(KeyCode::BackTab)),
            Some(Command::TaskList(TaskListCommand::ShowReports))
        );
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Tab)),
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
    fn uppercase_d_opens_bulk_archive_only_on_the_active_tab() {
        assert_eq!(
            map_task_list(
                TaskListMode::Normal,
                TaskView::Active,
                KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT),
            ),
            Some(Command::TaskList(
                TaskListCommand::OpenInactiveArchivePreview
            ))
        );
        assert_eq!(
            map_task_list(
                TaskListMode::Normal,
                TaskView::Archived,
                KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT),
            ),
            None
        );
        assert_eq!(
            map_task_list(
                TaskListMode::Normal,
                TaskView::Active,
                key(KeyCode::Char('D')),
            ),
            None
        );
        for key_event in [
            key(KeyCode::Enter),
            key(KeyCode::Esc),
            key(KeyCode::Char('n')),
        ] {
            assert_eq!(
                map_task_list(archiving_mode(), TaskView::Active, key_event,),
                None
            );
        }
    }

    #[test]
    fn preview_loading_accepts_only_unmodified_escape() {
        let loading = TaskListMode::PreviewingInactiveTasks {
            as_of: chrono::Utc::now(),
        };
        assert_eq!(
            map_task_list(loading.clone(), TaskView::Active, key(KeyCode::Esc)),
            Some(Command::TaskList(TaskListCommand::Cancel))
        );
        assert_eq!(
            map_task_list(loading.clone(), TaskView::Active, key(KeyCode::Char('x')),),
            None
        );
        assert_eq!(
            map_task_list(
                loading,
                TaskView::Active,
                KeyEvent::new(KeyCode::Esc, KeyModifiers::SHIFT),
            ),
            None
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
            map_task_list(TaskListMode::Normal, view, key(KeyCode::BackTab)),
            Some(Command::TaskList(TaskListCommand::ShowActiveTasks))
        );
        assert_eq!(
            map_task_list(TaskListMode::Normal, view, key(KeyCode::Tab)),
            Some(Command::TaskList(TaskListCommand::ShowAllWorklogs))
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
    fn shift_tab_wraps_and_old_view_keys_do_nothing() {
        for view in [TaskView::Active, TaskView::Archived] {
            for code in [KeyCode::Tab, KeyCode::BackTab] {
                assert_eq!(
                    map_task_list(
                        TaskListMode::Normal,
                        view,
                        KeyEvent::new(code, KeyModifiers::SHIFT)
                    ),
                    Some(Command::TaskList(match view {
                        TaskView::Active => TaskListCommand::ShowReports,
                        TaskView::Archived => TaskListCommand::ShowActiveTasks,
                    }))
                );
            }
            for code in [KeyCode::Char('h'), KeyCode::Char('l')] {
                assert_eq!(map_task_list(TaskListMode::Normal, view, key(code)), None);
            }
        }
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
    fn vim_list_motions_map_only_in_normal_mode() {
        for view in [TaskView::Active, TaskView::Archived] {
            for (key, command) in [
                (key(KeyCode::Char('g')), TaskListCommand::GPrefix),
                (key(KeyCode::Char('G')), TaskListCommand::Last),
                (ctrl('d'), TaskListCommand::PageDown),
                (ctrl('u'), TaskListCommand::PageUp),
            ] {
                assert_eq!(
                    map_task_list(TaskListMode::Normal, view, key),
                    Some(Command::TaskList(command))
                );
            }
        }
        assert_eq!(
            map_task_list(input(), TaskView::Active, key(KeyCode::Char('g'))),
            Some(Command::TaskList(TaskListCommand::Insert('g')))
        );
        assert_eq!(map_task_list(input(), TaskView::Active, ctrl('d')), None);
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

    /// Checks the normal-mode footers at both edges of each width tier, with
    /// and without a search filter.
    #[test]
    fn every_accepted_key_is_listed_in_the_footer() {
        for width in [60, 79, 80, 109, 110, 140] {
            for view in [TaskView::Active, TaskView::Archived] {
                for filtered in [false, true] {
                    let footer = if filtered {
                        filtered_task_list_footer(view, width)
                    } else {
                        task_list_footer(TaskListMode::Normal, view, width)
                    };
                    let context = format!("{view:?} at {width} columns, filtered: {filtered}");
                    let mut hints = vec!["j/k", "tab/⇧tab", "enter history", "s sort", "ctrl+c"];
                    match view {
                        TaskView::Active => hints.extend(["a/e/d", "D"]),
                        TaskView::Archived => hints.push("u "),
                    }
                    if width >= 80 {
                        hints.push("↑/↓");
                        if view == TaskView::Active {
                            hints.extend(["track", "edit"]);
                        }
                    }
                    // Esc clears an active filter, so it must not be listed as a quit key.
                    if filtered {
                        hints.extend(["esc", "q/ctrl+c"]);
                        if width >= 80 {
                            hints.push("esc clear");
                        }
                    } else {
                        hints.push("q/esc/ctrl+c");
                    }
                    for hint in hints {
                        assert!(
                            footer.contains(hint),
                            "{context}: footer misses {hint:?}: {footer:?}"
                        );
                    }
                    if view == TaskView::Active {
                        assert!(
                            footer.contains("space") || footer.contains('␣'),
                            "{context}: footer misses the tracking key: {footer:?}"
                        );
                    }
                    assert_eq!(footer.contains("q/esc"), !filtered, "{context}: {footer:?}");
                    assert!(
                        footer.chars().count() <= usize::from(width),
                        "{context}: footer too wide: {footer:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_history_footer_lists_every_key_at_eighty_columns() {
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
        assert!(
            history_keys.chars().count() <= 80,
            "footer too wide: {history_keys:?}"
        );
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
    fn a_110_column_footer_uses_the_expanded_bulk_archive_hint() {
        let footer = task_list_footer(TaskListMode::Normal, TaskView::Active, 110);
        assert!(footer.contains("D archive inactive"), "{footer:?}");
    }

    /// Checks that footer hints follow each interaction stage, including
    /// pending archive work, and stay the same at every width.
    #[test]
    fn footers_describe_search_editing_and_each_bulk_archive_stage_at_every_width() {
        use crate::screens::task_list::InactiveTaskPreview;

        let as_of = chrono::DateTime::<chrono::Utc>::from_timestamp(0, 0).unwrap();
        let preview = InactiveTaskPreview::local(as_of, Vec::new(), Vec::new());
        for (mode, expected) in [
            (
                TaskListMode::Search,
                "type/bs · ↑/↓ select · enter keep · esc cancel · ctrl+c quit",
            ),
            (
                input(),
                "type · backspace · enter save · esc cancel · ctrl+c quit",
            ),
            (confirm(), "y/enter confirm · n/esc cancel · ctrl+c quit"),
            (
                TaskListMode::PreviewingInactiveTasks { as_of },
                "loading preview · esc cancel · ctrl+c quit",
            ),
            (
                TaskListMode::ConfirmInactiveArchive { preview },
                "enter/y confirm · n/esc cancel · ctrl+c quit",
            ),
            (archiving_mode(), "archive in progress · ctrl+c quit"),
        ] {
            assert!(
                expected.chars().count() <= 60,
                "footer too wide: {expected:?}"
            );
            for width in [60, 80, 110] {
                assert_eq!(
                    task_list_footer(mode.clone(), TaskView::Active, width),
                    expected
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
        assert!(footers[0].contains("j/k"));
        assert!(footers[0].contains("tab/⇧tab"));
        assert!(footers[0].contains("space"));
        assert!(footers[0].contains("a/e/d/D"));
        assert!(footers[0].contains("enter history"));
        assert!(footers[0].contains("s sort"));
        assert!(footers[1].contains("enter history"));
        assert!(footers[1].contains("u restore"));
        assert!(footers[4].contains("o older"));
        assert!(footers[4].contains("r refresh"));
        assert!(footers[4].contains("esc back"));
    }
}
