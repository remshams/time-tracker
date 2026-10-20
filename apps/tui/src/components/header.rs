use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::styles;

/// Renders the title line and, when present, the running task timer.
pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    active_task_name: Option<&str>,
    elapsed: Option<&str>,
) {
    let mut spans = vec![Span::styled("Time Tracker", styles::title())];
    if let Some(name) = active_task_name {
        spans.push(Span::raw("  "));
        spans.push(Span::styled("▶", styles::active_marker()));
        spans.push(Span::raw(" "));
        spans.push(Span::raw(name));
        spans.push(Span::raw(format!("  {}", elapsed.unwrap_or_default())));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use tracker_application::TrackerApplication;
    use tracker_domain::{Task, TaskId, TaskName};
    use tracker_storage::SqliteRepository;

    use crate::app::App;
    use crate::test_support::render::{draw, row};

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

    #[test]
    fn an_idle_header_shows_only_the_title() {
        let app = app_with(&["alpha"]);
        let terminal = draw(&app);
        let header = row(&terminal, 0);
        assert!(header.contains("Time Tracker"));
        assert!(!header.contains("▶"));
    }
}
