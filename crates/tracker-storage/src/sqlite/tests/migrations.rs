// SQLite migrations integration tests.

use super::*;

#[test]
fn migrations_create_the_schema_triggers_and_are_idempotent() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("nested").join("tracker.db");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();

    {
        let repository = SqliteRepository::open(&path).unwrap();
        repository
            .create_task(stamped_task(1, "first", 100, 100))
            .unwrap();
    }
    // Reopening applies no migration again and keeps the data.
    let reopened = SqliteRepository::open(&path).unwrap();
    assert_eq!(user_version(&reopened), 6);
    let tasks = reopened.list_tasks().unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].name().as_str(), "first");
    let objects: Vec<(String, String)> = reopened
        .connection()
        .prepare("SELECT name, type FROM sqlite_master WHERE name LIKE 'worklogs%' ORDER BY name")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(objects.contains(&("worklogs".to_owned(), "table".to_owned())));
    assert!(objects.contains(&("worklogs_task_start".to_owned(), "index".to_owned())));
    assert!(objects.contains(&("worklogs_global_start".to_owned(), "index".to_owned())));
    for trigger in [
        "worklogs_bump_global_revision_insert",
        "worklogs_bump_global_revision_update",
        "worklogs_bump_global_revision_delete",
    ] {
        assert!(objects.contains(&(trigger.to_owned(), "trigger".to_owned())));
    }
    assert!(objects.contains(&(
        "worklogs_reject_archived_task".to_owned(),
        "trigger".to_owned()
    )));
    assert!(objects.contains(&(
        "worklogs_reject_same_task_overlap_insert".to_owned(),
        "trigger".to_owned()
    )));
    assert!(objects.contains(&(
        "worklogs_reject_same_task_overlap_update".to_owned(),
        "trigger".to_owned()
    )));
    assert!(objects.contains(&(
        "worklogs_reject_active_delete".to_owned(),
        "trigger".to_owned()
    )));
    let active_delete_trigger: String = reopened
        .connection()
        .query_row(
            "SELECT sql FROM sqlite_master
             WHERE type = 'trigger' AND name = 'worklogs_reject_active_delete'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(active_delete_trigger.contains("BEFORE DELETE ON worklogs"));
    assert!(active_delete_trigger.contains("OLD.end_us IS NULL"));
    let revision_trigger: String = reopened
        .connection()
        .query_row(
            "SELECT sql FROM sqlite_master
             WHERE type = 'trigger' AND name = 'worklogs_bump_history_order_revision'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(revision_trigger.contains("AFTER UPDATE OF task_id, start_us"));
    assert!(revision_trigger.contains("id = OLD.task_id"));
    assert!(revision_trigger.contains("id = NEW.task_id"));
}

#[test]
fn version_5_database_gains_global_history_without_changing_worklogs() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    {
        let repository = SqliteRepository::open(&path).unwrap();
        repository.create_task(named_task(1, "existing")).unwrap();
        repository
            .connection()
            .execute(
                "INSERT INTO worklogs (id, task_id, start_us, end_us) VALUES (?1, ?2, ?3, ?3)",
                rusqlite::params![
                    worklog_id(1).to_string(),
                    task_id(1).to_string(),
                    at(200).timestamp_micros()
                ],
            )
            .unwrap();
        repository
            .connection()
            .execute_batch(
                "DROP TRIGGER worklogs_bump_global_revision_insert;
             DROP TRIGGER worklogs_bump_global_revision_update;
             DROP TRIGGER worklogs_bump_global_revision_delete;
             DROP INDEX worklogs_global_start;
             DROP TABLE global_worklog_revision;
             PRAGMA user_version = 5;",
            )
            .unwrap();
    }
    let repository = SqliteRepository::open(&path).unwrap();
    assert_eq!(user_version(&repository), 6);
    assert_eq!(
        repository.global_worklog_page(None).unwrap().worklogs[0].id(),
        worklog_id(1)
    );
    let revision: i64 = repository
        .connection()
        .query_row(
            "SELECT revision FROM global_worklog_revision WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(revision, 0);
}

#[test]
fn a_version_1_database_migrates_to_version_6_and_keeps_every_record() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    create_v1_database(&path);

    // The backfill timestamp is captured between these two readings.
    let before = Utc::now();
    let repository = SqliteRepository::open(&path).unwrap();
    let after = Utc::now();
    assert_eq!(user_version(&repository), 6);
    let backfill = repository.list_tasks().unwrap()[0].created_at();

    // Every task id, name, and archive flag survived.
    let tasks = repository.list_tasks().unwrap();
    let names: Vec<(String, bool)> = tasks
        .iter()
        .map(|task| (task.name().to_string(), task.is_archived()))
        .collect();
    assert_eq!(
        names,
        [
            ("Write release notes".to_owned(), false),
            ("Fix the coffee machine".to_owned(), true),
            ("Plan Friday's demo".to_owned(), false),
        ]
    );
    assert_eq!(tasks[0].id(), task_id(1));
    assert_eq!(tasks[1].id(), task_id(2));
    assert_eq!(tasks[2].id(), task_id(3));

    // Every v1 task is backfilled with one timestamp captured at migration
    // time: the same value on every row, created_at equal to updated_at,
    // inside the window this open observed.
    for task in &tasks {
        assert_eq!(task.created_at(), task.updated_at());
        assert_eq!(task.created_at(), backfill, "every migrated row shares it");
    }
    assert!(backfill >= before && backfill <= after, "got {backfill}");

    // Every worklog survived, including the active one.
    let worklogs = repository.list_worklogs(task_id(1)).unwrap();
    assert_eq!(worklogs.len(), 2);
    assert_eq!(worklogs[0].id(), worklog_id(11));
    assert_eq!(worklogs[0].start(), at(50));
    assert_eq!(worklogs[0].end(), Some(at(75)));
    assert_eq!(worklogs[1].id(), worklog_id(10));
    assert_eq!(worklogs[1].start(), at(100));
    assert_eq!(worklogs[1].end(), None);
    assert_eq!(
        repository.active_worklog().unwrap().unwrap().id(),
        worklog_id(10)
    );

    // The read model derives the latest work start from the migrated rows.
    let items = repository.list_task_items().unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].latest_work_start, Some(at(100)));
    assert_eq!(items[1].latest_work_start, None);
    assert_eq!(items[2].latest_work_start, None);

    // The rebuilt schema enforces its invariants again: a second active
    // worklog is rejected, and so is archiving the task that runs one.
    let second = Worklog::begin(worklog_id(42), task_id(3), at(300));
    assert!(matches!(
        repository.insert_worklog(&second),
        Err(StorageError::ActiveWorklogExists)
    ));
    assert!(matches!(
        repository.archive_task(task_id(1), at(300)),
        Err(StorageError::TaskIsActive { id }) if id == task_id(1)
    ));
}

