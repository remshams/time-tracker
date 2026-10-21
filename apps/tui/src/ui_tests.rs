use crate::test_support::app_with;
use crate::test_support::render::{HEIGHT, draw_at, row};

#[test]
fn a_terminal_below_the_minimum_width_shows_a_resize_message() {
    let app = app_with(&["alpha"]);
    let terminal = draw_at(&app, 59, HEIGHT);
    assert!(row(&terminal, 0).contains("Time Tracker needs at least 60 columns."));
    assert!(!row(&terminal, 1).contains("Active │ Archived"));
}
