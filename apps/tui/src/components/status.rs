use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::Status;
use crate::styles;

/// Renders the current status message.
pub(crate) fn render(frame: &mut Frame, area: Rect, status: &Status) {
    let paragraph = match status {
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
    use chrono::{DateTime, Utc};
    use ratatui::style::{Color, Modifier};
    use ratatui::{Terminal, backend::TestBackend};
    use tracker_application::TrackerApplication;
    use tracker_domain::{Task, TaskId, TaskName};
    use tracker_storage::SqliteRepository;

    use crate::app::App;
    use crate::command::Command;

    fn app_with(names: &[&str]) -> App<TrackerApplication<SqliteRepository>> {
        let repository = SqliteRepository::open_in_memory().unwrap();
        for name in names {
            repository
                .create_task(Task::create(
                    TaskId::generate(),
                    TaskName::new(name).unwrap(),
                    DateTime::<Utc>::from_timestamp(100, 0).unwrap(),
                ))
                .unwrap();
        }
        App::load(TrackerApplication::load(repository).unwrap())
    }

    fn row(terminal: &Terminal<TestBackend>, y: u16) -> String {
        (0..terminal.backend().buffer().area.width)
            .map(|x| terminal.backend().buffer()[(x, y)].symbol())
            .collect()
    }

    fn cell(terminal: &Terminal<TestBackend>, x: u16, y: u16) -> ratatui::style::Style {
        terminal.backend().buffer()[(x, y)].style()
    }

    #[test]
    fn the_status_line_labels_errors_and_uses_default_colors() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::OpenAdd);
        app.handle(Command::Confirm);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| crate::ui::render(frame, &app))
            .unwrap();
        assert!(row(&terminal, 22).contains("Error: The task name must not be empty"));
        let label = cell(&terminal, 0, 22);
        assert_eq!(label.fg, Some(Color::Red));
        assert!(label.add_modifier.contains(Modifier::BOLD));
        let message = cell(&terminal, 7, 22);
        assert_eq!(message.fg, Some(Color::Reset));
        assert!(message.add_modifier.is_empty());
    }

    #[test]
    fn the_status_line_shows_info_in_the_terminal_foreground() {
        let app = app_with(&["alpha"]);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| crate::ui::render(frame, &app))
            .unwrap();
        let style = cell(&terminal, 0, 22);
        assert_eq!(style.fg, Some(Color::Reset));
        assert!(style.add_modifier.is_empty());
    }
}
