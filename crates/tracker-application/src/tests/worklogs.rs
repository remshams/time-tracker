use super::*;

#[test]
fn global_pages_include_archived_tasks_and_keep_tied_rows_in_id_order() {
    let alpha = task(1, "alpha");
    let mut archived = task(2, "archived");
    assert!(archived.archive(at(100)));
    let repository = MemoryRepository::with_tasks(vec![alpha.clone(), archived.clone()]);
    {
        let mut data = repository.0.borrow_mut();
        for tag in (1..=55).rev() {
            let task_id = if tag % 2 == 0 {
                archived.id()
            } else {
                alpha.id()
            };
            data.worklogs
                .push(completed_worklog(tag, task_id, 200, 200));
        }
    }
    let mut application = TrackerApplication::load(repository).unwrap();
    let first = application.all_worklogs(None).unwrap();
    assert_eq!(first.worklogs.len(), WORKLOG_PAGE_SIZE);
    assert_eq!(
        first.worklogs.first().unwrap().id(),
        worklog(1, alpha.id(), 200).id()
    );
    let second = application
        .all_worklogs(first.next_cursor.as_ref())
        .unwrap();
    let ids = first
        .worklogs
        .iter()
        .chain(&second.worklogs)
        .map(Worklog::id)
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        (1..=55)
            .map(|tag| worklog(tag, alpha.id(), 200).id())
            .collect::<Vec<_>>()
    );
    assert!(second.next_cursor.is_none());
}

#[test]
fn global_query_adopts_the_catalog_and_tracking_from_its_page() {
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    {
        let mut data = repository.0.borrow_mut();
        data.tasks.push(TaskListItem {
            task: beta.clone(),
            latest_work_start: None,
        });
        data.worklogs.push(worklog(10, beta.id(), 200));
    }
    let page = application.all_worklogs(None).unwrap();
    assert_eq!(page.snapshot.task_items.len(), 2);
    assert_eq!(
        application.tasks(TaskOrdering::RecentlyWorked)[0].task.id(),
        beta.id()
    );
    assert!(
        matches!(application.current_tracking(), TrackingState::Running { worklog: active } if active.task_id() == beta.id())
    );
}

#[test]
fn global_cursor_rejects_a_change_to_a_row_on_another_task() {
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone()]);
    {
        let mut data = repository.0.borrow_mut();
        for tag in 1_u32..=51 {
            let task_id = if tag % 2 == 0 { alpha.id() } else { beta.id() };
            data.worklogs.push(completed_worklog(
                u128::from(tag),
                task_id,
                i64::from(tag) * 10,
                i64::from(tag) * 10,
            ));
        }
    }
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    let cursor = application.all_worklogs(None).unwrap().next_cursor.unwrap();
    repository.0.borrow_mut().worklogs[0] = completed_worklog(1, beta.id(), 600, 600);
    assert!(matches!(
        application.all_worklogs(Some(&cursor)),
        Err(ApplicationError::Repository(
            RepositoryError::GlobalWorklogHistoryChanged
        ))
    ));
}

#[test]
fn memory_repository_honors_worklog_ordering_and_write_contracts() {
    let alpha = task(1, "alpha");
    let mut archived = task(2, "archived");
    assert!(archived.archive(at(100)));
    let repository = MemoryRepository::with_tasks(vec![archived.clone(), alpha.clone()]);
    let later = Worklog::new(
        worklog(2, alpha.id(), 20).id(),
        alpha.id(),
        at(20),
        Some(at(30)),
    )
    .unwrap();
    let earlier = Worklog::new(
        worklog(1, alpha.id(), 10).id(),
        alpha.id(),
        at(10),
        Some(at(15)),
    )
    .unwrap();
    repository.insert_worklog(&later).unwrap();
    repository.insert_worklog(&earlier).unwrap();
    // History order is start descending, so the later worklog leads.
    assert_eq!(WORKLOG_PAGE_SIZE, 50);
    assert_eq!(
        repository
            .worklog_page(alpha.id(), None)
            .unwrap()
            .worklogs
            .into_iter()
            .map(|worklog| worklog.id())
            .collect::<Vec<_>>(),
        vec![later.id(), earlier.id()]
    );
    assert!(matches!(
        repository.insert_worklog(&worklog(3, archived.id(), 40)),
        Err(RepositoryError::TaskArchived { id }) if id == archived.id()
    ));
    let active = worklog(4, alpha.id(), 50);
    repository.insert_worklog(&active).unwrap();
    assert!(matches!(
        repository.stop_worklog(active.id(), active.start(), at(49)),
        Err(RepositoryError::Constraint { .. })
    ));
    let next = worklog(5, archived.id(), 60);
    assert!(matches!(
        repository.switch_worklog(active.id(), active.start(), at(55), &next),
        Err(RepositoryError::TaskArchived { id }) if id == archived.id()
    ));
    assert!(matches!(
        repository.active_worklog(),
        Ok(Some(worklog)) if worklog.id() == active.id()
    ));
}

