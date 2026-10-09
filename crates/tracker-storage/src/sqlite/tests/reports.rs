use super::*;
use chrono::TimeDelta;
use tracker_application::ReportQueries;

struct ReportPause {
    paused: AtomicBool,
    reached: SyncSender<()>,
    release: Mutex<Receiver<()>>,
}

unsafe extern "C" fn pause_after_report_query(
    event: c_uint,
    context: *mut c_void,
    statement: *mut c_void,
    _: *mut c_void,
) -> c_int {
    if event != rusqlite::ffi::SQLITE_TRACE_PROFILE {
        return rusqlite::ffi::SQLITE_OK;
    }
    let sql = unsafe { rusqlite::ffi::sqlite3_sql(statement.cast()) };
    if sql.is_null()
        || !unsafe { CStr::from_ptr(sql) }
            .to_bytes()
            .starts_with(b"SELECT t.id, t.name, t.archived")
    {
        return rusqlite::ffi::SQLITE_OK;
    }
    let pause = unsafe { &*(context.cast::<ReportPause>()) };
    if !pause.paused.swap(true, Ordering::SeqCst) {
        let _ = pause.reached.send(());
        let _ = pause
            .release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5));
    }
    rusqlite::ffi::SQLITE_OK
}

#[test]
fn task_list_uses_indexed_latest_work_probes() {
    let repository = repo();
    let plan: Vec<String> = repository
        .connection()
        .prepare(&format!(
            "EXPLAIN QUERY PLAN {}",
            super::super::tasks::TASK_ITEMS_SQL
        ))
        .unwrap()
        .query_map([], |row| row.get(3))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();

    assert!(
        plan.iter()
            .any(|detail| { detail.contains("SEARCH w USING COVERING INDEX worklogs_task_start") })
    );
    assert!(!plan.iter().any(|detail| detail.contains("SCAN worklogs")));
}

#[test]
fn report_counts_overlap_for_active_and_archived_tasks() {
    let repository = repo();
    let alpha = named_task(1, "alpha");
    let beta = named_task(2, "beta");
    let unused = named_task(3, "unused");
    for task in [&alpha, &beta, &unused] {
        repository.create_task(task.clone()).unwrap();
    }
    repository
        .insert_worklog(&Worklog::new(worklog_id(1), alpha.id(), at(90), Some(at(120))).unwrap())
        .unwrap();
    repository
        .insert_worklog(&Worklog::new(worklog_id(2), alpha.id(), at(130), Some(at(130))).unwrap())
        .unwrap();
    repository
        .insert_worklog(&Worklog::new(worklog_id(3), beta.id(), at(110), Some(at(120))).unwrap())
        .unwrap();
    repository.archive_task(alpha.id(), at(300)).unwrap();
    repository
        .insert_worklog(&Worklog::begin(worklog_id(4), beta.id(), at(200)))
        .unwrap();
    let mut application = TrackerApplication::load(repository).unwrap();

    let totals = application
        .report_totals(at(100), at(300), at(210))
        .unwrap();

    assert_eq!(totals.total, TimeDelta::seconds(40));
    assert_eq!(totals.rows.len(), 2);
    assert_eq!(totals.rows[0].task.id(), alpha.id());
    assert!(totals.rows[0].task.is_archived());
    assert_eq!(totals.rows[0].duration, TimeDelta::seconds(20));
    assert_eq!(totals.rows[1].task.id(), beta.id());
    assert_eq!(totals.rows[1].duration, TimeDelta::seconds(20));
}

#[test]
fn report_excludes_touching_and_zero_length_worklogs() {
    let repository = repo();
    let task = named_task(1, "bounded");
    repository.create_task(task.clone()).unwrap();
    for (id, start, end) in [
        (1, 50, 100),
        (2, 120, 130),
        (3, 150, 150),
        (4, 190, 220),
        (5, 220, 230),
    ] {
        repository
            .insert_worklog(
                &Worklog::new(worklog_id(id), task.id(), at(start), Some(at(end))).unwrap(),
            )
            .unwrap();
    }
    let mut application = TrackerApplication::load(repository).unwrap();

    let totals = application
        .report_totals(at(100), at(200), at(300))
        .unwrap();

    assert_eq!(totals.total, TimeDelta::seconds(20));
    assert_eq!(totals.rows.len(), 1);
    assert_eq!(totals.rows[0].duration, TimeDelta::seconds(20));
}

#[test]
fn report_keeps_completed_future_work_but_caps_open_work_at_now() {
    let repository = repo();
    let completed_task = named_task(1, "completed");
    let active_task = named_task(2, "active");
    repository.create_task(completed_task.clone()).unwrap();
    repository.create_task(active_task.clone()).unwrap();
    repository
        .insert_worklog(
            &Worklog::new(worklog_id(1), completed_task.id(), at(300), Some(at(320))).unwrap(),
        )
        .unwrap();
    repository
        .insert_worklog(&Worklog::begin(worklog_id(2), active_task.id(), at(305)))
        .unwrap();
    let mut application = TrackerApplication::load(repository).unwrap();

    let totals = application
        .report_totals(at(300), at(330), at(310))
        .unwrap();

    assert_eq!(totals.total, TimeDelta::seconds(25));
    assert_eq!(totals.rows[0].task.id(), completed_task.id());
    assert_eq!(totals.rows[0].duration, TimeDelta::seconds(20));
    assert_eq!(totals.rows[1].task.id(), active_task.id());
    assert_eq!(totals.rows[1].duration, TimeDelta::seconds(5));

    let future = application
        .report_totals(at(300), at(330), at(290))
        .unwrap();
    assert_eq!(future.total, TimeDelta::seconds(20));
    assert_eq!(future.rows.len(), 1);
    assert_eq!(future.rows[0].task.id(), completed_task.id());
}

