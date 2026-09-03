//! Task creation, renaming, and selection-movement scenarios.

use termlens::Key;

use crate::context::TestContext;
use crate::page::TimeTrackerPage;

#[test]
fn adding_a_task_renders_selects_and_persists_it() {
    let context = TestContext::new();
    let mut tt = context.launch();
    tt.wait_for_first_frame("the first frame", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().is_idle() && page.task_panel().task_names().len() == 3
    });

    let page = tt.press_and_wait(Key::Char('a'), "the add dialog", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_input_dialog()
            .is_some_and(|dialog| {
                dialog.prompt() == "New task name:"
                    && dialog.text().is_empty()
                    && dialog.cursor_is_visible()
            })
    });
    assert_eq!(
        page.task_input_dialog()
            .expect("the add dialog is open")
            .prompt(),
        "New task name:"
    );

    tt.type_text("Prepare sprint review");
    tt.wait_for("the typed name", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_input_dialog()
            .is_some_and(|dialog| dialog.text() == "Prepare sprint review")
    });

    let page = tt.press_and_wait(Key::Enter, "the added task", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_input_dialog().is_none()
            && page.status_bar().text() == "Added \"Prepare sprint review\""
            && page.task_panel().task_names().len() == 4
            && page.task_panel().row(3).is_selected()
    });
    let panel = page.task_panel();
    assert_eq!(panel.task_names()[3], "Prepare sprint review");
    assert_eq!(panel.selected_index(), Some(3));
    assert_eq!(panel.row(3).name(), "Prepare sprint review");
    assert!(
        page.task_input_dialog().is_none(),
        "the dialog closed after saving:\n{}",
        page.screen()
    );

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
    let stored = database
        .task_by_name("Prepare sprint review")
        .expect("the added task is stored");
    assert!(!stored.archived, "a new task starts out not archived");
}

#[test]
fn renaming_a_task_replaces_the_name_and_persists_it() {
    let context = TestContext::new();
    let mut tt = context.launch();
    tt.wait_for_first_frame("the first frame", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().is_idle() && page.task_panel().task_names().len() == 3
    });
    let database = context.database();
    let original = database
        .task_by_name("Write release notes")
        .expect("the selected task is stored");

    let page = tt.press_and_wait(Key::Char('e'), "the rename dialog", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_input_dialog()
            .is_some_and(|dialog| {
                dialog.prompt() == "Rename task:"
                    && dialog.text() == "Write release notes"
                    && dialog.cursor_is_visible()
            })
    });
    assert_eq!(
        page.task_input_dialog()
            .expect("the rename dialog is open")
            .text(),
        "Write release notes"
    );

    // Replace the prefilled name through real key input: one backspace per
    // character, then the new name.
    for _ in 0.."Write release notes".chars().count() {
        tt.press(Key::Backspace);
    }
    tt.wait_for("the emptied input", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_input_dialog()
            .is_some_and(|dialog| dialog.text().is_empty())
    });

    tt.type_text("Ship the quarterly report");
    tt.wait_for("the replacement name", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_input_dialog()
            .is_some_and(|dialog| dialog.text() == "Ship the quarterly report")
    });

    let page = tt.press_and_wait(Key::Enter, "the renamed task", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_input_dialog().is_none()
            && page.status_bar().text() == "Renamed to \"Ship the quarterly report\""
            && page.task_panel().task_names()[0] == "Ship the quarterly report"
    });
    assert_eq!(page.task_panel().selected_index(), Some(0));
    assert_eq!(page.task_panel().row(0).name(), "Ship the quarterly report");

    tt.quit().assert_clean_exit();

    let names = database.task_names();
    assert_eq!(names.len(), 3);
    assert!(
        names.contains(&"Ship the quarterly report".to_owned()),
        "the renamed task is stored: {names:?}"
    );
    assert!(
        !names.contains(&"Write release notes".to_owned()),
        "the old name is gone: {names:?}"
    );
    let renamed = database
        .task_by_name("Ship the quarterly report")
        .expect("the renamed task is stored");
    assert_eq!(
        renamed.id, original.id,
        "the rename kept the task's stored identity"
    );
}

#[test]
fn selection_moves_with_j_and_k_and_never_wraps_at_the_list_edges() {
    let context = TestContext::new();
    let mut tt = context.launch();
    tt.wait_for_first_frame("the first frame", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().selected_index() == Some(0) && page.task_panel().task_names().len() == 3
    });

    // k on the first row is a no-op. The j that follows is the visible
    // sentinel: it only lands on the second row if the queued k did not
    // wrap the selection to the bottom.
    tt.press(Key::Char('k'));
    let page = tt.press_and_wait(Key::Char('j'), "the second row", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .selected_index()
            == Some(1)
    });
    assert_eq!(page.task_panel().row(1).name(), "Fix the coffee machine");

    // j reaches the last row.
    let page = tt.press_and_wait(Key::Char('j'), "the last row", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .selected_index()
            == Some(2)
    });
    assert_eq!(page.task_panel().row(2).name(), "Plan Friday's demo");

    // A further j stays on the last row. The k that follows only lands on
    // the middle row if the queued j did not wrap the selection to the top.
    tt.press(Key::Char('j'));
    let page = tt.press_and_wait(Key::Char('k'), "the middle row again", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .selected_index()
            == Some(1)
    });
    assert_eq!(page.task_panel().row(1).name(), "Fix the coffee machine");

    tt.quit().assert_clean_exit();
}
