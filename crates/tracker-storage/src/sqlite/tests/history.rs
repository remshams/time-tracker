// SQLite history integration tests.

use super::*;

#[test]
fn worklog_pages_walk_55_records_through_the_next_cursor_without_repeats_or_gaps() {
    let repository = repo();
    let task = named_task(1, "history");
    repository.create_task(task.clone()).unwrap();
    insert_numbered_worklogs(&repository, &task, 55);

    // The first page is bounded by the page size and ordered start
    // descending, newest first.
    let first = repository.worklog_page(task.id(), None).unwrap();
    assert_eq!(first.worklogs.len(), WORKLOG_PAGE_SIZE);
    let starts: Vec<i64> = first
        .worklogs
        .iter()
        .map(|worklog| worklog.start().timestamp())
        .collect();
    assert_eq!(starts, (6..=55).rev().collect::<Vec<_>>());
    let cursor = first.next_cursor.expect("more history follows");
    assert_eq!(cursor.id, worklog_id(6));

    // The second page carries the remaining five records, and the history
    // ends inside it.
    let second = repository.worklog_page(task.id(), Some(&cursor)).unwrap();
    assert_eq!(second.worklogs.len(), 5);
    assert_eq!(second.next_cursor, None);
    let rest_starts: Vec<i64> = second
        .worklogs
        .iter()
        .map(|worklog| worklog.start().timestamp())
        .collect();
    assert_eq!(rest_starts, [5, 4, 3, 2, 1]);

    // The two pages cover every record exactly once: none skipped, none
    // repeated. The active worklog is part of the history.
    let mut ids: Vec<u32> = first
        .worklogs
        .iter()
        .chain(&second.worklogs)
        .map(|worklog| worklog.id().as_uuid().as_u128() as u32)
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, (1..=55).collect::<Vec<_>>());
    assert_eq!(
        first.worklogs[0].end(),
        None,
        "the newest worklog is active"
    );
    assert_eq!(
        second.worklogs.last().unwrap().end(),
        Some(at(2)),
        "the oldest worklog is completed"
    );
}

#[test]
fn a_history_of_exactly_one_page_has_no_continuation_cursor() {
    let repository = repo();
    let task = named_task(1, "full page");
    repository.create_task(task.clone()).unwrap();
    insert_numbered_worklogs(&repository, &task, WORKLOG_PAGE_SIZE as u32);

    let page = repository.worklog_page(task.id(), None).unwrap();

    assert_eq!(page.worklogs.len(), WORKLOG_PAGE_SIZE);
    assert_eq!(page.next_cursor, None);
}

#[test]
fn task_and_global_pages_follow_corrected_starts_when_uuid_order_differs() {
    let repository = repo();
    let task = named_task(1, "corrected history");
    repository.create_task(task.clone()).unwrap();
    for tag in 1..=55 {
        let start = i64::from(tag) * 10;
        repository
            .insert_worklog(
                &Worklog::new(worklog_id(tag), task.id(), at(start), Some(at(start + 1))).unwrap(),
            )
            .unwrap();
    }
    let corrected = repository.find_worklog(worklog_id(1)).unwrap().unwrap();
    repository
        .compare_and_set_worklog_times(
            corrected.id(),
            corrected.times(),
            WorklogTimes::new(at(1_000), Some(at(1_001))),
        )
        .unwrap();

    let first = repository.worklog_page(task.id(), None).unwrap();
    let expected = std::iter::once(worklog_id(1))
        .chain((7..=55).rev().map(worklog_id))
        .collect::<Vec<_>>();
    assert_eq!(
        first.worklogs.iter().map(Worklog::id).collect::<Vec<_>>(),
        expected
    );
    let cursor = first.next_cursor.unwrap();
    assert_eq!(cursor.id, worklog_id(7));
    let second = repository.worklog_page(task.id(), Some(&cursor)).unwrap();
    assert_eq!(
        second.worklogs.iter().map(Worklog::id).collect::<Vec<_>>(),
        (2..=6).rev().map(worklog_id).collect::<Vec<_>>()
    );
    assert!(second.next_cursor.is_none());
    let global_first = repository.global_worklog_page(None).unwrap();
    assert_eq!(
        global_first
            .worklogs
            .iter()
            .map(Worklog::id)
            .collect::<Vec<_>>(),
        expected
    );
    let global_second = repository
        .global_worklog_page(global_first.next_cursor.as_ref())
        .unwrap();
    assert_eq!(
        global_second
            .worklogs
            .iter()
            .map(Worklog::id)
            .collect::<Vec<_>>(),
        (2..=6).rev().map(worklog_id).collect::<Vec<_>>()
    );
    assert!(global_second.next_cursor.is_none());
}

