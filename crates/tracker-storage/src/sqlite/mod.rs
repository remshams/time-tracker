//! SQLite repository implementation.

mod mapping;
mod reports;
mod tasks;
mod tracking;
mod worklogs;

use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags};
use tracker_application::{
    GlobalWorklogCursor, GlobalWorklogPage, InactiveTaskArchive, InactiveTaskPreviewRead,
    InactiveTaskRepository, ReportRead, ReportRepository, RepositoryError, TaskListItem,
    TaskRepository, TrackingRepository, WorklogCorrection, WorklogCursor, WorklogDeletion,
    WorklogMove, WorklogPage, WorklogRepository,
};
use tracker_domain::{InactivityPeriod, Task, TaskId, TaskName, Worklog, WorklogId, WorklogTimes};

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
        let path = paths::prepare_database_file(path.as_ref())?;
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE.union(OpenFlags::SQLITE_OPEN_NOFOLLOW);
        let conn = Connection::open_with_flags(&path, flags)?;
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

    /// Gives test support direct access to the SQLite connection.
    ///
    /// This is not part of the application repository ports.
    #[cfg(any(test, feature = "test-support"))]
    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

impl TaskRepository for SqliteRepository {
    fn load_task_item(&self, id: TaskId) -> Result<Option<TaskListItem>, RepositoryError> {
        tasks::task_item_by_id_on(&self.conn, id).map_err(Into::into)
    }

    fn load_task_catalog(&self) -> Result<Vec<TaskListItem>, RepositoryError> {
        self.list_task_items().map_err(Into::into)
    }

    fn create_task(&self, task: Task) -> Result<(), RepositoryError> {
        SqliteRepository::create_task(self, task).map_err(Into::into)
    }

    fn load_task_tracking_resources(
        &self,
    ) -> Result<(Vec<TaskListItem>, Option<Worklog>), RepositoryError> {
        SqliteRepository::load_task_tracking_resources(self).map_err(Into::into)
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

    fn preview_inactive_tasks(
        &self,
        as_of: DateTime<Utc>,
    ) -> Result<InactiveTaskPreviewRead, RepositoryError> {
        SqliteRepository::preview_inactive_tasks(self, as_of).map_err(Into::into)
    }

    fn archive_inactive_tasks(
        &self,
        expected_ids: &[TaskId],
        as_of: DateTime<Utc>,
    ) -> Result<InactiveTaskArchive, RepositoryError> {
        SqliteRepository::archive_inactive_tasks(self, expected_ids, as_of).map_err(Into::into)
    }
}

impl InactiveTaskRepository for SqliteRepository {
    fn preview_inactive_tasks_with_period(
        &self,
        as_of: DateTime<Utc>,
        period: InactivityPeriod,
    ) -> Result<InactiveTaskPreviewRead, RepositoryError> {
        SqliteRepository::preview_inactive_tasks_with_period(self, as_of, period)
            .map_err(Into::into)
    }

    fn archive_inactive_tasks_with_period(
        &self,
        expected_ids: &[TaskId],
        as_of: DateTime<Utc>,
        period: InactivityPeriod,
    ) -> Result<InactiveTaskArchive, RepositoryError> {
        SqliteRepository::archive_inactive_tasks_with_period(self, expected_ids, as_of, period)
            .map_err(Into::into)
    }
}

impl ReportRepository for SqliteRepository {
    fn task_list_report_read(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<(Vec<TaskListItem>, ReportRead), RepositoryError> {
        SqliteRepository::task_list_report_read(self, start, end, now).map_err(Into::into)
    }

    fn report_read(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<ReportRead, RepositoryError> {
        SqliteRepository::report_read(self, start, end, now).map_err(Into::into)
    }
}

impl TrackingRepository for SqliteRepository {
    fn active_worklog(&self) -> Result<Option<Worklog>, RepositoryError> {
        SqliteRepository::active_worklog(self).map_err(Into::into)
    }

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
    fn global_worklog_page(
        &self,
        after: Option<&GlobalWorklogCursor>,
    ) -> Result<GlobalWorklogPage, RepositoryError> {
        SqliteRepository::global_worklog_page(self, after).map_err(Into::into)
    }

    fn find_worklog(&self, id: WorklogId) -> Result<Option<Worklog>, RepositoryError> {
        SqliteRepository::find_worklog(self, id).map_err(Into::into)
    }

    fn compare_and_move_worklog(
        &self,
        id: WorklogId,
        expected_source_task_id: TaskId,
        expected: WorklogTimes,
        destination_task_id: TaskId,
    ) -> Result<WorklogMove, RepositoryError> {
        SqliteRepository::compare_and_move_worklog(
            self,
            id,
            expected_source_task_id,
            expected,
            destination_task_id,
        )
        .map_err(Into::into)
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
mod tests;

#[cfg(test)]
mod unit_tests {
    use super::mapping::{
        task_from_stored, task_id_from_stored, timestamp_to_us, us_to_timestamp,
        worklog_from_stored, worklog_id_from_stored,
    };
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_database_below_a_symlinked_ancestor_opens_through_a_physical_path() {
        let temp = tempfile::tempdir().unwrap();
        let real = temp.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let repository = SqliteRepository::open(link.join("tracker.db")).unwrap();

        assert!(repository.list_tasks().unwrap().is_empty());
        assert!(real.join("tracker.db").is_file());
    }

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

        assert_eq!(page.snapshot.as_ref().unwrap().active_worklog, Some(active));
        assert_eq!(
            page.snapshot
                .as_ref()
                .unwrap()
                .requested_task_latest_work_start,
            Some(DateTime::from_timestamp(200, 0).unwrap())
        );
        assert_eq!(
            page.snapshot
                .as_ref()
                .unwrap()
                .active_task_latest_work_start,
            page.snapshot
                .as_ref()
                .unwrap()
                .requested_task_latest_work_start
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
