use chrono::{NaiveDate, TimeZone, Utc};
use clap::Parser;

use crate::args::{Cli, Command, Preset};
use crate::reports::{preset_dates, range, select_timezone};

fn date(value: &str) -> NaiveDate {
    value.parse().unwrap()
}

#[test]
fn presets_follow_local_calendar_periods_and_monday_weeks() {
    let today = date("2026-10-04");
    for (preset, from, to) in [
        (Preset::Today, "2026-10-04", "2026-10-04"),
        (Preset::Yesterday, "2026-10-03", "2026-10-03"),
        (Preset::Week, "2026-09-28", "2026-10-04"),
        (Preset::Month, "2026-10-01", "2026-10-31"),
        (Preset::Year, "2026-01-01", "2026-12-31"),
    ] {
        assert_eq!(preset_dates(preset, today), Some((date(from), date(to))));
    }
    assert_eq!(preset_dates(Preset::Yesterday, NaiveDate::MIN), None);
}

#[test]
fn timezone_selection_uses_valid_environment_then_system_then_utc() {
    assert_eq!(
        select_timezone(Some("Europe/Berlin"), Some("UTC")),
        chrono_tz::Europe::Berlin
    );
    assert_eq!(
        select_timezone(Some("Bad/Zone"), Some("America/New_York")),
        chrono_tz::America::New_York
    );
    assert_eq!(select_timezone(None, Some("Bad/Zone")), chrono_tz::UTC);
    assert_eq!(select_timezone(None, None), chrono_tz::UTC);
    for value in [
        ":Europe/Berlin",
        "/usr/share/zoneinfo/Europe/Berlin",
        "../usr/share/zoneinfo/Europe/Berlin",
        "/usr/share/lib/zoneinfo/Europe/Berlin",
        "/etc/zoneinfo/Europe/Berlin",
        "../etc/zoneinfo/Europe/Berlin",
        "/var/db/timezone/zoneinfo/Europe/Berlin",
        "zoneinfo/Europe/Berlin",
        "posix/Europe/Berlin",
        "right/Europe/Berlin",
    ] {
        assert_eq!(
            select_timezone(Some(value), Some("UTC")),
            chrono_tz::Europe::Berlin
        );
    }
}

fn report_args(arguments: &[&str]) -> crate::args::ReportArgs {
    let cli = Cli::try_parse_from(
        ["tt-cli", "reports"]
            .into_iter()
            .chain(arguments.iter().copied()),
    )
    .unwrap();
    let Command::Reports(args) = cli.command else {
        panic!("expected reports")
    };
    args
}

/// CLI bounds must use stored precision and reject intervals that collapse after truncation.
#[test]
fn explicit_report_ranges_normalize_before_validation_and_output() {
    let now = Utc.with_ymd_and_hms(2026, 10, 4, 12, 0, 0).unwrap();
    let normalized = range(
        &report_args(&[
            "--start",
            "2026-01-01T00:00:00.123456789Z",
            "--end",
            "2026-01-01T00:00:00.123458999Z",
        ]),
        now,
    )
    .unwrap();
    assert_eq!(
        normalized.start.to_rfc3339(),
        "2026-01-01T00:00:00.123456+00:00"
    );
    assert_eq!(
        normalized.end.to_rfc3339(),
        "2026-01-01T00:00:00.123458+00:00"
    );

    let error = range(
        &report_args(&[
            "--start",
            "2026-01-01T00:00:00.000000100Z",
            "--end",
            "2026-01-01T00:00:00.000000900Z",
        ]),
        now,
    )
    .err()
    .unwrap();
    assert_eq!(error.message, "report end must be later than start");
    assert_eq!(error.exit_code, 2);
}

/// Calendar selections preserve local day boundaries and explicit UTC offsets.
#[test]
fn reports_default_to_today_in_selected_timezone_and_accept_inclusive_dates() {
    let now = Utc.with_ymd_and_hms(2026, 10, 4, 23, 30, 0).unwrap();
    let today = range(&report_args(&["--timezone", "Europe/Berlin"]), now).unwrap();
    assert_eq!(today.start.to_rfc3339(), "2026-10-04T22:00:00+00:00");
    assert_eq!(today.end.to_rfc3339(), "2026-10-05T22:00:00+00:00");
    assert_eq!(today.timezone, chrono_tz::Europe::Berlin);
    let custom = range(
        &report_args(&[
            "--from",
            "2026-01-01",
            "--to",
            "2026-01-02",
            "--timezone",
            "UTC",
        ]),
        now,
    )
    .unwrap();
    assert_eq!((custom.end - custom.start).num_days(), 2);
    let explicit = range(
        &report_args(&[
            "--start",
            "2026-01-01T01:00:00+01:00",
            "--end",
            "2026-01-01T02:00:00+01:00",
        ]),
        now,
    )
    .unwrap();
    assert_eq!((explicit.end - explicit.start).num_hours(), 1);
    assert!(
        range(
            &report_args(&["--from", "2026-01-02", "--to", "2026-01-01"]),
            now
        )
        .is_err()
    );
    assert!(
        range(
            &report_args(&[
                "--start",
                "2026-01-01T00:00:00Z",
                "--end",
                "2026-01-01T00:00:00Z"
            ]),
            now
        )
        .is_err()
    );
}

#[test]
fn implicit_timezone_uses_process_environment() {
    const MARKER: &str = "TT_CLI_TIMEZONE_UNIT_TEST";
    if std::env::var(MARKER).as_deref() == Ok("berlin") {
        let now = Utc.with_ymd_and_hms(2026, 10, 4, 23, 30, 0).unwrap();
        let range = range(&report_args(&[]), now).unwrap();
        assert_eq!(range.timezone, chrono_tz::Europe::Berlin);
        assert_eq!(range.start.to_rfc3339(), "2026-10-04T22:00:00+00:00");
        assert_eq!(range.end.to_rfc3339(), "2026-10-05T22:00:00+00:00");
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::reports::implicit_timezone_uses_process_environment",
            "--nocapture",
        ])
        .env(MARKER, "berlin")
        .env("TZ", "Europe/Berlin")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "child test failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}