#[test]
fn worklog_queries_adopt_the_backend_snapshot() {
    let alpha = task(1, "alpha");
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    repository
        .0
        .borrow_mut()
        .worklogs
        .push(worklog(10, alpha.id(), 100));
    assert_eq!(
        application
            .worklogs_for_task(alpha.id(), None)
            .unwrap()
            .worklogs
            .len(),
        1
    );
    assert!(matches!(
        application.current_tracking(),
        TrackingState::Running { worklog: active }
            if active.id() == worklog(10, alpha.id(), 100).id()
    ));
    repository.0.borrow_mut().worklogs.clear();
    assert!(
        application
            .worklogs_for_task(alpha.id(), None)
            .unwrap()
            .worklogs
            .is_empty()
    );
    assert_eq!(application.current_tracking(), &TrackingState::Idle);
}

#[test]
fn a_worklog_page_replaces_requested_and_active_task_aggregates_exactly() {
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    let alpha_work = completed_worklog(10, alpha.id(), 300, 310);
    let beta_work = worklog(11, beta.id(), 250);
    let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone()]);
    repository.insert_worklog(&alpha_work).unwrap();
    repository.insert_worklog(&beta_work).unwrap();
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    assert_eq!(
        ordered_names(&application, TaskOrdering::RecentlyWorked),
        ["alpha".to_owned(), "beta".to_owned()]
    );

    repository
        .compare_and_set_worklog_times(
            alpha_work.id(),
            alpha_work.times(),
            WorklogTimes::new(at(200), Some(at(210))),
        )
        .unwrap();
    application.worklogs_for_task(alpha.id(), None).unwrap();

    assert_eq!(
        ordered_names(&application, TaskOrdering::RecentlyWorked),
        ["beta".to_owned(), "alpha".to_owned()]
    );
}

#[test]
fn a_running_history_row_wins_a_race_after_the_tracking_refresh() {
    let alpha = task(1, "alpha");
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    let active = worklog(10, alpha.id(), 100);
    repository.0.borrow_mut().worklogs.push(active.clone());
    repository.hide_next_active_read();

    let page = application.worklogs_for_task(alpha.id(), None).unwrap();

    assert_eq!(page.worklogs, vec![active.clone()]);
    assert!(matches!(
        application.current_tracking(),
        TrackingState::Running { worklog } if worklog.id() == active.id()
    ));
}

#[test]
fn worklog_history_pages_are_bounded_ordered_and_continue_after_the_cursor() {
    let alpha = task(1, "alpha");
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    // 55 worklogs with distinct starts; the latest one stays active,
    // and active worklogs belong to the history.
    for tag in 1..=55u128 {
        let worklog = if tag == 55 {
            worklog(tag, alpha.id(), i64::try_from(tag).unwrap())
        } else {
            Worklog::new(
                WorklogId::from_uuid(uuid::Uuid::from_u128(tag)),
                alpha.id(),
                at(i64::try_from(tag).unwrap()),
                Some(at(i64::try_from(tag).unwrap() + 1)),
            )
            .unwrap()
        };
        repository.insert_worklog(&worklog).unwrap();
    }
    let mut application = TrackerApplication::load(repository).unwrap();

    let first = application.worklogs_for_task(alpha.id(), None).unwrap();
    assert_eq!(first.worklogs.len(), WORKLOG_PAGE_SIZE);
    let starts: Vec<i64> = first
        .worklogs
        .iter()
        .map(|w| w.start().timestamp())
        .collect();
    assert_eq!(
        starts,
        (6..=55).rev().collect::<Vec<_>>(),
        "start descending"
    );
    let cursor = first.next_cursor.expect("more history follows");
    assert_eq!(cursor.start, at(6));
    assert_eq!(cursor.id, WorklogId::from_uuid(uuid::Uuid::from_u128(6)));

    let second = application
        .worklogs_for_task(alpha.id(), Some(&cursor))
        .unwrap();
    assert_eq!(second.worklogs.len(), 5);
    assert_eq!(
        second.next_cursor, None,
        "the history ended inside the page"
    );
    let rest_starts: Vec<i64> = second
        .worklogs
        .iter()
        .map(|w| w.start().timestamp())
        .collect();
    assert_eq!(rest_starts, [5, 4, 3, 2, 1]);

    // The two pages cover every worklog exactly once: none skipped,
    // none repeated.
    let mut ids: Vec<u128> = first
        .worklogs
        .iter()
        .chain(&second.worklogs)
        .map(|w| w.id().as_uuid().as_u128())
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, (1..=55).collect::<Vec<_>>());
}

