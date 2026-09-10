//! SQLite repository implementation.

mod mapping;
mod tasks;
mod tracking;
mod worklogs;

use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags};
use tracker_application::{
    RepositoryError, TaskRepository, TrackerSnapshot, TrackingRepository, WorklogCorrection,
    WorklogCursor, WorklogDeletion, WorklogPage, WorklogRepository,
};
use tracker_domain::{Task, TaskId, TaskName, Worklog, WorklogId, WorklogTimes};

use crate::{StorageError, error, migrate, paths};

pub(crate) use self::mapping::timestamp_to_us;

/// How long a connection waits for a database locked by another process
/// before giving up.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// A tracker repository backed by a bundled SQLite database.
#[derive(Debug)]
pub struct SqliteRepository {
    conn: Connection,
}

impl SqliteRepository {
    /// Opens the database at the given path, creating the file and running
    /// pending migrations.
    ///
    /// A missing file is created with owner-only permissions. An existing
    /// file must be a regular file owned by the current user; a symbolic
    /// link is rejected, and permissions of an owned file are repaired to
    /// owner-only. The file itself is opened with `SQLITE_OPEN_NOFOLLOW`,
    /// so a link swapped in later cannot be followed. Parent directories
    /// must already exist; use [`crate::ensure_app_data_dir`] for the standard
    /// database location.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let path = path.as_ref();
        paths::prepare_database_file(path)?;
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE.union(OpenFlags::SQLITE_OPEN_NOFOLLOW);
        let conn = Connection::open_with_flags(path, flags)?;
        Self::prepare(conn)
    }

    /// Opens a private in-memory database, mainly for tests.
    pub fn open_in_memory() -> Result<Self, StorageError> {
        let conn = Connection::open_in_memory()?;
        Self::prepare(conn)
    }

    fn prepare(conn: Connection) -> Result<Self, StorageError> {
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.pragma_update(None, "foreign_keys", true)?;
        migrate::migrate(&conn)?;
        Ok(Self { conn })
    }

    /// Gives direct access to the SQLite connection for diagnostics and
    /// schema-level tests.
    ///
    /// This is not part of the application repository ports. Callers that
    /// only use those ports never need it.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

impl TaskRepository for SqliteRepository {
    fn create_task(&self, task: Task) -> Result<(), RepositoryError> {
        SqliteRepository::create_task(self, task).map_err(Into::into)
    }

    fn tracker_snapshot(&self) -> Result<TrackerSnapshot, RepositoryError> {
        SqliteRepository::tracker_snapshot(self).map_err(Into::into)
    }

