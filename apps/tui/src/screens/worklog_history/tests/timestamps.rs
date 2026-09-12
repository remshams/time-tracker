//! Tests for timestamps behavior.

use super::*;

#[test]
fn switching_uses_one_monotonic_timestamp_for_both_worklogs() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tracker.db");
    let repository = SqliteRepository::open(&path).unwrap();
    let alpha = task(1, "alpha");
    let beta = task(2, "beta");
    repository.create_task(alpha.clone()).unwrap();
    repository.create_task(beta.clone()).unwrap();
    let (mut app, clock) = app_with_test_clock(
        TrackerApplication::load(repository).unwrap(),
        chrono_tz::UTC,
        Utc::now(),
    );
    let alpha = alpha.id();
    let beta = beta.id();
    app.handle(Command::ToggleTracking);
    clock.advance_monotonic(Duration::from_secs(125));
    app.handle(Command::MoveDown);
    app.handle(Command::ToggleTracking);
    let mut verifier = TrackerApplication::load(SqliteRepository::open(&path).unwrap()).unwrap();
    let alpha_worklog = verifier
        .worklogs_for_task(alpha, None)
        .unwrap()
        .worklogs
        .pop()
        .unwrap();
    let beta_worklog = verifier
        .worklogs_for_task(beta, None)
        .unwrap()
        .worklogs
        .pop()
        .unwrap();
    assert_eq!(alpha_worklog.end(), Some(beta_worklog.start()));
}
#[test]
fn elapsed_clock_and_client_timestamp_share_one_duration() {
    let clock = ElapsedClock::at_anchor(Duration::from_secs(100), Instant::now());
    assert_eq!(clock.at(Duration::from_secs(5)), Duration::from_secs(105));
    let start = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    assert_eq!(
        tracking_timestamp(start, Duration::from_secs(5)),
        start + TimeDelta::seconds(5)
    );
}
#[test]
fn future_clock_anchors_at_zero_and_timestamp_overflow_saturates() {
    let now = Utc::now();
    assert_eq!(
        ElapsedClock::base_since(now + TimeDelta::seconds(1), now),
        Duration::ZERO
    );
    assert_eq!(
        ElapsedClock::base_since(now - TimeDelta::seconds(5), now),
        Duration::from_secs(5)
    );
    assert_eq!(
        tracking_timestamp(DateTime::<Utc>::MAX_UTC, Duration::MAX),
        DateTime::<Utc>::MAX_UTC
    );
}
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
fn timezone_resolution_accepts_common_tz_forms_and_reports_the_utc_fallback() {
    for (value, expected) in [
        ("UTC", chrono_tz::UTC),
        (":UTC", chrono_tz::UTC),
        (
            "/usr/share/zoneinfo/Europe/London",
            chrono_tz::Europe::London,
        ),
        (
            "../usr/share/zoneinfo/posix/Europe/London",
            chrono_tz::Europe::London,
        ),
        (":/etc/zoneinfo/Europe/London", chrono_tz::Europe::London),
        ("../etc/zoneinfo/Europe/London", chrono_tz::Europe::London),
        (
            "/usr/share/zoneinfo/right/Europe/London",
            chrono_tz::Europe::London,
        ),
    ] {
        assert_eq!(parse_timezone_name(value), Some(expected));
    }
    assert_eq!(
        resolve_timezone(
            Some(":/etc/zoneinfo/America/Phoenix"),
            Some("America/Denver")
        ),
        (chrono_tz::America::Phoenix, None),
        "a valid environment override wins over the OS timezone"
    );
    assert_eq!(
        resolve_timezone(Some("not a timezone"), Some("Europe/Paris")),
        (chrono_tz::Europe::Paris, None)
    );
    assert_eq!(
        resolve_timezone(Some("not a timezone"), Some("also invalid")),
        (
            chrono_tz::UTC,
            Some("Could not detect an IANA timezone; using UTC for this session.")
        )
    );
}
#[test]
fn timestamp_input_edits_and_moves_at_the_character_cursor() {
    let mut input = TimestampInput::new("123".to_owned());

    input.move_left();
    input.insert('9');
    assert_eq!(input.text(), "1293");
    assert_eq!(input.cursor(), 3);

    input.backspace();
    assert_eq!(input.text(), "123");
    assert_eq!(input.cursor(), 2);

    input.delete();
    assert_eq!(input.text(), "12");
    assert_eq!(input.cursor(), 2);

    input.move_left();
    assert_eq!(input.cursor(), 1);
    input.move_right();
    assert_eq!(input.cursor(), 2);
}
#[test]
fn correction_commands_adjust_and_edit_the_focused_timestamp() {
    let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
    let worklog = history_worklog(10, task_id, 100);
    let mut app = correction_app(worklog.clone(), vec![worklog.clone()]);

    app.handle(Command::AdjustForwardOneHour);
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
    app.handle(Command::Backspace);
    assert_eq!(
        app.app_view().correction().unwrap().start().text(),
        &original[..original.len() - 1]
    );
    app.handle(Command::Insert('1'));
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
fn local_to_utc_range_overflow_is_not_reported_as_a_daylight_saving_gap() {
    assert_eq!(
        parse_correction_timestamp(
            "-262143-01-01 00:00",
            &FixedOffset::east_opt(60).unwrap(),
            None,
        ),
        Err(OUTSIDE_EDITABLE_RANGE)
    );
    assert_eq!(
        parse_correction_timestamp(
            "+262142-12-31 23:59",
            &FixedOffset::west_opt(60).unwrap(),
            None,
        ),
        Err(OUTSIDE_EDITABLE_RANGE)
    );
}
#[test]
fn timestamp_input_accepts_every_supported_year_shape_up_to_chrono_limit() {
    for timestamp in [
        "-0001-01-02 03:04",
        "0000-01-02 03:04",
        "9999-01-02 03:04",
        "+10000-01-02 03:04",
    ] {
        let mut input = TimestampInput::new(String::new());
        for character in timestamp.chars() {
            input.insert(character);
        }
        assert_eq!(input.text(), timestamp);
    }

    for longest in ["-262143-01-01 00:00", "+262142-12-31 23:59"] {
        assert_eq!(longest.chars().count(), TimestampInput::MAX_LEN);
        let mut input = TimestampInput::new(String::new());
        for character in longest.chars() {
            input.insert(character);
        }
        input.insert('0');
        assert_eq!(input.text(), longest);
    }
}
#[test]
fn changed_ambiguous_input_uses_the_original_offset_or_is_rejected() {
    let zone = CorrectionTestZone;
    let mut input = TimestampInput::new("1970-01-01 00:00".to_owned());
    input.replace("1970-01-01 06:30".to_owned());

    assert_eq!(
        resolve_correction_timestamp(&input, &zone, at(10_000)).unwrap(),
        at(16_200)
    );
    assert_eq!(
        resolve_correction_timestamp(&input, &zone, at(20_000)).unwrap(),
        at(19_800)
    );
    assert_eq!(
        resolve_correction_timestamp(&input, &zone, at(100_000)),
        Err("Ambiguous local time")
    );
}
#[test]
fn minute_and_hour_adjustments_reformat_in_the_configured_timezone() {
    let utc = FixedOffset::east_opt(0).unwrap();
    let original = DateTime::from_timestamp(0, 123_456_000).unwrap();
    assert_eq!(
        adjusted_correction_timestamp(
            &TimestampInput::new("1970-01-01 00:00".to_owned()),
            TimeDelta::minutes(5),
            &utc,
            original
        )
        .unwrap(),
        ("1970-01-01 00:05".to_owned(), at(300))
    );
    assert_eq!(
        adjusted_correction_timestamp(
            &TimestampInput::new("1970-01-01 00:00".to_owned()),
            TimeDelta::hours(-1),
            &utc,
            original
        )
        .unwrap(),
        ("1969-12-31 23:00".to_owned(), at(-3_600))
    );
    let zone = CorrectionTestZone;
    let original = DateTime::from_timestamp(3300, 0).unwrap();
    assert_eq!(
        adjusted_correction_timestamp(
            &TimestampInput::new("1970-01-01 01:55".to_owned()),
            TimeDelta::minutes(5),
            &zone,
            original
        )
        .unwrap(),
        ("1970-01-01 03:00".to_owned(), at(3_600))
    );
}
#[test]
fn subminute_offset_transition_rejects_an_unrepresentable_absolute_adjustment() {
    assert_eq!(
        parse_correction_timestamp("1970-01-01 00:01", &SubminuteTransitionZone, None,),
        Ok(at(30)),
        "local wall-clock second 00 can map to nonzero UTC seconds",
    );

    let input = TimestampInput::new("1970-01-01 00:01".to_owned());
    assert_eq!(
        adjusted_correction_timestamp(
            &input,
            TimeDelta::minutes(5),
            &SubminuteTransitionZone,
            at(30),
        ),
        Err("Adjustment cannot be represented as a local minute")
    );
}
#[test]
fn fallback_adjustment_keeps_the_resolved_occurrence_when_the_text_repeats() {
    let original_start = DateTime::from_timestamp(16_230, 123_456_000).unwrap();
    let opening = correction_timestamp(original_start, &CorrectionTestZone).unwrap();
    let mut input = TimestampInput::new(opening);
    let (text, instant) = adjusted_correction_timestamp(
        &input,
        TimeDelta::hours(1),
        &CorrectionTestZone,
        original_start,
    )
    .unwrap();
    input.replace_with_adjustment(text, instant);

    assert_eq!(input.text(), "1970-01-01 06:30");
    assert_eq!(
        resolve_correction_timestamp(&input, &CorrectionTestZone, original_start),
        Ok(at(19_800))
    );
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
    app.handle(Command::OpenHistory);
    app.handle(Command::OpenCorrection);

    replace_start(&mut app, "2025-03-30 02:30");
    app.handle(Command::Confirm);

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
    app.handle(Command::Confirm);
    let replacement = spy.correction_calls()[0].2;
    assert_eq!(replacement.start(), at(120));
    assert_eq!(replacement.end(), Some(end));

    let (mut app, spy) = correction_app_with_spy(worklog.clone(), vec![worklog]);
    app.handle(Command::SwitchCorrectionField);
    replace_end(&mut app, "1970-01-01 02:04".to_owned());
    app.handle(Command::Confirm);
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

    app.handle(Command::Confirm);

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
    app.handle(Command::OpenCorrection);
    replace_start(&mut app, "2025-07-01 10:00".to_owned());

    let london = chrono_tz::Europe::London;
    assert_eq!(
        parse_correction_timestamp("2025-07-01 10:00", &london, Some(original)),
        Ok(Utc.with_ymd_and_hms(2025, 7, 1, 9, 0, 0).single().unwrap())
    );

    app.handle(Command::Confirm);

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

        app.handle(Command::OpenCorrection);

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

    app.handle(Command::Backspace);
    app.handle(Command::Insert('1'));
    app.handle(Command::Confirm);

    assert_eq!(spy.correction_calls()[0].2.start(), start);
}
