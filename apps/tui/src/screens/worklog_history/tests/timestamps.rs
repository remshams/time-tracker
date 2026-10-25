//! Tests for timestamps behavior.

use super::*;

#[test]
fn history_uses_the_session_timezone_snapshot() {
    let london = app_in_timezone(
        TestService::with_tasks(vec![task(1, "alpha")]),
        chrono_tz::Europe::London,
    );
    for seconds in [0, 1_700_000_000] {
        assert_eq!(
            london.app_view().local_time(at(seconds)),
            crate::support::timestamps::local_time(at(seconds), &chrono_tz::Europe::London)
        );
    }
    let johannesburg = app_in_timezone(
        TestService::with_tasks(vec![task(1, "alpha")]),
        chrono_tz::Africa::Johannesburg,
    );
    assert_eq!(
        johannesburg.app_view().local_time(at(0)),
        "1970-01-01 02:00"
    );
}
#[test]
fn correction_commands_adjust_and_edit_the_focused_timestamp() {
    let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
    let worklog = history_worklog(10, task_id, 100);
    let mut app = correction_app(worklog.clone(), vec![worklog.clone()]);

    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::AdjustForwardOneHour,
    ));
    assert_eq!(
        app.app_view().correction().unwrap().start().text(),
        "1970-01-01 03:01"
    );

    let mut app = correction_app(worklog.clone(), vec![worklog]);
    let original = app
        .app_view()
        .correction()
        .unwrap()
        .start()
        .text()
        .to_owned();
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Backspace));
    assert_eq!(
        app.app_view().correction().unwrap().start().text(),
        &original[..original.len() - 1]
    );
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Insert('1')));
    assert_eq!(
        app.app_view().correction().unwrap().start().text(),
        original
    );
    assert_eq!(
        app.app_view().correction().unwrap().start().cursor(),
        app.app_view()
            .correction()
            .unwrap()
            .start()
            .text()
            .chars()
            .count()
    );
}

#[test]
fn each_time_adjustment_changes_only_the_focused_field_without_writing() {
    let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
    let worklog = history_worklog(10, task_id, 100);
    for (command, expected_start, expected_end) in [
        (
            WorklogHistoryCommand::AdjustForwardFiveMinutes,
            "1970-01-01 02:06",
            "1970-01-01 02:07",
        ),
        (
            WorklogHistoryCommand::AdjustBackwardFiveMinutes,
            "1970-01-01 01:56",
            "1970-01-01 01:57",
        ),
        (
            WorklogHistoryCommand::AdjustForwardOneHour,
            "1970-01-01 03:01",
            "1970-01-01 03:02",
        ),
        (
            WorklogHistoryCommand::AdjustBackwardOneHour,
            "1970-01-01 01:01",
            "1970-01-01 01:02",
        ),
    ] {
        for focus in [CorrectionField::Start, CorrectionField::End] {
            let (mut app, spy) = correction_app_with_spy(worklog.clone(), vec![worklog.clone()]);
            if focus == CorrectionField::End {
                app.handle(Command::WorklogHistory(
                    WorklogHistoryCommand::SwitchCorrectionField,
                ));
            }
            app.handle(Command::WorklogHistory(command));
            let draft = app.app_view().correction().unwrap();
            assert_eq!(draft.focused(), focus);
            assert_eq!(
                draft.start().text(),
                if focus == CorrectionField::Start {
                    expected_start
                } else {
                    "1970-01-01 02:01"
                }
            );
            assert_eq!(
                draft.end().unwrap().text(),
                if focus == CorrectionField::End {
                    expected_end
                } else {
                    "1970-01-01 02:02"
                }
            );
            assert!(spy.correction_calls().is_empty());
        }
    }
}

