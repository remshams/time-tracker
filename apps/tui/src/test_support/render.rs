//! Render helpers for TUI view tests.

use ratatui::style::Style;
use ratatui::{Terminal, backend::TestBackend};
use tracker_application::TrackerApplicationService;

use crate::app::App;

/// The default test terminal width.
pub(crate) const WIDTH: u16 = 80;
/// The default test terminal height.
pub(crate) const HEIGHT: u16 = 24;

/// Renders the whole UI into a terminal of the default size.
pub(crate) fn draw<S: TrackerApplicationService>(app: &App<S>) -> Terminal<TestBackend> {
    draw_at(app, WIDTH, HEIGHT)
}

/// Renders the whole UI into a terminal of the given size.
pub(crate) fn draw_at<S: TrackerApplicationService>(
    app: &App<S>,
    width: u16,
    height: u16,
) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| crate::ui::render(frame, app.app_view()))
        .unwrap();
    terminal
}

/// Every rendered row as the concatenated cell symbols.
pub(crate) fn rows(terminal: &Terminal<TestBackend>) -> Vec<String> {
    let height = terminal.backend().buffer().area.height;
    (0..height).map(|y| row(terminal, y)).collect()
}

/// One rendered row as the concatenated cell symbols.
pub(crate) fn row(terminal: &Terminal<TestBackend>, y: u16) -> String {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect()
}

/// The style of one rendered cell.
pub(crate) fn cell(terminal: &Terminal<TestBackend>, x: u16, y: u16) -> Style {
    terminal.backend().buffer()[(x, y)].style()
}
