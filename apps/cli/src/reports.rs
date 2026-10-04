use chrono::{DateTime, Datelike, Duration, LocalResult, Months, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use serde_json::{Value, json};
use tracker_protocol::ReportRowDto;

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
        .map(|row| ReportRowDto {
            task: (&row.task).into(),
            duration_us: row
                .duration
                .num_microseconds()
                .expect("application validated report duration"),
        })
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
        if from > to {
            return Err(CliError::input("--from must be on or before --to"));
        }
        let next = to
            .succ_opt()
            .ok_or_else(|| CliError::input("report dates exceed the supported range"))?;
        (day_start(timezone, from)?, day_start(timezone, next)?)
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
    match preset {
        Preset::Today => Some((today, today)),
        Preset::Yesterday => {
            let day = today.pred_opt()?;
            Some((day, day))
        }
        Preset::Week => {
            let from = today.checked_sub_signed(Duration::days(
                today.weekday().num_days_from_monday() as i64,
            ))?;
            Some((from, from.checked_add_signed(Duration::days(6))?))
        }
        Preset::Month => {
            let from = NaiveDate::from_ymd_opt(today.year(), today.month(), 1)?;
            Some((from, from.checked_add_months(Months::new(1))?.pred_opt()?))
        }
        Preset::Year => Some((
            NaiveDate::from_ymd_opt(today.year(), 1, 1)?,
            NaiveDate::from_ymd_opt(today.year(), 12, 31)?,
        )),
    }
}

pub(crate) fn day_start(timezone: Tz, date: NaiveDate) -> Result<DateTime<Utc>, CliError> {
    let mut local = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| CliError::input("invalid date"))?;
    loop {
        if local.date() != date {
            return Err(CliError::input(
                "calendar day does not exist in this timezone",
            ));
        }
        match timezone.from_local_datetime(&local) {
            LocalResult::Single(at) => return Ok(at.with_timezone(&Utc)),
            LocalResult::Ambiguous(a, b) => return Ok(a.min(b).with_timezone(&Utc)),
            LocalResult::None => {
                local = local
                    .checked_add_signed(Duration::seconds(1))
                    .ok_or_else(|| CliError::input("date exceeds the supported range"))?
            }
        }
    }
}
