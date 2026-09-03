//! Component objects over one rendered `tt` screen.
//!
//! A [`TimeTrackerPage`] wraps a snapshot [`termlens::Screen`] taken at one
//! moment. The snapshot is immutable and never refreshes, while the
//! application keeps repainting, so any page goes stale as soon as the
//! terminal draws again. Wait methods on [`crate::driver::TuiDriver`]
//! recapture; a page held across a key press describes the past.
//!
//! Components know the public layout (which row holds what) and the visible
//! labels and accents. All coordinates and cell styles stay here; scenarios
//! ask semantic questions only.

use termlens::{Color, Screen};

/// The terminal geometry every scenario runs at.
pub(crate) const COLS: u16 = 80;
pub(crate) const ROWS: u16 = 24;

/// The exact empty-state hints the two views render instead of task rows.
const ACTIVE_EMPTY_HINT: &str = "No active tasks. Press a to add one.";
const ARCHIVED_EMPTY_HINT: &str = "No archived tasks.";

/// The geometry of the centered 56x3 modal over the panel. The body spans
/// rows 1..=21 of the fixed 80x24 screen, so the modal sits at rows
/// 10..=12, columns 12..=67.
const DIALOG_LEFT_COL: u16 = 12;
const DIALOG_RIGHT_COL: u16 = DIALOG_LEFT_COL + 55;
const DIALOG_TOP_ROW: u16 = 10;
const DIALOG_TEXT_ROW: u16 = DIALOG_TOP_ROW + 1;
const DIALOG_BOTTOM_ROW: u16 = DIALOG_TOP_ROW + 2;

/// The fixed frame layout: one header line, the bordered task panel, one
/// status line, one footer line.
const HEADER_ROW: u16 = 0;
const PANEL_TOP_ROW: u16 = 1;
const PANEL_FIRST_CONTENT_ROW: u16 = 2;
const PANEL_LAST_CONTENT_ROW: u16 = 20;
const STATUS_ROW: u16 = 22;
const FOOTER_ROW: u16 = 23;

/// The accents the interface draws with, as the terminal palette reports
/// them. Ratatui sends named ANSI colors as 256-color indexes: blue is
/// index 4, green 2, red 1.
const BLUE: Color = Color::Indexed(4);
const GREEN: Color = Color::Indexed(2);
const RED: Color = Color::Indexed(1);

/// One rendered screen, decomposed into the interface's components.
pub(crate) struct TimeTrackerPage {
    screen: Screen,
}

impl TimeTrackerPage {
    pub(crate) fn new(screen: Screen) -> Self {
        Self { screen }
    }

    /// The raw snapshot underneath, for conditions no component models yet.
    /// The staleness rules of the page apply.
    pub(crate) fn screen(&self) -> &Screen {
        &self.screen
    }

    pub(crate) fn header(&self) -> Header {
        Header {
            screen: self.screen.clone(),
        }
    }

    pub(crate) fn task_panel(&self) -> TaskPanel {
        TaskPanel {
            screen: self.screen.clone(),
        }
    }

    pub(crate) fn status_bar(&self) -> StatusBar {
        StatusBar {
            screen: self.screen.clone(),
        }
    }

    pub(crate) fn footer(&self) -> Footer {
        Footer {
            screen: self.screen.clone(),
        }
    }

    /// The one-line text input over the panel, if one is open.
    pub(crate) fn task_input_dialog(&self) -> Option<TaskInputDialog> {
        TaskInputDialog::find(&self.screen)
    }

    /// The archive confirmation over the panel, if one is open.
    pub(crate) fn archive_dialog(&self) -> Option<ArchiveDialog> {
        ArchiveDialog::find(&self.screen)
    }
}

/// The top line: application title, and the live timer while tracking runs.
pub(crate) struct Header {
    screen: Screen,
}

impl Header {
    /// The full text of the header line.
    pub(crate) fn text(&self) -> String {
        trimmed_row(&self.screen, HEADER_ROW)
    }

