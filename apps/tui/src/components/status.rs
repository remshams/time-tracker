use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::Status;
use crate::styles;

/// Renders the current status message.
pub(crate) fn render(frame: &mut Frame, area: Rect, status: &Status, busy_label: Option<&str>) {
    let paragraph = match status {
        Status::Empty => Paragraph::new(busy_label.unwrap_or("")),
        Status::Info(text) => Paragraph::new(text.as_str()),
        Status::Error(text) => Paragraph::new(Line::from(vec![
            Span::styled("Error: ", styles::error_label()),
            Span::raw(text),
        ])),
    };
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use ratatui::style::{Color, Modifier};

    use crate::command::Command;
    use crate::screens::TaskListCommand;
    use crate::test_support::app_with;
    use crate::test_support::render::{cell, draw, row};

    #[test]
    fn the_status_line_labels_errors_and_uses_default_colors() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::TaskList(TaskListCommand::OpenAdd));
        app.handle(Command::TaskList(TaskListCommand::Confirm));
        let terminal = draw(&app);
        assert!(row(&terminal, 22).contains("Error: The task name must not be empty"));
        let label = cell(&terminal, 0, 22);
        assert_eq!(label.fg, Some(Color::Red));
        assert!(label.add_modifier.contains(Modifier::BOLD));
        let message = cell(&terminal, 7, 22);
        assert_eq!(message.fg, Some(Color::Reset));
        assert!(message.add_modifier.is_empty());
    }

    #[test]
    fn the_idle_status_line_is_empty() {
        let app = app_with(&["alpha"]);
        let terminal = draw(&app);
        assert!(row(&terminal, 22).trim().is_empty());
        let style = cell(&terminal, 0, 22);
        assert_eq!(style.fg, Some(Color::Reset));
        assert!(style.add_modifier.is_empty());
    }
}