#[test]
fn an_active_worklog_is_part_of_a_history_page() {
    let repository = repo();
    let task = named_task(1, "running");
    let other = named_task(2, "other");
    repository.create_task(task.clone()).unwrap();
    repository.create_task(other.clone()).unwrap();
    let stopped = Worklog::new(worklog_id(1), task.id(), at(100), Some(at(150))).unwrap();
    repository.insert_worklog(&stopped).unwrap();
    let active = Worklog::begin(worklog_id(2), task.id(), at(200));
    repository.insert_worklog(&active).unwrap();
    let other_work = Worklog::new(worklog_id(3), other.id(), at(300), Some(at(350))).unwrap();
    repository.insert_worklog(&other_work).unwrap();

    let page = repository.worklog_page(task.id(), None).unwrap();
    assert_eq!(page.worklogs.len(), 2);
    // History order is start descending, so the active worklog leads.
    assert_eq!(page.worklogs[0].id(), active.id());
    assert_eq!(page.worklogs[0].end(), None);
    assert_eq!(page.worklogs[1].id(), stopped.id());
    assert_eq!(page.next_cursor, None);
    assert_eq!(page.adoption.as_ref().unwrap().active_worklog, Some(active));
    assert_eq!(
        page.adoption
            .as_ref()
            .unwrap()
            .requested_task_latest_work_start,
        Some(at(200))
    );
    assert_eq!(
        page.adoption
            .as_ref()
            .unwrap()
            .active_task_latest_work_start,
        Some(at(200))
    );
    assert_eq!(
        repository.list_task_items().unwrap()[1].latest_work_start,
        Some(at(300)),
        "the full task aggregate remains a load and tracking-refresh read"
    );
}

#[test]
fn a_two_connection_continuation_adopts_a_switched_active_worklog_without_cursor_invalidation() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let first = SqliteRepository::open(&path).unwrap();
    let requested = named_task(1, "requested");
    let next_task = named_task(2, "next");
    first.create_task(requested.clone()).unwrap();
    first.create_task(next_task.clone()).unwrap();
    for tag in 1..=51 {
        let worklog = if tag == 51 {
            Worklog::begin(worklog_id(tag), requested.id(), at(i64::from(tag) * 10))
        } else {
            Worklog::new(
                worklog_id(tag),
                requested.id(),
                at(i64::from(tag) * 10),
                Some(at(i64::from(tag) * 10 + 1)),
            )
            .unwrap()
        };
        first.insert_worklog(&worklog).unwrap();
    }
    let first_page = first.worklog_page(requested.id(), None).unwrap();
    let cursor = first_page
        .next_cursor
        .expect("the first page has one older row");
    let loaded_active = first_page
        .worklogs
        .iter()
        .find(|worklog| worklog.is_active())
        .cloned()
        .expect("the active worklog was loaded");

    let second = SqliteRepository::open(&path).unwrap();
    let replacement = Worklog::begin(worklog_id(99), next_task.id(), at(520));
    second
        .switch_worklog(
            loaded_active.id(),
            loaded_active.start(),
            at(515),
            &replacement,
        )
        .unwrap();

    let continuation = first.worklog_page(requested.id(), Some(&cursor)).unwrap();
    assert_eq!(continuation.worklogs.len(), 1);
    assert_eq!(
        continuation.adoption.as_ref().unwrap().active_worklog,
        Some(replacement)
    );
    assert_eq!(
        continuation
            .adoption
            .as_ref()
            .unwrap()
            .active_task_latest_work_start,
        Some(at(520))
    );
    assert_eq!(
        continuation
            .adoption
            .as_ref()
            .unwrap()
            .requested_task_latest_work_start,
        Some(loaded_active.start())
    );
}