#[test]
fn changed_gap_input_is_rejected_without_calling_the_application() {
    let task = task(1, "alpha");
    let start = Utc
        .with_ymd_and_hms(2025, 3, 30, 0, 30, 0)
        .single()
        .expect("the UTC timestamp is valid");
    let end = Utc
        .with_ymd_and_hms(2025, 3, 30, 1, 30, 0)
        .single()
        .expect("the UTC timestamp is valid");
    let worklog = Worklog::new(worklog_id(10), task.id(), start, Some(end)).unwrap();
    let mut service = TestService::with_tasks(vec![task]);
    service.worklog_pages = vec![
        Ok(page(vec![worklog.clone()], None)),
        Ok(page(vec![worklog], None)),
    ];
    let spy = service.spy();
    let mut app = app_in_timezone(service, chrono_tz::Europe::Berlin);
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::OpenCorrection,
    ));

    replace_start(&mut app, "2025-03-30 02:30");
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));

    assert!(spy.correction_calls().is_empty());
    assert_eq!(
        text(app.app_view().status()),
        "Start: Local time does not exist"
    );
}
#[test]
fn changed_and_unchanged_fields_keep_their_independent_precision() {
    let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
    let start = DateTime::from_timestamp(100, 123_456_000).unwrap();
    let end = DateTime::from_timestamp(200, 654_321_000).unwrap();
    let worklog = Worklog::new(worklog_id(10), task_id, start, Some(end)).unwrap();

    let (mut app, spy) = correction_app_with_spy(worklog.clone(), vec![worklog.clone()]);
    replace_start(&mut app, "1970-01-01 02:02".to_owned());
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));
    let replacement = spy.correction_calls()[0].2;
    assert_eq!(replacement.start(), at(120));
    assert_eq!(replacement.end(), Some(end));

    let (mut app, spy) = correction_app_with_spy(worklog.clone(), vec![worklog]);
    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::SwitchCorrectionField,
    ));
    replace_end(&mut app, "1970-01-01 02:04".to_owned());
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));
    let replacement = spy.correction_calls()[0].2;
    assert_eq!(replacement.start(), start);
    assert_eq!(replacement.end(), Some(at(240)));
}
#[test]
fn untouched_fields_preserve_exact_utc_when_the_timezone_is_stable() {
    let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
    let start = DateTime::from_timestamp(100, 123_456_000).unwrap();
    let end = DateTime::from_timestamp(200, 654_321_000).unwrap();
    let worklog = Worklog::new(worklog_id(10), task_id, start, Some(end)).unwrap();
    let (mut app, spy) = correction_app_with_spy(worklog.clone(), vec![worklog]);

    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));

    assert_eq!(
        spy.correction_calls()[0].2,
        WorklogTimes::new(start, Some(end))
    );
}
#[test]
fn a_timezone_snapshot_keeps_utc_rules_when_london_changes_later() {
    let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
    let original = Utc
        .with_ymd_and_hms(2025, 1, 15, 10, 0, 0)
        .single()
        .unwrap();
    let worklog = Worklog::begin(worklog_id(10), task_id, original);
    let (mut app, spy) =
        correction_history_app_with_spy_in(worklog.clone(), vec![worklog], chrono_tz::UTC);
    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::OpenCorrection,
    ));
    replace_start(&mut app, "2025-07-01 10:00".to_owned());

    let london = chrono_tz::Europe::London;
    assert_eq!(
        parse_correction_timestamp("2025-07-01 10:00", &london, Some(original)),
        Ok(Utc.with_ymd_and_hms(2025, 7, 1, 9, 0, 0).single().unwrap())
    );

    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));

    assert_eq!(
        spy.correction_calls()[0].2.start(),
        Utc.with_ymd_and_hms(2025, 7, 1, 10, 0, 0).single().unwrap()
    );
}
#[test]
fn out_of_range_local_timestamps_cannot_open_correction() {
    let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
    for (tag, timestamp, timezone) in [
        (1, DateTime::<Utc>::MIN_UTC, chrono_tz::America::New_York),
        (2, DateTime::<Utc>::MAX_UTC, chrono_tz::Asia::Tokyo),
    ] {
        assert_eq!(correction_timestamp(timestamp, &timezone), None);
        let worklog = Worklog::begin(worklog_id(tag), task_id, timestamp);
        let (mut app, spy) = correction_history_app_with_spy_in(worklog, Vec::new(), timezone);

        app.handle(Command::WorklogHistory(
            WorklogHistoryCommand::OpenCorrection,
        ));

        assert!(
            matches!(app.app_view().screen_state(), ScreenState::WorklogHistory(state) if matches!(state.mode(), WorklogHistoryMode::Normal))
        );
        assert_eq!(text(app.app_view().status()), OUTSIDE_EDITABLE_RANGE);
        assert!(spy.correction_calls().is_empty());
    }
}

#[test]
fn text_edited_back_to_its_opening_value_preserves_the_original_instant() {
    let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
    let start = DateTime::from_timestamp(100, 123_456_000).unwrap();
    let worklog = Worklog::begin(worklog_id(10), task_id, start);
    let (mut app, spy) = correction_app_with_spy(worklog.clone(), vec![worklog]);

    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Backspace));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Insert('1')));
    app.handle(Command::WorklogHistory(WorklogHistoryCommand::Confirm));

    assert_eq!(spy.correction_calls()[0].2.start(), start);
}
