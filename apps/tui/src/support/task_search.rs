use std::cmp::Reverse;

use chrono::{DateTime, Utc};
use tracker_domain::TaskId;

/// The same recent-activity order for every task search result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SearchRank {
    activity_at: Reverse<DateTime<Utc>>,
    created_at: Reverse<DateTime<Utc>>,
    id: TaskId,
}

impl SearchRank {
    pub(crate) fn new(id: TaskId, created_at: DateTime<Utc>, activity_at: DateTime<Utc>) -> Self {
        Self {
            activity_at: Reverse(activity_at),
            created_at: Reverse(created_at),
            id,
        }
    }
}

/// Returns whether the query occurs in the task name in order, ignoring case.
pub(crate) fn fuzzy_match(name: &str, query: &str) -> bool {
    let mut name = name.chars().flat_map(char::to_lowercase);
    query
        .chars()
        .flat_map(char::to_lowercase)
        .all(|wanted| name.by_ref().any(|character| character == wanted))
}

#[cfg(test)]
mod tests {
    use super::fuzzy_match;

    #[test]
    fn matching_accepts_ordered_non_adjacent_letters_without_case() {
        assert!(fuzzy_match("Build release", "BRe"));
        assert!(!fuzzy_match("Build release", "BZe"));
    }
}