#[test]
fn a_page_boundary_inside_equal_starts_neither_dups_nor_skips_rows() {
    let alpha = task(1, "alpha");
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    // 55 worklogs share one start, so identifier ascending decides the
    // whole order and the page boundary falls between two of them. A
    // weaker boundary rule would repeat or drop rows 51 to 55.
    for tag in 1..=55u128 {
        let worklog = if tag == 1 {
            Worklog::begin(
                WorklogId::from_uuid(uuid::Uuid::from_u128(tag)),
                alpha.id(),
                at(500),
            )
        } else {
            Worklog::new(
                WorklogId::from_uuid(uuid::Uuid::from_u128(tag)),
                alpha.id(),
                at(500),
                Some(at(500)),
            )
            .unwrap()
        };
        repository.insert_worklog(&worklog).unwrap();
    }
    let mut application = TrackerApplication::load(repository).unwrap();

    let page = application.worklogs_for_task(alpha.id(), None).unwrap();
    let ids: Vec<u128> = page
        .worklogs
        .iter()
        .map(|w| w.id().as_uuid().as_u128())
        .collect();
    assert_eq!(ids, (1..=50).collect::<Vec<_>>(), "id ascending");
    let cursor = page.next_cursor.expect("equal starts continue");
    assert_eq!(cursor.start, at(500));
    assert_eq!(cursor.id.as_uuid().as_u128(), 50);

    let rest = application
        .worklogs_for_task(alpha.id(), Some(&cursor))
        .unwrap()
        .worklogs;
    let rest_ids: Vec<u128> = rest.iter().map(|w| w.id().as_uuid().as_u128()).collect();
    assert_eq!(rest_ids, (51..=55).collect::<Vec<_>>());
}

#[test]
fn deletion_canonicalizes_expected_times_and_adopts_the_transaction_aggregate() {
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    let old_alpha = completed_worklog(10, alpha.id(), 100, 110);
    let latest_alpha = Worklog::new(
        WorklogId::from_uuid(uuid::Uuid::from_u128(11)),
        alpha.id(),
        at_nanos(300, 123_456_000),
        Some(at_nanos(310, 654_321_000)),
    )
    .unwrap();
    let beta_work = worklog(12, beta.id(), 250);
    let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone()]);
    for worklog in [&old_alpha, &latest_alpha, &beta_work] {
        repository.insert_worklog(worklog).unwrap();
    }
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    repository.0.borrow_mut().fail_reads = true;

    let outcome = application
        .delete_completed_worklog(
            latest_alpha.id(),
            alpha.id(),
            WorklogTimes::new(at_nanos(300, 123_456_999), Some(at_nanos(310, 654_321_999))),
        )
        .unwrap();

    assert_eq!(outcome, latest_alpha);
    assert_eq!(
        ordered_names(&application, TaskOrdering::RecentlyWorked),
        ["beta".to_owned(), "alpha".to_owned()]
    );
    let alpha_item = application
        .tasks(TaskOrdering::RecentlyWorked)
        .into_iter()
        .find(|item| item.task.id() == alpha.id())
        .unwrap();
    assert_eq!(alpha_item.latest_work_start, Some(at(100)));
    assert!(matches!(
        application.current_tracking(),
        TrackingState::Running { worklog } if worklog.id() == beta_work.id()
    ));
    assert_eq!(repository.0.borrow().worklogs, [old_alpha, beta_work]);
}

#[test]
fn deletion_rejects_active_missing_stale_and_task_moved_targets() {
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    let completed = completed_worklog(10, alpha.id(), 100, 150);
    let active = worklog(11, alpha.id(), 200);
    let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone()]);
    repository.insert_worklog(&completed).unwrap();
    repository.insert_worklog(&active).unwrap();

    assert_eq!(
        repository.compare_and_delete_completed_worklog(
            active.id(),
            active.task_id(),
            active.times(),
        ),
        Err(RepositoryError::WorklogIsActive { id: active.id() })
    );
    assert_eq!(
        repository.compare_and_delete_completed_worklog(
            WorklogId::from_uuid(uuid::Uuid::from_u128(99)),
            alpha.id(),
            completed.times(),
        ),
        Err(RepositoryError::WorklogNotFound {
            id: WorklogId::from_uuid(uuid::Uuid::from_u128(99))
        })
    );
    assert_eq!(
        repository.compare_and_delete_completed_worklog(
            completed.id(),
            alpha.id(),
            WorklogTimes::new(at(100), Some(at(151))),
        ),
        Err(RepositoryError::WorklogChanged { id: completed.id() })
    );
    let moved = Worklog::new(
        completed.id(),
        beta.id(),
        completed.start(),
        completed.end(),
    )
    .unwrap();
    repository.0.borrow_mut().worklogs[0] = moved.clone();
    assert_eq!(
        repository.compare_and_delete_completed_worklog(
            completed.id(),
            alpha.id(),
            completed.times(),
        ),
        Err(RepositoryError::WorklogChanged { id: completed.id() })
    );
    assert!(repository.0.borrow().worklogs.contains(&moved));
    assert!(repository.0.borrow().worklogs.contains(&active));
}

