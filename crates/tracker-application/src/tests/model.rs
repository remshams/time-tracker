use super::*;

#[test]
fn explicit_refresh_adopts_one_authoritative_snapshot_and_preserves_state_on_read_failure() {
    let first = stamped_task(1, "First", 100, 100);
    let second = stamped_task(2, "Second", 200, 200);
    let repository = MemoryRepository::with_tasks(vec![first]);
    let mut application = TrackerApplication::load(repository.clone()).unwrap();
    repository.0.borrow_mut().tasks.push(TaskListItem {
        task: second,
        latest_work_start: None,
    });
    assert_eq!(application.tasks(TaskOrdering::RecentlyCreated).len(), 1);
    application.refresh_authoritative_state().unwrap();
    assert_eq!(application.tasks(TaskOrdering::RecentlyCreated).len(), 2);

    repository.0.borrow_mut().fail_reads = true;
    assert!(application.refresh_authoritative_state().is_err());
    assert_eq!(application.tasks(TaskOrdering::RecentlyCreated).len(), 2);
}

#[test]
fn the_default_ordering_is_recently_worked() {
    assert_eq!(TaskOrdering::default(), TaskOrdering::RecentlyWorked);
}

#[test]
fn recently_worked_orders_by_latest_work_then_creation_then_id() {
    let one = stamped_task(1, "one", 100, 100);
    let two = stamped_task(2, "two", 300, 300);
    let three = stamped_task(3, "three", 200, 200);
    let repository = MemoryRepository::with_tasks(vec![one.clone(), two.clone(), three.clone()]);
    // one worked most recently, then three; two never worked.
    repository.0.borrow_mut().worklogs.extend([
        Worklog::new(
            worklog(10, one.id(), 900).id(),
            one.id(),
            at(900),
            Some(at(900)),
        )
        .unwrap(),
        Worklog::new(
            worklog(11, three.id(), 800).id(),
            three.id(),
            at(800),
            Some(at(800)),
        )
        .unwrap(),
    ]);
    let application = TrackerApplication::load(repository).unwrap();

    assert_eq!(
        ordered_names(&application, TaskOrdering::RecentlyWorked),
        ["one".to_owned(), "three".to_owned(), "two".to_owned()]
    );
}

#[test]
fn recently_worked_puts_tasks_without_worklogs_last() {
    let never = stamped_task(1, "never", 900, 900);
    let worked = stamped_task(2, "worked", 100, 100);
    let repository = MemoryRepository::with_tasks(vec![never, worked.clone()]);
    repository.0.borrow_mut().worklogs.push(
        Worklog::new(
            worklog(10, worked.id(), 50).id(),
            worked.id(),
            at(50),
            Some(at(50)),
        )
        .unwrap(),
    );
    let application = TrackerApplication::load(repository).unwrap();

    // Even a task created much later stays behind any worked task.
    assert_eq!(
        ordered_names(&application, TaskOrdering::RecentlyWorked),
        ["worked".to_owned(), "never".to_owned()]
    );
}

#[test]
fn ties_break_by_created_descending_then_id_ascending() {
    let early = stamped_task(3, "early", 100, 500);
    let late = stamped_task(1, "late", 400, 400);
    let same_created = stamped_task(2, "same created", 400, 600);
    let repository = MemoryRepository::with_tasks(vec![early, late, same_created]);
    // Every task shares the same latest work start, so the tie rules
    // decide the whole list.
    let task_ids = repository
        .0
        .borrow()
        .tasks
        .iter()
        .map(|item| item.task.id())
        .collect::<Vec<_>>();
    for (tag, task_id) in task_ids.into_iter().enumerate() {
        repository.0.borrow_mut().worklogs.push(
            Worklog::new(
                WorklogId::from_uuid(uuid::Uuid::from_u128(tag as u128 + 10)),
                task_id,
                at(700),
                Some(at(700)),
            )
            .unwrap(),
        );
    }
    let application = TrackerApplication::load(repository).unwrap();

    assert_eq!(
        ordered_names(&application, TaskOrdering::RecentlyWorked),
        [
            "late".to_owned(),
            "same created".to_owned(),
            "early".to_owned()
        ]
    );
}

