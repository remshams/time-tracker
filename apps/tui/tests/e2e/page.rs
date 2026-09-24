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
//! ask semantic questions only. The layout is derived from the snapshot's
//! own size, so the components read correctly at the supported scenario
//! geometries, including the relocated layout after a resize.

use termlens::{Color, Screen};

/// The exact empty-state hints the two views render instead of task rows.
const ACTIVE_EMPTY_HINT: &str = "No active tasks. Press a to add one.";
const ARCHIVED_EMPTY_HINT: &str = "No archived tasks.";
const SEARCH_NO_MATCH_HINT: &str = "No matching tasks.";

/// The empty-state hint the worklog history renders instead of rows.
const HISTORY_EMPTY_HINT: &str = "No worklogs yet.";
/// What the history renders instead of an end time while a worklog runs.
const RUNNING_LABEL: &str = "Running";
/// The arrow a history row renders between its start and its end.
const HISTORY_ARROW: &str = " → ";
/// How many screen lines one history row occupies: the interval line,
/// then the duration line.
const HISTORY_ROW_LINES: u16 = 2;

/// The accents the interface draws with, as the terminal palette reports
/// them. Ratatui sends named ANSI colors as 256-color indexes: blue is
/// index 4, green 2, red 1.
const BLUE: Color = Color::Indexed(4);
const GREEN: Color = Color::Indexed(2);
const RED: Color = Color::Indexed(1);

/// The public frame layout of one snapshot, derived from the snapshot's
/// own size.
///
/// The frame spends one row on the header, the rest of the top of the
/// screen on the bordered task panel, one row on the status line, and one
/// row on the footer; the modal dialogs are 56x3 boxes centered over the
/// panel area. Scenarios never name a coordinate, so a resize that moves
/// every component needs no changes here: the components simply read the
/// new geometry off the next snapshot.
#[derive(Clone, Copy)]
struct Layout {
    cols: u16,
    rows: u16,
}

impl Layout {
    fn of(screen: &Screen) -> Self {
        let (cols, rows) = screen.size();
        Self { cols, rows }
    }

    fn header_row(&self) -> u16 {
        0
    }

    fn panel_top_row(&self) -> u16 {
        1
    }

    fn panel_first_content_row(&self) -> u16 {
        2
    }

    fn panel_last_content_row(&self) -> u16 {
        self.rows - 4
    }

    fn panel_bottom_row(&self) -> u16 {
        self.rows - 3
    }

    fn status_row(&self) -> u16 {
        self.rows - 2
    }

    fn footer_row(&self) -> u16 {
        self.rows - 1
    }

    /// The centered 56x3 modal over the panel area.
    ///
    /// The panel area always spans at least the dialog's size in the
    /// geometries the scenarios run at; the saturating subtraction keeps a
    /// meaningless answer below that threshold from panicking.
    fn dialog_left_col(&self) -> u16 {
        self.cols.saturating_sub(56) / 2
    }

    fn dialog_right_col(&self) -> u16 {
        self.dialog_left_col() + 55
    }

    fn dialog_top_row(&self) -> u16 {
        1 + self.rows.saturating_sub(6) / 2
    }

    fn dialog_text_row(&self) -> u16 {
        self.dialog_top_row() + 1
    }

    fn dialog_bottom_row(&self) -> u16 {
        self.dialog_top_row() + 2
    }
}

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

    /// The terminal geometry the snapshot was taken at, as `(cols, rows)`.
    pub(crate) fn size(&self) -> (u16, u16) {
        self.screen.size()
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

    /// The deletion confirmation dialog, if one is open over worklog history.
    pub(crate) fn deletion_dialog(&self) -> Option<DeletionDialog> {
        DeletionDialog::find(&self.screen)
    }

    /// The correction dialog, if one is open over worklog history.
    pub(crate) fn correction_dialog(&self) -> Option<CorrectionDialog> {
        CorrectionDialog::find(&self.screen)
    }

    /// The worklog destination picker, if one is open.
    pub(crate) fn move_worklog_dialog(&self) -> Option<MoveWorklogDialog> {
        MoveWorklogDialog::find(&self.screen)
    }

    /// The read-only worklog history, however the screen looks. The
    /// components answer whether the history is shown at all; a task list
    /// snapshot reports `false` there.
    pub(crate) fn worklog_history_panel(&self) -> WorklogHistoryPanel {
        WorklogHistoryPanel {
            screen: self.screen.clone(),
        }
    }
}

