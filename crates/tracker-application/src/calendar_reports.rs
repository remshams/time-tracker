//! Calendar periods and timezone boundaries for reports.

use chrono::{DateTime, Datelike, Duration, LocalResult, Months, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;

/// Calendar periods available independently of a client's input controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalendarPreset {
    Today,
    Yesterday,
    Week,
    Month,
    Year,
}

/// Why calendar dates cannot form a report interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalendarRangeError {
    ReversedDates,
    DatesOutOfRange,
    MissingDay,
    BoundaryOutOfRange,
}

/// Returns inclusive local dates, with weeks starting on Monday.
pub fn preset_dates(preset: CalendarPreset, today: NaiveDate) -> Option<(NaiveDate, NaiveDate)> {
    match preset {
        CalendarPreset::Today => Some((today, today)),
        CalendarPreset::Yesterday => {
            let day = today.pred_opt()?;
            Some((day, day))
        }
        CalendarPreset::Week => {
            let from = today.checked_sub_signed(Duration::days(
                today.weekday().num_days_from_monday() as i64,
            ))?;
            Some((from, from.checked_add_signed(Duration::days(6))?))
        }
        CalendarPreset::Month => month_dates(today),
        CalendarPreset::Year => Some((
            NaiveDate::from_ymd_opt(today.year(), 1, 1)?,
            NaiveDate::from_ymd_opt(today.year(), 12, 31)?,
        )),
    }
}

fn month_dates(today: NaiveDate) -> Option<(NaiveDate, NaiveDate)> {
    let from = NaiveDate::from_ymd_opt(today.year(), today.month(), 1)?;
    Some((from, from.checked_add_months(Months::new(1))?.pred_opt()?))
}

/// Converts inclusive local dates to a UTC interval with an exclusive end.
pub fn inclusive_range(
    timezone: Tz,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<(DateTime<Utc>, DateTime<Utc>), CalendarRangeError> {
    if from > to {
        return Err(CalendarRangeError::ReversedDates);
    }
    let next = to.succ_opt().ok_or(CalendarRangeError::DatesOutOfRange)?;
    Ok((day_start(timezone, from)?, day_start(timezone, next)?))
}

/// Finds the first instant in a local day, choosing the earlier ambiguous time.
///
/// Gaps advance in minutes, then backtrack in seconds to retain historical
/// timezone transitions that did not happen on minute boundaries.
pub fn day_start(timezone: Tz, date: NaiveDate) -> Result<DateTime<Utc>, CalendarRangeError> {
    let mut local = date
        .and_hms_opt(0, 0, 0)
        .ok_or(CalendarRangeError::BoundaryOutOfRange)?;
    loop {
        if local.date() != date {
            return Err(CalendarRangeError::MissingDay);
        }
        match timezone.from_local_datetime(&local) {
            LocalResult::None => {
                local = local
                    .checked_add_signed(Duration::minutes(1))
                    .ok_or(CalendarRangeError::BoundaryOutOfRange)?;
            }
            valid => {
                let mut first = valid.earliest().expect("matched a valid local time");
                for _ in 0..59 {
                    let Some(previous) = local.checked_sub_signed(Duration::seconds(1)) else {
                        break;
                    };
                    if previous.date() != date {
                        break;
                    }
                    let Some(at) = timezone.from_local_datetime(&previous).earliest() else {
                        break;
                    };
                    local = previous;
                    first = at;
                }
                return Ok(first.with_timezone(&Utc));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(text: &str) -> NaiveDate {
        text.parse().unwrap()
    }

    #[test]
    fn presets_cover_monday_weeks_leap_months_and_date_limits() {
        let today = date("2024-02-29");
        for (preset, from, to) in [
            (CalendarPreset::Today, "2024-02-29", "2024-02-29"),
            (CalendarPreset::Yesterday, "2024-02-28", "2024-02-28"),
            (CalendarPreset::Week, "2024-02-26", "2024-03-03"),
            (CalendarPreset::Month, "2024-02-01", "2024-02-29"),
            (CalendarPreset::Year, "2024-01-01", "2024-12-31"),
        ] {
            assert_eq!(preset_dates(preset, today), Some((date(from), date(to))));
        }
        assert_eq!(
            preset_dates(CalendarPreset::Yesterday, NaiveDate::MIN),
            None
        );
        assert_eq!(preset_dates(CalendarPreset::Month, NaiveDate::MAX), None);
    }

    #[test]
    fn inclusive_dates_use_the_next_local_midnight_across_dst() {
        for (day, hours) in [("2026-03-29", 23), ("2026-10-25", 25)] {
            let (start, end) =
                inclusive_range(chrono_tz::Europe::Berlin, date(day), date(day)).unwrap();
            assert_eq!((end - start).num_hours(), hours);
            assert_eq!(
                start.with_timezone(&chrono_tz::Europe::Berlin).date_naive(),
                date(day)
            );
            assert_eq!(
                end.with_timezone(&chrono_tz::Europe::Berlin).date_naive(),
                date(day).succ_opt().unwrap()
            );
        }
        assert_eq!(
            inclusive_range(chrono_tz::UTC, date("2026-01-02"), date("2026-01-01")),
            Err(CalendarRangeError::ReversedDates)
        );
        assert_eq!(
            inclusive_range(chrono_tz::UTC, NaiveDate::MAX, NaiveDate::MAX),
            Err(CalendarRangeError::DatesOutOfRange)
        );
    }

    #[test]
    fn boundaries_cover_skipped_midnights_ambiguity_and_missing_days() {
        for (timezone, day, expected) in [
            (
                chrono_tz::America::Sao_Paulo,
                "2018-11-04",
                "2018-11-04T03:00:00+00:00",
            ),
            (
                chrono_tz::America::Havana,
                "2026-11-01",
                "2026-11-01T04:00:00+00:00",
            ),
            (
                chrono_tz::Africa::Monrovia,
                "1972-01-07",
                "1972-01-07T00:44:30+00:00",
            ),
        ] {
            assert_eq!(
                day_start(timezone, date(day)).unwrap().to_rfc3339(),
                expected
            );
        }
        assert_eq!(
            day_start(chrono_tz::Pacific::Apia, date("2011-12-30")),
            Err(CalendarRangeError::MissingDay)
        );
        assert_eq!(
            inclusive_range(
                chrono_tz::Pacific::Apia,
                date("2011-12-29"),
                date("2011-12-29")
            ),
            Err(CalendarRangeError::MissingDay)
        );
    }
}
