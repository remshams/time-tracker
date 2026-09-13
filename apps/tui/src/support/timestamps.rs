use chrono::{DateTime, FixedOffset, NaiveDateTime, Offset, TimeDelta, TimeZone, Utc};

/// One bounded timestamp input with a character-indexed cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimestampInput {
    pub(crate) text: String,
    pub(crate) initial_text: String,
    pub(crate) cursor: usize,
    pub(crate) adjusted_instant: Option<DateTime<Utc>>,
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
    let offset = timezone.offset_from_utc_datetime(&at.naive_utc()).fix();
    at.naive_utc()
        .checked_add_offset(offset)
        .map(|local| local.format(CORRECTION_FORMAT).to_string())
        .unwrap_or_else(|| "outside local range".to_owned())
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

pub(crate) fn local_to_utc(
    local: NaiveDateTime,
    offset: FixedOffset,
) -> Result<DateTime<Utc>, &'static str> {
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
