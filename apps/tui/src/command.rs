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
    /// Show the list of active tasks.
    ShowActiveTasks,
    /// Show the list of archived tasks.
    ShowArchivedTasks,
    /// Open the worklog history of the selected task.
    OpenHistory,
    /// Open timestamp correction for the selected worklog.
    OpenCorrection,
    /// Move focus between correction fields.
    SwitchCorrectionField,
    /// Move the correction cursor left.
    MoveCursorLeft,
    /// Move the correction cursor right.
    MoveCursorRight,
    /// Delete the character under the correction cursor.
    Delete,
    /// Move the focused correction timestamp forward five minutes.
    AdjustForwardFiveMinutes,
    /// Move the focused correction timestamp backward five minutes.
    AdjustBackwardFiveMinutes,
    /// Move the focused correction timestamp forward one hour.
    AdjustForwardOneHour,
    /// Move the focused correction timestamp backward one hour.
    AdjustBackwardOneHour,
    /// Append the next bounded older page to the open worklog history.
    LoadOlderWorklogs,
    /// Reload the open worklog history from its newest page.
    RefreshWorklogs,
    /// Leave the worklog history and return to the task list.
    BackToTaskList,
    /// Cycle the shared task-list ordering.
    CycleOrdering,
    /// Restore the selected archived task to the active list.
    UnarchiveSelected,
    /// Start, stop, or switch tracking for the selected task.
    ToggleTracking,
    /// Open the text input to add a task.
    OpenAdd,
    /// Open the text input to rename the selected task.
    OpenRename,
    /// Ask for confirmation before archiving the selected task.
    OpenArchiveConfirm,
    /// Confirm the pending input, correction, or archive dialog.
    Confirm,
    /// Dismiss the pending input, correction, or archive dialog.
    Cancel,
    /// Type a character into the open text input.
    Insert(char),
    /// Delete the character before the cursor in the open input.
    Backspace,
    /// Leave the application.
    Quit,
}
