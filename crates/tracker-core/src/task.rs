//! Reusable tasks.

use std::fmt;

use crate::ids::TaskId;

/// The name of a task.
///
/// Names are non-empty after trimming. The stored name is the trimmed text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TaskName(String);

/// Why a task name was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TaskNameError {
    /// The name is empty or only whitespace.
    #[error("task name must not be empty")]
    Empty,
}

impl TaskName {
    /// Validates and trims the given raw name.
    pub fn new(raw: &str) -> Result<Self, TaskNameError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(TaskNameError::Empty);
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

/// A reusable task that time entries can be recorded against.
///
/// Tasks are archived instead of deleted so past entries keep their context.
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
    fn names_reject_empty_and_whitespace_only_text() {
        assert_eq!(TaskName::new(""), Err(TaskNameError::Empty));
        assert_eq!(TaskName::new("   "), Err(TaskNameError::Empty));
        assert_eq!(TaskName::new("\t\n"), Err(TaskNameError::Empty));
    }

    #[test]
    fn names_compare_by_trimmed_value() {
        assert_eq!(
            TaskName::new("demos").unwrap(),
            TaskName::new("  demos ").unwrap()
        );
    }
}