#[test]
fn expected_active_deletion_is_rejected_before_the_repository_write() {
    let alpha = task(1, "alpha");
    let active = worklog(10, alpha.id(), 100);
    let repository = MemoryRepository::with_tasks(vec![alpha]);
    repository.insert_worklog(&active).unwrap();
    let mut application = TrackerApplication::load(repository.clone()).unwrap();

    assert_eq!(
        application.delete_completed_worklog(active.id(), active.task_id(), active.times()),
        Err(ApplicationError::WorklogDeletionWrite {
            write: RepositoryError::WorklogIsActive { id: active.id() }
        })
    );
    assert_eq!(repository.0.borrow().deletion_writes, 0);
    assert_eq!(repository.find_worklog(active.id()).unwrap(), Some(active));
}

#[test]
fn deletion_recovers_when_the_completed_target_became_active() {
    let alpha = task(1, "alpha");
    let completed = completed_worklog(10, alpha.id(), 100, 150);
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    repository.insert_worklog(&completed).unwrap();
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    repository
        .rename_task(alpha.id(), TaskName::new("renamed").unwrap(), at(175))
        .unwrap();
    let active = Worklog::begin(completed.id(), completed.task_id(), completed.start());
    repository.0.borrow_mut().worklogs[0] = active.clone();

    assert_eq!(
        application.delete_completed_worklog(
            completed.id(),
            completed.task_id(),
            completed.times(),
        ),
        Err(ApplicationError::WorklogDeletionWrite {
            write: RepositoryError::WorklogIsActive { id: completed.id() }
        })
    );
    assert_eq!(repository.0.borrow().deletion_writes, 1);
    assert!(matches!(
        application.current_tracking(),
        TrackingState::Running { worklog } if worklog.id() == active.id()
    ));
    assert_eq!(
        application.task(alpha.id()).unwrap().name().as_str(),
        "renamed"
    );
    assert_eq!(repository.find_worklog(active.id()).unwrap(), Some(active));
}

#[test]
fn deletion_write_errors_reload_authoritative_state() {
    let alpha = task(1, "alpha");
    let target = completed_worklog(10, alpha.id(), 100, 150);
    let foreign = worklog(11, alpha.id(), 200);
    let repository = MemoryRepository::with_tasks(vec![alpha]);
    repository.insert_worklog(&target).unwrap();
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    repository.fail_next_write(
        RepositoryError::Backend {
            message: "write failed".to_owned(),
        },
        Some(foreign.clone()),
    );

    assert_eq!(
        application.delete_completed_worklog(target.id(), target.task_id(), target.times()),
        Err(ApplicationError::WorklogDeletionWrite {
            write: RepositoryError::Backend {
                message: "write failed".to_owned()
            }
        })
    );
    assert!(matches!(
        application.current_tracking(),
        TrackingState::Running { worklog } if worklog == &ActiveWorklog::begin(
            foreign.id(), foreign.task_id(), foreign.start()
        )
    ));
}

#[test]
fn deletion_preserves_write_and_recovery_errors() {
    let alpha = task(1, "alpha");
    let target = completed_worklog(10, alpha.id(), 100, 150);
    let repository = MemoryRepository::with_tasks(vec![alpha]);
    repository.insert_worklog(&target).unwrap();
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    let write = RepositoryError::Backend {
        message: "write failed".to_owned(),
    };
    repository.fail_next_write(write.clone(), None);
    repository.fail_recovery_after_next_write();

    assert_eq!(
        application.delete_completed_worklog(target.id(), target.task_id(), target.times()),
        Err(ApplicationError::WorklogDeletionRecovery {
            write,
            recovery: RepositoryError::Backend {
                message: "read failed".to_owned()
            }
        })
    );
    assert!(repository.0.borrow().worklogs.contains(&target));
}

