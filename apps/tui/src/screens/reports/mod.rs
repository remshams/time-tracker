mod actions;
mod keymap;
mod state;
pub mod view;

pub(crate) use keymap::{footer_hints, map};
pub use state::{ReportMode, ReportPreset, ReportState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReportCommand {
    ShowActive,
    ShowArchived,
    MoveUp,
    MoveDown,
    First,
    Last,
    PageUp,
    PageDown,
    PreviousPeriod,
    NextPeriod,
    Refresh,
    OpenPresets,
    PresetUp,
    PresetDown,
    ChoosePreset,
    SwitchField,
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
