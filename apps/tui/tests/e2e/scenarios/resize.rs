//! Resize scenarios: the relocated layout after the terminal changes size.
//!
//! Every wait predicate here names the target geometry plus the relocated
//! components, because termlens clips or pads the pre-repaint grid to the
//! new size the moment the resize lands: only text on the new bottom rows,
//! corners on the new edges, or a dialog at its new center separates the
//! stale grid from the post-SIGWINCH repaint.

use termlens::{Key, Screen};

use crate::context::TestContext;
use crate::page::TimeTrackerPage;

/// The grown geometry: wider and taller than the 80x24 baseline, so the
/// pre-repaint grid is padded with blank rows and columns that only a
/// fresh repaint fills.
const GROWN: (u16, u16) = (110, 34);

/// The shrunken geometry: shorter and narrower than the baseline, so the
/// pre-repaint grid is clipped instead of padded. Sixty columns still fit
/// the task names and the compact footer.
const SHRUNKEN: (u16, u16) = (60, 20);

/// The seeded tasks, in storage order, as every resize predicate expects
/// them on the panel.
fn seeded_task_names() -> Vec<String> {
    [
        "Write release notes".to_owned(),
        "Fix the coffee machine".to_owned(),
        "Plan Friday's demo".to_owned(),
    ]
    .into_iter()
    .collect()
}

/// The full normal-mode postconditions at the shrunken geometry, with the
/// selection parked on the second row.
fn the_shrunken_layout_holds(screen: &Screen) -> bool {
    let page = TimeTrackerPage::new(screen.clone());
    page.size() == SHRUNKEN
        && page.header().title_is_accented()
        && page.header().is_idle()
        && page.task_panel().shows_active_tasks()
        && page.task_panel().shows_ordering("recently worked")
        && page.task_panel().frame_corners_fit_current_geometry()
        && page.task_panel().task_names() == seeded_task_names()
        && page.task_panel().selected_index() == Some(1)
        && page.status_bar().text().is_empty()
        && page.footer().hints_open_history()
        && page.footer().hints_sorting()
        && page.footer().hints_quit()
}

/// The full normal-mode postconditions at the grown geometry, with the
/// selection parked on the second row.
fn the_grown_layout_holds(screen: &Screen) -> bool {
    let page = TimeTrackerPage::new(screen.clone());
    page.size() == GROWN
        && page.header().title_is_accented()
        && page.header().is_idle()
        && page.task_panel().shows_active_tasks()
        && page.task_panel().shows_ordering("recently worked")
        && page.task_panel().frame_corners_fit_current_geometry()
        && page.task_panel().task_names() == seeded_task_names()
        && page.task_panel().selected_index() == Some(1)
        && page.status_bar().text().is_empty()
        && page.footer().hints_quit()
}

/// The add-dialog postconditions at the shrunken geometry: the target
/// size, the relocated status and footer, the panel corners, title,
/// tasks, and selection, and the dialog at its new center still holding
/// the prompt, the full typed text, and the cursor.
fn the_shrunken_dialog_holds(screen: &Screen) -> bool {
    let page = TimeTrackerPage::new(screen.clone());
    page.size() == SHRUNKEN
        && page.status_bar().text().is_empty()
        && page.footer().hints_input()
        && page.task_panel().shows_active_tasks()
        && page.task_panel().shows_ordering("recently worked")
        && page.task_panel().frame_corners_fit_current_geometry()
        && page.task_panel().task_names() == seeded_task_names()
        && page.task_panel().selected_index() == Some(0)
        && page.task_input_dialog().is_some_and(|dialog| {
            dialog.prompt() == "New task name:"
                && dialog.text() == "Prepare sprint review"
                && dialog.cursor_is_visible()
        })
}

/// The add-dialog postconditions at the grown geometry, mirroring the
/// shrunken predicate at the padded size.
fn the_grown_dialog_holds(screen: &Screen) -> bool {
    let page = TimeTrackerPage::new(screen.clone());
    page.size() == GROWN
        && page.status_bar().text().is_empty()
        && page.footer().hints_input()
        && page.task_panel().shows_active_tasks()
        && page.task_panel().shows_ordering("recently worked")
        && page.task_panel().frame_corners_fit_current_geometry()
        && page.task_panel().task_names() == seeded_task_names()
        && page.task_panel().selected_index() == Some(0)
        && page.task_input_dialog().is_some_and(|dialog| {
            dialog.prompt() == "New task name:"
                && dialog.text() == "Prepare sprint review"
                && dialog.cursor_is_visible()
        })
}

