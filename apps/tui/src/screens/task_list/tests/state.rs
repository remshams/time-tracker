//! Tests for stored task-list state.

use super::*;

#[test]
fn cancel_commands_preserve_expected_state() {
    let mut app = app_with(&["one"]);
    app.handle(Command::OpenAdd);
    app.handle(Command::Insert('x'));
    app.handle(Command::Cancel);
    assert!(matches!(
        app.app_view().task_list().mode(),
        TaskListMode::Normal
    ));
    assert_eq!(app.app_view().tasks().len(), 1);
}
