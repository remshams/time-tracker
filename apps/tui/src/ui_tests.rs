use chrono::{DateTime, Utc};
use tracker_application::TrackerApplication;
use tracker_domain::{Task, TaskId, TaskName};
use tracker_storage::SqliteRepository;

use crate::app::App;
use crate::test_support::render::{HEIGHT, draw_at, row};

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
fn a_terminal_below_the_minimum_width_shows_a_resize_message() {
    let app = app_with(&["alpha"]);
    let terminal = draw_at(&app, 59, HEIGHT);
    assert!(row(&terminal, 0).contains("Time Tracker needs at least 60 columns."));
    assert!(!row(&terminal, 1).contains("Active │ Archived"));
}