#[test]
fn version_2_migration_preserves_identity_and_timestamp_values_exactly() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let intervals = [
        (10, 1_000_001, Some(1_000_007)),
        (11, 1_000_007, None),
        (12, 1_000_004, Some(1_000_004)),
    ];
    create_v2_database(&path, &intervals);

    let repository = SqliteRepository::open(&path).unwrap();

    assert_eq!(user_version(&repository), 6);
    let task_values: (String, i64, i64) = repository
        .connection()
        .query_row(
            "SELECT id, created_at_us, updated_at_us FROM tasks WHERE id = ?1",
            [task_id(1).to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(task_values, (task_id(1).to_string(), 1_234_567, 2_345_678));
    let worklog_values: Vec<(String, String, i64, Option<i64>)> = repository
        .connection()
        .prepare("SELECT id, task_id, start_us, end_us FROM worklogs ORDER BY id")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        worklog_values,
        vec![
            (
                worklog_id(10).to_string(),
                task_id(1).to_string(),
                1_000_001,
                Some(1_000_007)
            ),
            (
                worklog_id(11).to_string(),
                task_id(1).to_string(),
                1_000_007,
                None
            ),
            (
                worklog_id(12).to_string(),
                task_id(1).to_string(),
                1_000_004,
                Some(1_000_004)
            ),
        ]
    );
}

#[test]
fn version_2_migration_rejects_overlap_and_rolls_back_every_schema_change() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    create_v2_database(&path, &[(10, 100, Some(200)), (11, 150, Some(250))]);

    let error = SqliteRepository::open(&path).expect_err("overlapping v2 rows must fail migration");
    assert!(matches!(
        error,
        StorageError::CorruptData("same-task worklog overlap")
    ));

    let connection = rusqlite::Connection::open(&path).unwrap();
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 2);
    let rows: Vec<(String, i64, Option<i64>)> = connection
        .prepare("SELECT id, start_us, end_us FROM worklogs ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        rows,
        vec![
            (worklog_id(10).to_string(), 100, Some(200)),
            (worklog_id(11).to_string(), 150, Some(250)),
        ]
    );
    let overlap_triggers: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'trigger' AND name LIKE 'worklogs_reject_same_task_overlap_%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(overlap_triggers, 0);
}

