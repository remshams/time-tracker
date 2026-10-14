//! Timestamp precision shared by application services and client adapters.

use chrono::{DateTime, Utc};
use tracker_domain::WorklogTimes;

/// Discards sub-microsecond digits before validation or comparison.
///
/// This matches persisted UTC timestamps, including instants before the epoch.
pub fn canonical_timestamp(timestamp: DateTime<Utc>) -> DateTime<Utc> {
    DateTime::from_timestamp_micros(timestamp.timestamp_micros())
        .expect("every UTC timestamp fits the canonical microsecond range")
}

/// Normalizes both worklog boundaries without validating their order.
pub fn canonical_worklog_times(times: WorklogTimes) -> WorklogTimes {
    WorklogTimes::new(
        canonical_timestamp(times.start()),
        times.end().map(canonical_timestamp),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Truncation preserves pre-epoch microseconds and never rounds fractional digits up.
    #[test]
    fn timestamp_truncation_discards_fractional_digits_without_rounding() {
        for seconds in [-1, 0, 100] {
            let input = DateTime::from_timestamp(seconds, 123_456_789).unwrap();
            let expected = DateTime::from_timestamp(seconds, 123_456_000).unwrap();
            assert_eq!(canonical_timestamp(input), expected);
            assert_eq!(canonical_timestamp(expected), expected);
        }
        let before_epoch = DateTime::from_timestamp(-1, 999_999_999).unwrap();
        assert_eq!(canonical_timestamp(before_epoch).timestamp_micros(), -1);
    }

    /// Chrono UTC range boundaries must normalize without an overflow or panic.
    #[test]
    fn chrono_utc_extremes_fit_the_canonical_microsecond_range() {
        for instant in [DateTime::<Utc>::MIN_UTC, DateTime::<Utc>::MAX_UTC] {
            assert_eq!(
                canonical_timestamp(instant).timestamp_micros(),
                instant.timestamp_micros()
            );
        }
    }

    /// Sub-microsecond differences collapse before order validation, while open ends stay open.
    #[test]
    fn worklog_boundaries_normalize_before_order_validation() {
        let earlier = DateTime::from_timestamp(100, 100).unwrap();
        let later = DateTime::from_timestamp(100, 900).unwrap();
        let expected = DateTime::from_timestamp(100, 0).unwrap();
        let completed = canonical_worklog_times(WorklogTimes::new(later, Some(earlier)));
        assert_eq!(completed.start(), expected);
        assert_eq!(completed.end(), Some(expected));
        assert!(completed.validate().is_ok());

        let active = canonical_worklog_times(WorklogTimes::new(later, None));
        assert_eq!(active.start(), expected);
        assert_eq!(active.end(), None);
    }
}