#[test]
fn recently_updated_orders_by_updated_then_created_then_id() {
    let one = stamped_task(1, "one", 100, 500);
    let two = stamped_task(2, "two", 500, 900);
    let three = stamped_task(3, "three", 600, 800);
    let four = stamped_task(4, "four", 600, 800);
    let five = stamped_task(5, "five", 400, 800);
    let repository =
        MemoryRepository::with_tasks(vec![one, two.clone(), three.clone(), four.clone(), five]);
    let application = TrackerApplication::load(repository).unwrap();

    assert_eq!(
        ordered_names(&application, TaskOrdering::RecentlyUpdated),
        [
            "two".to_owned(),
            "three".to_owned(),
            "four".to_owned(),
            "five".to_owned(),
            "one".to_owned()
        ]
    );
}

#[test]
fn recently_created_orders_by_created_then_id() {
    let one = stamped_task(1, "one", 300, 900);
    let two = stamped_task(2, "two", 700, 700);
    let three = stamped_task(3, "three", 700, 800);
    let repository = MemoryRepository::with_tasks(vec![one, two, three]);
    let application = TrackerApplication::load(repository).unwrap();

    assert_eq!(
        ordered_names(&application, TaskOrdering::RecentlyCreated),
        ["two".to_owned(), "three".to_owned(), "one".to_owned()]
    );
}

#[test]
fn load_aligns_an_active_worklog_missing_from_the_task_aggregate_read() {
    let active_task = stamped_task(1, "active", 100, 100);
    let newer_unworked = stamped_task(2, "newer unworked", 500, 500);
    let repository = MemoryRepository::with_tasks(vec![active_task.clone(), newer_unworked]);
    repository
        .0
        .borrow_mut()
        .worklogs
        .push(worklog(10, active_task.id(), 900));

    let application = TrackerApplication::load(repository).unwrap();

    assert_eq!(
        ordered_names(&application, TaskOrdering::RecentlyWorked),
        ["active".to_owned(), "newer unworked".to_owned()]
    );
}

#[test]
fn load_exposes_tasks_and_recovered_tracking() {
    let alpha = task(1, "alpha");
    let repository = MemoryRepository::with_tasks(vec![alpha.clone()]);
    repository
        .0
        .borrow_mut()
        .worklogs
        .push(worklog(10, alpha.id(), 100));
    let application = TrackerApplication::load(repository).unwrap();
    assert_eq!(
        application
            .tasks(TaskOrdering::RecentlyWorked)
            .into_iter()
            .map(|item| item.task)
            .collect::<Vec<_>>(),
        std::slice::from_ref(&alpha)
    );
    assert_eq!(application.task(alpha.id()), Some(&alpha));
    assert_eq!(
        application.task(TaskId::from_uuid(uuid::Uuid::from_u128(99))),
        None
    );
    assert_eq!(
        application.current_tracking(),
        &TrackingState::Running {
            worklog: ActiveWorklog::begin(
                WorklogId::from_uuid(uuid::Uuid::from_u128(10)),
                alpha.id(),
                at(100)
            )
        }
    );
}

#[test]
fn snapshot_alignment_keeps_an_active_task_recently_worked() {
    let active_task = stamped_task(1, "active", 100, 100);
    let other_task = stamped_task(2, "other", 500, 500);
    let active = worklog(10, active_task.id(), 200);
    let (items, _) = TrackerApplication::<MemoryRepository>::state_from_snapshot(TrackerSnapshot {
        task_items: vec![
            TaskListItem {
                task: active_task.clone(),
                latest_work_start: None,
            },
            TaskListItem {
                task: other_task.clone(),
                latest_work_start: None,
            },
        ],
        active_worklog: Some(active),
    })
    .unwrap();
    assert_eq!(
        items
            .into_iter()
            .find(|item| item.task.id() == active_task.id())
            .unwrap()
            .latest_work_start,
        Some(at(200))
    );
}
