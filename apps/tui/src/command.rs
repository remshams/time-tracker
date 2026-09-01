//! Semantic commands the interface acts on.
//!
//! Key handling translates raw key events into these commands, so the app and
//! the rendered widgets never inspect key codes themselves.

/// An action the interface understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Move the task selection up one row.
    MoveUp,
    /// Move the task selection down one row.
    MoveDown,
    /// Start, stop, or switch tracking for the selected task.
    ToggleTracking,
    /// Open the text input to add a task.
    OpenAdd,
    /// Open the text input to rename the selected task.
    OpenRename,
    /// Ask for confirmation before archiving the selected task.
    OpenArchiveConfirm,
    /// Confirm the pending text input or archive dialog.
    Confirm,
    /// Dismiss the pending text input or archive dialog.
    Cancel,
    /// Type a character into the open text input.
    Insert(char),
    /// Delete the last character of the open text input.
    Backspace,
    /// Leave the application.
    Quit,
    /// A key that is deliberately unmapped on this screen.
    ///
    /// `h` and `l` stay reserved for horizontal or parent/child navigation
    /// once a screen has that concept; they perform no fake action here.
    Reserved,
}
