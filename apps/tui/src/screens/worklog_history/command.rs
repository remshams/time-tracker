//! Semantic commands owned by the worklog-history screen.

/// An action interpreted by the worklog-history screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorklogHistoryCommand {
    /// Move the worklog selection up one row.
    MoveUp,
    /// Move the worklog selection down one row.
    MoveDown,
    First,
    Last,
    PageUp,
    PageDown,
    GPrefix,
    /// Open timestamp correction for the selected worklog.
    OpenCorrection,
    /// Ask for confirmation before deleting the selected completed worklog.
    OpenDeletion,
    /// Choose another active task for the selected worklog.
    OpenMove,
    /// Move focus between correction fields.
    SwitchCorrectionField,
    /// Move the correction cursor left.
    MoveCursorLeft,
    /// Move the correction cursor right.
    MoveCursorRight,
    /// Switch between move-dialog search input and result selection.
    ToggleMoveFocus,
    /// Move the selected move-dialog destination up one row.
    MoveDestinationUp,
    /// Move the selected move-dialog destination down one row.
    MoveDestinationDown,
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
    /// Confirm the pending correction or deletion dialog.
    Confirm,
    /// Dismiss the pending correction or deletion dialog.
    Cancel,
    /// Type a character into the focused correction field.
    Insert(char),
    /// Type a character into the focused move search field.
    InsertMoveQuery(char),
    /// Delete the character before the correction cursor.
    Backspace,
    /// Delete the final character in the move search field.
    BackspaceMoveQuery,
}