#[test]
fn history_page_reads_use_indexed_bounded_rows_and_two_task_aggregates() {
    let repository = repo();
    let requested = named_task(1, "requested");
    let unrelated = named_task(2, "unrelated");
    repository.create_task(requested.clone()).unwrap();
    repository.create_task(unrelated.clone()).unwrap();
    for tag in 1..=WORKLOG_PAGE_SIZE as u32 + 1 {
        let start = i64::from(tag) * 10;
        repository
            .insert_worklog(
                &Worklog::new(
                    worklog_id(tag),
                    requested.id(),
                    at(start),
                    Some(at(start + 1)),
                )
                .unwrap(),
            )
            .unwrap();
    }
    for tag in 1_000..=3_000 {
        let start = i64::from(tag) * 10;
        repository
            .insert_worklog(
                &Worklog::new(
                    worklog_id(tag),
                    unrelated.id(),
                    at(start),
                    Some(at(start + 1)),
                )
                .unwrap(),
            )
            .unwrap();
    }
    let active = Worklog::begin(worklog_id(9_999), unrelated.id(), at(50_000));
    repository.insert_worklog(&active).unwrap();

    let page = repository.worklog_page(requested.id(), None).unwrap();
    assert_eq!(page.worklogs.len(), WORKLOG_PAGE_SIZE);
    assert_eq!(
        page.adoption
            .as_ref()
            .unwrap()
            .requested_task_latest_work_start,
        Some(at(510))
    );
    assert_eq!(page.adoption.as_ref().unwrap().active_worklog, Some(active));
    assert_eq!(
        page.adoption
            .as_ref()
            .unwrap()
            .active_task_latest_work_start,
        Some(at(50_000))
    );
    let plan: Vec<String> = repository
        .connection()
        .prepare(
            "EXPLAIN QUERY PLAN SELECT id, task_id, start_us, end_us FROM worklogs
             WHERE task_id = ?1 ORDER BY start_us DESC, id LIMIT ?2",
        )
        .unwrap()
        .query_map(
            rusqlite::params![requested.id().to_string(), 51_i64],
            |row| row.get(3),
        )
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        plan.iter()
            .any(|detail| detail.contains("worklogs_task_start"))
    );
    let max_plan: Vec<String> = repository
        .connection()
        .prepare("EXPLAIN QUERY PLAN SELECT MAX(start_us) FROM worklogs WHERE task_id = ?1")
        .unwrap()
        .query_map([requested.id().to_string()], |row| row.get(3))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        max_plan
            .iter()
            .any(|detail| detail.contains("worklogs_task_start"))
    );
}

#[test]
fn a_cursor_stays_stable_when_a_newer_worklog_is_inserted_between_page_reads() {
    let repository = repo();
    let task = named_task(1, "history");
    repository.create_task(task.clone()).unwrap();
    insert_numbered_worklogs(&repository, &task, 55);

    let first = repository.worklog_page(task.id(), None).unwrap();
    assert_eq!(first.worklogs.len(), WORKLOG_PAGE_SIZE);
    let cursor = first.next_cursor.expect("more history follows");

    // A newer worklog lands between the two page reads. The keyset cursor
    // still continues after the page's last row, so the second page neither
    // repeats a row of the first page nor skips one.
    repository
        .stop_worklog(worklog_id(55), at(55), at(56))
        .unwrap();
    let newer = Worklog::new(worklog_id(100), task.id(), at(1000), Some(at(1001))).unwrap();
    repository.insert_worklog(&newer).unwrap();

    let second = repository.worklog_page(task.id(), Some(&cursor)).unwrap();
    let rest_ids: Vec<u32> = second
        .worklogs
        .iter()
        .map(|worklog| worklog.id().as_uuid().as_u128() as u32)
        .collect();
    assert_eq!(rest_ids, (1..=5).rev().collect::<Vec<_>>());
    assert_eq!(second.next_cursor, None);

    // The newer worklog is not lost; a fresh first page leads with it.
    let fresh = repository.worklog_page(task.id(), None).unwrap();
    assert_eq!(fresh.worklogs[0].id(), newer.id());
}

