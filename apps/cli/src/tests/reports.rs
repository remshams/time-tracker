use chrono::{NaiveDate, TimeZone, Utc};
use clap::Parser;

use crate::args::{Cli, Command, Preset};
use crate::reports::{day_start, preset_dates, range, select_timezone};

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

#[test]
fn local_day_boundaries_follow_dst_skipped_midnights_and_ambiguity() {
    for (day, hours) in [("2026-03-29", 23), ("2026-10-25", 25)] {
        let start = day_start(chrono_tz::Europe::Berlin, date(day)).unwrap();
        let end = day_start(chrono_tz::Europe::Berlin, date(day).succ_opt().unwrap()).unwrap();
        assert_eq!((end - start).num_hours(), hours);
    }
    let skipped = day_start(chrono_tz::America::Sao_Paulo, date("2018-11-04")).unwrap();
    assert_eq!(skipped.to_rfc3339(), "2018-11-04T03:00:00+00:00");
    let ambiguous = day_start(chrono_tz::America::Havana, date("2026-11-01")).unwrap();
    assert_eq!(ambiguous.to_rfc3339(), "2026-11-01T04:00:00+00:00");
    assert!(day_start(chrono_tz::Pacific::Apia, date("2011-12-30")).is_err());
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
