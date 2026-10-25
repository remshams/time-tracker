use chrono::{DateTime, FixedOffset, NaiveDateTime, Offset, TimeDelta, TimeZone, Utc};

/// One bounded timestamp input with a character-indexed cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimestampInput {
    text: String,
    initial_text: String,
    cursor: usize,
    adjusted_instant: Option<DateTime<Utc>>,
}

impl TimestampInput {
    pub(crate) const MAX_LEN: usize = 19;

    pub(crate) fn new(text: String) -> Self {
        let cursor = text.chars().count();
        Self {
            initial_text: text.clone(),
            text,
            cursor,
            adjusted_instant: None,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) fn insert(&mut self, character: char) {
        if self.text.chars().count() >= Self::MAX_LEN || !is_timestamp_character(character) {
            return;
        }
        let byte = byte_index(&self.text, self.cursor);
        self.text.insert(byte, character);
        self.cursor += 1;
        self.adjusted_instant = None;
    }

    pub(crate) fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let end = byte_index(&self.text, self.cursor);
        let start = byte_index(&self.text, self.cursor - 1);
        self.text.replace_range(start..end, "");
        self.cursor -= 1;
        self.adjusted_instant = None;
    }

    pub(crate) fn delete(&mut self) {
        if self.cursor == self.text.chars().count() {
            return;
        }
        let start = byte_index(&self.text, self.cursor);
        let end = byte_index(&self.text, self.cursor + 1);
        self.text.replace_range(start..end, "");
        self.adjusted_instant = None;
    }

    pub(crate) fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub(crate) fn move_right(&mut self) {
        self.cursor = self.cursor.saturating_add(1).min(self.text.chars().count());
    }

    pub(crate) fn replace_with_adjustment(&mut self, text: String, instant: DateTime<Utc>) {
        self.cursor = text.chars().count();
        self.text = text;
        self.adjusted_instant = Some(instant);
    }

    pub(crate) fn is_unchanged(&self) -> bool {
        self.adjusted_instant.is_none() && self.text == self.initial_text
    }
}

fn byte_index(text: &str, character_index: usize) -> usize {
    text.char_indices()
        .nth(character_index)
        .map_or(text.len(), |(index, _)| index)
}

pub(crate) fn is_timestamp_character(character: char) -> bool {
    character.is_ascii_digit() || matches!(character, '+' | '-' | ':' | ' ')
}

/// Formats an instant in `timezone` as a local minute timestamp.
pub(crate) fn local_time<Tz>(at: DateTime<Utc>, timezone: &Tz) -> String
where
    Tz: TimeZone,
{
    correction_timestamp(at, timezone).unwrap_or_else(|| "outside local range".to_owned())
}

pub(crate) const CORRECTION_FORMAT: &str = "%Y-%m-%d %H:%M";
pub(crate) const OUTSIDE_EDITABLE_RANGE: &str = "Timestamp is outside editable range";

pub(crate) fn startup_timezone() -> (chrono_tz::Tz, Option<&'static str>) {
    let environment = std::env::var("TZ").ok();
    let system = iana_time_zone::get_timezone().ok();
    resolve_timezone(environment.as_deref(), system.as_deref())
}

pub(crate) fn resolve_timezone(
    environment: Option<&str>,
    system: Option<&str>,
) -> (chrono_tz::Tz, Option<&'static str>) {
    let from_environment = environment.and_then(parse_timezone_name);
    let from_system = system.and_then(parse_timezone_name);
    match from_environment.or(from_system) {
        Some(timezone) => (timezone, None),
        None => (
            chrono_tz::UTC,
            Some("Could not detect an IANA timezone; using UTC for this session."),
        ),
    }
}

