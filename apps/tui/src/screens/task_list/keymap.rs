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

pub(crate) fn footer_hints(state: &TaskListState, width: u16) -> &'static str {
    if width < 80 {
        return compact_footer_hints(state);
    }
    if width < 110 {
        return medium_footer_hints(state);
    }
    wide_footer_hints(state)
}

fn compact_footer_hints(state: &TaskListState) -> &'static str {
    match state.mode() {
        TaskListMode::Normal => compact_normal_footer_hints(state),
        TaskListMode::Search => {
            "type · backspace · ↑/↓ select · enter keep · esc cancel · ctrl+c quit"
        }
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

fn compact_normal_footer_hints(state: &TaskListState) -> &'static str {
    match (state.view(), state.search_query().is_some()) {
        (TaskView::Active, false) => "j/k tab/⇧tab a/e/d/D space enter history s sort q/esc/ctrl+c",
        (TaskView::Archived, false) => "j/k tab/⇧tab / enter history s sort u restore q/esc/ctrl+c",
        (TaskView::Active, true) => "j/k tab/⇧tab /find a/e/d/D ␣ enter history esc q/esc/ctrl+c",
        (TaskView::Archived, true) => "j/k tab/⇧tab / enter history s sort u restore esc q/ctrl+c",
    }
}

fn medium_footer_hints(state: &TaskListState) -> &'static str {
    match state.mode() {
        TaskListMode::Normal => medium_normal_footer_hints(state),
        TaskListMode::Search => "type · ↑/↓ select · enter keep · esc cancel · ctrl+c quit",
        TaskListMode::Input { .. } => {
            "type · backspace delete · enter save · esc cancel · ctrl+c quit"
        }
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

fn medium_normal_footer_hints(state: &TaskListState) -> &'static str {
    match (state.view(), state.search_query().is_some()) {
        (TaskView::Active, false) => {
            "j/k/↑/↓ tab/⇧tab space track enter history s sort a/e/d edit D q/esc/ctrl+c quit"
        }
        (TaskView::Archived, false) => {
            "j/k/↑/↓ tab/⇧tab view / enter history s sort u unarchive q/esc/ctrl+c quit"
        }
        (TaskView::Active, true) => {
            "j/k/↑/↓ tab/⇧tab /find space track enter history s sort a/e/d edit D bulk esc q/ctrl+c"
        }
        (TaskView::Archived, true) => {
            "j/k/↑/↓ tab/⇧tab /find enter history s sort u restore esc q/esc/ctrl+c"
        }
    }
}

fn wide_footer_hints(state: &TaskListState) -> &'static str {
    match state.mode() {
        TaskListMode::Normal => wide_normal_footer_hints(state),
        TaskListMode::Search => "type · ↑/↓ select · enter keep · esc cancel · ctrl+c quit",
        TaskListMode::Input { .. } => {
            "type · backspace delete · enter save · esc cancel · ctrl+c quit"
        }
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

fn wide_normal_footer_hints(state: &TaskListState) -> &'static str {
    match (state.view(), state.search_query().is_some()) {
        (TaskView::Active, false) => {
            "j/k/↑/↓ tab/⇧tab / space track enter history s sort a/e/d edit D archive inactive q/esc/ctrl+c quit"
        }
        (TaskView::Archived, false) => {
            "j/k/↑/↓ tab/⇧tab view / enter history s sort u unarchive q/esc/ctrl+c quit"
        }
        (TaskView::Active, true) => {
            "j/k/↑/↓ tab/⇧tab /find ␣ enter history s sort D bulk esc clear q/ctrl+c"
        }
        (TaskView::Archived, true) => {
            "j/k/↑/↓ tab/⇧tab /find enter history s sort u restore esc clear q/ctrl+c"
        }
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

    #[test]
    fn every_accepted_key_is_listed_in_the_footer() {
        let active_keys = task_list_footer(TaskListMode::Normal, TaskView::Active, 80);
        for hint in [
            "j/k/↑/↓",
            "tab/⇧tab",
            "space",
            "enter history",
            "s sort",
            "a/e/d",
            "edit",
            "D",
            "q/esc",
            "ctrl+c",
        ] {
            assert!(active_keys.contains(hint), "active footer misses {hint:?}");
        }
        assert!(active_keys.contains("s sort"));
        let archived_keys = task_list_footer(TaskListMode::Normal, TaskView::Archived, 80);
        for hint in [
            "j/k/↑/↓",
            "tab/⇧tab",
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
    fn a_110_column_footer_uses_the_expanded_bulk_archive_hint() {
        let footer = task_list_footer(TaskListMode::Normal, TaskView::Active, 110);
        assert!(footer.contains("D archive inactive"), "{footer:?}");
    }

    #[test]
    fn wide_footers_describe_search_editing_and_each_bulk_archive_stage() {
        use crate::screens::task_list::InactiveTaskPreview;

        let as_of = chrono::DateTime::<chrono::Utc>::from_timestamp(0, 0).unwrap();
        let preview = InactiveTaskPreview::Local {
            as_of,
            candidate_ids: Vec::new(),
            sample_names: Vec::new(),
        };
        for (mode, expected) in [
            (
                TaskListMode::Search,
                "type · ↑/↓ select · enter keep · esc cancel · ctrl+c quit",
            ),
            (
                input(),
                "type · backspace delete · enter save · esc cancel · ctrl+c quit",
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
            assert_eq!(task_list_footer(mode, TaskView::Active, 110), expected);
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
