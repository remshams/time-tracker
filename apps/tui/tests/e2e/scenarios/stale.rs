//! Stale-process scenarios: two live `tt` processes on one database, where
//! one process's in-memory tracking state lags behind what the other has
//! persisted.
//!
//! Both processes run the real binary against the real SQLite adapter, so
//! every conflict goes through the application service's refresh and
//! recovery paths, not through an injected fake.

use termlens::Key;

use crate::context::TestContext;
use crate::driver::TuiDriver;
use crate::page::TimeTrackerPage;

/// Launches two processes on one context and waits for both first frames.
/// The first launch becomes the stale process: it loads the database
/// before the second one changes anything.
fn launch_stale_and_current(context: &TestContext) -> (TuiDriver, TuiDriver) {
    let mut stale = context.launch();
    stale.wait_for_first_frame("the stale process's first frame", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().is_idle() && page.task_panel().task_names().len() == 3
    });
    let mut current = context.launch();
    current.wait_for_first_frame("the current process's first frame", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().is_idle() && page.task_panel().task_names().len() == 3
    });
    (stale, current)
}

/// Exercises two processes so a stale same-task start adopts the timer without creating work.
#[test]
fn a_stale_same_task_start_rejects_the_command_and_adopts_the_running_worklog() {
    let context = TestContext::new_with_tasks();
    let (mut stale, mut current) = launch_stale_and_current(&context);
    let database = context.database();
    let task = database
        .task_by_name("Write release notes")
        .expect("the tracked task is stored");

    // The fresh process starts tracking the first task. The stale process
    // has seen none of this: its header is still idle.
    current.press_and_wait(Key::Char(' '), "the running header", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header()
            .active_task()
            .is_some_and(|active| active.name() == "Write release notes")
            && page.task_panel().active_marker_index() == Some(0)
    });
    let adopted = database
        .active_worklog()
        .expect("the started worklog is stored");
    assert_eq!(
        adopted.task_id, task.id,
        "the worklog tracks the first task"
    );
    assert_eq!(adopted.task_name, "Write release notes");
    assert!(adopted.end.is_none(), "the worklog is active: {adopted:?}");

    // The captured idle expectation is stale even when the selected task
    // already runs. Reject the command and adopt the authoritative timer.
    let page = stale.press_and_wait(Key::Char(' '), "the adopted timer", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().active_task().is_some_and(|active| {
            active.name() == "Write release notes" && active.elapsed_is_hhmmss()
        }) && page.task_panel().active_marker_index() == Some(0)
            && page.status_bar().text()
                == "Error: Tracking state changed in another client. Refreshed state."
            && page.status_bar().is_error()
            && page.status_bar().label_is_accented()
    });
    assert!(
        page.header()
            .active_task()
            .expect("the header names the running task")
            .elapsed_is_hhmmss(),
        "the adopted clock runs on the stored start:\n{}",
        page.screen()
    );

    stale.quit().assert_clean_exit();
    current.quit().assert_clean_exit();

    // Adoption changed nothing in storage: one active worklog, same id,
    // same task, same start.
    let active = database.active_worklogs();
    assert_eq!(active.len(), 1, "adopting opened no second worklog");
    assert_eq!(active[0].id, adopted.id);
    assert_eq!(active[0].task_id, task.id);
    assert_eq!(active[0].start, adopted.start);
    assert_eq!(
        active[0].end, None,
        "the adopted worklog stayed active: {:?}",
        active[0]
    );
    let worklogs = database.worklogs_for_task("Write release notes");
    assert_eq!(worklogs, active);
}