#[test]
fn correction_canonicalizes_expected_and_replacement_before_validation() {
    let alpha = task(1, "alpha");
    let original = Worklog::new(
        WorklogId::from_uuid(uuid::Uuid::from_u128(10)),
        alpha.id(),
        at_nanos(100, 123_456_000),
        Some(at_nanos(100, 123_457_000)),
    )
    .unwrap();
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    repository.insert_worklog(&original).unwrap();
    let mut application = TrackerApplication::load(repository.clone()).unwrap();

    let outcome = application
        .correct_worklog(
            original.id(),
            WorklogTimes::new(at_nanos(100, 123_456_999), Some(at_nanos(100, 123_457_999))),
            WorklogTimes::new(at_nanos(200, 900), Some(at_nanos(200, 100))),
            at_nanos(200, 999),
        )
        .unwrap();

    let worklog = outcome;
    assert_eq!(worklog.id(), original.id());
    assert_eq!(worklog.task_id(), alpha.id());
    assert_eq!(worklog.start(), at(200));
    assert_eq!(worklog.end(), Some(at(200)));
    assert_eq!(
        repository.find_worklog(original.id()).unwrap(),
        Some(worklog)
    );
}

#[test]
fn correction_rejects_future_and_active_state_changes() {
    let alpha = task(1, "alpha");
    let active = worklog(10, alpha.id(), 100);
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    repository.insert_worklog(&active).unwrap();
    let mut application = TrackerApplication::load(repository.clone()).unwrap();

    assert_eq!(
        application.correct_worklog(
            active.id(),
            active.times(),
            WorklogTimes::new(at(301), None),
            at(300),
        ),
        Err(ApplicationError::InvalidWorklogCorrection(
            WorklogCorrectionError::StartAfterOccurredAt
        ))
    );
    assert_eq!(
        application.correct_worklog(
            active.id(),
            active.times(),
            WorklogTimes::new(at(100), Some(at(200))),
            at(300),
        ),
        Err(ApplicationError::InvalidWorklogCorrection(
            WorklogCorrectionError::CompletionStateChanged
        ))
    );
    assert_eq!(repository.find_worklog(active.id()).unwrap(), Some(active));
}

#[test]
fn active_correction_refreshes_tracking_and_latest_work() {
    let alpha = task(1, "alpha");
    let active = worklog(10, alpha.id(), 100);
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    repository.insert_worklog(&active).unwrap();
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    let list_reads = repository.0.borrow().list_reads;

    let outcome = application
        .correct_worklog(
            active.id(),
            active.times(),
            WorklogTimes::new(at(150), None),
            at(200),
        )
        .unwrap();

    assert_eq!(outcome.id(), active.id());
    assert_eq!(outcome.task_id(), alpha.id());
    assert_eq!(outcome.start(), at(150));
    assert!(outcome.is_active());
    assert!(matches!(
        application.current_tracking(),
        TrackingState::Running { worklog }
            if worklog.id() == active.id() && worklog.start() == at(150)
    ));
    assert_eq!(
        application.tasks(TaskOrdering::RecentlyWorked)[0].latest_work_start,
        Some(at(150))
    );
    assert_eq!(repository.0.borrow().list_reads, list_reads);
}

#[test]
fn correcting_another_task_adopts_the_active_tasks_exact_aggregate() {
    let active_task = task(1, "active");
    let corrected_task = task(2, "corrected");
    let active = worklog(10, active_task.id(), 600);
    let corrected = completed_worklog(11, corrected_task.id(), 400, 410);
    let repository =
        MemoryRepository::with_tasks(vec![active_task.clone(), corrected_task.clone()]);
    repository.insert_worklog(&active).unwrap();
    repository.insert_worklog(&corrected).unwrap();
    let mut application = TrackerApplication::load(repository.clone()).unwrap();

    repository
        .compare_and_set_worklog_times(
            active.id(),
            active.times(),
            WorklogTimes::new(at(540), None),
        )
        .unwrap();
    application
        .correct_worklog(
            corrected.id(),
            corrected.times(),
            WorklogTimes::new(at(350), Some(at(360))),
            at(500),
        )
        .unwrap();

    assert!(matches!(
        application.current_tracking(),
        TrackingState::Running { worklog }
            if worklog.id() == active.id() && worklog.start() == at(540)
    ));
    let active_item = application
        .tasks(TaskOrdering::RecentlyWorked)
        .into_iter()
        .find(|item| item.task.id() == active_task.id())
        .unwrap();
    assert_eq!(active_item.latest_work_start, Some(at(540)));
}