/// The top line: application title, and the live timer while tracking runs.
pub(crate) struct Header {
    screen: Screen,
}

impl Header {
    /// The full text of the header line.
    pub(crate) fn text(&self) -> String {
        trimmed_row(&self.screen, Layout::of(&self.screen).header_row())
    }

    /// Whether the title keeps its blue bold accent.
    pub(crate) fn title_is_accented(&self) -> bool {
        let header_row = Layout::of(&self.screen).header_row();
        matches!(self.screen.cell(header_row, 0), Some(cell)
            if cell.style().fg == BLUE && cell.style().bold)
    }

    /// Whether the header names no running task.
    pub(crate) fn is_idle(&self) -> bool {
        !self.text().contains('▶')
    }

    /// The running task the header names, if tracking is on.
    pub(crate) fn active_task(&self) -> Option<ActiveTask> {
        let header_row = Layout::of(&self.screen).header_row();
        // Task rows carry the same marker, so the marker must sit in the
        // header row to count.
        let (marker_row, marker_col) = self.screen.find("▶")?;
        if marker_row != header_row {
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
            header_row,
            marker_col,
            name: after_marker[..split].trim_end().to_owned(),
            elapsed: after_marker[split..].to_owned(),
        })
    }
}

/// The running task as the header shows it.
pub(crate) struct ActiveTask {
    screen: Screen,
    header_row: u16,
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

    /// Whether the elapsed time reads exactly `HH:MM:SS`.
    pub(crate) fn elapsed_is_hhmmss(&self) -> bool {
        text_is_hhmmss(&self.elapsed)
    }

    /// The elapsed time in whole seconds, when it is valid `HH:MM:SS`.
    pub(crate) fn elapsed_seconds(&self) -> Option<u64> {
        hhmmss_seconds(&self.elapsed)
    }

    /// Whether the ▶ marker keeps its green accent.
    pub(crate) fn marker_is_green(&self) -> bool {
        matches!(self.screen.cell(self.header_row, self.marker_col), Some(cell)
            if cell.style().fg == GREEN)
    }
}

/// The bordered task list of the current view.
pub(crate) struct TaskPanel {
    screen: Screen,
}

impl TaskPanel {
    /// The block title of the panel, including tabs and current ordering.
    pub(crate) fn title(&self) -> String {
        panel_title(&self.screen)
    }

    /// Whether the active view's panel is shown.
    pub(crate) fn shows_active_tasks(&self) -> bool {
        self.title().starts_with("Tasks · [Active]  Archived · ")
    }

    /// Whether the archived view's panel is shown.
    pub(crate) fn shows_archived_tasks(&self) -> bool {
        self.title().starts_with("Tasks · Active  [Archived] · ")
    }

    /// Whether the current tab has the selection highlight.
    pub(crate) fn selected_tab_is_highlighted(&self) -> bool {
        let col = if self.shows_active_tasks() {
            9
        } else if self.shows_archived_tasks() {
            17
        } else {
            return false;
        };
        self.screen
            .cell(Layout::of(&self.screen).panel_top_row(), col)
            .is_some_and(|cell| cell.style().reverse)
    }

    /// Whether the panel names the given ordering after the tabs.
    pub(crate) fn shows_ordering(&self, ordering: &str) -> bool {
        self.title()
            .rsplit_once(" · ")
            .is_some_and(|(_, shown)| shown == ordering)
    }

    /// Whether the panel border carries the focused blue accent, which
    /// normal mode gives it.
    pub(crate) fn is_focused(&self) -> bool {
        matches!(self.screen.cell(Layout::of(&self.screen).panel_top_row(), 0), Some(cell)
            if cell.style().fg == BLUE)
    }

    /// Whether the panel's four corner glyphs sit where the snapshot's own
    /// geometry puts them.
    ///
    /// This checks the four corners only, not the full border. After a
    /// resize, termlens clips or pads the pre-repaint grid to the new
    /// size, so the stale grid keeps the *old* corners. Corners that fit
    /// the current geometry are therefore something only a fresh repaint
    /// can show, and resize predicates lean on that.
    pub(crate) fn frame_corners_fit_current_geometry(&self) -> bool {
        let layout = Layout::of(&self.screen);
        [
            (layout.panel_top_row(), 0, "┌"),
            (layout.panel_top_row(), layout.cols - 1, "┐"),
            (layout.panel_bottom_row(), 0, "└"),
            (layout.panel_bottom_row(), layout.cols - 1, "┘"),
        ]
        .into_iter()
        .all(|(row, col, glyph)| {
            self.screen
                .cell(row, col)
                .is_some_and(|cell| cell.contents() == glyph)
        })
    }

