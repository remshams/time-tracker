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

#[test]
fn stopping_tracking_idles_the_header_and_closes_the_worklog() {
    let context = TestContext::new();
    let mut tt = context.launch();
    tt.wait_for_first_frame("the first frame", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().is_idle() && page.task_panel().task_names().len() == 3
    });
    let database = context.database();

    let page = tt.press_and_wait(Key::Char(' '), "the running header", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().active_task().is_some_and(|active| {
            active.name() == "Write release notes" && active.elapsed_is_hhmmss()
        }) && page.task_panel().active_marker_index() == Some(0)
    });
    assert_eq!(
        page.header()
            .active_task()
            .expect("the header names the running task")
            .name(),
        "Write release notes"
    );
    let active = database
        .active_worklog()
        .expect("the started worklog is stored");
    assert_eq!(active.task_name, "Write release notes");
    assert!(
        active.end.is_none(),
        "the started worklog is active: {active:?}"
    );

    let page = tt.press_and_wait(Key::Char(' '), "the idle header", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().is_idle()
            && page.status_bar().text() == "Stopped \"Write release notes\""
            && page.task_panel().active_marker_index().is_none()
    });
    assert!(
        page.header().is_idle(),
        "the header no longer names a running task:\n{}",
        page.screen()
    );
    assert_eq!(
        page.task_panel().active_marker_index(),
        None,
        "no row carries the active marker:\n{}",
        page.screen()
    );
    assert_eq!(page.task_panel().selected_index(), Some(0));

    tt.quit().assert_clean_exit();

    let worklogs = database.worklogs_for_task("Write release notes");
    assert_eq!(worklogs.len(), 1, "one worklog was recorded");
    let closed = &worklogs[0];
    assert_eq!(
        closed.id, active.id,
        "stopping closed the worklog the start opened"
    );
    assert_eq!(
        closed.start, active.start,
        "the start timestamp survived the stop"
    );
    assert!(
        closed.end.is_some(),
        "stopping closed the worklog: {closed:?}"
    );
    assert_eq!(
        database.active_worklog(),
        None,
        "nothing tracks after the stop"
    );
}

#[test]
fn switching_tasks_moves_the_marker_and_shares_one_boundary_timestamp() {
    let context = TestContext::new();
    let mut tt = context.launch();
    tt.wait_for_first_frame("the first frame", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().is_idle() && page.task_panel().task_names().len() == 3
    });
    let database = context.database();

    tt.press_and_wait(Key::Char(' '), "the running header", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header()
            .active_task()
            .is_some_and(|active| active.name() == "Write release notes")
    });
    let old = database
        .active_worklog()
        .expect("the started worklog is stored");
    tt.press_and_wait(Key::Char('j'), "the second task's selection", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .selected_index()
            == Some(1)
    });

    let page = tt.press_and_wait(Key::Char(' '), "the switched header", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header()
            .active_task()
            .is_some_and(|active| active.name() == "Fix the coffee machine")
            && page.task_panel().active_marker_index() == Some(0)
            && page.task_panel().row(0).name() == "Fix the coffee machine"
            && !page.task_panel().row(1).has_active_marker()
            && page.status_bar().text() == "Switched to \"Fix the coffee machine\""
    });
    assert_eq!(
        page.header()
            .active_task()
            .expect("the header names the new task")
            .name(),
        "Fix the coffee machine"
    );
    assert_eq!(
        page.task_panel().active_marker_index(),
        Some(0),
        "the marker follows the tracked task to its new row:\n{}",
        page.screen()
    );
    assert_eq!(page.task_panel().selected_index(), Some(0));

    tt.quit().assert_clean_exit();

    let stopped = database.worklogs_for_task("Write release notes");
    let started = database.worklogs_for_task("Fix the coffee machine");
    assert_eq!(stopped.len(), 1, "the old task has one stopped worklog");
    assert_eq!(started.len(), 1, "the new task has one active worklog");
    assert_eq!(
        stopped[0].id, old.id,
        "switching stopped the worklog the start opened"
    );
    assert_eq!(
        stopped[0].start, old.start,
        "the old worklog kept its start timestamp"
    );
    assert!(
        stopped[0].end.is_some(),
        "the old worklog stopped: {:?}",
        stopped[0]
    );
    assert!(
        started[0].end.is_none(),
        "the new worklog is active: {:?}",
        started[0]
    );
    assert_ne!(started[0].id, old.id, "the switch opened a fresh worklog");
    // The stop and the new start landing on one shared boundary timestamp
    // is what the switch does to the row pair; it is a timestamp the two
    // rows agree on, not proof that the two writes committed atomically.
    assert_eq!(
        stopped[0].end,
        Some(started[0].start),
        "the stop and the new start share one boundary timestamp"
    );
    assert_eq!(
        database.active_worklog().map(|worklog| worklog.id),
        Some(started[0].id),
        "the new worklog is the one active worklog"
    );
    assert_eq!(
        database.active_worklog_task_name().as_deref(),
        Some("Fix the coffee machine")
    );
}