#[test]
fn completed_correction_reloads_the_true_latest_work_aggregate() {
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    let old_alpha = completed_worklog(10, alpha.id(), 100, 110);
    let latest_alpha = completed_worklog(11, alpha.id(), 300, 310);
    let beta_work = completed_worklog(12, beta.id(), 250, 260);
    let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone()]);
    for worklog in [&old_alpha, &latest_alpha, &beta_work] {
        repository.insert_worklog(worklog).unwrap();
    }
    let mut application = TrackerApplication::load(repository).unwrap();
    assert_eq!(
        ordered_names(&application, TaskOrdering::RecentlyWorked),
        ["alpha".to_owned(), "beta".to_owned()]
    );

    application
        .correct_worklog(
            latest_alpha.id(),
            latest_alpha.times(),
            WorklogTimes::new(at(200), Some(at(210))),
            at(400),
        )
        .unwrap();

    assert_eq!(
        ordered_names(&application, TaskOrdering::RecentlyWorked),
        ["beta".to_owned(), "alpha".to_owned()]
    );
    let alpha_item = application
        .tasks(TaskOrdering::RecentlyWorked)
        .into_iter()
        .find(|item| item.task.id() == alpha.id())
        .unwrap();
    assert_eq!(alpha_item.latest_work_start, Some(at(200)));
}

#[test]
fn stale_active_correction_recovers_the_stopped_state() {
    let alpha = task(1, "alpha");
    let active = worklog(10, alpha.id(), 100);
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    repository.insert_worklog(&active).unwrap();
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    repository
        .rename_task(alpha.id(), TaskName::new("renamed").unwrap(), at(120))
        .unwrap();
    repository
        .stop_worklog(active.id(), active.start(), at(150))
        .unwrap();

    assert_eq!(
        application.correct_worklog(
            active.id(),
            active.times(),
            WorklogTimes::new(at(90), None),
            at(200),
        ),
        Err(ApplicationError::WorklogCorrectionWrite {
            write: RepositoryError::WorklogChanged { id: active.id() },
        })
    );
    assert_eq!(application.current_tracking(), &TrackingState::Idle);
    assert_eq!(
        application.task(alpha.id()).unwrap().name().as_str(),
        "renamed"
    );
    assert_eq!(
        application.tasks(TaskOrdering::RecentlyWorked)[0].latest_work_start,
        Some(at(100))
    );
}

#[test]
fn a_missing_correction_target_recovers_authoritative_state() {
    let alpha = task(1, "alpha");
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    let foreign = worklog(10, alpha.id(), 100);
    repository.insert_worklog(&foreign).unwrap();
    let missing = WorklogId::from_uuid(uuid::Uuid::from_u128(99));

    assert_eq!(
        application.correct_worklog(
            missing,
            WorklogTimes::new(at(50), Some(at(60))),
            WorklogTimes::new(at(55), Some(at(65))),
            at(200),
        ),
        Err(ApplicationError::WorklogCorrectionWrite {
            write: RepositoryError::WorklogNotFound { id: missing },
        })
    );
    assert!(matches!(
        application.current_tracking(),
        TrackingState::Running { worklog } if worklog.id() == foreign.id()
    ));
}

#[test]
fn correction_write_errors_recover_state_without_returning_success() {
    let alpha = task(1, "alpha");
    let original = completed_worklog(10, alpha.id(), 100, 110);
    let repository = MemoryRepository::with_tasks(vec![alpha]);
    repository.insert_worklog(&original).unwrap();
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    repository.fail_next_write(
        RepositoryError::Backend {
            message: "write failed".to_owned(),
        },
        None,
    );

    assert_eq!(
        application.correct_worklog(
            original.id(),
            original.times(),
            WorklogTimes::new(at(120), Some(at(130))),
            at(200),
        ),
        Err(ApplicationError::WorklogCorrectionWrite {
            write: RepositoryError::Backend {
                message: "write failed".to_owned(),
            },
        })
    );
    assert_eq!(
        repository.find_worklog(original.id()).unwrap(),
        Some(original)
    );
    assert_eq!(application.current_tracking(), &TrackingState::Idle);
}

#[test]
fn overlap_errors_propagate_and_leave_the_original_worklog() {
    let alpha = task(1, "alpha");
    let first = completed_worklog(10, alpha.id(), 100, 150);
    let second = completed_worklog(11, alpha.id(), 200, 250);
    let repository = MemoryRepository::with_tasks(vec![alpha]);
    repository.insert_worklog(&first).unwrap();
    repository.insert_worklog(&second).unwrap();
    let mut application = TrackerApplication::load(repository.clone()).unwrap();

    assert_eq!(
        application.correct_worklog(
            second.id(),
            second.times(),
            WorklogTimes::new(at(140), Some(at(220))),
            at(300),
        ),
        Err(ApplicationError::WorklogCorrectionWrite {
            write: RepositoryError::SameTaskWorklogOverlap { id: second.id() },
        })
    );
    assert_eq!(repository.find_worklog(second.id()).unwrap(), Some(second));
    assert_eq!(application.current_tracking(), &TrackingState::Idle);
}

