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

    pub(crate) fn at(&self, since_anchor: Duration) -> Duration {
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