#[test]
fn restarting_a_task_opens_a_second_distinct_worklog() {
    let context = TestContext::new();
    let mut tt = context.launch();
    tt.wait_for_first_frame("the first frame", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().is_idle() && page.task_panel().task_names().len() == 3
    });
    let database = context.database();

    tt.press_and_wait(Key::Char(' '), "the running header", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header()
            .active_task()
            .is_some_and(|active| active.name() == "Write release notes")
    });
    let first = database
        .active_worklog()
        .expect("the first started worklog is stored");
    tt.press_and_wait(Key::Char(' '), "the stopped status", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().is_idle() && page.status_bar().text() == "Stopped \"Write release notes\""
    });
    let page = tt.press_and_wait(Key::Char(' '), "the running header again", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header()
            .active_task()
            .is_some_and(|active| active.name() == "Write release notes")
            && page.task_panel().active_marker_index() == Some(0)
    });
    assert!(
        page.header().active_task().is_some(),
        "the same task tracks again:\n{}",
        page.screen()
    );

    tt.quit().assert_clean_exit();

    let worklogs = database.worklogs_for_task("Write release notes");
    assert_eq!(worklogs.len(), 2, "each start recorded its own worklog");
    let stopped = worklogs
        .iter()
        .find(|worklog| worklog.id == first.id)
        .expect("the first worklog is stored");
    assert_eq!(
        stopped.start, first.start,
        "the first worklog kept its start timestamp"
    );
    assert!(
        stopped.end.is_some(),
        "the first worklog stopped: {stopped:?}"
    );
    let active = database
        .active_worklog()
        .expect("the restart left one active worklog");
    assert_ne!(
        active.id, first.id,
        "the restart opened a different worklog"
    );
    assert_eq!(active.task_name, "Write release notes");
    assert!(
        active.end.is_none(),
        "the second worklog is active: {active:?}"
    );
}

#[test]
fn an_active_timer_recovers_after_a_relaunch() {
    let context = TestContext::new();
    let mut tt = context.launch();
    tt.wait_for_first_frame("the first frame", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().is_idle() && page.task_panel().task_names().len() == 3
    });
    let database = context.database();

    tt.press_and_wait(Key::Char(' '), "the running header", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header()
            .active_task()
            .is_some_and(|active| active.name() == "Write release notes")
    });

    // Quitting must not stop the timer.
    tt.quit().assert_clean_exit();
    let baseline = database
        .active_worklog()
        .expect("the active worklog is stored");
    assert_eq!(baseline.task_name, "Write release notes");
    assert!(
        baseline.end.is_none(),
        "the worklog stayed active across the quit: {baseline:?}"
    );

    let mut relaunched = context.launch();
    let page = relaunched.wait_for_first_frame("the recovered frame", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().active_task().is_some_and(|active| {
            active.name() == "Write release notes" && active.elapsed_is_hhmmss()
        }) && page.task_panel().active_marker_index() == Some(0)
            && page.status_bar().text() == "Recovered the previous active timer"
    });
    assert_eq!(
        page.header()
            .active_task()
            .expect("the header names the recovered task")
            .name(),
        "Write release notes"
    );
    assert_eq!(
        page.task_panel().active_marker_index(),
        Some(0),
        "the marker recovered onto the tracked task:\n{}",
        page.screen()
    );
    assert_eq!(
        page.status_bar().text(),
        "Recovered the previous active timer"
    );

    relaunched.quit().assert_clean_exit();

    let after = database
        .active_worklog()
        .expect("the active worklog survived the relaunch");
    assert_eq!(after.id, baseline.id, "the same worklog stayed active");
    assert_eq!(
        after.start, baseline.start,
        "the recovery kept the original start timestamp"
    );
    assert!(
        after.end.is_none(),
        "the recovered worklog is still active: {after:?}"
    );
}
