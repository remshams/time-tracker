use super::*;
use chrono::TimeDelta;

#[test]
fn report_clips_completed_and_active_work_and_orders_ties_by_id() {
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone()]);
    {
        let mut data = repository.0.borrow_mut();
        data.worklogs
            .push(completed_worklog(1, alpha.id(), 90, 120));
        data.worklogs
            .push(completed_worklog(2, alpha.id(), 130, 130));
        data.worklogs
            .push(completed_worklog(3, beta.id(), 110, 120));
        data.worklogs.push(worklog(4, beta.id(), 200));
    }
    let mut application = TrackerApplication::load(repository).unwrap();

    let totals = application
        .report_totals(at(100), at(300), at(210))
        .unwrap();

    assert_eq!(totals.total, TimeDelta::seconds(40));
    assert_eq!(totals.rows.len(), 2);
    assert_eq!(totals.rows[0].task.id(), alpha.id());
    assert_eq!(totals.rows[0].duration, TimeDelta::seconds(20));
    assert_eq!(totals.rows[1].task.id(), beta.id());
    assert_eq!(totals.rows[1].duration, TimeDelta::seconds(20));
}

#[test]
fn report_rejects_invalid_range_and_returns_empty_without_worklogs() {
    let mut application = TrackerApplication::load(MemoryRepository::default()).unwrap();
    assert_eq!(
        application.report_totals(at(100), at(100), at(200)),
        Err(ApplicationError::InvalidReportRange)
    );
    assert_eq!(
        application.report_totals(at(100), at(200), at(50)).unwrap(),
        ReportTotals {
            rows: Vec::new(),
            total: TimeDelta::zero(),
        }
    );
}

#[test]
fn report_keeps_completed_future_work_but_caps_open_work_at_now() {
    let completed_task = task(1, "completed");
    let active_task = task(2, "active");
    let repository =
        MemoryRepository::with_tasks(vec![completed_task.clone(), active_task.clone()]);
    {
        let mut data = repository.0.borrow_mut();
        data.worklogs
            .push(completed_worklog(1, completed_task.id(), 300, 320));
        data.worklogs.push(worklog(2, active_task.id(), 305));
    }
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
fn report_adopts_tasks_and_tracking_from_the_same_read() {
    let initial = task(1, "initial");
    let added = task(2, "added elsewhere");
    let repository = MemoryRepository::with_tasks(vec![initial]);
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    {
        let mut data = repository.0.borrow_mut();
        data.tasks.push(TaskListItem {
            task: added.clone(),
            latest_work_start: None,
        });
        data.worklogs.push(worklog(1, added.id(), 200));
    }

    let totals = application
        .report_totals(at(100), at(300), at(210))
        .unwrap();

    assert_eq!(totals.rows.len(), 1);
    assert_eq!(totals.rows[0].task.id(), added.id());
    assert_eq!(application.task(added.id()), Some(&added));
    assert!(matches!(
        application.current_tracking(),
        TrackingState::Running { worklog } if worklog.task_id() == added.id()
    ));
}

#[test]
fn report_rejects_a_combined_duration_that_exceeds_signed_microseconds() {
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    let repository = MemoryRepository::with_tasks(vec![alpha.clone(), beta.clone()]);
    let end = DateTime::from_timestamp_micros(5_000_000_000_000_000_000).unwrap();
    {
        let mut data = repository.0.borrow_mut();
        data.worklogs.push(
            Worklog::new(
                WorklogId::from_uuid(uuid::Uuid::from_u128(1)),
                alpha.id(),
                at(0),
                Some(end),
            )
            .unwrap(),
        );
        data.worklogs.push(
            Worklog::new(
                WorklogId::from_uuid(uuid::Uuid::from_u128(2)),
                beta.id(),
                at(0),
                Some(end),
            )
            .unwrap(),
        );
    }
    let mut application = TrackerApplication::load(repository).unwrap();

    assert_eq!(
        application.report_totals(at(0), end, end),
        Err(ApplicationError::ReportDurationOverflow)
    );
}
