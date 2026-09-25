//! Task tabs and worklog breadcrumb scenarios.

use termlens::Key;

use crate::context::TestContext;
use crate::page::TimeTrackerPage;

#[test]
fn task_tabs_and_worklog_breadcrumb_keep_the_source_view_visible() {
    let context = TestContext::new();
    {
        let database = context.database();
        database.create_task("Build tools");
        database.create_task("Past project");
        database.archive_task("Past project");
    }

    let mut tt = context.launch();
    tt.wait_for_first_frame("the active tab", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_active_tasks()
            && page.task_panel().selected_tab_is_highlighted()
            && page.task_panel().task_names() == ["Build tools".to_owned()]
    });
    tt.resize_and_wait(60, 20, "the compact active tab", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.size() == (60, 20)
            && page.task_panel().shows_active_tasks()
            && page.task_panel().selected_tab_is_highlighted()
            && page.task_panel().frame_corners_fit_current_geometry()
            && page.task_panel().shows_ordering("recently worked")
    });

    tt.press_and_wait(Key::Enter, "the active worklog breadcrumb", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        let history = page.worklog_history_panel();
        history.is_shown()
            && history.source_view() == Some("Active")
            && history.task_name() == "Build tools"
            && page.footer().text().contains("esc back")
    });
    tt.press_and_wait(Key::Esc, "the restored active tab", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_active_tasks()
            && page.task_panel().selected_tab_is_highlighted()
            && page.task_panel().selected_index() == Some(0)
    });

    tt.press_and_wait(Key::Tab, "the archived tab", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_archived_tasks()
            && page.task_panel().selected_tab_is_highlighted()
            && page.task_panel().task_names() == ["Past project".to_owned()]
    });
    tt.press_and_wait(Key::Enter, "the archived worklog breadcrumb", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        let history = page.worklog_history_panel();
        history.is_shown()
            && history.source_view() == Some("Archived")
            && history.task_name() == "Past project"
    });
    tt.press_and_wait(Key::Esc, "the restored archived tab", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_archived_tasks()
            && page.task_panel().selected_tab_is_highlighted()
            && page.task_panel().selected_index() == Some(0)
    });
    tt.press_and_wait(Key::BackTab, "the active tab again", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_active_tasks() && page.task_panel().selected_tab_is_highlighted()
    });
    tt.quit().assert_clean_exit();
}

#[test]
fn long_task_names_keep_the_worklog_breadcrumb_visible_at_sixty_columns() {
    let context = TestContext::new();
    let name = "Prepare the complete customer migration plan for the autumn review";
    context.database().create_task(name);

    let mut tt = context.launch();
    tt.wait_for_first_frame("the selected task", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.task_panel().shows_active_tasks() && page.task_panel().selected_index() == Some(0)
    });
    tt.resize_and_wait(60, 20, "the compact task panel", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        page.size() == (60, 20)
            && page.task_panel().shows_active_tasks()
            && page.task_panel().frame_corners_fit_current_geometry()
    });
    let page = tt.press_and_wait(Key::Enter, "the shortened breadcrumb", |screen| {
        let page = TimeTrackerPage::new(screen.clone());
        let history = page.worklog_history_panel();
        history.is_shown()
            && history.source_view() == Some("Active")
            && history.task_name().starts_with("Prepare the complete")
            && history.task_name().ends_with('…')
            && history.title().ends_with(" › Worklogs")
    });
    assert!(
        page.worklog_history_panel()
            .title()
            .ends_with(" › Worklogs")
    );
    tt.quit().assert_clean_exit();
}