#[test]
fn start_corrections_invalidate_continuations_but_inserts_and_end_corrections_do_not() {
    let repository = repo();
    let task = named_task(1, "history");
    repository.create_task(task.clone()).unwrap();
    for tag in 1..=51 {
        let start = i64::from(tag) * 10;
        repository
            .insert_worklog(
                &Worklog::new(worklog_id(tag), task.id(), at(start), Some(at(start + 1))).unwrap(),
            )
            .unwrap();
    }

    let first = repository.worklog_page(task.id(), None).unwrap();
    let cursor = first.next_cursor.unwrap();
    let unloaded = repository.find_worklog(worklog_id(1)).unwrap().unwrap();
    repository
        .compare_and_set_worklog_times(
            unloaded.id(),
            unloaded.times(),
            WorklogTimes::new(at(1_000), Some(at(1_001))),
        )
        .unwrap();
    assert!(matches!(
        repository.worklog_page(task.id(), Some(&cursor)),
        Err(StorageError::WorklogHistoryChanged { task_id }) if task_id == task.id()
    ));

    let refreshed = repository.worklog_page(task.id(), None).unwrap();
    let cursor = refreshed.next_cursor.unwrap();
    let loaded = refreshed.worklogs[0].clone();
    repository
        .compare_and_set_worklog_times(
            loaded.id(),
            loaded.times(),
            WorklogTimes::new(at(-10), Some(at(-9))),
        )
        .unwrap();
    assert!(matches!(
        repository.worklog_page(task.id(), Some(&cursor)),
        Err(StorageError::WorklogHistoryChanged { task_id }) if task_id == task.id()
    ));

    let refreshed = repository.worklog_page(task.id(), None).unwrap();
    let cursor = refreshed.next_cursor.unwrap();
    let unchanged_order = repository.find_worklog(worklog_id(2)).unwrap().unwrap();
    repository
        .compare_and_set_worklog_times(
            unchanged_order.id(),
            unchanged_order.times(),
            WorklogTimes::new(unchanged_order.start(), Some(at(21))),
        )
        .unwrap();
    assert!(repository.worklog_page(task.id(), Some(&cursor)).is_ok());

    repository
        .insert_worklog(
            &Worklog::new(worklog_id(99), task.id(), at(2_000), Some(at(2_001))).unwrap(),
        )
        .unwrap();
    assert!(repository.worklog_page(task.id(), Some(&cursor)).is_ok());
}

#[test]
fn direct_task_moves_invalidate_both_history_cursors_and_start_changes_bump_once() {
    let repository = repo();
    let alpha = named_task(1, "alpha");
    let beta = named_task(2, "beta");
    repository.create_task(alpha.clone()).unwrap();
    repository.create_task(beta.clone()).unwrap();
    for tag in 1..=51 {
        repository
            .insert_worklog(
                &Worklog::new(
                    worklog_id(tag),
                    alpha.id(),
                    at(i64::from(tag) * 10),
                    Some(at(i64::from(tag) * 10 + 1)),
                )
                .unwrap(),
            )
            .unwrap();
        repository
            .insert_worklog(
                &Worklog::new(
                    worklog_id(tag + 100),
                    beta.id(),
                    at(10_000 + i64::from(tag) * 10),
                    Some(at(10_000 + i64::from(tag) * 10 + 1)),
                )
                .unwrap(),
            )
            .unwrap();
    }

    let alpha_cursor = repository
        .worklog_page(alpha.id(), None)
        .unwrap()
        .next_cursor
        .unwrap();
    let beta_cursor = repository
        .worklog_page(beta.id(), None)
        .unwrap()
        .next_cursor
        .unwrap();
    let moved = worklog_id(1);
    repository
        .connection()
        .execute(
            "UPDATE worklogs SET task_id = ?1 WHERE id = ?2",
            [beta.id().to_string(), moved.to_string()],
        )
        .unwrap();

    assert_eq!(history_revision(&repository, alpha.id()), 1);
    assert_eq!(history_revision(&repository, beta.id()), 1);
    for (task_id, cursor) in [(alpha.id(), alpha_cursor), (beta.id(), beta_cursor)] {
        assert!(matches!(
            repository.worklog_page(task_id, Some(&cursor)),
            Err(StorageError::WorklogHistoryChanged { task_id: changed }) if changed == task_id
        ));
    }

    let beta_cursor = repository
        .worklog_page(beta.id(), None)
        .unwrap()
        .next_cursor
        .unwrap();
    repository
        .connection()
        .execute(
            "UPDATE worklogs SET start_us = ?1 WHERE id = ?2",
            rusqlite::params![20_000_i64, moved.as_uuid().to_string()],
        )
        .unwrap();
    assert_eq!(history_revision(&repository, alpha.id()), 1);
    assert_eq!(history_revision(&repository, beta.id()), 2);
    assert!(matches!(
        repository.worklog_page(beta.id(), Some(&beta_cursor)),
        Err(StorageError::WorklogHistoryChanged { task_id }) if task_id == beta.id()
    ));
}