#[test]
fn a_migration_is_not_applied_twice() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    create_v1_database(&path);
    let first = SqliteRepository::open(&path).unwrap();
    let task = first
        .find_task(task_id(1))
        .unwrap()
        .expect("the migrated task is stored");
    drop(first);

    let second = SqliteRepository::open(&path).unwrap();
    assert_eq!(user_version(&second), 6);
    assert_eq!(second.find_task(task_id(1)).unwrap(), Some(task));
}

#[test]
fn strict_tables_reject_null_ids_and_non_integer_timestamps() {
    let repository = repo();
    let conn = repository.connection();
    assert!(
        conn.execute(
            "INSERT INTO tasks (id, name, archived) VALUES (NULL, 'name', 0)",
            []
        )
        .is_err()
    );
    // The timestamp columns are strict integers and cannot be missing.
    assert!(
        conn.execute(
            "INSERT INTO tasks (id, name, archived) VALUES
             ('00000000-0000-7000-8000-000000000001', 'name', 0)",
            [],
        )
        .is_err(),
        "the NOT NULL timestamp columns must reject a timestamp-less insert"
    );
    assert!(
        conn.execute(
            "INSERT INTO tasks (id, name, archived, created_at_us, updated_at_us) VALUES
             ('00000000-0000-7000-8000-000000000001', 'name', 0, 'not an integer', 0)",
            [],
        )
        .is_err()
    );
    // updated_at can never precede created_at.
    assert!(
        conn.execute(
            "INSERT INTO tasks (id, name, archived, created_at_us, updated_at_us) VALUES
             ('00000000-0000-7000-8000-000000000001', 'name', 0, 200, 199)",
            [],
        )
        .is_err(),
        "the updated >= created CHECK must reject a backwards pair"
    );
    conn.execute(
        "INSERT INTO tasks (id, name, archived, created_at_us, updated_at_us) VALUES
         ('00000000-0000-7000-8000-000000000001', 'name', 0, 0, 0)",
        [],
    )
    .unwrap();
    assert!(
        conn.execute(
            "INSERT INTO worklogs (id, task_id, start_us, end_us)
             VALUES (NULL, '00000000-0000-7000-8000-000000000001', 1, NULL)",
            [],
        )
        .is_err()
    );
    assert!(
        conn.execute(
            "INSERT INTO worklogs (id, task_id, start_us, end_us)
             VALUES ('00000000-0000-7000-8000-000000000002',
                     '00000000-0000-7000-8000-000000000001', 'not an integer', NULL)",
            [],
        )
        .is_err()
    );
}

#[test]
fn a_database_from_a_newer_version_is_rejected_without_changes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    SqliteRepository::open(&path).unwrap();
    {
        let repository = SqliteRepository::open(&path).unwrap();
        repository
            .connection()
            .pragma_update(None, "user_version", 99)
            .unwrap();
    }
    let error = SqliteRepository::open(&path).expect_err("a future schema must be rejected");
    match error {
        StorageError::DatabaseTooNew { found, latest } => {
            assert_eq!(found, 99);
            assert_eq!(latest, 6);
            assert!(error.to_string().contains("newer"));
        }
        other => panic!("expected DatabaseTooNew, got {other:?}"),
    }
    // The file is untouched: the version is still the future one.
    let raw = rusqlite::Connection::open(&path).unwrap();
    let version: i64 = raw
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 99);
}

#[test]
fn a_version_4_database_gains_the_active_delete_guard() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let task = named_task(1, "migrated");
    let completed = Worklog::new(worklog_id(1), task.id(), at(50), Some(at(75))).unwrap();
    let active = Worklog::begin(worklog_id(2), task.id(), at(100));
    {
        let repository = SqliteRepository::open(&path).unwrap();
        repository.create_task(task.clone()).unwrap();
        repository.insert_worklog(&completed).unwrap();
        repository.insert_worklog(&active).unwrap();
        repository
            .connection()
            .execute_batch(
                "DROP TRIGGER worklogs_reject_active_delete;
                 DROP TRIGGER worklogs_bump_global_revision_insert;
                 DROP TRIGGER worklogs_bump_global_revision_update;
                 DROP TRIGGER worklogs_bump_global_revision_delete;
                 DROP INDEX worklogs_global_start;
                 DROP TABLE global_worklog_revision;
                 PRAGMA user_version = 4;",
            )
            .unwrap();
    }

    let migrated = SqliteRepository::open(&path).unwrap();
    assert_eq!(user_version(&migrated), 6);
    assert!(
        migrated
            .connection()
            .execute(
                "DELETE FROM worklogs WHERE id = ?1",
                [active.id().to_string()],
            )
            .is_err()
    );
    migrated
        .connection()
        .execute(
            "DELETE FROM worklogs WHERE id = ?1",
            [completed.id().to_string()],
        )
        .unwrap();
    assert_eq!(migrated.find_worklog(active.id()).unwrap(), Some(active));
}

