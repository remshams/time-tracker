use std::time::{Duration, Instant};

use chrono::{DateTime, TimeDelta, Utc};

/// Derives displayed elapsed time from a monotonic clock.
#[derive(Debug, Clone)]
pub(crate) struct ElapsedClock {
    anchor: Instant,
    base: Duration,
}

impl ElapsedClock {
    pub(crate) fn at_anchor(base: Duration, anchor: Instant) -> Self {
        Self { anchor, base }
    }

    pub(crate) fn base_since(start: DateTime<Utc>, now: DateTime<Utc>) -> Duration {
        (now - start).to_std().unwrap_or(Duration::ZERO)
    }

    fn at(&self, since_anchor: Duration) -> Duration {
        self.base + since_anchor
    }

    pub(crate) fn elapsed_at(&self, now: Instant) -> Duration {
        self.at(now.saturating_duration_since(self.anchor))
    }
}

pub(crate) fn tracking_timestamp(start: DateTime<Utc>, elapsed: Duration) -> DateTime<Utc> {
    let delta = TimeDelta::from_std(elapsed).unwrap_or(TimeDelta::MAX);
    start
        .checked_add_signed(delta)
        .unwrap_or(DateTime::<Utc>::MAX_UTC)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