/// Checks that stopping a replaced timer refreshes the display and preserves the newer worklog.
#[test]
fn a_stale_stop_refreshes_without_stopping_the_newer_worklog() {
    let context = TestContext::new_with_tasks();
    let (mut stale, mut current) = launch_stale_and_current(&context);
    let database = context.database();
    let first_task = database
        .task_by_name("Write release notes")
        .expect("the first task is stored");
    let second_task = database
        .task_by_name("Fix the coffee machine")
        .expect("the second task is stored");

    // The stale process starts tracking the first task.
    stale.press_and_wait(Key::Char(' '), "the running header", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header()
            .active_task()
            .is_some_and(|active| active.name() == "Write release notes")
    });
    let first = database
        .active_worklog()
        .expect("the started worklog is stored");
    assert_eq!(first.task_id, first_task.id);
    assert!(first.end.is_none(), "the started worklog is active");

    // Reject the second process's stale idle command and await its completed
    // state recovery before it intentionally switches tasks.
    current.press_and_wait(Key::Char(' '), "the refreshed running timer", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_active_tasks()
            && page
                .header()
                .active_task()
                .is_some_and(|active| active.name() == "Write release notes")
            && page.task_panel().active_marker_index() == Some(0)
            && page.status_bar().text()
                == "Error: Tracking state changed in another client. Refreshed state."
    });

    // The refreshed process switches to the second task. The stale process's
    // memory is now one worklog behind: it still believes its own worklog
    // is the active one.
    current.press_and_wait(Key::Char('j'), "the second task's selection", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .selected_index()
            == Some(1)
    });
    current.press_and_wait(Key::Char(' '), "the switched header", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header()
            .active_task()
            .is_some_and(|active| active.name() == "Fix the coffee machine")
            && page.task_panel().active_marker_index() == Some(0)
            && page.task_panel().row(0).name() == "Fix the coffee machine"
    });
    let newer = database
        .active_worklog()
        .expect("the switched worklog is stored");
    assert_eq!(newer.task_id, second_task.id);
    assert_ne!(newer.id, first.id, "the switch opened a fresh worklog");

    // The stale process presses space to stop what it believes is its
    // timer. The stored active worklog is no longer the one it remembers,
    // so the stop is refused, the state refreshes from the store, and the
    // newer worklog takes over the header and the marker.
    let page = stale.press_and_wait(Key::Char(' '), "the refresh error", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.status_bar().text()
            == "Error: Tracking state changed in another client. Refreshed state."
            && page.status_bar().is_error()
            && page.status_bar().label_is_accented()
            && page.header().active_task().is_some_and(|active| {
                active.name() == "Fix the coffee machine" && active.elapsed_is_hhmmss()
            })
            && page.task_panel().active_marker_index() == Some(0)
            && page.task_panel().row(0).name() == "Fix the coffee machine"
            && !page.task_panel().row(1).has_active_marker()
    });
    assert_eq!(
        page.status_bar().text(),
        "Error: Tracking state changed in another client. Refreshed state."
    );
    assert!(
        page.status_bar().label_is_accented(),
        "the error label keeps its red bold accent:\n{}",
        page.screen()
    );

    stale.quit().assert_clean_exit();
    current.quit().assert_clean_exit();

    // The refused stop left exactly one active worklog, and the newer
    // worklog kept its task, id, start, and open end unchanged.
    let active = database.active_worklogs();
    assert_eq!(active.len(), 1, "the refused stop stopped nothing");
    assert_eq!(active[0].id, newer.id);
    assert_eq!(active[0].task_id, second_task.id);
    assert_eq!(active[0].start, newer.start);
    assert_eq!(active[0].end, None, "the newer worklog is still active");

    // The older worklog still keeps its task and its original start, and
    // ends exactly at the newer one's start, where the fresh process's
    // switch put it.
    let stopped = database.worklogs_for_task("Write release notes");
    assert_eq!(stopped.len(), 1, "the stale stop recorded no worklog");
    assert_eq!(stopped[0].id, first.id);
    assert_eq!(stopped[0].task_id, first_task.id);
    assert_eq!(stopped[0].start, first.start, "the start was not rewritten");
    assert_eq!(stopped[0].end, Some(newer.start));
}

/// Checks that a command captured while idle cannot switch an unseen running timer.
#[test]
fn a_stale_set_active_rejects_the_command_and_preserves_the_authoritative_timer() {
    let context = TestContext::new_with_tasks();
    let (mut stale, mut current) = launch_stale_and_current(&context);
    let database = context.database();
    let task_a = database
        .task_by_name("Write release notes")
        .expect("the first task is stored");
    let task_c = database
        .task_by_name("Plan Friday's demo")
        .expect("the third task is stored");

    // The fresh process starts tracking the first task. The stale process
    // still believes the tracker idle.
    current.press_and_wait(Key::Char(' '), "the running header", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header()
            .active_task()
            .is_some_and(|active| active.name() == "Write release notes")
    });
    let started = database
        .active_worklog()
        .expect("the started worklog is stored");
    assert_eq!(started.task_id, task_a.id);
    assert!(started.end.is_none(), "the started worklog is active");

    // A direct snapshot of the stale process before its action proves it
    // still renders the idle state it loaded at startup: no running
    // header, no marker on any row.
    let stale_view = stale.page();
    assert!(
        stale_view.header().is_idle(),
        "the stale process still renders idle:\n{}",
        stale_view.screen()
    );
    assert_eq!(
        stale_view.task_panel().active_marker_index(),
        None,
        "the stale process shows no running marker:\n{}",
        stale_view.screen()
    );

    // The stale process selected another task while it still showed idle.
    // Its captured idle expectation must prevent an unintended switch.
    stale.press_and_wait(Key::Char('j'), "the second task's selection", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .selected_index()
            == Some(1)
    });
    stale.press_and_wait(Key::Char('j'), "the third task's selection", |screen| {
        TimeTrackerPage::new(screen.clone())
            .task_panel()
            .selected_index()
            == Some(2)
    });
    let page = stale.press_and_wait(Key::Char(' '), "the rejected stale start", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.header().active_task().is_some_and(|active| {
            active.name() == "Write release notes" && active.elapsed_is_hhmmss()
        }) && page.task_panel().active_marker_index() == Some(0)
            && page.task_panel().row(0).name() == "Write release notes"
            && !page.task_panel().row(1).has_active_marker()
            && !page.task_panel().row(2).has_active_marker()
            && page.status_bar().text()
                == "Error: Tracking state changed in another client. Refreshed state."
            && page.status_bar().is_error()
            && page.status_bar().label_is_accented()
    });
    assert_eq!(
        page.status_bar().text(),
        "Error: Tracking state changed in another client. Refreshed state."
    );
    assert_eq!(page.task_panel().selected_index(), Some(2));

    stale.quit().assert_clean_exit();
    current.quit().assert_clean_exit();

    // The rejected command leaves the original timer unchanged and creates
    // no worklog for the selected destination task.
    let active = database.active_worklogs();
    assert_eq!(active.as_slice(), std::slice::from_ref(&started));
    assert_eq!(active[0].task_id, task_a.id);
    assert_eq!(database.worklogs_for_task("Write release notes"), active);
    assert!(database.worklogs_for_task(&task_c.name).is_empty());
}
