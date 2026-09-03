//! Tracking and input-dialog scenarios.

use termlens::Key;

use crate::context::TestContext;
use crate::page::TimeTrackerPage;

#[test]
fn tracking_shows_the_timer_the_modal_and_the_validation_error_with_their_styles() {
    let context = TestContext::new();
    let mut tt = context.launch();

    let page = tt.wait_for_first_frame("the first frame", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().is_idle() && page.task_panel().task_names().len() == 3
    });
    assert!(page.header().is_idle());

    // Start tracking the selected task. The predicate names the final
    // state: the header runs the right task on a valid HH:MM:SS clock,
    // and the marker and selection sit on its row alone.
    let page = tt.press_and_wait(Key::Char(' '), "the tracking header", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().active_task().is_some_and(|active| {
            active.name() == "Write release notes" && active.elapsed_is_hhmmss()
        }) && page.task_panel().row(0).has_active_marker()
            && page.task_panel().row(0).is_selected()
            && !page.task_panel().row(1).has_active_marker()
            && !page.task_panel().row(1).is_selected()
    });
    let active = page
        .header()
        .active_task()
        .expect("the header names the running task");
    assert_eq!(active.name(), "Write release notes");
    assert!(
        active.elapsed_is_hhmmss(),
        "the elapsed time is exactly HH:MM:SS, got {:?}",
        active.elapsed()
    );
    assert!(
        active.marker_is_green(),
        "the running marker keeps its green accent:\n{}",
        page.screen()
    );
    // The selection highlight and the marker coexist on one row, and only
    // the running task's row is marked. On the selected row the highlight
    // colors override the marker's own green; the header carries it.
    let selected_row = page.task_panel().row(0);
    assert_eq!(selected_row.name(), "Write release notes");
    assert!(selected_row.has_active_marker());
    assert!(selected_row.is_selected());
    assert!(!page.task_panel().row(1).has_active_marker());
    assert!(!page.task_panel().row(1).is_selected());

    // The add dialog opens over the list, empty, with its cursor.
    let page = tt.press_and_wait(Key::Char('a'), "the add dialog", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_input_dialog()
            .is_some_and(|dialog| {
                dialog.prompt() == "New task name:"
                    && dialog.text().is_empty()
                    && dialog.cursor_is_visible()
            })
    });
    let dialog = page.task_input_dialog().expect("the add dialog is open");
    assert_eq!(dialog.prompt(), "New task name:");
    assert_eq!(dialog.text(), "");
    assert!(
        dialog.cursor_is_visible(),
        "the input cursor is visible:\n{}",
        page.screen()
    );

    // An empty name is rejected; the dialog stays open for another try.
    let page = tt.press_and_wait(Key::Enter, "the validation error", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.status_bar().text() == "Error: The task name must not be empty"
            && page.status_bar().is_error()
            && page.status_bar().label_is_accented()
            && page.task_input_dialog().is_some()
    });
    let status = page.status_bar();
    assert_eq!(
        status.text(),
        "Error: The task name must not be empty",
        "the empty name is rejected:\n{}",
        page.screen()
    );
    assert!(
        status.label_is_accented(),
        "the error label keeps its red bold accent:\n{}",
        page.screen()
    );
    assert!(
        page.task_input_dialog().is_some(),
        "the dialog stays open after invalid input:\n{}",
        page.screen()
    );

    tt.quit().assert_clean_exit();
}
