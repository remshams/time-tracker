mod actions;
mod keymap;
mod state;
pub mod view;

pub(crate) use keymap::{footer_hints, map};
pub(crate) use state::DateShift;
pub use state::{ReportFocus, ReportMode, ReportPreset, ReportState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReportCommand {
    ShowActive,
    ShowAllWorklogs,
    MoveUp,
    MoveDown,
    First,
    Last,
    PageUp,
    PageDown,
    PreviousPeriod,
    NextPeriod,
    Refresh,
    FocusPresets,
    FocusRows,
    FocusTabs,
    PresetPrevious,
    PresetNext,
    ChoosePreset,
    SwitchField,
    ShiftCustomDate(DateShift),
    Insert(char),
    Backspace,
    ConfirmCustom,
    Cancel,
    OpenHistory,
    CopyName,
    CopyExact,
    CopyRounded,
    GPrefix,
}