#[test]
fn compare_and_swap_distinguishes_stale_and_missing_worklogs() {
    let alpha = task(1, "alpha");
    let original = completed_worklog(10, alpha.id(), 100, 150);
    let repository = MemoryRepository::with_tasks(vec![alpha]);
    repository.insert_worklog(&original).unwrap();

    assert_eq!(
        repository.compare_and_set_worklog_times(
            original.id(),
            WorklogTimes::new(at(100), Some(at(151))),
            WorklogTimes::new(at(110), Some(at(160))),
        ),
        Err(RepositoryError::WorklogChanged { id: original.id() })
    );
    let missing = WorklogId::from_uuid(uuid::Uuid::from_u128(99));
    assert_eq!(
        repository.compare_and_set_worklog_times(
            missing,
            WorklogTimes::new(at(100), Some(at(150))),
            WorklogTimes::new(at(110), Some(at(160))),
        ),
        Err(RepositoryError::WorklogNotFound { id: missing })
    );
    assert_eq!(
        repository.find_worklog(original.id()).unwrap(),
        Some(original)
    );
}

#[test]
fn half_open_overlap_rules_allow_touching_zero_duration_and_other_tasks() {
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone()]);
    let first = completed_worklog(10, alpha.id(), 100, 150);
    let touching = completed_worklog(11, alpha.id(), 150, 200);
    let zero = completed_worklog(12, alpha.id(), 125, 125);
    let other_task = completed_worklog(13, beta.id(), 120, 180);
    repository.insert_worklog(&first).unwrap();
    repository.insert_worklog(&touching).unwrap();
    repository.insert_worklog(&zero).unwrap();
    repository.insert_worklog(&other_task).unwrap();

    let overlapping = completed_worklog(14, alpha.id(), 149, 151);
    assert_eq!(
        repository.insert_worklog(&overlapping),
        Err(RepositoryError::SameTaskWorklogOverlap {
            id: overlapping.id()
        })
    );
    let moved = repository
        .compare_and_set_worklog_times(
            touching.id(),
            touching.times(),
            WorklogTimes::new(at(200), Some(at(210))),
        )
        .unwrap()
        .worklog;
    assert_eq!(moved.start(), at(200));
}
#[test]
fn correction_preserves_write_and_recovery_errors() {
    let alpha = task(1, "alpha");
    let original = completed_worklog(10, alpha.id(), 100, 110);
    let repository = MemoryRepository::with_tasks(vec![alpha]);
    repository.insert_worklog(&original).unwrap();
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    let write = RepositoryError::Backend {
        message: "write failed".to_owned(),
    };
    repository.fail_next_write(write.clone(), None);
    repository.fail_recovery_after_next_write();

    assert_eq!(
        application.correct_worklog(
            original.id(),
            original.times(),
            WorklogTimes::new(at(120), Some(at(130))),
            at(200),
        ),
        Err(ApplicationError::WorklogCorrectionRecovery {
            write,
            recovery: RepositoryError::Backend {
                message: "read failed".to_owned(),
            },
        })
    );
}

#[test]
fn completed_move_updates_both_task_aggregates_without_a_follow_up_read() {
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    let earlier_alpha = completed_worklog(10, alpha.id(), 100, 110);
    let moved = completed_worklog(11, alpha.id(), 300, 310);
    let beta_work = completed_worklog(12, beta.id(), 200, 210);
    let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone()]);
    for worklog in [&earlier_alpha, &moved, &beta_work] {
        repository.insert_worklog(worklog).unwrap();
    }
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    let list_reads = repository.0.borrow().list_reads;

    let result = application
        .move_worklog(moved.id(), alpha.id(), moved.times(), beta.id())
        .unwrap();

    assert_eq!(result.id(), moved.id());
    assert_eq!(result.task_id(), beta.id());
    assert_eq!(result.times(), moved.times());
    assert_eq!(
        ordered_names(&application, TaskOrdering::RecentlyWorked),
        ["beta".to_owned(), "alpha".to_owned()]
    );
    for (task_id, latest_work_start) in [(alpha.id(), Some(at(100))), (beta.id(), Some(at(300)))] {
        let item = application
            .tasks(TaskOrdering::RecentlyWorked)
            .into_iter()
            .find(|item| item.task.id() == task_id)
            .unwrap();
        assert_eq!(item.latest_work_start, latest_work_start);
    }
    assert_eq!(repository.0.borrow().list_reads, list_reads);
}

