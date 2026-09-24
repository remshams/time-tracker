//! Tests for stored task-list state.

use super::*;
use crate::screens::TaskListState;

#[test]
fn cancel_commands_preserve_expected_state() {
    let mut app = app_with(&["one"]);
    app.handle(Command::TaskList(TaskListCommand::OpenAdd));
    app.handle(Command::TaskList(TaskListCommand::Insert('x')));
    app.handle(Command::TaskList(TaskListCommand::Cancel));
    assert!(matches!(
        app.app_view().task_list().mode(),
        TaskListMode::Normal
    ));
    assert_eq!(app.app_view().tasks().len(), 1);
}

#[test]
fn search_cancel_restores_query_and_selection_before_editing() {
    let original = TaskId::generate();
    let result = TaskId::generate();
    let mut state = TaskListState::new(Some(original));

    state.open_search();
    state.insert_search('b', 256);
    state.set_selection(Some(result));
    state.cancel_search();
    assert_eq!(state.search_query(), None);
    assert_eq!(state.selection(), Some(original));
    assert_eq!(state.mode(), &TaskListMode::Normal);

    state.open_search();
    state.insert_search('b', 256);
    state.commit_search();
    state.open_search();
    state.insert_search('x', 256);
    state.set_selection(None);
    state.cancel_search();
    assert_eq!(state.search_query(), Some("b"));
    assert_eq!(state.selection(), Some(original));
}

#[test]
fn clearing_a_search_with_no_match_restores_the_prior_selection() {
    let original = TaskId::generate();
    let mut state = TaskListState::new(Some(original));
    state.open_search();
    state.insert_search('z', 256);
    state.set_selection(None);
    state.commit_search();
    state.clear_search();
    assert_eq!(state.search_query(), None);
    assert_eq!(state.selection(), Some(original));

    state.open_search();
    state.insert_search('b', 256);
    state.commit_search();
    state.show(TaskView::Archived, None);
    assert_eq!(state.search_query(), None);
    assert_eq!(state.selection(), None);
}

#[test]
fn task_search_stops_at_the_task_name_limit() {
    let mut state = TaskListState::new(None);
    state.open_search();
    for _ in 0..=TaskName::MAX_LEN {
        state.insert_search('a', TaskName::MAX_LEN);
    }
    assert_eq!(
        state.search_query().unwrap().chars().count(),
        TaskName::MAX_LEN
    );
}
