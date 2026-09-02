//! Reusable tasks.

use std::fmt;

use crate::ids::TaskId;

/// The name of a task.
///
/// Names are non-empty after trimming, contain no C0 or C1 control
/// characters, and are at most [`TaskName::MAX_LEN`] Unicode scalar values
/// long. The stored name is the trimmed text. The same rules protect names
/// decoded from stored data as protect names typed by a caller.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TaskName(String);

/// Why a task name was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TaskNameError {
    /// The name is empty or only whitespace.
    #[error("task name must not be empty")]
    Empty,
    /// The name contains a C0 or C1 control character, such as escape,
    /// newline, carriage return, tab, or DEL.
    #[error("task name must not contain control characters")]
    Control,
    /// The trimmed name is longer than [`TaskName::MAX_LEN`] Unicode scalar
    /// values.
    #[error("task name must be at most 256 characters")]
    TooLong,
}

impl TaskName {
    /// The maximum length of a task name, counted in Unicode scalar values
    /// after trimming.
    pub const MAX_LEN: usize = 256;

    /// Validates and trims the given raw name.
    ///
    /// The stored name is the trimmed text. Control characters between
    /// non-whitespace characters are rejected; whitespace-only control
    /// characters at the edges are removed by trimming instead.
    pub fn new(raw: &str) -> Result<Self, TaskNameError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(TaskNameError::Empty);
        }
        if trimmed.chars().any(|character| character.is_control()) {
            return Err(TaskNameError::Control);
        }
        if trimmed.chars().count() > Self::MAX_LEN {
            return Err(TaskNameError::TooLong);
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// Returns the trimmed name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TaskName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A reusable task that worklogs can be recorded against.
///
/// Tasks are archived instead of deleted so past worklogs keep their context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// The stable UUIDv7 identifier.
    pub id: TaskId,
    /// The non-empty trimmed name.
    pub name: TaskName,
    /// Whether the task is archived. Archived tasks cannot start tracking.
    pub archived: bool,
}

impl Task {
    /// Creates a new, not archived task.
    pub fn new(id: TaskId, name: TaskName) -> Self {
        Self {
            id,
            name,
            archived: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(tag: u32) -> TaskId {
        TaskId::from_uuid(uuid::Uuid::from_u128(tag as u128))
    }

    #[test]
    fn new_trims_the_name_and_starts_not_archived() {
        let task = Task::new(id(1), TaskName::new("  Write tests  ").unwrap());
        assert_eq!(task.name.as_str(), "Write tests");
        assert!(!task.archived);
    }

    #[test]
    fn names_accept_plain_text() {
        let name = TaskName::new("Fix the coffee machine").unwrap();
        assert_eq!(name.as_str(), "Fix the coffee machine");
        assert_eq!(name.to_string(), "Fix the coffee machine");
    }

    #[test]
    fn names_accept_multibyte_english_text_and_edge_printables() {
        let name = TaskName::new("  Review the report ✓ ").unwrap();
        assert_eq!(name.as_str(), "Review the report ✓");
        // The printable characters around DEL and the C1 range are fine.
        assert!(TaskName::new("~").is_ok());
        assert!(TaskName::new("\u{7e}\u{80}").is_err());
    }

    #[test]
    fn names_reject_empty_and_whitespace_only_text() {
        assert_eq!(TaskName::new(""), Err(TaskNameError::Empty));
        assert_eq!(TaskName::new("   "), Err(TaskNameError::Empty));
        assert_eq!(TaskName::new("\t\n"), Err(TaskNameError::Empty));
        // Newlines are whitespace, so they trim away rather than fail.
        assert_eq!(TaskName::new("\nhello\n").unwrap().as_str(), "hello");
    }

    #[test]
    fn names_reject_c0_and_c1_control_characters() {
        for raw in [
            "\0", "\u{1}", "\u{7}", "\u{1b}", "a\nb", "a\rb", "a\tb", "\u{7f}", "\u{80}", "\u{9f}",
        ] {
            assert_eq!(
                TaskName::new(raw),
                Err(TaskNameError::Control),
                "{raw:?} must be rejected"
            );
        }
    }

    #[test]
    fn names_reject_osc_and_ansi_escape_input() {
        assert_eq!(
            TaskName::new("\u{1b}]0;window title\u{7}"),
            Err(TaskNameError::Control)
        );
        assert_eq!(
            TaskName::new("\u{1b}[31mred\u{1b}[0m"),
            Err(TaskNameError::Control)
        );
        // An escape at the very start, where trimming cannot remove it.
        assert_eq!(TaskName::new("\u{1b} escape"), Err(TaskNameError::Control));
    }

    #[test]
    fn control_characters_are_reported_before_length() {
        let long = format!("{}\0{}", "a".repeat(TaskName::MAX_LEN), "a".repeat(10));
        assert_eq!(TaskName::new(&long), Err(TaskNameError::Control));
    }

    #[test]
    fn names_accept_exactly_the_maximum_length() {
        let ascii = TaskName::new(&"a".repeat(TaskName::MAX_LEN)).unwrap();
        assert_eq!(ascii.as_str().chars().count(), TaskName::MAX_LEN);
        // The limit counts Unicode scalar values, not bytes.
        let suffix = " task ✓";
        let multibyte = format!(
            "{}{}",
            "a".repeat(TaskName::MAX_LEN - suffix.chars().count()),
            suffix
        );
        let name = TaskName::new(&multibyte).unwrap();
        assert_eq!(name.as_str().chars().count(), TaskName::MAX_LEN);
        assert!(name.as_str().len() > TaskName::MAX_LEN);
    }

    #[test]
    fn names_reject_one_scalar_over_the_maximum_length() {
        assert_eq!(
            TaskName::new(&"a".repeat(TaskName::MAX_LEN + 1)),
            Err(TaskNameError::TooLong)
        );
        let suffix = " task ✓x";
        let multibyte = format!(
            "{}{}",
            "a".repeat(TaskName::MAX_LEN + 1 - suffix.chars().count()),
            suffix
        );
        assert_eq!(TaskName::new(&multibyte), Err(TaskNameError::TooLong));
    }

    #[test]
    fn length_is_checked_after_trimming() {
        // Trimming can bring an oversized raw name back into bounds.
        let padded = format!(" {} ", "a".repeat(TaskName::MAX_LEN));
        assert!(TaskName::new(&padded).is_ok());
        // But it cannot rescue a name that is still too long.
        let padded = format!(" {} ", "a".repeat(TaskName::MAX_LEN + 1));
        assert_eq!(TaskName::new(&padded), Err(TaskNameError::TooLong));
    }

    #[test]
    fn names_compare_by_trimmed_value() {
        assert_eq!(
            TaskName::new("demos").unwrap(),
            TaskName::new("  demos ").unwrap()
        );
    }
}