#[test]
fn a_cursor_for_another_task_is_rejected_before_paging() {
    let repository = repo();
    let alpha = named_task(1, "alpha");
    let beta = named_task(2, "beta");
    repository.create_task(alpha.clone()).unwrap();
    repository.create_task(beta.clone()).unwrap();
    for (id, task, start) in [(1, alpha.id(), 100), (2, beta.id(), 200)] {
        repository
            .insert_worklog(
                &Worklog::new(worklog_id(id), task, at(start), Some(at(start + 1))).unwrap(),
            )
            .unwrap();
    }
    let cursor = repository
        .worklog_page(alpha.id(), None)
        .unwrap()
        .next_cursor;
    let cursor = cursor.unwrap_or(WorklogCursor {
        id: worklog_id(1),
        revision: 0,
    });
    assert!(matches!(
        repository.worklog_page(beta.id(), Some(&cursor)),
        Err(StorageError::WorklogHistoryChanged { task_id }) if task_id == beta.id()
    ));
}

#[test]
fn predecessor_lookup_uses_the_partial_index_for_large_history_writes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let rows: Vec<_> = (1..=2_000)
        .map(|tag| (tag, i64::from(tag) * 10, Some(i64::from(tag) * 10 + 1)))
        .collect();
    create_v2_database(&path, &rows);
    let repository = SqliteRepository::open(&path).unwrap();
    let plan: Vec<String> = repository
        .connection()
        .prepare(
            "EXPLAIN QUERY PLAN SELECT end_us FROM worklogs
             WHERE task_id = ?1
               AND (end_us IS NULL OR end_us > start_us)
               AND start_us < ?2
             ORDER BY start_us DESC LIMIT 1",
        )
        .unwrap()
        .query_map(
            rusqlite::params![task_id(1).to_string(), 30_000_i64],
            |row| row.get(3),
        )
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(plan.iter().any(|detail| {
        detail.contains("worklogs_nonzero_task_start") && detail.contains("task_id=?")
    }));
    repository
        .insert_worklog(
            &Worklog::new(worklog_id(3_000), task_id(1), at(20_001), Some(at(20_002))).unwrap(),
        )
        .unwrap();
}

#[test]
fn missing_task_cursor_anchors_require_a_fresh_first_page() {
    let repository = repo();
    let task = named_task(1, "history");
    repository.create_task(task.clone()).unwrap();
    let cursor = WorklogCursor {
        id: worklog_id(99),
        revision: 0,
    };
    assert!(matches!(repository.worklog_page(task.id(), Some(&cursor)),
        Err(StorageError::WorklogHistoryChanged { task_id }) if task_id == task.id()));
    assert!(
        repository
            .worklog_page(task.id(), None)
            .unwrap()
            .worklogs
            .is_empty()
    );
}

#[test]
fn a_deleted_anchor_conflicts_even_with_the_current_task_revision() {
    let repository = repo();
    let task = named_task(1, "history");
    repository.create_task(task.clone()).unwrap();
    insert_numbered_worklogs(&repository, &task, 55);
    let mut cursor = repository
        .worklog_page(task.id(), None)
        .unwrap()
        .next_cursor
        .unwrap();
    let anchor = repository.find_worklog(cursor.id).unwrap().unwrap();
    repository
        .compare_and_delete_completed_worklog(anchor.id(), task.id(), anchor.times())
        .unwrap();
    cursor.revision = history_revision(&repository, task.id());
    assert!(matches!(repository.worklog_page(task.id(), Some(&cursor)),
        Err(StorageError::WorklogHistoryChanged { task_id }) if task_id == task.id()));
}

