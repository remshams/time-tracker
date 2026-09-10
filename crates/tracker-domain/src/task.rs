//! Reusable tasks.

use std::fmt;

use chrono::{DateTime, Utc};

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

/// Why a task could not be built from stored values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TaskError {
    /// `updated_at` precedes `created_at`, which no task history can
    /// produce.
    #[error("task updated_at must not precede created_at")]
    UpdatedBeforeCreated,
}

/// A reusable task that worklogs can be recorded against.
///
/// Tasks are archived instead of deleted so past worklogs keep their context.
/// Both timestamps are client-created UTC values. Creating a task sets them
/// to the same instant; only semantic metadata changes (rename, archive,
/// restore) ever advance `updated_at`, and it never moves backward.
/// Tracking operations leave both timestamps untouched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// The stable UUIDv7 identifier.
    id: TaskId,
    /// The non-empty trimmed name.
    name: TaskName,
    /// Whether the task is archived. Archived tasks cannot start tracking.
    archived: bool,
    /// When the task was created, in UTC.
    created_at: DateTime<Utc>,
    /// When the task's metadata last changed semantically, in UTC.
    updated_at: DateTime<Utc>,
}

impl Task {
    /// Returns the stable task identifier.
    pub fn id(&self) -> TaskId {
        self.id
    }

    /// Returns the task name.
    pub fn name(&self) -> &TaskName {
        &self.name
    }

    /// Whether the task is archived.
    pub fn is_archived(&self) -> bool {
        self.archived
    }

    /// Returns the task creation timestamp.
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    /// Returns the last semantic metadata-change timestamp.
    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    /// Creates a new, not archived task stamped with the client's creation
    /// timestamp. Creation sets `created_at` and `updated_at` to the same
    /// value.
    pub fn create(id: TaskId, name: TaskName, created_at: DateTime<Utc>) -> Self {
        Self {
            id,
            name,
            archived: false,
            created_at,
            updated_at: created_at,
        }
    }

    /// Rebuilds a task from stored values, rejecting an `updated_at` that
    /// precedes `created_at`.
    pub fn rehydrate(
        id: TaskId,
        name: TaskName,
        archived: bool,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, TaskError> {
        if updated_at < created_at {
            return Err(TaskError::UpdatedBeforeCreated);
        }
        Ok(Self {
            id,
            name,
            archived,
            created_at,
            updated_at,
        })
    }

    /// Renames the task, advancing `updated_at` by the domain rules.
    ///
    /// Renaming to the current name is an idempotent no-op that leaves
    /// `updated_at` alone. A real rename advances `updated_at` to the later
    /// of the stored value and `occurred_at`, so a client clock that reports
    /// an earlier time cannot move the timestamp backward. Returns whether
    /// the task changed.
    pub fn rename(&mut self, name: TaskName, occurred_at: DateTime<Utc>) -> bool {
        if name == self.name {
            return false;
        }
        self.name = name;
        self.touch(occurred_at);
        true
    }

    /// Archives the task, advancing `updated_at` by the domain rules.
    ///
    /// Archiving an already archived task is an idempotent no-op. Returns
    /// whether the task changed.
    pub fn archive(&mut self, occurred_at: DateTime<Utc>) -> bool {
        if self.archived {
            return false;
        }
        self.archived = true;
        self.touch(occurred_at);
        true
    }

    /// Restores an archived task, advancing `updated_at` by the domain
    /// rules.
    ///
    /// Restoring a task that is not archived is an idempotent no-op.
    /// Returns whether the task changed.
    pub fn restore(&mut self, occurred_at: DateTime<Utc>) -> bool {
        if !self.archived {
            return false;
        }
        self.archived = false;
        self.touch(occurred_at);
        true
    }

    /// Advances `updated_at` to the later of the stored value and `at`.
    fn touch(&mut self, at: DateTime<Utc>) {
        self.updated_at = self.updated_at.max(at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(tag: u32) -> TaskId {
        TaskId::from_uuid(uuid::Uuid::from_u128(tag as u128))
    }

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).unwrap()
    }

    fn created(tag: u32, seconds: i64) -> Task {
        Task::create(id(tag), TaskName::new("task").unwrap(), at(seconds))
    }

    #[test]
    fn create_sets_both_timestamps_to_the_same_client_value() {
        let task = Task::create(id(1), TaskName::new("  Write tests  ").unwrap(), at(500));
        assert_eq!(task.id(), id(1));
        assert_eq!(task.name.as_str(), "Write tests");
        assert!(!task.archived);
        assert_eq!(task.created_at, at(500));
        assert_eq!(task.updated_at, at(500));
    }

    #[test]
    fn rehydrate_accepts_equal_and_later_updated_timestamps() {
        let equal = Task::rehydrate(
            id(1),
            TaskName::new("kept").unwrap(),
            true,
            at(100),
            at(100),
        )
        .unwrap();
        assert!(equal.archived);
        assert_eq!(equal.created_at, at(100));
        assert_eq!(equal.updated_at, at(100));

        let later = Task::rehydrate(
            id(2),
            TaskName::new("edited").unwrap(),
            false,
            at(100),
            at(350),
        )
        .unwrap();
        assert_eq!(later.updated_at, at(350));
    }

    #[test]
    fn rehydrate_rejects_an_updated_timestamp_before_creation() {
        let error = Task::rehydrate(
            id(1),
            TaskName::new("broken").unwrap(),
            false,
            at(200),
            at(199),
        )
        .expect_err("updated before created is impossible");
        assert_eq!(error, TaskError::UpdatedBeforeCreated);
    }

    #[test]
    fn rename_advances_updated_at_to_the_operation_timestamp() {
        let mut task = created(1, 100);
        let renamed = task.rename(TaskName::new("new name").unwrap(), at(300));
        assert!(renamed);
        assert_eq!(task.name.as_str(), "new name");
        assert_eq!(task.created_at, at(100));
        assert_eq!(task.updated_at, at(300));
    }

    #[test]
    fn rename_never_moves_updated_at_backward() {
        let mut task = created(1, 100);
        task.rename(TaskName::new("first").unwrap(), at(500));
        let renamed = task.rename(TaskName::new("second").unwrap(), at(200));
        assert!(renamed);
        assert_eq!(task.name.as_str(), "second");
        assert_eq!(task.updated_at, at(500), "the stored value stays");
    }

    #[test]
    fn renaming_to_the_current_name_changes_nothing() {
        let mut task = created(1, 100);
        task.rename(TaskName::new("same").unwrap(), at(300));
        let renamed = task.rename(TaskName::new(" same ").unwrap(), at(400));
        assert!(!renamed, "the trimmed name is the current name");
        assert_eq!(task.updated_at, at(300), "a no-op rename does not advance");
    }

    #[test]
    fn archive_and_restore_advance_updated_at() {
        let mut task = created(1, 100);
        assert!(task.archive(at(200)));
        assert!(task.archived);
        assert_eq!(task.updated_at, at(200));
        assert!(task.restore(at(300)));
        assert!(!task.archived);
        assert_eq!(task.updated_at, at(300));
        assert_eq!(task.created_at, at(100), "creation is never rewritten");
    }

    #[test]
    fn archive_and_restore_forward_only_and_idempotent() {
        let mut task = created(1, 100);
        assert!(task.archive(at(500)));
        assert!(!task.archive(at(600)), "already archived is a no-op");
        assert_eq!(task.updated_at, at(500));
        assert!(task.restore(at(200)), "a late client clock cannot rewind");
        assert_eq!(task.updated_at, at(500));
        assert!(!task.restore(at(700)), "already restored is a no-op");
        assert_eq!(task.updated_at, at(500));
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