    /// Whether the title keeps its blue bold accent.
    pub(crate) fn title_is_accented(&self) -> bool {
        matches!(self.screen.cell(HEADER_ROW, 0), Some(cell)
            if cell.style().fg == BLUE && cell.style().bold)
    }

    /// Whether the header names no running task.
    pub(crate) fn is_idle(&self) -> bool {
        !self.text().contains('▶')
    }

    /// The running task the header names, if tracking is on.
    pub(crate) fn active_task(&self) -> Option<ActiveTask> {
        // Task rows carry the same marker, so the marker must sit in the
        // header row to count.
        let (marker_row, marker_col) = self.screen.find("▶")?;
        if marker_row != HEADER_ROW {
            return None;
        }
        // The header reads "Time Tracker  ▶ Name  HH:MM:SS"; the elapsed
        // time is always the last eight characters.
        let text = self.text();
        let marker_pos = text.find("▶ ")?;
        let after_marker = text[marker_pos + "▶ ".len()..].trim_start();
        if after_marker.len() < 10 {
            return None;
        }
        let split = after_marker.len() - 8;
        Some(ActiveTask {
            screen: self.screen.clone(),
            marker_col,
            name: after_marker[..split].trim_end().to_owned(),
            elapsed: after_marker[split..].to_owned(),
        })
    }
}

/// The running task as the header shows it.
pub(crate) struct ActiveTask {
    screen: Screen,
    marker_col: u16,
    name: String,
    elapsed: String,
}

impl ActiveTask {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// The elapsed time as `HH:MM:SS`.
    pub(crate) fn elapsed(&self) -> &str {
        &self.elapsed
    }

    /// Whether the elapsed time reads exactly `HH:MM:SS`: eight bytes,
    /// ASCII digits in every numeric position, colons at indexes 2 and 5.
    pub(crate) fn elapsed_is_hhmmss(&self) -> bool {
        let bytes = self.elapsed.as_bytes();
        bytes.len() == 8
            && bytes.iter().enumerate().all(|(index, byte)| match index {
                2 | 5 => *byte == b':',
                _ => byte.is_ascii_digit(),
            })
    }

    /// Whether the ▶ marker keeps its green accent.
    pub(crate) fn marker_is_green(&self) -> bool {
        matches!(self.screen.cell(HEADER_ROW, self.marker_col), Some(cell)
            if cell.style().fg == GREEN)
    }
}

/// The bordered task list of the current view.
pub(crate) struct TaskPanel {
    screen: Screen,
}

impl TaskPanel {
    /// The block title of the panel, e.g. "Active tasks".
    pub(crate) fn title(&self) -> String {
        self.screen
            .row_text(PANEL_TOP_ROW)
            .trim_matches(|character| matches!(character, '┌' | '─' | '┐'))
            .trim()
            .to_owned()
    }

    /// Whether the active view's panel is shown.
    pub(crate) fn shows_active_tasks(&self) -> bool {
        self.title() == "Active tasks"
    }

    /// Whether the archived view's panel is shown.
    pub(crate) fn shows_archived_tasks(&self) -> bool {
        self.title() == "Archived tasks"
    }

    /// Whether the panel border carries the focused blue accent, which
    /// normal mode gives it.
    pub(crate) fn is_focused(&self) -> bool {
        matches!(self.screen.cell(PANEL_TOP_ROW, 0), Some(cell)
            if cell.style().fg == BLUE)
    }

    /// The empty-state hint shown instead of task rows, if the list is
    /// empty.
    ///
    /// Only the two exact view-specific hints count. A task row that
    /// happens to start with "No " can never pass the list off as empty.
    pub(crate) fn empty_hint(&self) -> Option<String> {
        let hint = self.content_row_text(PANEL_FIRST_CONTENT_ROW);
        ([ACTIVE_EMPTY_HINT, ARCHIVED_EMPTY_HINT].contains(&hint.as_str())).then_some(hint)
    }