pub(crate) fn parse_timezone_name(value: &str) -> Option<chrono_tz::Tz> {
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

fn local_naive<Tz>(at: DateTime<Utc>, timezone: &Tz) -> Option<NaiveDateTime>
where
    Tz: TimeZone,
{
    let offset = timezone.offset_from_utc_datetime(&at.naive_utc()).fix();
    at.naive_utc().checked_add_offset(offset)
}

pub(crate) fn correction_timestamp<Tz>(at: DateTime<Utc>, timezone: &Tz) -> Option<String>
where
    Tz: TimeZone,
{
    local_naive(at, timezone).map(|local| local.format(CORRECTION_FORMAT).to_string())
}

pub(crate) fn parse_correction_timestamp<Tz>(
    text: &str,
    timezone: &Tz,
    original: Option<DateTime<Utc>>,
) -> Result<DateTime<Utc>, &'static str>
where
    Tz: TimeZone,
{
    let local = NaiveDateTime::parse_from_str(text, CORRECTION_FORMAT)
        .map_err(|_| "Use YYYY-MM-DD HH:MM")?;
    if local.format(CORRECTION_FORMAT).to_string() != text {
        return Err("Use YYYY-MM-DD HH:MM");
    }
    match timezone.offset_from_local_datetime(&local) {
        chrono::LocalResult::Single(offset) => local_to_utc(local, offset.fix()),
        chrono::LocalResult::Ambiguous(first, second) => {
            let wanted =
                original.map(|at| timezone.offset_from_utc_datetime(&at.naive_utc()).fix());
            let first = first.fix();
            let second = second.fix();
            match (
                wanted.is_some_and(|offset| offset == first),
                wanted.is_some_and(|offset| offset == second),
            ) {
                (true, false) => local_to_utc(local, first),
                (false, true) => local_to_utc(local, second),
                _ => Err("Ambiguous local time"),
            }
        }
        chrono::LocalResult::None => Err("Local time does not exist"),
    }
}

fn local_to_utc(local: NaiveDateTime, offset: FixedOffset) -> Result<DateTime<Utc>, &'static str> {
    local
        .checked_sub_offset(offset)
        .map(|utc| DateTime::from_naive_utc_and_offset(utc, Utc))
        .ok_or(OUTSIDE_EDITABLE_RANGE)
}

pub(crate) fn resolve_correction_timestamp<Tz>(
    input: &TimestampInput,
    timezone: &Tz,
    original: DateTime<Utc>,
) -> Result<DateTime<Utc>, &'static str>
where
    Tz: TimeZone,
{
    if let Some(instant) = input.adjusted_instant {
        Ok(instant)
    } else if input.is_unchanged() {
        Ok(original)
    } else {
        parse_correction_timestamp(input.text(), timezone, Some(original))
    }
}

pub(crate) fn adjusted_correction_timestamp<Tz>(
    input: &TimestampInput,
    delta: TimeDelta,
    timezone: &Tz,
    original: DateTime<Utc>,
) -> Result<(String, DateTime<Utc>), &'static str>
where
    Tz: TimeZone,
{
    let timestamp = match input.adjusted_instant {
        Some(instant) => instant,
        None => parse_correction_timestamp(input.text(), timezone, Some(original))?,
    };
    let adjusted = timestamp
        .checked_add_signed(delta)
        .ok_or("Timestamp is out of range")?;
    let text = correction_timestamp(adjusted, timezone).ok_or(OUTSIDE_EDITABLE_RANGE)?;
    if parse_correction_timestamp(&text, timezone, Some(adjusted)) != Ok(adjusted) {
        return Err("Adjustment cannot be represented as a local minute");
    }
    Ok((text, adjusted))
}

#[cfg(test)]
mod rendering_tests {
    use super::*;
    use chrono::{DateTime, FixedOffset, MappedLocalTime, NaiveDate, TimeZone, Utc};

