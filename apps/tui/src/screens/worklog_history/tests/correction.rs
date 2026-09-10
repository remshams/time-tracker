//! Tests for correction behavior.

use super::*;

#[test]
fn correction_prefills_local_minutes() {
    let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
    let start = DateTime::from_timestamp(100, 123_456_000).unwrap();
    let end = DateTime::from_timestamp(200, 654_321_000).unwrap();
    let completed = Worklog::new(worklog_id(10), task_id, start, Some(end)).unwrap();
    let app = correction_app(completed.clone(), vec![completed]);
    let draft = app.correction().unwrap();
    assert_eq!(draft.start().text(), "1970-01-01 02:01");
    assert_eq!(draft.end().unwrap().text(), "1970-01-01 02:03");

    let active = Worklog::begin(worklog_id(11), task_id, start);
    let mut app = correction_app(active.clone(), vec![active]);
    assert!(app.correction().unwrap().end().is_none());
    app.handle(Command::SwitchCorrectionField);
    assert_eq!(app.correction().unwrap().focused(), CorrectionField::Start);
}
#[test]
fn correction_switches_fields_and_edits_at_a_bounded_character_cursor() {
    let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
    let worklog = history_worklog(10, task_id, 100);
    let mut app = correction_app(worklog.clone(), vec![worklog]);
    app.handle(Command::SwitchCorrectionField);
    assert_eq!(app.correction().unwrap().focused(), CorrectionField::End);
    let original = app.correction().unwrap().end().unwrap().text().to_owned();
    let original_len = original.chars().count();
    app.handle(Command::MoveCursorLeft);
    assert_eq!(
        app.correction().unwrap().end().unwrap().cursor(),
        original_len - 1
    );
    app.handle(Command::Backspace);
    app.handle(Command::Insert('9'));
    assert_ne!(app.correction().unwrap().end().unwrap().text(), original);

    let cursor = app.correction().unwrap().end().unwrap().cursor();
    app.handle(Command::MoveCursorRight);
    assert_eq!(
        app.correction().unwrap().end().unwrap().cursor(),
        cursor + 1
    );
    app.handle(Command::MoveCursorLeft);
    let before_delete = app.correction().unwrap().end().unwrap().text().to_owned();
    app.handle(Command::Delete);
    assert_ne!(
        app.correction().unwrap().end().unwrap().text(),
        before_delete
    );
    app.handle(Command::SwitchCorrectionField);
    assert_eq!(app.correction().unwrap().focused(), CorrectionField::Start);

    let mut input = TimestampInput::new("1🕒".to_owned());
    input.backspace();
    assert_eq!(input.text(), "1");
    input.insert('x');
    assert_eq!(input.text(), "1", "invalid timestamp text is ignored");
    for _ in 0..100 {
        input.insert('2');
    }
    assert_eq!(input.text().chars().count(), TimestampInput::MAX_LEN);
}
#[test]
fn correction_parsing_requires_local_minute_format() {
    let timezone = FixedOffset::east_opt(2 * 3600).unwrap();
    assert_eq!(
        parse_correction_timestamp("1970-01-01 02:00", &timezone, None).unwrap(),
        DateTime::from_timestamp(0, 0).unwrap()
    );
    for invalid in [
        "1970-01-01 00:00+02:00",
        "1970-01-01 00:00:01",
        "1970-01-01 00:00:00",
        "+9999-01-01 00:00",
        "10000-01-01 00:00",
        "not-a-timestamp",
    ] {
        assert_eq!(
            parse_correction_timestamp(invalid, &timezone, None),
            Err("Use YYYY-MM-DD HH:MM")
        );
    }
}
#[test]
fn correction_years_format_and_parse_with_chrono_strict_expanded_form() {
    let timezone = FixedOffset::east_opt(0).unwrap();
    for (year, expected) in [
        (-1, "-0001-01-02 03:04"),
        (0, "0000-01-02 03:04"),
        (9999, "9999-01-02 03:04"),
        (10000, "+10000-01-02 03:04"),
    ] {
        let instant = Utc.with_ymd_and_hms(year, 1, 2, 3, 4, 0).single().unwrap();
        assert_eq!(
            correction_timestamp(instant, &timezone).as_deref(),
            Some(expected)
        );
        assert_eq!(
            parse_correction_timestamp(expected, &timezone, None),
            Ok(instant)
        );
    }
}
#[test]
fn an_unparseable_adjustment_keeps_the_draft() {
    let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
    let worklog = history_worklog(10, task_id, 100);
    let mut app = correction_app(worklog.clone(), vec![worklog]);
    app.correction_mut_for_tests()
        .start
        .replace("bad".to_owned());
    let before = app.correction().unwrap().clone();
    app.handle(Command::AdjustForwardFiveMinutes);
    assert_eq!(app.correction().unwrap(), &before);
    assert_eq!(text(app.status()), "Use YYYY-MM-DD HH:MM");
}
#[test]
fn escape_cancels_correction_without_writing() {
    let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
    let worklog = history_worklog(10, task_id, 100);
    let mut app = correction_app(worklog.clone(), vec![worklog]);
    app.handle(Command::Insert('2'));
    app.handle(Command::Cancel);
    assert!(
        matches!(&app.screen, ScreenState::WorklogHistory(state) if matches!(state.mode(), WorklogHistoryMode::Normal))
    );
    assert_eq!(
        app.status(),
        &Status::Info("Correction cancelled".to_owned())
    );
    assert!(app.application.correction_calls.is_empty());
}
#[test]
fn correction_failure_reports_write_and_recovery_causes() {
    let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
    let worklog = history_worklog(10, task_id, 100);
    let mut app = correction_app(worklog.clone(), vec![worklog]);
    app.application.correction_error = Some(
        ApplicationError::correction_changed_with_recovery_failure(worklog_id(10), "reload failed"),
    );

    app.handle(Command::Confirm);

    assert_eq!(
        text(app.status()),
        "Worklog changed. State recovery also failed: Storage error. Cancel and press r to refresh."
    );
    assert!(app.correction().is_some());
}
#[test]
fn overlap_recovery_failure_keeps_the_overlap_error_text() {
    let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
    let worklog = history_worklog(10, task_id, 100);
    let mut app = correction_app(worklog.clone(), vec![worklog]);
    app.application.correction_error = Some(
        ApplicationError::correction_overlap_with_recovery_failure(worklog_id(10), "reload failed"),
    );

    app.handle(Command::Confirm);

    assert_eq!(
        text(app.status()),
        "The corrected time overlaps another worklog. State recovery also failed: Storage error."
    );
}
#[test]
fn quitting_from_correction_does_not_write() {
    let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
    let worklog = history_worklog(10, task_id, 100);
    let mut app = correction_app(worklog.clone(), vec![worklog]);
    app.handle(Command::Quit);
    assert!(!app.is_running());
    assert!(app.application.correction_calls.is_empty());
    assert!(app.correction().is_some());
}
#[test]
fn every_correction_failure_keeps_the_full_draft_open() {
    let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
    let worklog = history_worklog(10, task_id, 100);
    let failures = [
        ApplicationError::InvalidWorklogCorrection(WorklogCorrectionError::EndBeforeStart),
        ApplicationError::worklog_overlap(worklog.id()),
        ApplicationError::storage_failure("write failed"),
        ApplicationError::correction_changed_with_recovery_failure(worklog.id(), "reload failed"),
        ApplicationError::worklog_changed(worklog.id()),
    ];
    for failure in failures {
        let mut app = correction_app(worklog.clone(), vec![worklog.clone()]);
        app.application.correction_error = Some(failure.clone());
        let before = app.correction().unwrap().clone();
        app.handle(Command::Confirm);
        assert_eq!(
            app.correction().unwrap(),
            &before,
            "draft changed for {failure:?}"
        );
        let classified = failure.failure();
        if classified.category() == ApplicationFailureCategory::WorklogChanged
            && !classified.recovery_failed()
        {
            assert_eq!(
                text(app.status()),
                "Worklog changed. Cancel and press r to refresh."
            );
        }
    }
}
#[test]
fn parse_failures_keep_both_drafts_and_skip_the_application() {
    let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
    let worklog = history_worklog(10, task_id, 100);
    let mut app = correction_app(worklog.clone(), vec![worklog]);
    app.correction_mut_for_tests()
        .start
        .replace("bad".to_owned());
    let before = app.correction().unwrap().clone();
    app.handle(Command::Confirm);
    assert_eq!(app.correction().unwrap(), &before);
    assert!(app.application.correction_calls.is_empty());
}
#[test]
fn successful_correction_discards_older_pages_and_resolves_selection() {
    let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
    let corrected = history_worklog(10, task_id, 100);
    let newest = history_worklog(11, task_id, 300);
    let mut app = correction_app(corrected.clone(), vec![newest.clone(), corrected.clone()]);
    app.handle(Command::Confirm);
    assert!(
        matches!(&app.screen, ScreenState::WorklogHistory(state) if matches!(state.mode(), WorklogHistoryMode::Normal))
    );
    assert_eq!(app.history().unwrap().worklogs.len(), 2);
    assert_eq!(app.history().unwrap().next_cursor, None);
    assert_eq!(app.history_selected_index(), Some(1));
    assert_eq!(app.application.worklog_reads.get(), 2);
    let (id, expected, replacement, occurred_at) = app.application.correction_calls[0];
    assert_eq!(id, corrected.id());
    assert_eq!(expected, corrected.times());
    assert_eq!(replacement, corrected.times());
    assert!(occurred_at <= Utc::now());

    let mut app = correction_app(corrected.clone(), vec![newest.clone()]);
    app.handle(Command::Confirm);
    assert_eq!(app.history().unwrap().selected, Some(newest.id()));
    assert_eq!(app.history_selected_index(), Some(0));
}
#[test]
fn correction_and_history_commands_require_their_own_screen_and_mode() {
    let task = task(1, "alpha");
    let worklog = history_worklog(10, task.id(), 100);
    let mut service = TestService::with_tasks(vec![task]);
    service.worklog_pages = vec![Ok(page(vec![worklog], None))];
    let mut app = App::load(service);
    app.handle(Command::OpenHistory);
    app.handle(Command::BackToTaskList);
    app.handle(Command::OpenCorrection);
    assert!(
        matches!(&app.screen, ScreenState::TaskList(state) if matches!(state.mode(), TaskListMode::Normal))
    );
    assert_eq!(app.screen(), Screen::TaskList);

    let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
    let worklog = history_worklog(10, task_id, 100);
    let mut app = correction_app(worklog.clone(), vec![worklog]);
    let reads = app.application.worklog_reads.get();
    app.handle(Command::LoadOlderWorklogs);
    assert_eq!(app.application.worklog_reads.get(), reads);
    app.handle(Command::RefreshWorklogs);
    assert_eq!(app.application.worklog_reads.get(), reads);
    app.handle(Command::BackToTaskList);
    assert_eq!(app.screen(), Screen::WorklogHistory);
    assert!(app.correction().is_some());
}
#[test]
fn saved_correction_marks_history_unavailable_until_retry_succeeds() {
    let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
    let original = history_worklog(10, task_id, 100);
    let corrected = Worklog::new(worklog_id(10), task_id, at(110), Some(at(170))).unwrap();
    let mut service = TestService::with_tasks(vec![task(1, "alpha")]);
    service.worklog_pages = vec![
        Ok(page(vec![original.clone()], Some(cursor(100, 10)))),
        Err(TestService::failure()),
        Ok(page(vec![corrected.clone()], None)),
    ];
    let mut app = App::load(service);
    app.freeze_offset_for_tests(FixedOffset::east_opt(0).unwrap());
    app.handle(Command::OpenHistory);
    app.handle(Command::OpenCorrection);
    {
        let draft = app.correction_mut_for_tests();
        draft.start.replace("1970-01-01 00:01".to_owned());
        draft
            .end
            .as_mut()
            .unwrap()
            .replace("1970-01-01 00:02".to_owned());
    }

    app.handle(Command::Confirm);

    let history = app.history().unwrap();
    assert_eq!(history.availability, HistoryAvailability::Unavailable);
    assert!(!history.is_available());
    assert!(history.worklogs.is_empty());
    assert_eq!(history.next_cursor, None);
    assert_eq!(history.selected, Some(original.id()));
    assert_eq!(app.history_selected_index(), None);
    assert!(
        matches!(&app.screen, ScreenState::WorklogHistory(state) if matches!(state.mode(), WorklogHistoryMode::Normal))
    );
    assert_eq!(
        app.status(),
        &Status::Error("Correction saved, but history refresh failed".to_owned())
    );

    app.handle(Command::MoveDown);
    app.handle(Command::MoveUp);
    assert_eq!(app.history().unwrap().selected, Some(original.id()));

    app.handle(Command::OpenCorrection);
    app.handle(Command::LoadOlderWorklogs);
    assert_eq!(app.application.worklog_reads.get(), 2);
    assert!(app.correction().is_none());

    app.handle(Command::RefreshWorklogs);
    let history = app.history().unwrap();
    assert_eq!(history.availability, HistoryAvailability::Available);
    assert_eq!(history.worklogs, vec![corrected]);
    assert_eq!(history.next_cursor, None);
    assert_eq!(app.history_selected_index(), Some(0));
    assert_eq!(app.status(), &Status::Info("Refreshed".to_owned()));
}
#[test]
fn failed_post_save_reload_keeps_the_externally_corrected_active_aggregate() {
    let active_task = task(1, "active");
    let corrected_task = task(2, "corrected");
    let active = Worklog::begin(worklog_id(10), active_task.id(), at(600));
    let corrected = history_worklog(11, corrected_task.id(), 480);
    let mut service = TestService::with_tasks(vec![active_task.clone(), corrected_task.clone()]);
    service.latest_work_starts = vec![(active_task.id(), at(600)), (corrected_task.id(), at(480))];
    service.tracking = TrackingState::Running {
        worklog: ActiveWorklog::begin(active.id(), active.task_id(), active.start()),
    };
    service.worklog_pages = vec![
        Ok(page_with_active(
            vec![corrected.clone()],
            Some(active.clone()),
            None,
        )),
        Err(TestService::failure()),
    ];
    let mut app = App::load(service);
    app.freeze_offset_for_tests(FixedOffset::east_opt(0).unwrap());
    app.handle(Command::MoveDown);
    app.handle(Command::OpenHistory);
    app.handle(Command::OpenCorrection);
    app.application.latest_work_starts[0].1 = at(540);
    app.application.tracking = TrackingState::Running {
        worklog: ActiveWorklog::begin(active.id(), active.task_id(), at(540)),
    };
    {
        let draft = app.correction_mut_for_tests();
        draft.start.replace("1970-01-01 00:07".to_owned());
        draft
            .end
            .as_mut()
            .unwrap()
            .replace("1970-01-01 00:08".to_owned());
    }

    app.handle(Command::Confirm);

    assert_eq!(
        app.application
            .tasks(TaskOrdering::RecentlyWorked)
            .into_iter()
            .find(|item| item.task.id() == active_task.id())
            .unwrap()
            .latest_work_start,
        Some(at(540))
    );
    assert_eq!(
        app.status(),
        &Status::Error("Correction saved, but history refresh failed".to_owned())
    );
}
#[test]
fn correction_is_available_from_archived_history() {
    let mut archived = task(1, "archived");
    archived.archive(at(200));
    let worklog = history_worklog(10, archived.id(), 100);
    let mut service = TestService::with_tasks(vec![archived]);
    service.worklog_pages = vec![
        Ok(page(vec![worklog.clone()], None)),
        Ok(page(vec![worklog], None)),
    ];
    let mut app = App::load(service);
    app.handle(Command::ShowArchivedTasks);
    app.handle(Command::OpenHistory);
    app.handle(Command::OpenCorrection);
    assert!(app.correction().is_some());
    app.handle(Command::Confirm);
    assert!(
        matches!(&app.screen, ScreenState::WorklogHistory(state) if matches!(state.mode(), WorklogHistoryMode::Normal))
    );
    assert_eq!(app.view(), TaskView::Archived);
}
#[test]
fn successful_correction_refreshes_recently_worked_ordering() {
    let repository = SqliteRepository::open_in_memory().unwrap();
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    repository.create_task(alpha.clone()).unwrap();
    repository.create_task(beta.clone()).unwrap();
    repository
        .insert_worklog(&history_worklog(10, alpha.id(), 200))
        .unwrap();
    repository
        .insert_worklog(&history_worklog(11, beta.id(), 300))
        .unwrap();
    let mut app = App::load(TrackerApplication::load(repository).unwrap());
    app.freeze_offset_for_tests(FixedOffset::east_opt(0).unwrap());
    assert_eq!(app.tasks()[0].id(), beta.id());
    app.handle(Command::OpenHistory);
    app.handle(Command::OpenCorrection);
    {
        let draft = app.correction_mut_for_tests();
        draft.start.replace("1970-01-01 00:01".to_owned());
        draft
            .end
            .as_mut()
            .unwrap()
            .replace("1970-01-01 00:02".to_owned());
    }
    app.handle(Command::Confirm);
    assert_eq!(app.tasks()[0].id(), alpha.id());
    assert_eq!(app.tasks()[1].id(), beta.id());
}
#[test]
fn correction_reload_uses_the_final_active_start_for_elapsed_and_stopping() {
    let task = task(1, "alpha");
    let now = Utc::now();
    let initial = Worklog::begin(worklog_id(10), task.id(), now - TimeDelta::minutes(10));
    let correction_start = now - TimeDelta::minutes(1);
    let final_start = now - TimeDelta::minutes(2);
    let final_active = Worklog::begin(worklog_id(10), task.id(), final_start);
    let mut service = TestService::with_tasks(vec![task]);
    service.worklog_pages = vec![
        Ok(page_with_active(vec![initial.clone()], Some(initial), None)),
        Ok(page_with_active(
            vec![final_active.clone()],
            Some(final_active.clone()),
            None,
        )),
    ];
    let mut app = App::load(service);
    app.freeze_offset_for_tests(FixedOffset::east_opt(0).unwrap());
    app.handle(Command::OpenHistory);
    app.handle(Command::OpenCorrection);
    app.correction_mut_for_tests().start.replace(
        correction_timestamp(correction_start, &FixedOffset::east_opt(0).unwrap())
            .expect("the test timestamp is representable"),
    );

    app.handle(Command::Confirm);

    app.freeze_elapsed_for_tests(Duration::from_secs(180));
    assert!(app.elapsed().unwrap() >= Duration::from_secs(180));
    assert_eq!(app.history_row_duration(&final_active).as_secs(), 180);
    app.handle(Command::BackToTaskList);
    app.handle(Command::ToggleTracking);
    let (_, stopped_at) = app.application.clear_calls[0];
    assert!(stopped_at >= final_start + TimeDelta::seconds(180));
    assert!(stopped_at < final_start + TimeDelta::seconds(181));
}
#[test]
fn completed_correction_preserves_a_frozen_timer_across_a_wall_clock_jump() {
    let task = task(1, "alpha");
    let active = Worklog::begin(worklog_id(10), task.id(), at(100));
    let completed = Worklog::new(worklog_id(11), task.id(), at(50), Some(at(60))).unwrap();
    let mut service = TestService::with_tasks(vec![task]);
    service.worklog_pages = vec![
        Ok(page_with_active(
            vec![active.clone(), completed.clone()],
            Some(active.clone()),
            None,
        )),
        Ok(page_with_active(
            vec![active.clone(), completed],
            Some(active.clone()),
            None,
        )),
    ];
    let mut app = App::load(service);
    app.freeze_offset_for_tests(FixedOffset::east_opt(0).unwrap());
    app.handle(Command::OpenHistory);
    app.handle(Command::MoveDown);
    app.handle(Command::OpenCorrection);
    app.freeze_elapsed_for_tests(Duration::from_secs(600));

    app.handle(Command::Confirm);
    app.sync_tracking_after_history_reload_at(at(10_000), Instant::now());

    app.handle(Command::BackToTaskList);
    app.handle(Command::ToggleTracking);
    let (_, stopped_at) = app.application.clear_calls[0];
    assert!(stopped_at >= at(700));
    assert!(stopped_at < at(701));
}
#[test]
fn correcting_active_start_reanchors_header_and_running_row_without_negatives() {
    let task = task(1, "alpha");
    let now = Utc::now();
    let original_start = now - TimeDelta::minutes(10);
    let corrected_start = now - TimeDelta::minutes(1);
    let original = Worklog::begin(worklog_id(10), task.id(), original_start);
    let corrected = Worklog::begin(worklog_id(10), task.id(), corrected_start);
    let mut service = TestService::with_tasks(vec![task.clone()]);
    service.tracking = TrackingState::Running {
        worklog: ActiveWorklog::begin(original.id(), task.id(), original_start),
    };
    service.worklog_pages = vec![
        Ok(page(vec![original], None)),
        Ok(page(vec![corrected.clone()], None)),
    ];
    let mut app = App::load(service);
    app.freeze_offset_for_tests(FixedOffset::east_opt(0).unwrap());
    app.handle(Command::OpenHistory);
    app.handle(Command::OpenCorrection);
    app.correction_mut_for_tests().start.replace(
        correction_timestamp(corrected_start, &FixedOffset::east_opt(0).unwrap())
            .expect("the test timestamp is representable"),
    );
    app.handle(Command::Confirm);

    let header = app.elapsed().unwrap();
    let row = app.history_row_duration(&corrected);
    assert!(header >= Duration::from_secs(59) && header < Duration::from_secs(62));
    assert!(row >= Duration::from_secs(59) && row < Duration::from_secs(62));

    assert!(app.elapsed().unwrap() >= Duration::from_secs(59));
}
