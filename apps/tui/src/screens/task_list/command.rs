//! Semantic commands owned by the task-list screen.

/// An action interpreted by the task-list screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskListCommand {
    /// Move the task selection up one row.
    MoveUp,
    /// Move the task selection down one row.
    MoveDown,
    First,
    Last,
    PageUp,
    PageDown,
    GPrefix,
    /// Show the list of active tasks.
    ShowActiveTasks,
    /// Show the list of archived tasks.
    ShowArchivedTasks,
    ShowReports,
    ShowAllWorklogs,
    CopySelectedName,
    /// Open the worklog history of the selected task.
    OpenHistory,
    /// Cycle the shared task-list ordering.
    CycleOrdering,
    /// Start editing a task-name search.
    OpenSearch,
    /// Keep the current search and return to task actions.
    CommitSearch,
    /// Restore the query and selection from before editing.
    CancelSearch,
    /// Remove the current task-name filter.
    ClearSearch,
    /// Add one character to the search query.
    InsertSearch(char),
    /// Remove the final character from the search query.
    BackspaceSearch,
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
    /// Preview all inactive tasks before asking to archive them.
    OpenInactiveArchivePreview,
    /// Confirm the pending input or archive dialog.
    Confirm,
    /// Dismiss the pending input or archive dialog.
    Cancel,
    /// Type a character into the open text input.
    Insert(char),
    /// Delete the character before the cursor in the open input.
    Backspace,
}