    /// The empty-state hint shown instead of task rows, if the list is
    /// empty.
    ///
    /// Only the two exact view-specific hints count. A task row that
    /// happens to start with "No " can never pass the list off as empty.
    pub(crate) fn empty_hint(&self) -> Option<String> {
        let hint = self.content_row_text(Layout::of(&self.screen).panel_first_content_row());
        ([ACTIVE_EMPTY_HINT, ARCHIVED_EMPTY_HINT].contains(&hint.as_str())).then_some(hint)
    }

    /// The visible task-list query, including a committed filter.
    pub(crate) fn search_query(&self) -> Option<String> {
        self.content_row_text(Layout::of(&self.screen).panel_first_content_row())
            .strip_prefix("Search: ")
            .map(|query| query.trim_end_matches('▏').trim_end().to_owned())
    }

    /// Whether a search found no tasks in the current view.
    pub(crate) fn search_has_no_matches(&self) -> bool {
        self.search_query().is_some()
            && self.content_row_text(self.first_task_row()) == SEARCH_NO_MATCH_HINT
    }

    /// The names of the visible task rows, top to bottom.
    pub(crate) fn task_names(&self) -> Vec<String> {
        if self.empty_hint().is_some() || self.search_has_no_matches() {
            return Vec::new();
        }
        let layout = Layout::of(&self.screen);
        (self.first_task_row()..=layout.panel_last_content_row())
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
            row: self.first_task_row() + index as u16,
        }
    }

    /// The zero-based index of the selected row, if a task row is selected.
    ///
    /// Panics when more than one row is selected. A repaint bug that
    /// draws the highlight twice must fail a scenario, not let it pick
    /// whichever duplicate comes first.
    pub(crate) fn selected_index(&self) -> Option<usize> {
        let selected: Vec<usize> = (0..self.task_names().len())
            .filter(|&index| self.row(index).is_selected())
            .collect();
        assert!(
            selected.len() <= 1,
            "more than one task row is selected: {selected:?}"
        );
        selected.into_iter().next()
    }

    /// The zero-based index of the running task's row, if the list shows
    /// the active marker.
    ///
    /// Panics when more than one row carries the marker, for the same
    /// reason the selection check does.
    pub(crate) fn active_marker_index(&self) -> Option<usize> {
        let marked: Vec<usize> = (0..self.task_names().len())
            .filter(|&index| self.row(index).has_active_marker())
            .collect();
        assert!(
            marked.len() <= 1,
            "more than one task row carries the active marker: {marked:?}"
        );
        marked.into_iter().next()
    }

    /// The text of one content row, without the border cells.
    fn content_row_text(&self, row: u16) -> String {
        panel_row_text(&self.screen, row)
    }

    fn first_task_row(&self) -> u16 {
        Layout::of(&self.screen).panel_first_content_row()
            + u16::from(self.search_query().is_some())
    }
}

/// The text of one bordered-panel content row, without the border cells
/// and trailing blanks.
fn panel_title(screen: &Screen) -> String {
    screen
        .row_text(Layout::of(screen).panel_top_row())
        .trim_matches(|character| matches!(character, '┌' | '─' | '┐'))
        .trim()
        .to_owned()
}

fn panel_row_text(screen: &Screen, row: u16) -> String {
    let cols = Layout::of(screen).cols;
    screen
        .rect_text(1..(cols - 1), row..row + 1)
        .trim_end()
        .to_owned()
}

/// Whether a time reads exactly `HH:MM:SS`: eight bytes, ASCII digits in
/// every numeric position, colons at indexes 2 and 5.
fn text_is_hhmmss(text: &str) -> bool {
    hhmmss_seconds(text).is_some()
}

/// Parses an exact `HH:MM:SS` time into whole seconds.
fn hhmmss_seconds(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    if bytes.len() != 8
        || !bytes.iter().enumerate().all(|(index, byte)| match index {
            2 | 5 => *byte == b':',
            _ => byte.is_ascii_digit(),
        })
    {
        return None;
    }
    let hours = text[0..2].parse::<u64>().ok()?;
    let minutes = text[3..5].parse::<u64>().ok()?;
    let seconds = text[6..8].parse::<u64>().ok()?;
    (minutes < 60 && seconds < 60).then_some(hours * 3600 + minutes * 60 + seconds)
}

