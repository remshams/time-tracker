use super::*;

fn seed_global_history(repository: &SqliteRepository) {
    repository
        .create_task(named_task(1, "active task"))
        .unwrap();
    repository
        .create_task(named_task(2, "archived task"))
        .unwrap();
    for tag in (1..=55).rev() {
        let task = if tag % 2 == 0 { task_id(2) } else { task_id(1) };
        repository
            .connection()
            .execute(
                "INSERT INTO worklogs (id, task_id, start_us, end_us) VALUES (?1, ?2, ?3, ?3)",
                rusqlite::params![
                    worklog_id(tag).to_string(),
                    task.to_string(),
                    at(200).timestamp_micros()
                ],
            )
            .unwrap();
    }
    repository.archive_task(task_id(2), at(300)).unwrap();
}

#[test]
fn global_history_pages_cross_tasks_and_archived_history_without_losing_ties() {
    let repository = repo();
    seed_global_history(&repository);
    let first = repository.global_worklog_page(None).unwrap();
    assert_eq!(first.worklogs.len(), WORKLOG_PAGE_SIZE);
    assert_eq!(first.worklogs.first().unwrap().id(), worklog_id(1));
    assert_eq!(first.worklogs.last().unwrap().id(), worklog_id(50));
    assert_eq!(first.snapshot.task_items.len(), 2);
    assert!(
        first
            .snapshot
            .task_items
            .iter()
            .any(|item| item.task.id() == task_id(2) && item.task.is_archived())
    );
    let second = repository
        .global_worklog_page(first.next_cursor.as_ref())
        .unwrap();
    assert_eq!(
        second.worklogs.iter().map(Worklog::id).collect::<Vec<_>>(),
        (51..=55).map(worklog_id).collect::<Vec<_>>()
    );
    assert!(second.next_cursor.is_none());
}

#[test]
fn global_cursor_rejects_other_client_insert_update_and_delete() {
    let dir = tempfile::tempdir().unwrap();
    let reader = file_repo(&dir);
    seed_global_history(&reader);
    let writer = file_repo(&dir);

    let cursor = reader
        .global_worklog_page(None)
        .unwrap()
        .next_cursor
        .unwrap();
    writer
        .connection()
        .execute(
            "INSERT INTO worklogs (id, task_id, start_us, end_us) VALUES (?1, ?2, ?3, ?3)",
            rusqlite::params![
                worklog_id(56).to_string(),
                task_id(1).to_string(),
                at(100).timestamp_micros()
            ],
        )
        .unwrap();
    assert!(matches!(
        reader.global_worklog_page(Some(&cursor)),
        Err(StorageError::GlobalWorklogHistoryChanged)
    ));

    let cursor = reader
        .global_worklog_page(None)
        .unwrap()
        .next_cursor
        .unwrap();
    writer
        .connection()
        .execute(
            "UPDATE worklogs SET end_us = start_us + 1 WHERE id = ?1",
            [worklog_id(56).to_string()],
        )
        .unwrap();
    assert!(matches!(
        reader.global_worklog_page(Some(&cursor)),
        Err(StorageError::GlobalWorklogHistoryChanged)
    ));

    let cursor = reader
        .global_worklog_page(None)
        .unwrap()
        .next_cursor
        .unwrap();
    writer
        .connection()
        .execute(
            "DELETE FROM worklogs WHERE id = ?1",
            [worklog_id(56).to_string()],
        )
        .unwrap();
    assert!(matches!(
        reader.global_worklog_page(Some(&cursor)),
        Err(StorageError::GlobalWorklogHistoryChanged)
    ));
}

#[test]
fn global_history_reads_active_work_and_current_catalog() {
    let repository = repo();
    repository.create_task(named_task(1, "first")).unwrap();
    repository.create_task(named_task(2, "second")).unwrap();
    repository
        .insert_worklog(&Worklog::new(worklog_id(1), task_id(1), at(200), None).unwrap())
        .unwrap();
    let page = repository.global_worklog_page(None).unwrap();
    assert_eq!(page.worklogs.len(), 1);
    assert_eq!(page.snapshot.active_worklog.unwrap().id(), worklog_id(1));
    assert_eq!(page.snapshot.task_items.len(), 2);
    assert_eq!(
        page.snapshot
            .task_items
            .iter()
            .find(|item| item.task.id() == task_id(1))
            .unwrap()
            .latest_work_start,
        Some(at(200))
    );
}