    fn rename_task(
        &self,
        id: TaskId,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, RepositoryError> {
        SqliteRepository::rename_task(self, id, name, occurred_at).map_err(Into::into)
    }

    fn archive_task(
        &self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, RepositoryError> {
        SqliteRepository::archive_task(self, id, occurred_at).map_err(Into::into)
    }

    fn unarchive_task(
        &self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, RepositoryError> {
        SqliteRepository::unarchive_task(self, id, occurred_at).map_err(Into::into)
    }
}

impl TrackingRepository for SqliteRepository {
    fn insert_worklog(&self, worklog: &Worklog) -> Result<(), RepositoryError> {
        SqliteRepository::insert_worklog(self, worklog).map_err(Into::into)
    }

    fn stop_worklog(
        &self,
        id: WorklogId,
        expected_start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Worklog, RepositoryError> {
        SqliteRepository::stop_worklog(self, id, expected_start, end).map_err(Into::into)
    }

    fn switch_worklog(
        &self,
        id: WorklogId,
        expected_start: DateTime<Utc>,
        stop_at: DateTime<Utc>,
        next: &Worklog,
    ) -> Result<(), RepositoryError> {
        SqliteRepository::switch_worklog(self, id, expected_start, stop_at, next)
            .map_err(Into::into)
    }
}

impl WorklogRepository for SqliteRepository {
    fn find_worklog(&self, id: WorklogId) -> Result<Option<Worklog>, RepositoryError> {
        SqliteRepository::find_worklog(self, id).map_err(Into::into)
    }

    fn compare_and_set_worklog_times(
        &self,
        id: WorklogId,
        expected: WorklogTimes,
        replacement: WorklogTimes,
    ) -> Result<WorklogCorrection, RepositoryError> {
        SqliteRepository::compare_and_set_worklog_times(self, id, expected, replacement)
            .map_err(Into::into)
    }

    fn compare_and_delete_completed_worklog(
        &self,
        id: WorklogId,
        expected_task_id: TaskId,
        expected: WorklogTimes,
    ) -> Result<WorklogDeletion, RepositoryError> {
        SqliteRepository::compare_and_delete_completed_worklog(self, id, expected_task_id, expected)
            .map_err(Into::into)
    }

    fn worklog_page(
        &self,
        task_id: TaskId,
        after: Option<&WorklogCursor>,
    ) -> Result<WorklogPage, RepositoryError> {
        SqliteRepository::worklog_page(self, task_id, after).map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::mapping::{
        task_from_stored, task_id_from_stored, timestamp_to_us, us_to_timestamp,
        worklog_from_stored, worklog_id_from_stored,
    };
    use super::*;

    #[test]
    fn a_history_page_reports_the_active_tasks_authoritative_aggregate() {
        let repository = SqliteRepository::open_in_memory().unwrap();
        let task = Task::create(
            TaskId::generate(),
            TaskName::new("active").unwrap(),
            DateTime::from_timestamp(100, 0).unwrap(),
        );
        repository.create_task(task.clone()).unwrap();
        let active = Worklog::begin(
            WorklogId::generate(),
            task.id(),
            DateTime::from_timestamp(200, 0).unwrap(),
        );
        repository.insert_worklog(&active).unwrap();

        let page = repository.worklog_page(task.id(), None).unwrap();

        assert_eq!(page.snapshot.active_worklog, Some(active));
        assert_eq!(
            page.snapshot.requested_task_latest_work_start,
            Some(DateTime::from_timestamp(200, 0).unwrap())
        );
        assert_eq!(
            page.snapshot.active_task_latest_work_start,
            page.snapshot.requested_task_latest_work_start
        );
    }

    #[test]
    fn timestamp_round_trips_through_microseconds() {
        let time = DateTime::from_timestamp(1_700_000_000, 123_456_000).unwrap();
        assert_eq!(us_to_timestamp(timestamp_to_us(time)).unwrap(), time);
    }

    #[test]
    fn out_of_range_microseconds_are_corrupt_data() {
        assert!(matches!(
            us_to_timestamp(i64::MAX),
            Err(StorageError::CorruptData("timestamp"))
        ));
    }

    #[test]
    fn identifiers_round_trip_through_text() {
        let task_id = TaskId::generate();
        let restored = task_id_from_stored(&task_id.to_string()).unwrap();
        assert_eq!(restored, task_id);
        let worklog_id = WorklogId::generate();
        let restored = worklog_id_from_stored(&worklog_id.to_string()).unwrap();
        assert_eq!(restored, worklog_id);
        assert!(matches!(
            task_id_from_stored("not-a-uuid"),
            Err(StorageError::InvalidId(_))
        ));
    }

    #[test]
    fn stored_task_values_reject_an_empty_name() {
        let error = task_from_stored(
            TaskId::generate().to_string(),
            "   ".to_owned(),
            false,
            0,
            0,
        )
        .expect_err("whitespace name is corrupt");
        assert!(matches!(error, StorageError::CorruptData("task name")));
    }

    #[test]
    fn stored_task_values_reject_control_and_oversized_names() {
        let error = task_from_stored(
            TaskId::generate().to_string(),
            "bad \u{1b} name".to_owned(),
            false,
            0,
            0,
        )
        .expect_err("control characters make a name corrupt");
        assert!(matches!(error, StorageError::CorruptData("task name")));

        let error = task_from_stored(
            TaskId::generate().to_string(),
            "a".repeat(TaskName::MAX_LEN + 1),
            false,
            0,
            0,
        )
        .expect_err("an oversized name is corrupt");
        assert!(matches!(error, StorageError::CorruptData("task name")));
    }

    #[test]
    fn stored_task_values_reject_updated_before_created() {
        let error = task_from_stored(
            TaskId::generate().to_string(),
            "backwards".to_owned(),
            false,
            200,
            199,
        )
        .expect_err("updated before created is corrupt");
        assert!(matches!(
            error,
            StorageError::CorruptData("task timestamps")
        ));
    }

    #[test]
    fn stored_task_values_reject_out_of_range_timestamps() {
        let error = task_from_stored(
            TaskId::generate().to_string(),
            "huge".to_owned(),
            false,
            i64::MAX,
            i64::MAX,
        )
        .expect_err("an unrepresentable timestamp is corrupt");
        assert!(matches!(error, StorageError::CorruptData("timestamp")));
    }

    #[test]
    fn stored_worklog_values_reject_a_backwards_interval() {
        let error = worklog_from_stored(
            WorklogId::generate().to_string(),
            TaskId::generate().to_string(),
            200,
            Some(100),
        )
        .expect_err("end before start is corrupt");
        assert!(matches!(
            error,
            StorageError::CorruptData("worklog interval")
        ));
    }

    #[test]
    fn seeding_an_empty_database_inserts_every_name_with_one_shared_timestamp() {
        let repository = SqliteRepository::open_in_memory().unwrap();
        let names: Vec<TaskName> = ["one", "two"]
            .iter()
            .map(|name| TaskName::new(name).unwrap())
            .collect();
        let created_at = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        assert!(repository.seed_default_tasks(&names, created_at).unwrap());
        let tasks = repository.list_tasks().unwrap();
        let stored: Vec<String> = tasks.iter().map(|task| task.name().to_string()).collect();
        assert_eq!(stored, ["one".to_owned(), "two".to_owned()]);
        for task in &tasks {
            assert_eq!(task.created_at(), created_at);
            assert_eq!(task.updated_at(), created_at);
        }

        // A second call sees a non-empty database and changes nothing.
        assert!(!repository.seed_default_tasks(&names, created_at).unwrap());
        assert_eq!(repository.list_tasks().unwrap().len(), 2);
    }

    #[test]
    fn seeding_canonicalizes_the_timestamp_before_creating_tasks() {
        let repository = SqliteRepository::open_in_memory().unwrap();
        let names = vec![TaskName::new("precise").unwrap()];
        let timestamp = DateTime::from_timestamp(100, 123_456_789).unwrap();

        assert!(repository.seed_default_tasks(&names, timestamp).unwrap());

        let task = repository.list_tasks().unwrap().pop().unwrap();
        let canonical = DateTime::from_timestamp(100, 123_456_000).unwrap();
        assert_eq!(task.created_at(), canonical);
        assert_eq!(task.updated_at(), canonical);
    }

    #[test]
    fn seeding_skips_a_database_that_has_only_archived_tasks() {
        let repository = SqliteRepository::open_in_memory().unwrap();
        let mut task = Task::create(
            TaskId::generate(),
            TaskName::new("mine").unwrap(),
            DateTime::from_timestamp(100, 0).unwrap(),
        );
        assert!(task.archive(DateTime::from_timestamp(100, 0).unwrap()));
        repository.create_task(task).unwrap();
        let names = vec![TaskName::new("default").unwrap()];
        assert!(
            !repository
                .seed_default_tasks(&names, DateTime::from_timestamp(100, 0).unwrap())
                .unwrap()
        );
        assert_eq!(repository.list_tasks().unwrap().len(), 1);
    }

    #[test]
    fn a_failed_seed_leaves_no_partial_tasks() {
        let repository = SqliteRepository::open_in_memory().unwrap();
        // A trigger aborts the insert of the middle name.
        repository
            .connection()
            .execute_batch(
                "CREATE TRIGGER reject_boom BEFORE INSERT ON tasks
                 WHEN NEW.name = 'boom'
                 BEGIN SELECT RAISE(ABORT, 'boom'); END;",
            )
            .unwrap();
        let names: Vec<TaskName> = ["first", "boom", "third"]
            .iter()
            .map(|name| TaskName::new(name).unwrap())
            .collect();
        let error = repository
            .seed_default_tasks(&names, DateTime::from_timestamp(0, 0).unwrap())
            .expect_err("the aborted insert fails the seed");
        assert!(matches!(error, StorageError::Sql(_)));
        assert!(
            repository.list_tasks().unwrap().is_empty(),
            "no partial seed may remain"
        );
        // The connection is usable afterwards: the transaction was rolled
        // back, not left open.
        let fresh = vec![TaskName::new("works").unwrap()];
        assert!(
            repository
                .seed_default_tasks(&fresh, DateTime::from_timestamp(0, 0).unwrap())
                .unwrap()
        );
        assert_eq!(repository.list_tasks().unwrap().len(), 1);
    }

    #[test]
    fn seeding_serializes_between_two_connections() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tracker.db");
        let names: Vec<TaskName> = ["alpha", "beta"]
            .iter()
            .map(|name| TaskName::new(name).unwrap())
            .collect();
        let created_at = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let first = std::thread::spawn({
            let path = path.clone();
            let names = names.clone();
            move || -> Result<bool, StorageError> {
                SqliteRepository::open(&path)?.seed_default_tasks(&names, created_at)
            }
        });
        let second = {
            let path = path.clone();
            let names = names.clone();
            std::thread::spawn(move || -> Result<bool, StorageError> {
                SqliteRepository::open(&path)?.seed_default_tasks(&names, created_at)
            })
        };
        let first = first.join().unwrap().unwrap();
        let second = second.join().unwrap().unwrap();
        assert!(
            first ^ second,
            "exactly one of two concurrent seeds must seed, got {first} and {second}"
        );
        let repository = SqliteRepository::open(&path).unwrap();
        let stored: Vec<String> = repository
            .list_tasks()
            .unwrap()
            .into_iter()
            .map(|task| task.name().to_string())
            .collect();
        assert_eq!(stored, ["alpha".to_owned(), "beta".to_owned()]);
    }

    #[test]
    fn connections_wait_for_each_other_within_the_busy_timeout() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tracker.db");
        let writer = SqliteRepository::open(&path).unwrap();
        writer
            .create_task(Task::create(
                TaskId::generate(),
                TaskName::new("held").unwrap(),
                DateTime::from_timestamp(100, 0).unwrap(),
            ))
            .unwrap();
        // A second connection on the same file can read and write while the
        // first is open; SQLite serializes through the busy timeout.
        let reader = SqliteRepository::open(&path).unwrap();
        assert_eq!(reader.list_tasks().unwrap().len(), 1);
        reader
            .create_task(Task::create(
                TaskId::generate(),
                TaskName::new("next").unwrap(),
                DateTime::from_timestamp(100, 0).unwrap(),
            ))
            .unwrap();
        assert_eq!(writer.list_tasks().unwrap().len(), 2);
    }
}
