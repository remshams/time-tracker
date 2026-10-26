use super::*;

#[test]
fn tui_service_contract_preserves_task_tracking_and_history_workflows() {
    let source = task(1, "Planning");
    let destination = task(2, "Delivery");
    let repository = MemoryRepository::with_tasks(vec![source.clone(), destination.clone()]);
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    let service: &mut dyn TrackerApplicationService = &mut application;

    let created = service
        .create_task(TaskName::new("Review").unwrap(), at(101))
        .unwrap();
    assert_eq!(created.name().as_str(), "Review");
    assert!(
        service
            .tasks(TaskOrdering::default())
            .iter()
            .any(|item| item.task == created)
    );
    assert_eq!(service.tasks(TaskOrdering::RecentlyCreated).len(), 3);
    let renamed = service
        .rename_task_with_expected_name(
            created.id(),
            created.name(),
            TaskName::new("Release").unwrap(),
            at(102),
        )
        .unwrap();
    assert_eq!(renamed.name().as_str(), "Release");
    assert!(
        service
            .archive_task_with_expected_name(created.id(), renamed.name(), at(103))
            .unwrap()
            .is_archived()
    );
    assert!(
        !service
            .unarchive_task(created.id(), at(104))
            .unwrap()
            .is_archived()
    );

    service
        .set_active_task_with_expected_active(source.id(), None, at(110))
        .unwrap();
    let active = match service.current_tracking() {
        TrackingState::Running { worklog } => worklog.clone(),
        TrackingState::Idle => panic!("tracking must be active after starting a task"),
    };
    assert_eq!(active.task_id(), source.id());
    service.clear_active_task(active.id(), at(120)).unwrap();
    assert!(matches!(service.current_tracking(), TrackingState::Idle));
    let page = service.worklogs_for_task(source.id(), None).unwrap();
    assert_eq!(page.worklogs.len(), 1);
    assert_eq!(service.all_worklogs(None).unwrap().worklogs, page.worklogs);
    let original = &page.worklogs[0];
    let replacement = WorklogTimes::new(at(111), Some(at(119)));
    let corrected = service
        .correct_worklog(original.id(), original.times(), replacement, at(121))
        .unwrap();
    assert_eq!(corrected.times(), replacement);
    let moved = service
        .move_worklog(
            corrected.id(),
            source.id(),
            corrected.times(),
            destination.id(),
        )
        .unwrap();
    assert_eq!(moved.task_id(), destination.id());
    let totals = service.report_totals(at(100), at(130), at(130)).unwrap();
    assert_eq!(totals.rows.len(), 1);
    assert_eq!(totals.rows[0].task.id(), destination.id());
    assert_eq!(totals.rows[0].duration, TimeDelta::seconds(8));
    let deleted = service
        .delete_completed_worklog(moved.id(), destination.id(), moved.times())
        .unwrap();
    assert_eq!(deleted, moved);
    assert!(service.all_worklogs(None).unwrap().worklogs.is_empty());

    let external = task(4, "External task");
    repository.create_task(external.clone()).unwrap();
    service.refresh_task_list().unwrap();
    assert!(
        service
            .tasks(TaskOrdering::default())
            .iter()
            .any(|item| item.task == external)
    );
}

#[test]
fn tui_service_contract_preserves_reviewed_inactivity_candidates() {
    let old = task(1, "Old planning");
    let repository = MemoryRepository::with_tasks(vec![old.clone()]);
    let mut application = TrackerApplication::load(repository).unwrap();
    let service: &mut dyn TrackerApplicationService = &mut application;
    let as_of = at(4_000_000);
    assert_eq!(
        service.preview_inactive_tasks(as_of).unwrap(),
        vec![old.clone()]
    );
    let archived = service.archive_inactive_tasks(&[old.id()], as_of).unwrap();
    assert_eq!(archived.len(), 1);
    assert!(archived[0].is_archived());
    assert!(service.tasks(TaskOrdering::default())[0].task.is_archived());
}