    /// The names of the visible task rows, top to bottom.
    pub(crate) fn task_names(&self) -> Vec<String> {
        if self.empty_hint().is_some() {
            return Vec::new();
        }
        (PANEL_FIRST_CONTENT_ROW..=PANEL_LAST_CONTENT_ROW)
            .map(|row| task_name_text(&self.screen, row))
            .take_while(|text| !text.is_empty())
            .collect()
    }

    /// One visible task row.
    ///
    /// Panics for an index past the visible rows; scenarios index rows they
    /// have already counted.
    pub(crate) fn row(&self, index: usize) -> TaskRow {
        assert!(
            index < self.task_names().len(),
            "task row {index} is not on the screen"
        );
        TaskRow {
            screen: self.screen.clone(),
            row: PANEL_FIRST_CONTENT_ROW + index as u16,
        }
    }

    /// The text of one content row, without the border cells.
    fn content_row_text(&self, row: u16) -> String {
        self.screen
            .rect_text(1..(COLS - 1), row..row + 1)
            .trim_end()
            .to_owned()
    }
}

/// The task name on one panel content row: the text after the two marker
/// cells, with trailing blanks removed.
fn task_name_text(screen: &Screen, row: u16) -> String {
    screen
        .rect_text(3..(COLS - 1), row..row + 1)
        .trim_end()
        .to_owned()
}

/// One task row of the panel.
pub(crate) struct TaskRow {
    screen: Screen,
    row: u16,
}

impl TaskRow {
    /// The task name, without the two marker cells.
    pub(crate) fn name(&self) -> String {
        task_name_text(&self.screen, self.row)
    }

    /// Whether the running-task marker sits on this row.
    pub(crate) fn has_active_marker(&self) -> bool {
        self.screen
            .rect_text(1..3, self.row..self.row + 1)
            .contains('▶')
    }

    /// Whether this row carries the selection highlight.
    pub(crate) fn is_selected(&self) -> bool {
        (1..COLS - 1).any(
            |col| matches!(self.screen.cell(self.row, col), Some(cell) if cell.style().reverse),
        )
    }
}

/// The status or error line above the footer.
pub(crate) struct StatusBar {
    screen: Screen,
}

impl StatusBar {
    /// The full text of the status line.
    pub(crate) fn text(&self) -> String {
        trimmed_row(&self.screen, STATUS_ROW)
    }

    /// Whether the line reports an error through its label.
    pub(crate) fn is_error(&self) -> bool {
        self.text().starts_with("Error: ")
    }

    /// Whether the "Error: " label keeps its red bold accent.
    pub(crate) fn label_is_accented(&self) -> bool {
        (0.."Error: ".len() as u16).all(|col| {
            matches!(self.screen.cell(STATUS_ROW, col), Some(cell)
                if cell.style().fg == RED && cell.style().bold)
        })
    }
}

/// The context-sensitive key help on the bottom line.
pub(crate) struct Footer {
    screen: Screen,
}

impl Footer {
    /// The full text of the footer line.
    pub(crate) fn text(&self) -> String {
        trimmed_row(&self.screen, FOOTER_ROW)
    }

    /// Whether the footer names the movement keys.
    pub(crate) fn hints_movement(&self) -> bool {
        self.text().contains("j/k/↑/↓")
    }

    /// Whether the footer names the view-switch keys.
    pub(crate) fn hints_view_switching(&self) -> bool {
        self.text().contains("h/l view")
    }

    /// Whether the footer names the tracking toggle.
    pub(crate) fn hints_tracking(&self) -> bool {
        self.text().contains("space track")
    }

    /// Whether the footer names the add, rename, and archive actions.
    pub(crate) fn hints_task_actions(&self) -> bool {
        self.text().contains("a/e/d add/rename/archive")
    }

    /// Whether the footer names the unarchive action of the archived view.
    pub(crate) fn hints_unarchive(&self) -> bool {
        self.text().contains("u unarchive")
    }

    /// Whether the footer names the quit keys.
    pub(crate) fn hints_quit(&self) -> bool {
        self.text().contains("q/esc/ctrl+c quit")
    }
}