/// The task name on one panel content row: the text after the two marker
/// cells, with trailing blanks removed.
fn task_name_text(screen: &Screen, row: u16) -> String {
    let cols = Layout::of(screen).cols;
    screen
        .rect_text(3..(cols - 1), row..row + 1)
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
        let cols = Layout::of(&self.screen).cols;
        (1..cols - 1).any(
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
        trimmed_row(&self.screen, Layout::of(&self.screen).status_row())
    }

    /// Whether the line reports an error through its label.
    pub(crate) fn is_error(&self) -> bool {
        self.text().starts_with("Error: ")
    }

    /// Whether the "Error: " label keeps its red bold accent.
    pub(crate) fn label_is_accented(&self) -> bool {
        let status_row = Layout::of(&self.screen).status_row();
        (0.."Error: ".len() as u16).all(|col| {
            matches!(self.screen.cell(status_row, col), Some(cell)
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
        trimmed_row(&self.screen, Layout::of(&self.screen).footer_row())
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

    /// Whether the footer names the add, rename, and archive keys.
    pub(crate) fn hints_task_actions(&self) -> bool {
        self.text().contains("a/e/d edit")
    }

    /// Whether the task-list footer names the worklog-history shortcut.
    pub(crate) fn hints_open_history(&self) -> bool {
        self.text().contains("enter history")
    }

    /// Whether the footer names the ordering control.
    pub(crate) fn hints_sorting(&self) -> bool {
        self.text().contains("s sort")
    }

    /// Whether the footer names the text-input keys.
    pub(crate) fn hints_input(&self) -> bool {
        self.text().contains("enter save")
    }

    /// Whether the footer names the unarchive action of the archived view.
    pub(crate) fn hints_unarchive(&self) -> bool {
        self.text().contains("u unarchive")
    }

    /// Whether the footer names the worklog-history keys.
    pub(crate) fn hints_correction(&self) -> bool {
        self.text().contains("e correct") || self.text().contains("e edit")
    }

    /// Whether the footer names the worklog-history keys and correction shortcut.
    pub(crate) fn hints_history(&self) -> bool {
        let text = self.text();
        let has_q = text.contains(" q ") || text.contains("q/");
        text.contains("o older")
            && text.contains("r refresh")
            && text.contains("m move")
            && text.contains("d delete")
            && text.contains("esc back")
            && has_q
            && text.contains("ctrl+c")
            && self.hints_correction()
    }

    /// Whether the footer names the quit keys.
    pub(crate) fn hints_quit(&self) -> bool {
        self.text().contains("q/esc/ctrl+c quit")
    }
}

/// The bordered read-only worklog history of one task.
///
/// The history takes the task panel's place in the frame: the same
/// bordered body area, with `<source> › <task> › Worklogs` as its title.
/// One worklog occupies two content lines, the interval line then the
/// duration line, and the list scrolls in whole worklogs, so the first
/// content line always begins a worklog and parsing can walk the rows
/// top down in pairs.
pub(crate) struct WorklogHistoryPanel {
    screen: Screen,
}

impl WorklogHistoryPanel {
    /// The block title of the panel, including the task's name.
    pub(crate) fn title(&self) -> String {
        panel_title(&self.screen)
    }

    /// Whether the history screen is shown at all.
    pub(crate) fn is_shown(&self) -> bool {
        self.source_view().is_some() && self.title().ends_with(" › Worklogs")
    }

    /// The task view from which this history was opened.
    pub(crate) fn source_view(&self) -> Option<&'static str> {
        let title = self.title();
        if title.starts_with("Active › ") {
            Some("Active")
        } else if title.starts_with("Archived › ") {
            Some("Archived")
        } else {
            None
        }
    }

    /// The name of the task whose history is open.
    pub(crate) fn task_name(&self) -> String {
        let title = self.title();
        title
            .split_once(" › ")
            .and_then(|(_, rest)| rest.strip_suffix(" › Worklogs"))
            .map_or_else(|| title.clone(), str::to_owned)
    }

    /// The empty-state hint shown instead of worklog rows, if the open
    /// task has no worklogs.
    pub(crate) fn empty_hint(&self) -> Option<&'static str> {
        let hint = panel_row_text(
            &self.screen,
            Layout::of(&self.screen).panel_first_content_row(),
        );
        (hint == HISTORY_EMPTY_HINT).then_some(HISTORY_EMPTY_HINT)
    }

    /// How many worklog rows are visible, top to bottom.
    ///
    /// A blank or hint line stops the count, so an empty history counts
    /// no rows and a partially filled screen counts only its worklogs.
    pub(crate) fn row_count(&self) -> usize {
        let layout = Layout::of(&self.screen);
        let mut count = 0;
        let mut top = layout.panel_first_content_row();
        while top + HISTORY_ROW_LINES - 1 <= layout.panel_last_content_row() {
            if !panel_row_text(&self.screen, top).contains(HISTORY_ARROW) {
                break;
            }
            count += 1;
            top += HISTORY_ROW_LINES;
        }
        count
    }

    /// One visible worklog row.
    ///
    /// Panics for an index past the visible rows; scenarios index rows
    /// they have already counted.
    pub(crate) fn row(&self, index: usize) -> WorklogRow {
        assert!(
            index < self.row_count(),
            "history row {index} is not on the screen"
        );
        WorklogRow {
            screen: self.screen.clone(),
            top_row: Layout::of(&self.screen).panel_first_content_row()
                + index as u16 * HISTORY_ROW_LINES,
        }
    }

    /// The zero-based index of the selected row among the visible rows,
    /// if a worklog row is selected.
    ///
    /// Panics when more than one row is selected, for the same reason the
    /// task selection check does: a repaint bug that draws the highlight
    /// twice must fail a scenario, not let it pick a duplicate.
    pub(crate) fn selected_index(&self) -> Option<usize> {
        let selected: Vec<usize> = (0..self.row_count())
            .filter(|&index| self.row(index).is_selected())
            .collect();
        assert!(
            selected.len() <= 1,
            "more than one history row is selected: {selected:?}"
        );
        selected.into_iter().next()
    }
}

/// One visible worklog row of the history: its interval line, then its
/// duration line.
pub(crate) struct WorklogRow {
    screen: Screen,
    /// The screen row of the interval line; the duration line follows it.
    top_row: u16,
}

impl WorklogRow {
    /// The interval line as rendered: the local start, the arrow, and the
    /// local end or `Running`.
    pub(crate) fn interval_line(&self) -> String {
        panel_row_text(&self.screen, self.top_row).trim().to_owned()
    }

    /// The row's start time as rendered, e.g. `2026-07-12 16:45`.
    pub(crate) fn start_text(&self) -> String {
        self.interval_line()
            .split_once(HISTORY_ARROW)
            .map_or_else(|| self.interval_line(), |(start, _)| start.to_owned())
    }

    /// The row's end as rendered: the local end time, or `Running` while
    /// the worklog is active.
    pub(crate) fn end_text(&self) -> String {
        self.interval_line()
            .split_once(HISTORY_ARROW)
            .map_or(String::new(), |(_, end)| end.to_owned())
    }

    /// Whether this row is the running worklog.
    pub(crate) fn is_running(&self) -> bool {
        self.end_text() == RUNNING_LABEL
    }

    /// The duration line below the interval, as `HH:MM:SS`.
    pub(crate) fn duration_text(&self) -> String {
        panel_row_text(&self.screen, self.top_row + 1)
            .trim()
            .to_owned()
    }

    /// Whether the duration reads exactly `HH:MM:SS`.
    pub(crate) fn duration_is_hhmmss(&self) -> bool {
        text_is_hhmmss(&self.duration_text())
    }

    /// The duration in whole seconds, when it is valid `HH:MM:SS`.
    pub(crate) fn duration_seconds(&self) -> Option<u64> {
        hhmmss_seconds(&self.duration_text())
    }

    /// Whether this row carries the selection highlight.
    pub(crate) fn is_selected(&self) -> bool {
        let cols = Layout::of(&self.screen).cols;
        (self.top_row..self.top_row + HISTORY_ROW_LINES).any(|row| {
            (1..cols - 1)
                .any(|col| matches!(self.screen.cell(row, col), Some(cell) if cell.style().reverse))
        })
    }
}

/// The one-line text input dialog.
pub(crate) struct TaskInputDialog {
    screen: Screen,
    layout: Layout,
    prompt_start_col: u16,
    prompt: &'static str,
}

impl TaskInputDialog {
    fn find(screen: &Screen) -> Option<Self> {
        let layout = Layout::of(screen);
        for prompt in ["New task name:", "Rename task:"] {
            if let Some((row, col)) = screen.find(prompt)
                && dialog_geometry_holds(screen, layout, row, col)
            {
                return Some(Self {
                    screen: screen.clone(),
                    layout,
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
                self.prompt_end_col() + 1..self.layout.dialog_right_col(),
                self.layout.dialog_text_row()..self.layout.dialog_text_row() + 1,
            )
            .trim_end_matches('▏')
            .trim_end()
            .to_owned()
    }

    /// Whether the text cursor glyph is visible at the end of the input.
    pub(crate) fn cursor_is_visible(&self) -> bool {
        (self.prompt_end_col()..self.layout.dialog_right_col()).any(|col| {
            matches!(self.screen.cell(self.layout.dialog_text_row(), col), Some(cell)
                if cell.contents() == "▏")
        })
    }
}

/// The archive confirmation dialog.
pub(crate) struct ArchiveDialog {
    screen: Screen,
    layout: Layout,
}

impl ArchiveDialog {
    fn find(screen: &Screen) -> Option<Self> {
        let layout = Layout::of(screen);
        let (row, col) = screen.find("Confirm archive")?;
        dialog_geometry_holds(screen, layout, row, col).then_some(Self {
            screen: screen.clone(),
            layout,
        })
    }

    /// The question the dialog asks, e.g. `Archive "Write release notes"?`.
    pub(crate) fn question(&self) -> String {
        self.screen
            .rect_text(
                self.layout.dialog_left_col() + 1..self.layout.dialog_right_col(),
                self.layout.dialog_text_row()..self.layout.dialog_text_row() + 1,
            )
            .trim_end()
            .to_owned()
    }
}

/// The deletion confirmation dialog over worklog history.
pub(crate) struct DeletionDialog {
    screen: Screen,
    left: u16,
    top: u16,
}

impl DeletionDialog {
    fn find(screen: &Screen) -> Option<Self> {
        let (top, title_col) = screen.find("Delete worklog")?;
        let (cols, rows) = screen.size();
        let left = cols.saturating_sub(58) / 2;
        let expected_title_col = left + 1;
        if title_col != expected_title_col || top + 5 >= rows {
            return None;
        }
        let right = left + 57;
        let borders = [
            (top, left, "┌"),
            (top, right, "┐"),
            (top + 1, left, "│"),
            (top + 1, right, "│"),
            (top + 4, left, "│"),
            (top + 4, right, "│"),
            (top + 5, left, "└"),
            (top + 5, right, "┘"),
        ];
        borders
            .iter()
            .all(|(row, col, glyph)| {
                screen
                    .cell(*row, *col)
                    .is_some_and(|cell| cell.contents() == *glyph)
            })
            .then_some(Self {
                screen: screen.clone(),
                left,
                top,
            })
    }

    pub(crate) fn title(&self) -> String {
        "Delete worklog".to_owned()
    }

    pub(crate) fn question(&self) -> String {
        self.line(1)
    }

    pub(crate) fn interval(&self) -> String {
        self.line(2)
    }

    pub(crate) fn warning(&self) -> String {
        self.line(3)
    }

    fn line(&self, offset: u16) -> String {
        self.screen
            .rect_text(
                self.left + 1..self.left + 57,
                self.top + offset..self.top + offset + 1,
            )
            .trim_end()
            .to_owned()
    }
}

/// The timestamp correction dialog over worklog history.
pub(crate) struct CorrectionDialog {
    screen: Screen,
    top: u16,
    height: u16,
}

/// The worklog destination picker over history.
pub(crate) struct MoveWorklogDialog {
    screen: Screen,
    left: u16,
    top: u16,
    bottom: u16,
}

impl MoveWorklogDialog {
    fn find(screen: &Screen) -> Option<Self> {
        let (top, title_col) = screen.find("Move worklog")?;
        let left = title_col.saturating_sub(1);
        let right = left + 57;
        let (cols, rows) = screen.size();
        if top + 2 >= rows || right >= cols {
            return None;
        }
        let bottom = ((top + 1)..rows).find(|row| {
            screen
                .cell(*row, left)
                .is_some_and(|cell| cell.contents() == "└")
                && screen
                    .cell(*row, right)
                    .is_some_and(|cell| cell.contents() == "┘")
        })?;
        let borders = [
            (top, left, "┌"),
            (top, right, "┐"),
            (top + 1, left, "│"),
            (top + 1, right, "│"),
        ];
        borders
            .iter()
            .all(|(row, col, glyph)| {
                screen
                    .cell(*row, *col)
                    .is_some_and(|cell| cell.contents() == *glyph)
            })
            .then_some(Self {
                screen: screen.clone(),
                left,
                top,
                bottom,
            })
    }

    pub(crate) fn source(&self) -> String {
        self.line(1)
    }

    pub(crate) fn query(&self) -> String {
        self.line(2)
            .strip_prefix("Search: ")
            .unwrap_or_default()
            .trim_end_matches('▏')
            .trim_end()
            .to_owned()
    }

    pub(crate) fn search_is_focused(&self) -> bool {
        self.line(2).contains('▏')
    }

    pub(crate) fn results(&self) -> Vec<String> {
        ((self.top + 3)..self.bottom)
            .map(|row| {
                self.screen
                    .rect_text(self.left + 1..self.left + 57, row..row + 1)
                    .trim_end()
                    .trim_start_matches("› ")
                    .trim_start()
                    .to_owned()
            })
            .filter(|line| !line.is_empty() && line != "No matching active tasks.")
            .collect()
    }

    pub(crate) fn selected_result(&self) -> Option<String> {
        ((self.top + 3)..self.bottom).find_map(|row| {
            let text = self
                .screen
                .rect_text(self.left + 1..self.left + 57, row..row + 1)
                .trim_end()
                .to_owned();
            text.strip_prefix("› ").map(ToOwned::to_owned)
        })
    }

    fn line(&self, offset: u16) -> String {
        self.screen
            .rect_text(
                self.left + 1..self.left + 57,
                self.top + offset..self.top + offset + 1,
            )
            .trim_end()
            .to_owned()
    }
}

impl CorrectionDialog {
    fn find(screen: &Screen) -> Option<Self> {
        let (title_row, _) = screen.find("Correct worklog")?;
        let top = title_row;
        let height = if screen.row_text(top + 2).contains("End  :") {
            4
        } else {
            3
        };
        Some(Self {
            screen: screen.clone(),
            top,
            height,
        })
    }

    pub(crate) fn start_text(&self) -> String {
        self.field_text(self.top + 1, "Start")
    }

    pub(crate) fn end_text(&self) -> Option<String> {
        (self.height == 4).then(|| self.field_text(self.top + 2, "End"))
    }

    pub(crate) fn focused_field(&self) -> &'static str {
        if self.field_has_cursor(self.top + 1) {
            "Start"
        } else {
            "End"
        }
    }

    pub(crate) fn is_full_draft_visible(&self) -> bool {
        self.end_text().is_some()
    }

    fn field_text(&self, row: u16, label: &str) -> String {
        let line = self.screen.row_text(row);
        line.split_once(&format!("{label:<5}: "))
            .map(|(_, text)| text.split(['▏', '│']).next().unwrap_or(text))
            .unwrap_or_default()
            .trim_end()
            .to_owned()
    }

    fn field_has_cursor(&self, row: u16) -> bool {
        self.screen.row_text(row).contains('▏')
    }
}

/// Whether a found dialog text sits where the centered 56x3 modal renders
/// it at the snapshot's geometry, and the box borders are all present.
///
/// The modal's prompt or title starts one cell inside the left border:
/// the title on the top border row, a prompt on the single text row below
/// it. Without these checks a task named, say, "New task name:" could
/// pass as an open dialog. Because every position comes from the
/// snapshot's own size, a dialog painted for a different geometry, such
/// as the clipped or padded pre-repaint grid a resize leaves behind,
/// never matches.
fn dialog_geometry_holds(screen: &Screen, layout: Layout, text_row: u16, text_col: u16) -> bool {
    let is_title = text_row == layout.dialog_top_row();
    if (!is_title && text_row != layout.dialog_text_row())
        || text_col != layout.dialog_left_col() + 1
    {
        return false;
    }
    [
        (layout.dialog_top_row(), layout.dialog_left_col(), "┌"),
        (layout.dialog_top_row(), layout.dialog_right_col(), "┐"),
        (layout.dialog_text_row(), layout.dialog_left_col(), "│"),
        (layout.dialog_text_row(), layout.dialog_right_col(), "│"),
        (layout.dialog_bottom_row(), layout.dialog_left_col(), "└"),
        (layout.dialog_bottom_row(), layout.dialog_right_col(), "┘"),
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