#[test]
fn a_resized_terminal_relocates_every_component_and_stays_drivable() {
    let context = TestContext::new();
    let mut tt = context.launch();
    tt.wait_for_first_frame("the first frame", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().is_idle() && page.task_panel().task_names().len() == 3
    });

    // Park the selection on the second row before resizing, so the resize
    // predicates can prove the repaint kept it.
    let page = tt.press_and_wait(Key::Char('j'), "the second row", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .selected_index()
            == Some(1)
    });
    assert_eq!(page.task_panel().row(1).name(), "Fix the coffee machine");

    // Shrinking clips the stale grid; only the repaint puts status and
    // footer text on the new bottom rows and the corners on the new edges.
    let page = tt.resize_and_wait(
        SHRUNKEN.0,
        SHRUNKEN.1,
        "the shrunken layout",
        the_shrunken_layout_holds,
    );
    assert!(
        page.task_panel().frame_corners_fit_current_geometry(),
        "the panel corners follow the shrunken edges:\n{}",
        page.screen()
    );
    assert_eq!(
        page.status_bar().text(),
        "",
        "the status line is empty at the new bottom rows:\n{}",
        page.screen()
    );
    assert!(
        page.footer().hints_open_history(),
        "the compact footer names the worklog-history shortcut:\n{}",
        page.screen()
    );
    assert_eq!(
        page.task_panel().selected_index(),
        Some(1),
        "the shrunken repaint kept the selection:\n{}",
        page.screen()
    );

    // Growing pads the stale grid with blanks; only the repaint fills the
    // new bottom rows and edges.
    let page = tt.resize_and_wait(GROWN.0, GROWN.1, "the grown layout", the_grown_layout_holds);
    assert!(
        page.task_panel().frame_corners_fit_current_geometry(),
        "the panel corners follow the grown edges:\n{}",
        page.screen()
    );
    assert!(
        page.footer().hints_quit(),
        "the footer sits on the new bottom row:\n{}",
        page.screen()
    );
    assert_eq!(
        page.task_panel().selected_index(),
        Some(1),
        "the grown repaint kept the selection:\n{}",
        page.screen()
    );

    // A visible navigation key proves input still works after both
    // resizes.
    let page = tt.press_and_wait(Key::Char('k'), "the moved selection", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .selected_index()
            == Some(0)
    });
    assert_eq!(page.task_panel().row(0).name(), "Write release notes");

    tt.quit().assert_clean_exit();
}

#[test]
fn an_open_dialog_recenters_on_resize_and_saves_what_was_typed() {
    let context = TestContext::new();
    let mut tt = context.launch();
    tt.wait_for_first_frame("the first frame", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().is_idle() && page.task_panel().task_names().len() == 3
    });

    tt.press_and_wait(Key::Char('a'), "the add dialog", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_input_dialog()
            .is_some_and(|dialog| dialog.prompt() == "New task name:" && dialog.text().is_empty())
    });
    tt.type_text("Prepare sprint review");
    tt.wait_for("the typed name", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_input_dialog()
            .is_some_and(|dialog| dialog.text() == "Prepare sprint review")
    });

    // Shrinking: the stale clipped grid still shows the dialog at the old
    // center, so nothing in the predicate can hold before the repaint.
    let page = tt.resize_and_wait(
        SHRUNKEN.0,
        SHRUNKEN.1,
        "the recentered dialog",
        the_shrunken_dialog_holds,
    );
    let dialog = page
        .task_input_dialog()
        .expect("the dialog survived the shrink");
    assert_eq!(dialog.prompt(), "New task name:");
    assert_eq!(dialog.text(), "Prepare sprint review");
    assert!(
        dialog.cursor_is_visible(),
        "the input cursor is visible after the shrink:\n{}",
        page.screen()
    );

    // Growing back: the padded stale grid fails the same predicate until
    // the repaint relocates the dialog again.
    let page = tt.resize_and_wait(
        GROWN.0,
        GROWN.1,
        "the recentered dialog",
        the_grown_dialog_holds,
    );
    let dialog = page
        .task_input_dialog()
        .expect("the dialog survived the growth");
    assert_eq!(dialog.prompt(), "New task name:");
    assert_eq!(dialog.text(), "Prepare sprint review");
    assert!(
        dialog.cursor_is_visible(),
        "the input cursor is visible after the growth:\n{}",
        page.screen()
    );

    // The buffered text saves at the new geometry.
    let page = tt.press_and_wait(Key::Enter, "the added task", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_input_dialog().is_none()
            && page.status_bar().text() == "Added \"Prepare sprint review\""
            && page.task_panel().task_names().len() == 4
            && page.task_panel().row(0).is_selected()
    });
    assert_eq!(page.task_panel().row(0).name(), "Prepare sprint review");

    tt.quit().assert_clean_exit();

    let database = context.database();
    assert_eq!(
        database.task_names(),
        [
            "Write release notes".to_owned(),
            "Fix the coffee machine".to_owned(),
            "Plan Friday's demo".to_owned(),
            "Prepare sprint review".to_owned(),
        ]
    );
}
