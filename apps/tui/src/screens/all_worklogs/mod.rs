mod actions;
mod keymap;
mod state;
pub(crate) mod view;

pub(crate) use keymap::{footer_hints, map};
pub(crate) use state::{AllWorklogsFocus, AllWorklogsState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AllWorklogsCommand {
    ShowArchived,
    ShowReports,
    FocusRows,
    FocusTabs,
    MoveUp,
    MoveDown,
    First,
    Last,
    PageUp,
    PageDown,
    GPrefix,
    LoadOlder,
    Refresh,
    OpenMove,
    ToggleMoveFocus,
    MoveDestinationUp,
    MoveDestinationDown,
    InsertMoveQuery(char),
    BackspaceMoveQuery,
    ConfirmMove,
    CancelMove,
}
