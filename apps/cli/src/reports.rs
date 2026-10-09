use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use serde_json::{Value, json};
use tracker_application::calendar_reports::{self, CalendarPreset, CalendarRangeError};
use tracker_protocol::TaskDto;

use crate::CliError;
use crate::args::{Preset, ReportArgs};
use crate::backend::Backend;

pub(crate) struct Range {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub timezone: Tz,
}

pub(crate) async fn execute(
    backend: &mut Backend,
    args: ReportArgs,
    now: DateTime<Utc>,
) -> Result<Value, CliError> {
    let range = range(&args, now)?;
    let totals = backend.report_totals(range.start, range.end, now).await?;
    let rows: Vec<_> = totals
        .rows
        .iter()
        .map(|row| json!({
            "task": TaskDto::from(&row.task),
            "duration_us": row.duration.num_microseconds().expect("application validated report duration"),
        }))
        .collect();
    Ok(json!({
        "start": range.start, "end": range.end, "timezone": range.timezone.to_string(), "as_of": now,
        "rows": rows, "total_us": totals.total.num_microseconds().expect("application validated report duration"),
    }))
}

pub(crate) fn range(args: &ReportArgs, now: DateTime<Utc>) -> Result<Range, CliError> {
    let timezone = args.timezone.unwrap_or_else(default_timezone);
    let (start, end) = if let (Some(start), Some(end)) = (args.start, args.end) {
        (start, end)
    } else {
        let (from, to) = if let (Some(from), Some(to)) = (args.from, args.to) {
            (from, to)
        } else {
            preset_dates(
                args.preset.unwrap_or(Preset::Today),
                now.with_timezone(&timezone).date_naive(),
            )
            .ok_or_else(|| CliError::input("report dates exceed the supported range"))?
        };
        calendar_reports::inclusive_range(timezone, from, to).map_err(calendar_error)?
    };
    if end <= start {
        return Err(CliError::input("report end must be later than start"));
    }
    Ok(Range {
        start,
        end,
        timezone,
    })
}

fn default_timezone() -> Tz {
    select_timezone(
        std::env::var("TZ").ok().as_deref(),
        iana_time_zone::get_timezone().ok().as_deref(),
    )
}

pub(crate) fn select_timezone(environment: Option<&str>, system: Option<&str>) -> Tz {
    environment
        .and_then(parse_timezone)
        .or_else(|| system.and_then(parse_timezone))
        .unwrap_or(chrono_tz::UTC)
}

fn parse_timezone(value: &str) -> Option<Tz> {
    let value = value.strip_prefix(':').unwrap_or(value);
    let value = [
        "/usr/share/zoneinfo/",
        "../usr/share/zoneinfo/",
        "/usr/share/lib/zoneinfo/",
        "/etc/zoneinfo/",
        "../etc/zoneinfo/",
        "/var/db/timezone/zoneinfo/",
        "zoneinfo/",
    ]
    .iter()
    .find_map(|prefix| value.strip_prefix(prefix))
    .unwrap_or(value);
    let value = value
        .strip_prefix("posix/")
        .or_else(|| value.strip_prefix("right/"))
        .unwrap_or(value);
    value.parse().ok()
}

pub(crate) fn preset_dates(preset: Preset, today: NaiveDate) -> Option<(NaiveDate, NaiveDate)> {
    let preset = match preset {
        Preset::Today => CalendarPreset::Today,
        Preset::Yesterday => CalendarPreset::Yesterday,
        Preset::Week => CalendarPreset::Week,
        Preset::Month => CalendarPreset::Month,
        Preset::Year => CalendarPreset::Year,
    };
    calendar_reports::preset_dates(preset, today)
}

#[cfg(test)]
pub(crate) fn day_start(timezone: Tz, date: NaiveDate) -> Result<DateTime<Utc>, CliError> {
    calendar_reports::day_start(timezone, date).map_err(calendar_error)
}

fn calendar_error(error: CalendarRangeError) -> CliError {
    CliError::input(match error {
        CalendarRangeError::ReversedDates => "--from must be on or before --to",
        CalendarRangeError::DatesOutOfRange => "report dates exceed the supported range",
        CalendarRangeError::MissingDay => "calendar day does not exist in this timezone",
        CalendarRangeError::BoundaryOutOfRange => "date exceeds the supported range",
    })
}
