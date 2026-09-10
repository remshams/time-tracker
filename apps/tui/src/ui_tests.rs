use chrono::{DateTime, Utc};
use ratatui::{Terminal, backend::TestBackend};
use tracker_application::TrackerApplication;
use tracker_domain::{Task, TaskId, TaskName};
use tracker_storage::SqliteRepository;

use crate::app::App;

const HEIGHT: u16 = 24;

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

fn draw_at(
    app: &App<TrackerApplication<SqliteRepository>>,
    width: u16,
    height: u16,
) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| crate::ui::render(frame, app))
        .unwrap();
    terminal
}

fn row(terminal: &Terminal<TestBackend>, y: u16) -> String {
    (0..terminal.backend().buffer().area.width)
        .map(|x| terminal.backend().buffer()[(x, y)].symbol())
        .collect()
}

#[test]
fn a_terminal_below_the_minimum_width_shows_a_resize_message() {
    let app = app_with(&["alpha"]);
    let terminal = draw_at(&app, 59, HEIGHT);
    assert!(row(&terminal, 0).contains("Time Tracker needs at least 60 columns."));
    assert!(!row(&terminal, 1).contains("Active tasks"));
}