struct HistoryReadPause {
    paused: AtomicBool,
    reached: SyncSender<()>,
    release: Mutex<Receiver<()>>,
}

unsafe extern "C" fn pause_history_anchor_read(
    context: *mut c_void,
    action: c_int,
    table: *const c_char,
    column: *const c_char,
    _: *const c_char,
    _: *const c_char,
) -> c_int {
    let pause = unsafe { &*context.cast::<HistoryReadPause>() };
    if action == rusqlite::ffi::SQLITE_READ
        && unsafe { CStr::from_ptr(table) }.to_bytes() == b"worklogs"
        && unsafe { CStr::from_ptr(column) }.to_bytes() == b"start_us"
        && !pause.paused.swap(true, Ordering::SeqCst)
        && (pause.reached.send(()).is_err()
            || pause.release.lock().map_or(true, |release| {
                release.recv_timeout(Duration::from_secs(5)).is_err()
            }))
    {
        return rusqlite::ffi::SQLITE_DENY;
    }
    rusqlite::ffi::SQLITE_OK
}

#[test]
fn task_and_global_continuations_read_revision_anchor_and_rows_from_one_snapshot() {
    for global in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let reader = file_repo(&directory);
        reader
            .connection()
            .execute_batch("PRAGMA journal_mode = WAL")
            .unwrap();
        let task = named_task(1, "snapshot");
        reader.create_task(task.clone()).unwrap();
        for tag in 1..=55 {
            let start = i64::from(tag) * 10;
            reader
                .insert_worklog(
                    &Worklog::new(worklog_id(tag), task.id(), at(start), Some(at(start + 1)))
                        .unwrap(),
                )
                .unwrap();
        }
        let task_cursor = reader
            .worklog_page(task.id(), None)
            .unwrap()
            .next_cursor
            .unwrap();
        let global_cursor = reader
            .global_worklog_page(None)
            .unwrap()
            .next_cursor
            .unwrap();
        let writer = file_repo(&directory);
        let anchor = writer.find_worklog(task_cursor.id).unwrap().unwrap();
        let (reached_tx, reached_rx) = sync_channel(0);
        let (release_tx, release_rx) = sync_channel(0);
        let task_id = task.id();
        let reading = thread::spawn(move || {
            let pause = Box::new(HistoryReadPause {
                paused: AtomicBool::new(false),
                reached: reached_tx,
                release: Mutex::new(release_rx),
            });
            assert_eq!(
                unsafe {
                    rusqlite::ffi::sqlite3_set_authorizer(
                        reader.connection().handle(),
                        Some(pause_history_anchor_read),
                        (&*pause as *const HistoryReadPause).cast_mut().cast(),
                    )
                },
                rusqlite::ffi::SQLITE_OK
            );
            let rows = if global {
                reader
                    .global_worklog_page(Some(&global_cursor))
                    .unwrap()
                    .worklogs
            } else {
                reader
                    .worklog_page(task_id, Some(&task_cursor))
                    .unwrap()
                    .worklogs
            };
            unsafe {
                rusqlite::ffi::sqlite3_set_authorizer(
                    reader.connection().handle(),
                    None,
                    std::ptr::null_mut(),
                );
            }
            if global {
                assert!(matches!(
                    reader.global_worklog_page(Some(&global_cursor)),
                    Err(StorageError::GlobalWorklogHistoryChanged)
                ));
            } else {
                assert!(matches!(
                    reader.worklog_page(task_id, Some(&task_cursor)),
                    Err(StorageError::WorklogHistoryChanged { .. })
                ));
            }
            rows.into_iter().map(|row| row.id()).collect::<Vec<_>>()
        });
        reached_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        writer
            .compare_and_set_worklog_times(
                anchor.id(),
                anchor.times(),
                WorklogTimes::new(at(1_000), Some(at(1_001))),
            )
            .unwrap();
        release_tx.send(()).unwrap();
        assert_eq!(
            reading.join().unwrap(),
            (1..=5).rev().map(worklog_id).collect::<Vec<_>>()
        );
    }
}
