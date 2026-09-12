//! Root frame composition for the terminal interface.

use crate::app::AppView;
use crate::components::{header, status, text};
use crate::screens::{self, ScreenState, worklog_history};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::widgets::Paragraph;

/// The narrowest terminal that renders the full interactive interface.
pub(crate) const MIN_TERMINAL_WIDTH: u16 = 60;

/// Renders one frame of the interface.
pub fn render(frame: &mut Frame, app: AppView<'_>) {
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
            screens::task_list::view::render(
                frame,
                body,
                app.task_list(),
                app.tasks(),
                app.ordering_label(),
                app.active_task_id(),
            );
        }
        ScreenState::WorklogHistory(state) => {
            let history = state.history();
            let rows: Vec<worklog_history::view::Row> = history
                .worklogs()
                .iter()
                .map(|worklog| worklog_history::view::Row {
                    start: app.local_time(worklog.start()),
                    end: worklog.end().map(|end| app.local_time(end)),
                    duration: text::format_elapsed(app.history_row_duration(worklog)),
                })
                .collect();
            let deletion = match state.mode() {
                worklog_history::WorklogHistoryMode::ConfirmDeletion { worklog } => {
                    Some(worklog_history::view::Deletion {
                        start: app.local_time(worklog.start()),
                        end: worklog
                            .end()
                            .map(|end| app.local_time(end))
                            .unwrap_or_else(|| "Running".to_owned()),
                    })
                }
                _ => None,
            };
            worklog_history::view::render(
                frame,
                body,
                state,
                app.history_task_name().unwrap_or("unknown task"),
                &rows,
                deletion.as_ref(),
            );
        }
    }
    status::render(frame, status_area, app.status());
    frame.render_widget(Paragraph::new(app.footer_hints(footer.width)), footer);
}