    #[test]
    fn local_times_render_as_local_minutes() {
        let plus_two = FixedOffset::east_opt(2 * 3600).unwrap();
        assert_eq!(
            local_time(DateTime::<Utc>::from_timestamp(0, 0).unwrap(), &plus_two),
            "1970-01-01 02:00"
        );
        assert_eq!(
            local_time(
                DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
                &FixedOffset::west_opt(5 * 3600 + 1800).unwrap()
            ),
            "1969-12-31 18:30"
        );
        assert_eq!(
            local_time(
                DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
                &FixedOffset::east_opt(0).unwrap()
            ),
            "1970-01-01 00:00"
        );
    }

    #[test]
    fn local_times_render_chrono_signed_expanded_years() {
        for (year, expected) in [
            (-1, "-0001-01-02 03:04"),
            (0, "0000-01-02 03:04"),
            (9999, "9999-01-02 03:04"),
            (10000, "+10000-01-02 03:04"),
        ] {
            let instant = Utc.with_ymd_and_hms(year, 1, 2, 3, 4, 0).single().unwrap();
            assert_eq!(local_time(instant, &Utc), expected);
            assert_eq!(
                parse_correction_timestamp(expected, &Utc, None),
                Ok(instant)
            );
        }
    }

    #[test]
    fn local_times_outside_chrono_range_render_a_safe_marker() {
        assert_eq!(
            local_time(DateTime::<Utc>::MIN_UTC, &FixedOffset::west_opt(1).unwrap()),
            "outside local range"
        );
        assert_eq!(
            local_time(DateTime::<Utc>::MAX_UTC, &FixedOffset::east_opt(1).unwrap()),
            "outside local range"
        );
    }

    /// A zone that jumps from UTC+01:00 to UTC+02:00 at a fixed UTC instant.
    #[derive(Clone, Copy, Debug)]
    struct SwitchingZone {
        switch: i64,
    }

    impl SwitchingZone {
        fn pick(&self, utc_seconds: i64) -> FixedOffset {
            if utc_seconds < self.switch {
                FixedOffset::east_opt(3600).unwrap()
            } else {
                FixedOffset::east_opt(2 * 3600).unwrap()
            }
        }
    }

    impl TimeZone for SwitchingZone {
        type Offset = FixedOffset;

        fn from_offset(_offset: &FixedOffset) -> Self {
            unimplemented!("the formatter never recovers the zone")
        }

        fn offset_from_local_date(&self, local: &NaiveDate) -> MappedLocalTime<FixedOffset> {
            self.offset_from_local_datetime(
                &local
                    .and_hms_opt(0, 0, 0)
                    .expect("midnight exists on every date"),
            )
        }

        fn offset_from_local_datetime(
            &self,
            local: &NaiveDateTime,
        ) -> MappedLocalTime<FixedOffset> {
            MappedLocalTime::Single(self.pick(local.and_utc().timestamp()))
        }

        fn offset_from_utc_date(&self, utc: &NaiveDate) -> FixedOffset {
            self.pick(
                utc.and_hms_opt(0, 0, 0)
                    .expect("midnight exists on every date")
                    .and_utc()
                    .timestamp(),
            )
        }

        fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> FixedOffset {
            self.pick(utc.and_utc().timestamp())
        }
    }

    #[test]
    fn local_times_use_the_offset_valid_at_each_instant() {
        let zone = SwitchingZone { switch: 3600 };
        assert_eq!(
            local_time(DateTime::<Utc>::from_timestamp(3599, 0).unwrap(), &zone),
            "1970-01-01 01:59"
        );
        assert_eq!(
            local_time(DateTime::<Utc>::from_timestamp(3600, 0).unwrap(), &zone),
            "1970-01-01 03:00"
        );
    }
}

#[cfg(test)]
mod timezone_tests {
    use super::*;

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
}