#[test]
fn report_adopts_changes_written_by_another_connection() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let repository = SqliteRepository::open(&path).unwrap();
    let mut application = TrackerApplication::load(repository).unwrap();
    let writer = SqliteRepository::open(&path).unwrap();
    let task = named_task(1, "added elsewhere");
    writer.create_task(task.clone()).unwrap();
    writer
        .insert_worklog(&Worklog::begin(worklog_id(1), task.id(), at(200)))
        .unwrap();

    let totals = application
        .report_totals(at(100), at(300), at(210))
        .unwrap();

    assert_eq!(totals.rows.len(), 1);
    assert_eq!(totals.rows[0].task.id(), task.id());
    assert_eq!(application.task(task.id()), Some(&task));
    assert!(matches!(
        application.current_tracking(),
        TrackingState::Running { worklog } if worklog.task_id() == task.id()
    ));
}

#[test]
fn report_rows_and_active_tracking_use_one_read_version() {
    assert_report_read_is_coherent(false);
}

#[test]
fn selected_task_list_report_and_tracking_use_one_read_version() {
    assert_report_read_is_coherent(true);
}

fn assert_report_read_is_coherent(include_task_list: bool) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let repository = SqliteRepository::open(&path).unwrap();
    repository
        .connection()
        .pragma_update(None, "journal_mode", "WAL")
        .unwrap();
    let task = named_task(1, "before");
    repository.create_task(task.clone()).unwrap();
    repository
        .insert_worklog(&Worklog::new(worklog_id(1), task.id(), at(100), None).unwrap())
        .unwrap();

    let (reached_send, reached) = sync_channel(1);
    let (release_send, release) = sync_channel(1);
    let pause = ReportPause {
        paused: AtomicBool::new(false),
        reached: reached_send,
        release: Mutex::new(release),
    };
    let pause_ptr = std::ptr::from_ref(&pause).cast_mut().cast();
    assert_eq!(
        unsafe {
            rusqlite::ffi::sqlite3_trace_v2(
                repository.connection().handle(),
                rusqlite::ffi::SQLITE_TRACE_PROFILE,
                Some(pause_after_report_query),
                pause_ptr,
            )
        },
        rusqlite::ffi::SQLITE_OK
    );
    let writer = thread::spawn(move || {
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        let writer = SqliteRepository::open(path).unwrap();
        writer
            .rename_task(task.id(), TaskName::new("after").unwrap(), at(300))
            .unwrap();
        writer
            .stop_worklog(worklog_id(1), at(100), at(320))
            .unwrap();
        release_send.send(()).unwrap();
    });

    let read = if include_task_list {
        let (items, read) = repository
            .task_list_report_read(at(100), at(130), at(130))
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].task.name().as_str(), "before");
        assert_eq!(items[0].latest_work_start, Some(at(100)));
        read
    } else {
        repository.report_read(at(100), at(130), at(130)).unwrap()
    };

    assert_eq!(
        unsafe {
            rusqlite::ffi::sqlite3_trace_v2(
                repository.connection().handle(),
                0,
                None,
                std::ptr::null_mut(),
            )
        },
        rusqlite::ffi::SQLITE_OK
    );
    writer.join().unwrap();
    assert!(pause.paused.load(Ordering::SeqCst));
    assert_eq!(read.rows[0].task.name().as_str(), "before");
    assert!(read.tracking.active_worklog.unwrap().is_active());
    assert_eq!(
        read.tracking.active_task_item.unwrap().task.name().as_str(),
        "before"
    );
    assert_eq!(
        repository
            .find_task(task_id(1))
            .unwrap()
            .unwrap()
            .name()
            .as_str(),
        "after"
    );
}

#[test]
fn report_rejects_a_duration_that_exceeds_signed_microseconds() {
    let repository = repo();
    let task = named_task(1, "long task");
    repository.create_task(task.clone()).unwrap();
    repository
        .insert_worklog(
            &Worklog::new(
                worklog_id(1),
                task.id(),
                DateTime::from_timestamp_micros(-5_000_000_000_000_000_000).unwrap(),
                Some(DateTime::from_timestamp_micros(5_000_000_000_000_000_000).unwrap()),
            )
            .unwrap(),
        )
        .unwrap();
    let mut application = TrackerApplication::load(repository).unwrap();

    assert!(matches!(
        application.report_totals(
            DateTime::from_timestamp_micros(-5_000_000_000_000_000_000).unwrap(),
            DateTime::from_timestamp_micros(5_000_000_000_000_000_000).unwrap(),
            DateTime::from_timestamp_micros(5_000_000_000_000_000_000).unwrap(),
        ),
        Err(ApplicationError::ReportDurationOverflow)
    ));
}

#[test]
fn report_read_ignores_invalid_metadata_of_tasks_without_report_rows() {
    let repository = repo();
    repository.create_task(named_task(1, "reported")).unwrap();
    repository.create_task(named_task(2, "unrelated")).unwrap();
    repository
        .insert_worklog(&Worklog::new(worklog_id(1), task_id(1), at(100), Some(at(120))).unwrap())
        .unwrap();
    repository
        .connection()
        .execute(
            "UPDATE tasks SET name = '' WHERE id = ?1",
            [task_id(2).to_string()],
        )
        .unwrap();

    let read = repository.report_read(at(100), at(130), at(130)).unwrap();

    assert_eq!(read.rows.len(), 1);
    assert_eq!(read.rows[0].task.id(), task_id(1));
    assert_eq!(read.rows[0].duration, TimeDelta::seconds(20));
    assert!(read.tracking.active_worklog.is_none());
    assert!(read.tracking.active_task_item.is_none());
    assert!(repository.load_task_tracking_resources().is_err());
}
