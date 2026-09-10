//! SQLite persistence for the Time Tracker.
//!
//! [`SqliteRepository`] implements the repository ports from
//! `tracker-application` with a bundled SQLite database. The schema is created and upgraded by
//! migrations, and database-level rules back the domain invariants: at most
//! one active worklog, no stopped worklog whose end precedes its start, no
//! same-task interval overlap, no worklogs on archived tasks, and no archiving
//! of a task with an active worklog. Task timestamps are stored as strict
//! integer microseconds, and `updated_at` never moves backward. This crate
//! depends on the application and domain crates, never the other way around.

mod error;
mod migrate;
mod paths;
mod sqlite;

pub use error::StorageError;
pub use paths::{app_data_dir, default_database_path, ensure_app_data_dir};
pub use sqlite::SqliteRepository;
pub(crate) use sqlite::timestamp_to_us;
