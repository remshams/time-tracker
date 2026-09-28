use chrono::{DateTime, Datelike, Duration, LocalResult, Months, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use std::sync::atomic::{AtomicU64, Ordering};
use tracker_application::ReportTotals;
use tracker_domain::TaskId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportPreset {
    Today,
    Yesterday,
    Week,
    Month,
    Year,
    Custom,
}

impl ReportPreset {
    pub const ALL: [Self; 6] = [
        Self::Today,
        Self::Yesterday,
        Self::Week,
        Self::Month,
        Self::Year,
        Self::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Today => "Today",
            Self::Yesterday => "Yesterday",
            Self::Week => "Week",
            Self::Month => "Month",
            Self::Year => "Year",
            Self::Custom => "Custom",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportMode {
    Normal,
    Custom {
        from: String,
        to: String,
        focus_to: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFocus {
    TopTabs,
    Presets,
    Rows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DateShift {
    PreviousDay,
    NextDay,
    PreviousMonth,
    NextMonth,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportState {
    pub(crate) session_id: u64,
    pub(crate) period_generation: u64,
    pub(crate) preset: ReportPreset,
    pub(crate) from: NaiveDate,
    pub(crate) to: NaiveDate,
    pub(crate) mode: ReportMode,
    pub(crate) focus: ReportFocus,
    pub(crate) preset_cursor: usize,
    pub(crate) totals: Option<ReportTotals>,
    pub(crate) selected: Option<TaskId>,
    pub(crate) g_prefix: bool,
    pub(crate) follow_calendar: bool,
    pub(crate) calendar_today: NaiveDate,
    pub(crate) report_error: bool,
    pub(crate) last_refresh_second: Option<i64>,
}

impl ReportState {
    pub(crate) fn new(now: DateTime<Utc>, timezone: Tz) -> Self {
        static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);
        let today = now.with_timezone(&timezone).date_naive();
        Self {
            session_id: NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed),
            period_generation: 0,
            preset: ReportPreset::Today,
            from: today,
            to: today,
            mode: ReportMode::Normal,
            focus: ReportFocus::TopTabs,
            preset_cursor: 0,
            totals: None,
            selected: None,
            g_prefix: false,
            follow_calendar: true,
            calendar_today: today,
            report_error: false,
            last_refresh_second: None,
        }
    }

    pub(crate) fn range(&self, timezone: Tz) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        let end_date = self.to.succ_opt()?;
        Some((
            local_day_start(timezone, self.from)?,
            local_day_start(timezone, end_date)?,
        ))
    }

    pub(crate) fn selected_index(&self) -> Option<usize> {
        let id = self.selected?;
        self.totals
            .as_ref()?
            .rows
            .iter()
            .position(|row| row.task.id() == id)
    }

    pub(crate) fn select_index(&mut self, index: usize) {
        self.selected = self
            .totals
            .as_ref()
            .and_then(|totals| totals.rows.get(index))
            .map(|row| row.task.id());
    }

    pub(crate) fn set_totals(&mut self, totals: ReportTotals) {
        if totals.rows.is_empty() && self.focus == ReportFocus::Rows {
            self.focus = ReportFocus::Presets;
        }
        let previous = self.selected;
        self.selected = previous
            .filter(|id| totals.rows.iter().any(|row| row.task.id() == *id))
            .or_else(|| totals.rows.first().map(|row| row.task.id()));
        self.totals = Some(totals);
        self.report_error = false;
    }

    pub(crate) fn clear_totals(&mut self) {
        self.totals = None;
        self.selected = None;
        if self.focus == ReportFocus::Rows {
            self.focus = ReportFocus::Presets;
        }
    }

    pub(crate) fn period_label(&self) -> &'static str {
        if matches!(self.preset, ReportPreset::Today | ReportPreset::Yesterday) {
            if self.from == self.calendar_today {
                "Today"
            } else if self.calendar_today.pred_opt() == Some(self.from) {
                "Yesterday"
            } else {
                "Day"
            }
        } else {
            self.preset.label()
        }
    }

    pub(crate) fn highlighted_preset(&self) -> Option<ReportPreset> {
        match self.period_label() {
            "Today" => Some(ReportPreset::Today),
            "Yesterday" => Some(ReportPreset::Yesterday),
            "Day" => None,
            _ => Some(self.preset),
        }
    }

    pub(crate) fn choose(&mut self, preset: ReportPreset, today: NaiveDate) -> bool {
        if preset == ReportPreset::Custom {
            self.mode = ReportMode::Custom {
                from: self.from.to_string(),
                to: self.to.to_string(),
                focus_to: false,
            };
            return false;
        }
        let Some((from, to)) = preset_dates(preset, today) else {
            return false;
        };
        self.preset = preset;
        self.period_generation = self.period_generation.wrapping_add(1);
        self.from = from;
        self.to = to;
        self.follow_calendar = true;
        self.mode = ReportMode::Normal;
        true
    }

    pub(crate) fn step(&mut self, direction: i32) -> bool {
        let shifted = (|| -> Option<(NaiveDate, NaiveDate)> {
            match self.preset {
                ReportPreset::Today | ReportPreset::Yesterday => Some((
                    shift_days(self.from, direction as i64)?,
                    shift_days(self.to, direction as i64)?,
                )),
                ReportPreset::Week => Some((
                    shift_days(self.from, direction as i64 * 7)?,
                    shift_days(self.to, direction as i64 * 7)?,
                )),
                ReportPreset::Month => {
                    let from = shift_months(self.from, direction.is_negative())?;
                    let to = from.checked_add_months(Months::new(1))?.pred_opt()?;
                    Some((from, to))
                }
                ReportPreset::Year => {
                    let year = self.from.year().checked_add(direction)?;
                    let from = NaiveDate::from_ymd_opt(year, 1, 1)?;
                    let to = NaiveDate::from_ymd_opt(year, 12, 31)?;
                    Some((from, to))
                }
                ReportPreset::Custom => {
                    let days = (self.to - self.from).num_days().checked_add(1)?;
                    Some((
                        shift_days(self.from, direction as i64 * days)?,
                        shift_days(self.to, direction as i64 * days)?,
                    ))
                }
            }
        })();
        if let Some((from, to)) = shifted {
            self.period_generation = self.period_generation.wrapping_add(1);
            self.from = from;
            self.to = to;
            self.follow_calendar = false;
            true
        } else {
            false
        }
    }

    pub(crate) fn apply_custom(&mut self) -> Result<(), &'static str> {
        let ReportMode::Custom { from, to, .. } = &self.mode else {
            return Err("Custom dates are not open");
        };
        let from =
            NaiveDate::parse_from_str(from, "%Y-%m-%d").map_err(|_| "From must be YYYY-MM-DD")?;
        let to = NaiveDate::parse_from_str(to, "%Y-%m-%d").map_err(|_| "To must be YYYY-MM-DD")?;
        if from > to {
            return Err("From must be on or before To");
        }
        if to.succ_opt().is_none() {
            return Err("To is outside the supported date range");
        }
        self.from = from;
        self.period_generation = self.period_generation.wrapping_add(1);
        self.to = to;
        self.preset = ReportPreset::Custom;
        self.follow_calendar = false;
        self.mode = ReportMode::Normal;
        Ok(())
    }

    pub(crate) fn shift_custom_date(&mut self, shift: DateShift) -> bool {
        let ReportMode::Custom { from, to, focus_to } = &mut self.mode else {
            return false;
        };
        let field = if *focus_to { to } else { from };
        let Ok(date) = NaiveDate::parse_from_str(field, "%Y-%m-%d") else {
            return false;
        };
        let shifted = match shift {
            DateShift::PreviousDay => date.pred_opt(),
            DateShift::NextDay => date.succ_opt(),
            DateShift::PreviousMonth => date.checked_sub_months(Months::new(1)),
            DateShift::NextMonth => date.checked_add_months(Months::new(1)),
        };
        let Some(shifted) = shifted else {
            return false;
        };
        let text = shifted.format("%Y-%m-%d").to_string();
        if text.len() != 10 {
            return false;
        }
        *field = text;
        true
    }

    pub(crate) fn follow_calendar_date(&mut self, today: NaiveDate) {
        self.calendar_today = today;
        if self.follow_calendar
            && let Some((from, to)) = preset_dates(self.preset, today)
        {
            if self.from != from || self.to != to {
                self.period_generation = self.period_generation.wrapping_add(1);
            }
            self.from = from;
            self.to = to;
        }
    }
}

fn shift_days(date: NaiveDate, days: i64) -> Option<NaiveDate> {
    date.checked_add_signed(Duration::days(days))
}

fn shift_months(date: NaiveDate, backwards: bool) -> Option<NaiveDate> {
    if backwards {
        date.checked_sub_months(Months::new(1))
    } else {
        date.checked_add_months(Months::new(1))
    }
}

fn preset_dates(preset: ReportPreset, today: NaiveDate) -> Option<(NaiveDate, NaiveDate)> {
    match preset {
        ReportPreset::Today => Some((today, today)),
        ReportPreset::Yesterday => {
            let day = today.pred_opt()?;
            Some((day, day))
        }
        ReportPreset::Week => {
            let offset = today.weekday().num_days_from_monday() as i64;
            let from = shift_days(today, -offset)?;
            Some((from, shift_days(from, 6)?))
        }
        ReportPreset::Month => {
            let from = NaiveDate::from_ymd_opt(today.year(), today.month(), 1)?;
            Some((from, from.checked_add_months(Months::new(1))?.pred_opt()?))
        }
        ReportPreset::Year => Some((
            NaiveDate::from_ymd_opt(today.year(), 1, 1)?,
            NaiveDate::from_ymd_opt(today.year(), 12, 31)?,
        )),
        ReportPreset::Custom => None,
    }
}

fn local_day_start(timezone: Tz, date: NaiveDate) -> Option<DateTime<Utc>> {
    let mut local = date.and_hms_opt(0, 0, 0)?;
    loop {
        if local.date() != date {
            return None;
        }
        match timezone.from_local_datetime(&local) {
            LocalResult::None => local = local.checked_add_signed(Duration::minutes(1))?,
            _ => {
                let mut first = local;
                for _ in 0..59 {
                    let previous = first.checked_sub_signed(Duration::seconds(1))?;
                    if previous.date() != date
                        || matches!(timezone.from_local_datetime(&previous), LocalResult::None)
                    {
                        break;
                    }
                    first = previous;
                }
                return match timezone.from_local_datetime(&first) {
                    LocalResult::Single(at) => Some(at.with_timezone(&Utc)),
                    LocalResult::Ambiguous(a, b) => Some(a.min(b).with_timezone(&Utc)),
                    LocalResult::None => None,
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::task;
    use chrono::TimeDelta;
    use tracker_application::ReportRow;

    #[test]
    fn calendar_following_advances_generation_when_only_start_changes() {
        let now = DateTime::from_timestamp(1_774_896_000, 0).unwrap();
        let mut state = ReportState::new(now, chrono_tz::UTC);
        let today = state.from;
        state.from = today.pred_opt().unwrap();
        let generation = state.period_generation;

        state.follow_calendar_date(today);

        assert_eq!(state.from, today);
        assert_eq!(state.to, today);
        assert_eq!(state.period_generation, generation + 1);
    }

    #[test]
    fn calendar_following_advances_generation_when_only_end_changes() {
        let now = DateTime::from_timestamp(1_774_896_000, 0).unwrap();
        let mut state = ReportState::new(now, chrono_tz::UTC);
        let today = state.from;
        state.to = today.succ_opt().unwrap();
        let generation = state.period_generation;

        state.follow_calendar_date(today);

        assert_eq!(state.from, today);
        assert_eq!(state.to, today);
        assert_eq!(state.period_generation, generation + 1);
    }

    #[test]
    fn berlin_day_boundaries_follow_both_dst_changes() {
        let spring = NaiveDate::from_ymd_opt(2026, 3, 29).unwrap();
        let autumn = NaiveDate::from_ymd_opt(2026, 10, 25).unwrap();
        for (day, hours) in [(spring, 23), (autumn, 25)] {
            let state = ReportState {
                from: day,
                to: day,
                ..ReportState::new(Utc::now(), chrono_tz::Europe::Berlin)
            };
            let (start, end) = state.range(chrono_tz::Europe::Berlin).unwrap();
            assert_eq!((end - start).num_hours(), hours);
        }
    }

    #[test]
    fn custom_dates_require_valid_ordered_calendar_days() {
        let mut state = ReportState::new(Utc::now(), chrono_tz::UTC);
        state.mode = ReportMode::Custom {
            from: "2026-02-30".into(),
            to: "2026-03-01".into(),
            focus_to: false,
        };
        assert_eq!(state.apply_custom(), Err("From must be YYYY-MM-DD"));
        state.mode = ReportMode::Custom {
            from: "2026-03-02".into(),
            to: "2026-03-01".into(),
            focus_to: false,
        };
        assert_eq!(state.apply_custom(), Err("From must be on or before To"));
        state.mode = ReportMode::Custom {
            from: "2026-03-01".into(),
            to: "2026-03-01".into(),
            focus_to: false,
        };
        assert_eq!(state.apply_custom(), Ok(()));
        assert_eq!(state.from, state.to);
    }

    #[test]
    fn custom_date_keys_edit_the_focused_field_across_month_boundaries() {
        let mut state = ReportState::new(Utc::now(), chrono_tz::UTC);
        state.mode = ReportMode::Custom {
            from: "2024-01-31".into(),
            to: "2024-02-29".into(),
            focus_to: false,
        };
        assert!(state.shift_custom_date(DateShift::NextDay));
        assert!(matches!(&state.mode, ReportMode::Custom { from, .. } if from == "2024-02-01"));
        assert!(state.shift_custom_date(DateShift::PreviousDay));
        assert!(state.shift_custom_date(DateShift::NextMonth));
        assert!(matches!(&state.mode, ReportMode::Custom { from, .. } if from == "2024-02-29"));
        if let ReportMode::Custom { focus_to, .. } = &mut state.mode {
            *focus_to = true;
        }
        assert!(state.shift_custom_date(DateShift::PreviousMonth));
        assert!(
            matches!(&state.mode, ReportMode::Custom { from, to, .. } if from == "2024-02-29" && to == "2024-01-29")
        );
        if let ReportMode::Custom { to, .. } = &mut state.mode {
            *to = "2024-01-".into();
        }
        assert!(!state.shift_custom_date(DateShift::NextDay));
        assert!(matches!(&state.mode, ReportMode::Custom { to, .. } if to == "2024-01-"));
    }

    #[test]
    fn midnight_gap_starts_at_the_first_valid_local_minute() {
        let date = NaiveDate::from_ymd_opt(2026, 9, 6).unwrap();
        assert_eq!(
            local_day_start(chrono_tz::America::Santiago, date)
                .unwrap()
                .to_rfc3339(),
            "2026-09-06T04:00:00+00:00"
        );
    }

    #[test]
    fn week_starts_monday_and_today_uses_local_date() {
        let now = DateTime::parse_from_rfc3339("2026-03-29T22:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut state = ReportState::new(now, chrono_tz::Europe::Berlin);
        assert_eq!(state.from.to_string(), "2026-03-30");
        state.choose(ReportPreset::Week, state.from);
        assert_eq!(state.from.to_string(), "2026-03-30");
        assert_eq!(state.to.to_string(), "2026-04-05");
        assert_eq!(
            state
                .range(chrono_tz::Europe::Berlin)
                .unwrap()
                .1
                .to_rfc3339(),
            "2026-04-05T22:00:00+00:00"
        );
        assert!(state.step(-1));
        assert_eq!(state.from.to_string(), "2026-03-23");
        assert_eq!(state.to.to_string(), "2026-03-29");
        assert!(state.step(1));
        assert_eq!(state.from.to_string(), "2026-03-30");
    }

    #[test]
    fn refreshed_totals_keep_a_matching_selection_and_fall_back_when_it_disappears() {
        let first = task(1, "first");
        let second = task(2, "second");
        let report = |rows: Vec<_>| ReportTotals {
            total: TimeDelta::seconds(rows.len() as i64),
            rows,
        };
        let row = |task| ReportRow {
            task,
            duration: TimeDelta::seconds(1),
        };
        let mut state = ReportState::new(Utc::now(), chrono_tz::UTC);
        state.set_totals(report(vec![row(first.clone()), row(second.clone())]));
        state.select_index(1);
        state.set_totals(report(vec![row(second.clone()), row(first.clone())]));
        assert_eq!(state.selected, Some(second.id()));
        state.set_totals(report(vec![row(first.clone())]));
        assert_eq!(state.selected, Some(first.id()));
    }

    #[test]
    fn losing_all_report_rows_returns_focus_to_presets() {
        let mut state = ReportState::new(Utc::now(), chrono_tz::UTC);
        state.set_totals(ReportTotals {
            rows: vec![ReportRow {
                task: task(1, "tracked"),
                duration: TimeDelta::minutes(1),
            }],
            total: TimeDelta::minutes(1),
        });
        state.focus = ReportFocus::Rows;
        state.set_totals(ReportTotals {
            rows: Vec::new(),
            total: TimeDelta::zero(),
        });
        assert_eq!(state.focus, ReportFocus::Presets);
        state.focus = ReportFocus::Rows;
        state.clear_totals();
        assert_eq!(state.focus, ReportFocus::Presets);
    }

    #[test]
    fn following_today_rolls_to_the_next_local_day() {
        let now = DateTime::parse_from_rfc3339("2026-03-29T21:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut state = ReportState::new(now, chrono_tz::Europe::Berlin);
        assert_eq!(state.from.to_string(), "2026-03-29");
        state.follow_calendar_date(NaiveDate::from_ymd_opt(2026, 3, 30).unwrap());
        assert_eq!(state.from.to_string(), "2026-03-30");
    }

    #[test]
    fn stepped_days_use_truthful_labels_in_both_directions() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 25).unwrap();
        let mut state = ReportState::new(
            today.and_hms_opt(12, 0, 0).unwrap().and_utc(),
            chrono_tz::UTC,
        );
        assert_eq!(state.period_label(), "Today");
        assert_eq!(state.highlighted_preset(), Some(ReportPreset::Today));
        assert!(state.step(-1));
        assert_eq!(state.period_label(), "Yesterday");
        assert_eq!(state.highlighted_preset(), Some(ReportPreset::Yesterday));
        assert!(state.step(-1));
        assert_eq!(state.period_label(), "Day");
        assert_eq!(state.highlighted_preset(), None);
        assert!(state.step(1));
        assert_eq!(state.period_label(), "Yesterday");
        assert!(state.step(1));
        assert_eq!(state.period_label(), "Today");
        assert!(state.step(1));
        assert_eq!(state.period_label(), "Day");
        assert_eq!(state.highlighted_preset(), None);

        let mut from_yesterday = ReportState::new(
            today.and_hms_opt(12, 0, 0).unwrap().and_utc(),
            chrono_tz::UTC,
        );
        assert!(from_yesterday.choose(ReportPreset::Yesterday, today));
        assert!(from_yesterday.step(1));
        assert_eq!(from_yesterday.period_label(), "Today");
        assert_eq!(
            from_yesterday.highlighted_preset(),
            Some(ReportPreset::Today)
        );
    }

    #[test]
    fn calendar_presets_follow_week_month_and_year_rollovers_until_stepped() {
        let initial = NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();
        for (preset, next_from, next_to) in [
            (ReportPreset::Week, "2026-12-28", "2027-01-03"),
            (ReportPreset::Month, "2027-01-01", "2027-01-31"),
            (ReportPreset::Year, "2027-01-01", "2027-12-31"),
        ] {
            let mut state = ReportState::new(
                initial.and_hms_opt(12, 0, 0).unwrap().and_utc(),
                chrono_tz::UTC,
            );
            assert!(state.choose(preset, initial));
            state.follow_calendar_date(NaiveDate::from_ymd_opt(2027, 1, 1).unwrap());
            assert_eq!(state.from.to_string(), next_from);
            assert_eq!(state.to.to_string(), next_to);
            assert!(state.step(-1));
            let stepped = state.from;
            state.follow_calendar_date(NaiveDate::from_ymd_opt(2027, 2, 1).unwrap());
            assert_eq!(state.from, stepped);
        }
    }

    #[test]
    fn stepped_months_preserve_calendar_boundaries() {
        let today = NaiveDate::from_ymd_opt(2026, 3, 17).unwrap();
        let mut state = ReportState::new(
            today.and_hms_opt(12, 0, 0).unwrap().and_utc(),
            chrono_tz::UTC,
        );
        assert!(state.choose(ReportPreset::Month, today));
        assert!(state.step(-1));
        assert_eq!(state.from.to_string(), "2026-02-01");
        assert_eq!(state.to.to_string(), "2026-02-28");
        assert!(state.step(1));
        assert_eq!(state.from.to_string(), "2026-03-01");
        assert_eq!(state.to.to_string(), "2026-03-31");
    }

    #[test]
    fn stepped_custom_ranges_move_by_their_inclusive_length() {
        let mut state = ReportState::new(Utc::now(), chrono_tz::UTC);
        state.mode = ReportMode::Custom {
            from: "2026-03-01".into(),
            to: "2026-03-03".into(),
            focus_to: false,
        };
        assert_eq!(state.apply_custom(), Ok(()));
        assert!(state.step(-1));
        assert_eq!(state.from.to_string(), "2026-02-26");
        assert_eq!(state.to.to_string(), "2026-02-28");
        assert!(state.step(1));
        assert_eq!(state.from.to_string(), "2026-03-01");
        assert_eq!(state.to.to_string(), "2026-03-03");
    }
}