#[test]
fn active_move_keeps_tracking_on_the_destination_task() {
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    let earlier_alpha = completed_worklog(10, alpha.id(), 100, 110);
    let beta_work = completed_worklog(11, beta.id(), 200, 210);
    let active = worklog(12, alpha.id(), 300);
    let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone()]);
    for worklog in [&earlier_alpha, &beta_work, &active] {
        repository.insert_worklog(worklog).unwrap();
    }
    let mut application = TrackerApplication::load(repository).unwrap();

    let result = application
        .move_worklog(active.id(), alpha.id(), active.times(), beta.id())
        .unwrap();

    assert_eq!(result.id(), active.id());
    assert_eq!(result.task_id(), beta.id());
    assert!(matches!(
        application.current_tracking(),
        TrackingState::Running { worklog }
            if worklog.id() == active.id() && worklog.task_id() == beta.id()
    ));
    for (task_id, latest_work_start) in [(alpha.id(), Some(at(100))), (beta.id(), Some(at(300)))] {
        let item = application
            .tasks(TaskOrdering::RecentlyWorked)
            .into_iter()
            .find(|item| item.task.id() == task_id)
            .unwrap();
        assert_eq!(item.latest_work_start, latest_work_start);
    }
}

#[test]
fn move_rejects_the_current_task_without_writing() {
    let alpha = task(1, "alpha");
    let original = completed_worklog(10, alpha.id(), 100, 110);
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    repository.insert_worklog(&original).unwrap();
    let mut application = TrackerApplication::load(repository.clone()).unwrap();

    assert_eq!(
        application.move_worklog(original.id(), alpha.id(), original.times(), alpha.id()),
        Err(ApplicationError::InvalidWorklogMove(
            tracker_domain::WorklogMoveError::DestinationUnchanged
        ))
    );
    assert_eq!(
        repository.find_worklog(original.id()).unwrap(),
        Some(original)
    );
}

#[test]
fn move_rejects_each_stale_source_value_before_writing() {
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    let original = completed_worklog(10, alpha.id(), 100, 110);

    for (source_task_id, expected) in [
        (beta.id(), original.times()),
        (alpha.id(), WorklogTimes::new(at(101), Some(at(110)))),
    ] {
        let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone()]);
        repository.insert_worklog(&original).unwrap();
        let mut application = TrackerApplication::load(repository.clone()).unwrap();

        assert_eq!(
            application.move_worklog(original.id(), source_task_id, expected, beta.id()),
            Err(ApplicationError::WorklogMoveWrite {
                write: RepositoryError::WorklogChanged { id: original.id() },
            })
        );
        assert_eq!(repository.0.borrow().move_writes, 0);
    }
}

#[test]
fn failed_move_recovers_authoritative_tracking_state() {
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    let original = completed_worklog(10, alpha.id(), 100, 110);
    let replacement_active = worklog(11, beta.id(), 200);
    let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone()]);
    repository.insert_worklog(&original).unwrap();
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    let write = RepositoryError::Backend {
        message: "write failed".to_owned(),
    };
    repository.fail_next_write(write.clone(), Some(replacement_active.clone()));

    assert_eq!(
        application.move_worklog(original.id(), alpha.id(), original.times(), beta.id()),
        Err(ApplicationError::WorklogMoveWrite { write })
    );
    assert!(matches!(
        application.current_tracking(),
        TrackingState::Running { worklog }
            if worklog.id() == replacement_active.id()
                && worklog.task_id() == replacement_active.task_id()
                && worklog.start() == replacement_active.start()
    ));
    assert_eq!(
        application
            .tasks(TaskOrdering::RecentlyWorked)
            .into_iter()
            .find(|item| item.task.id() == beta.id())
            .unwrap()
            .latest_work_start,
        Some(at(200))
    );
}

#[test]
fn move_preserves_write_and_recovery_errors() {
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    let original = completed_worklog(10, alpha.id(), 100, 110);
    let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone()]);
    repository.insert_worklog(&original).unwrap();
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    let write = RepositoryError::Backend {
        message: "write failed".to_owned(),
    };
    repository.fail_next_write(write.clone(), None);
    repository.fail_recovery_after_next_write();

    assert_eq!(
        application.move_worklog(original.id(), alpha.id(), original.times(), beta.id()),
        Err(ApplicationError::WorklogMoveRecovery {
            write,
            recovery: RepositoryError::Backend {
                message: "read failed".to_owned(),
            },
        })
    );
}