#[test]
fn overlap_migration_uses_an_ordered_indexed_sweep_for_large_valid_history() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    let rows: Vec<_> = (1..=2_000)
        .map(|tag| (tag, i64::from(tag) * 10, Some(i64::from(tag) * 10 + 1)))
        .collect();
    create_v2_database(&path, &rows);

    let repository = SqliteRepository::open(&path).unwrap();
    assert_eq!(user_version(&repository), 6);
    let plan: Vec<String> = repository
        .connection()
        .prepare("EXPLAIN QUERY PLAN SELECT id FROM worklogs WHERE task_id = ?1 ORDER BY start_us DESC, id")
        .unwrap()
        .query_map([task_id(1).to_string()], |row| row.get(3))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        plan.iter()
            .any(|detail| detail.contains("worklogs_task_start"))
    );
}

#[test]
fn version_3_database_gets_indexed_overlap_and_active_delete_guards() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tracker.db");
    create_v2_database(&path, &[(1, 100, None)]);
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO tasks (id, name, archived, created_at_us, updated_at_us)
             VALUES (?1, 'archived', 0, 100, 100)",
            [task_id(2).to_string()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO worklogs (id, task_id, start_us, end_us) VALUES (?1, ?2, 10, 20)",
            [worklog_id(2).to_string(), task_id(2).to_string()],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE tasks SET archived = 1 WHERE id = ?1",
            [task_id(2).to_string()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO tasks (id, name, archived, created_at_us, updated_at_us)
             VALUES (?1, 'open', 0, 100, 100)",
            [task_id(3).to_string()],
        )
        .unwrap();
    connection
        .execute_batch(
            "ALTER TABLE tasks ADD COLUMN history_order_revision INTEGER NOT NULL DEFAULT 0;
             CREATE TRIGGER worklogs_reject_same_task_overlap_insert
             BEFORE INSERT ON worklogs BEGIN SELECT 1; END;
             CREATE TRIGGER worklogs_reject_same_task_overlap_update
             BEFORE UPDATE OF task_id, start_us, end_us ON worklogs BEGIN SELECT 1; END;
             CREATE TRIGGER worklogs_bump_history_order_revision
             AFTER UPDATE OF start_us ON worklogs BEGIN SELECT 1; END;",
        )
        .unwrap();
    connection.pragma_update(None, "user_version", 3).unwrap();
    drop(connection);

    let repository = SqliteRepository::open(&path).unwrap();
    assert_eq!(user_version(&repository), 6);
    let active_id = worklog_id(1);
    let error = repository
        .connection()
        .execute(
            "UPDATE worklogs SET task_id = ?1 WHERE id = ?2",
            [task_id(2).to_string(), active_id.to_string()],
        )
        .expect_err("an active worklog cannot move to an archived task");
    assert!(matches!(
        error,
        rusqlite::Error::SqliteFailure(_, Some(message)) if message == "task is archived"
    ));

    // History imported before a task was archived remains editable.
    repository
        .connection()
        .execute(
            "UPDATE worklogs SET start_us = 11, end_us = 21 WHERE id = ?1",
            [worklog_id(2).to_string()],
        )
        .unwrap();

    repository
        .connection()
        .execute(
            "UPDATE worklogs SET task_id = ?1 WHERE id = ?2",
            [task_id(3).to_string(), worklog_id(2).to_string()],
        )
        .unwrap();
    assert_eq!(history_revision(&repository, task_id(2)), 2);
    assert_eq!(history_revision(&repository, task_id(3)), 1);
    repository
        .connection()
        .execute(
            "UPDATE worklogs SET start_us = 12 WHERE id = ?1",
            [worklog_id(2).to_string()],
        )
        .unwrap();
    assert_eq!(history_revision(&repository, task_id(2)), 2);
    assert_eq!(history_revision(&repository, task_id(3)), 2);
}