/// The one-line text input dialog.
pub(crate) struct TaskInputDialog {
    screen: Screen,
    prompt_start_col: u16,
    prompt: &'static str,
}

impl TaskInputDialog {
    fn find(screen: &Screen) -> Option<Self> {
        for prompt in ["New task name:", "Rename task:"] {
            if let Some((row, col)) = screen.find(prompt)
                && dialog_geometry_holds(screen, row, col)
            {
                return Some(Self {
                    screen: screen.clone(),
                    prompt_start_col: col,
                    prompt,
                });
            }
        }
        None
    }

    /// The column just past the prompt text.
    fn prompt_end_col(&self) -> u16 {
        self.prompt_start_col + self.prompt.chars().count() as u16
    }

    /// The prompt the dialog asks, e.g. "New task name:".
    pub(crate) fn prompt(&self) -> &'static str {
        self.prompt
    }

    /// The text typed so far, without the cursor glyph.
    pub(crate) fn text(&self) -> String {
        self.screen
            .rect_text(
                self.prompt_end_col() + 1..DIALOG_RIGHT_COL,
                DIALOG_TEXT_ROW..DIALOG_TEXT_ROW + 1,
            )
            .trim_end_matches('▏')
            .trim_end()
            .to_owned()
    }

    /// Whether the text cursor glyph is visible at the end of the input.
    pub(crate) fn cursor_is_visible(&self) -> bool {
        (self.prompt_end_col()..DIALOG_RIGHT_COL).any(|col| {
            matches!(self.screen.cell(DIALOG_TEXT_ROW, col), Some(cell)
                if cell.contents() == "▏")
        })
    }
}

/// The archive confirmation dialog.
pub(crate) struct ArchiveDialog {
    screen: Screen,
}

impl ArchiveDialog {
    fn find(screen: &Screen) -> Option<Self> {
        let (row, col) = screen.find("Confirm archive")?;
        dialog_geometry_holds(screen, row, col).then(|| Self {
            screen: screen.clone(),
        })
    }

    /// The question the dialog asks, e.g. `Archive "Write release notes"?`.
    pub(crate) fn question(&self) -> String {
        self.screen
            .rect_text(
                DIALOG_LEFT_COL + 1..DIALOG_RIGHT_COL,
                DIALOG_TEXT_ROW..DIALOG_TEXT_ROW + 1,
            )
            .trim_end()
            .to_owned()
    }
}

/// Whether a found dialog text sits where the centered 56x3 modal renders
/// it, and the box borders are all present.
///
/// The modal's prompt or title starts one cell inside the left border:
/// the title on the top border row, a prompt on the single text row below
/// it. Without these checks a task named, say, "New task name:" could
/// pass as an open dialog.
fn dialog_geometry_holds(screen: &Screen, text_row: u16, text_col: u16) -> bool {
    let is_title = text_row == DIALOG_TOP_ROW;
    if (!is_title && text_row != DIALOG_TEXT_ROW) || text_col != DIALOG_LEFT_COL + 1 {
        return false;
    }
    [
        (DIALOG_TOP_ROW, DIALOG_LEFT_COL, "┌"),
        (DIALOG_TOP_ROW, DIALOG_RIGHT_COL, "┐"),
        (DIALOG_TEXT_ROW, DIALOG_LEFT_COL, "│"),
        (DIALOG_TEXT_ROW, DIALOG_RIGHT_COL, "│"),
        (DIALOG_BOTTOM_ROW, DIALOG_LEFT_COL, "└"),
        (DIALOG_BOTTOM_ROW, DIALOG_RIGHT_COL, "┘"),
    ]
    .iter()
    .all(|(row, col, glyph)| {
        screen
            .cell(*row, *col)
            .is_some_and(|cell| cell.contents() == *glyph)
    })
}

/// The text of one row with trailing blanks removed.
fn trimmed_row(screen: &Screen, row: u16) -> String {
    screen.row_text(row).trim_end().to_owned()
}