#[cfg(test)]
mod correction_tests {
    use super::*;
    use chrono::{MappedLocalTime, NaiveDate};

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).unwrap()
    }

    #[derive(Clone, Copy, Debug)]
    struct CorrectionTestZone;

    impl CorrectionTestZone {
        fn offset(seconds: i64) -> FixedOffset {
            let offset = if seconds < 3_600 {
                3_600
            } else if seconds < 18_000 {
                7_200
            } else if seconds < 100_000 {
                3_600
            } else {
                10_800
            };
            FixedOffset::east_opt(offset).unwrap()
        }
    }

    fn early_local_offset(seconds: i64) -> MappedLocalTime<FixedOffset> {
        if seconds < 7_200 {
            MappedLocalTime::Single(FixedOffset::east_opt(3_600).unwrap())
        } else if seconds < 10_800 {
            MappedLocalTime::None
        } else {
            MappedLocalTime::Single(FixedOffset::east_opt(7_200).unwrap())
        }
    }

    fn late_local_offset(seconds: i64) -> MappedLocalTime<FixedOffset> {
        if seconds < 25_200 {
            MappedLocalTime::Ambiguous(
                FixedOffset::east_opt(7_200).unwrap(),
                FixedOffset::east_opt(3_600).unwrap(),
            )
        } else if seconds < 103_600 {
            MappedLocalTime::Single(FixedOffset::east_opt(3_600).unwrap())
        } else if seconds < 110_800 {
            MappedLocalTime::None
        } else {
            MappedLocalTime::Single(FixedOffset::east_opt(10_800).unwrap())
        }
    }

    impl TimeZone for CorrectionTestZone {
        type Offset = FixedOffset;

        fn from_offset(_offset: &Self::Offset) -> Self {
            Self
        }

        fn offset_from_local_date(&self, local: &NaiveDate) -> MappedLocalTime<Self::Offset> {
            self.offset_from_local_datetime(&local.and_hms_opt(0, 0, 0).unwrap())
        }

        fn offset_from_local_datetime(
            &self,
            local: &NaiveDateTime,
        ) -> MappedLocalTime<Self::Offset> {
            let seconds = local.and_utc().timestamp();
            if seconds < 21_600 {
                early_local_offset(seconds)
            } else {
                late_local_offset(seconds)
            }
        }

        fn offset_from_utc_date(&self, utc: &NaiveDate) -> Self::Offset {
            Self::offset(utc.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp())
        }

        fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> Self::Offset {
            Self::offset(utc.and_utc().timestamp())
        }
    }

    #[derive(Clone, Copy, Debug)]
    struct SubminuteTransitionZone;

    impl TimeZone for SubminuteTransitionZone {
        type Offset = FixedOffset;

        fn from_offset(_offset: &Self::Offset) -> Self {
            Self
        }

        fn offset_from_local_date(&self, local: &NaiveDate) -> MappedLocalTime<Self::Offset> {
            self.offset_from_local_datetime(&local.and_hms_opt(0, 0, 0).unwrap())
        }

        fn offset_from_local_datetime(
            &self,
            local: &NaiveDateTime,
        ) -> MappedLocalTime<Self::Offset> {
            let seconds = local.and_utc().timestamp();
            if seconds < 330 {
                MappedLocalTime::Single(FixedOffset::east_opt(30).unwrap())
            } else if seconds < 360 {
                MappedLocalTime::None
            } else {
                MappedLocalTime::Single(FixedOffset::east_opt(60).unwrap())
            }
        }

        fn offset_from_utc_date(&self, utc: &NaiveDate) -> Self::Offset {
            self.offset_from_utc_datetime(&utc.and_hms_opt(0, 0, 0).unwrap())
        }

        fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> Self::Offset {
            if utc.and_utc().timestamp() < 300 {
                FixedOffset::east_opt(30).unwrap()
            } else {
                FixedOffset::east_opt(60).unwrap()
            }
        }
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
    fn timestamp_input_removes_whole_characters_and_rejects_invalid_or_excess_input() {
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
        for _ in 0..input.text().chars().count() {
            input.backspace();
        }
        for character in "1970-01-01 06:30".chars() {
            input.insert(character);
        }

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
}
