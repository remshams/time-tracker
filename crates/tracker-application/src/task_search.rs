//! Task-name matching and ordering shared by clients.

use std::cmp::Reverse;

use chrono::{DateTime, Utc};
use tracker_domain::TaskId;

/// Sorts recent activity first, then creation time, then task identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SearchRank {
    activity_at: Reverse<DateTime<Utc>>,
    created_at: Reverse<DateTime<Utc>>,
    id: TaskId,
}

impl SearchRank {
    /// Builds a rank from the later metadata update or work start.
    pub fn from_task_activity(
        id: TaskId,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        latest_work_start: Option<DateTime<Utc>>,
    ) -> Self {
        let activity_at = latest_work_start.map_or(updated_at, |worked| worked.max(updated_at));
        Self::new(id, created_at, activity_at)
    }

    /// Builds a rank using the caller's latest task activity timestamp.
    pub fn new(id: TaskId, created_at: DateTime<Utc>, activity_at: DateTime<Utc>) -> Self {
        Self {
            activity_at: Reverse(activity_at),
            created_at: Reverse(created_at),
            id,
        }
    }
}

/// Matches ordered, possibly non-adjacent characters without case distinctions.
pub fn fuzzy_match(name: &str, query: &str) -> bool {
    let mut name = name.chars().flat_map(char::to_lowercase);
    query
        .chars()
        .flat_map(char::to_lowercase)
        .all(|wanted| name.by_ref().any(|character| character == wanted))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_respects_character_order_and_unicode_lowercase_expansion() {
        assert!(fuzzy_match("Build release", "BRe"));
        assert!(!fuzzy_match("Build release", "rB"));
        assert!(!fuzzy_match("Build release", "BZe"));
        assert!(fuzzy_match("", ""));
        assert!(!fuzzy_match("", "a"));
        assert!(!fuzzy_match("a", "aa"));
        assert!(fuzzy_match("İssue", "i\u{307}s"));
        assert!(fuzzy_match("i\u{307}ssue", "İs"));
    }

    #[test]
    fn rank_orders_each_tie_breaker_and_stays_equal_for_identical_inputs() {
        let at = |seconds| DateTime::from_timestamp(seconds, 0).unwrap();
        let id = |number| TaskId::from_uuid(uuid::Uuid::from_u128(number));
        let rank =
            |id_value, created, activity| SearchRank::new(id(id_value), at(created), at(activity));
        assert!(rank(4, 1, 3) < rank(1, 2, 2));
        assert!(rank(4, 3, 3) < rank(1, 2, 3));
        assert!(rank(1, 2, 3) < rank(2, 2, 3));
        assert_eq!(rank(1, 2, 3), rank(1, 2, 3));
    }

    #[test]
    fn task_activity_uses_the_latest_update_or_work_start() {
        let at = |seconds| DateTime::from_timestamp(seconds, 0).unwrap();
        let id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        for (worked, activity) in [
            (None, 200),
            (Some(150), 200),
            (Some(200), 200),
            (Some(300), 300),
        ] {
            assert_eq!(
                SearchRank::from_task_activity(id, at(100), at(200), worked.map(at)),
                SearchRank::new(id, at(100), at(activity))
            );
        }
    }
}
