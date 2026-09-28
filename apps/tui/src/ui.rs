//! Root frame composition for the terminal interface.

use crate::app::AppView;
use crate::components::{header, status, text};
use crate::screens::{self, ScreenState, worklog_history};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::widgets::Paragraph;

/// The narrowest terminal that renders the full interactive interface.
pub(crate) const MIN_TERMINAL_WIDTH: u16 = 60;

/// Renders a frame without pending-request feedback for component tests.
#[cfg(test)]
pub(crate) fn render(frame: &mut Frame, app: AppView<'_>) {
    render_with_busy(frame, app, None);
}

/// Renders one frame, including pending-request feedback in the status row.
pub(crate) fn render_with_busy(frame: &mut Frame, app: AppView<'_>, busy_label: Option<&str>) {
    if frame.area().width < MIN_TERMINAL_WIDTH {
        frame.render_widget(
            Paragraph::new("Time Tracker needs at least 60 columns."),
            frame.area(),
        );
        return;
    }

    let [header_area, body, status_area, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    let elapsed = app.elapsed().map(text::format_elapsed);
    header::render(
        frame,
        header_area,
        app.active_task_name(),
        elapsed.as_deref(),
    );
    match app.screen_state() {
        ScreenState::TaskList(_) => {
            let tasks = app.visible_tasks();
            screens::task_list::view::render(
                frame,
                body,
                app.task_list(),
                &tasks,
                app.ordering_label(),
                app.active_task_id(),
            );
        }
        ScreenState::WorklogHistory(state) => {
            worklog_history::view::render(frame, body, app, state);
        }
        ScreenState::Reports(state) => screens::reports::view::render(frame, body, state),
        ScreenState::AllWorklogs(state) => {
            screens::all_worklogs::view::render(frame, body, app, state)
        }
    }
    status::render(frame, status_area, app.status(), busy_label);
    frame.render_widget(Paragraph::new(app.footer_hints(footer.width)), footer);
}
