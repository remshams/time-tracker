//! Rendering for the task list interface.
//!
//! Everything here only reads state. Layout, styles, and widgets draw the
//! current [`App`]; mutating state happens in [`crate::app`].

use std::time::Duration;

use crate::app::{App, InputPurpose, Mode, Status};
use crate::keymap;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph};

/// The background and foreground used for the selected row.
fn selected_style() -> Style {
    Style::default().bg(Color::Blue).fg(Color::White)
}

/// The style of the status line for errors.
fn error_style() -> Style {
    Style::default().fg(Color::Red)
}

/// The style of dimmed helper text, such as the footer.
fn dim_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

/// The style of the text-input cursor block.
fn cursor_style() -> Style {
    Style::default().fg(Color::Cyan)
}

/// Renders one frame of the interface.
pub fn render(frame: &mut Frame, app: &App) {
    let [header, body, status_area, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    render_header(frame, header, app);
    render_tasks(frame, body, app);
    render_status(frame, status_area, app);
    render_footer(frame, footer, app);
    render_modal(frame, body, app);
}

/// Renders the title line, with the live timer while tracking runs.
fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let mut spans = vec![Span::styled(
        "Time Tracker",
        Style::default().add_modifier(Modifier::BOLD),
    )];
    if let Some(name) = app.active_task_name() {
        let elapsed = app.elapsed().map(format_elapsed).unwrap_or_default();
        spans.push(Span::raw("  ▶ "));
        spans.push(Span::raw(name));
        spans.push(Span::raw(format!("  {elapsed}")));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Renders the task list, its selection, and the active-task marker.
fn render_tasks(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::bordered().title("Tasks");
    if app.tasks().is_empty() {
        frame.render_widget(
            Paragraph::new("No tasks. Press a to add one.").block(block),
            area,
        );
        return;
    }
    let active_task_id = app.active_task_id();
    let items: Vec<ListItem> = app
        .tasks()
        .iter()
        .map(|task| {
            let marker = if active_task_id == Some(task.id) {
                "▶ "
            } else {
                "  "
            };
            ListItem::new(Line::from(format!("{marker}{}", task.name)))
        })
        .collect();
    let list = List::new(items)
        .block(block)
        .highlight_style(selected_style());
    let mut state = ListState::default().with_selected(app.selected());
    frame.render_stateful_widget(list, area, &mut state);
}

/// Renders the status or error line.
fn render_status(frame: &mut Frame, area: Rect, app: &App) {
    let (text, style) = match app.status() {
        Status::Info(text) => (text.as_str(), dim_style()),
        Status::Error(text) => (text.as_str(), error_style()),
    };
    frame.render_widget(Paragraph::new(text).style(style), area);
}

/// Renders the context-sensitive key help for the current mode.
fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    frame.render_widget(
        Paragraph::new(keymap::footer_hints(app.mode())).style(dim_style()),
        area,
    );
}

/// Renders the modal dialog of the current mode, if any.
fn render_modal(frame: &mut Frame, area: Rect, app: &App) {
    match app.mode() {
        Mode::Input { purpose, buffer } => render_input_modal(frame, area, *purpose, buffer),
        Mode::ConfirmArchive { name, .. } => render_confirm_modal(frame, area, name),
        Mode::Normal => {}
    }
}

/// Renders the one-line text input with a visible cursor.
fn render_input_modal(frame: &mut Frame, area: Rect, purpose: InputPurpose, buffer: &str) {
    let modal = centered(56, 3, area);
    frame.render_widget(Clear, modal);
    let prompt = match purpose {
        InputPurpose::Add => "New task name:",
        InputPurpose::Rename { .. } => "Rename task:",
    };
    let line = Line::from(vec![
        Span::raw(prompt),
        Span::raw(" "),
        Span::raw(buffer),
        Span::styled("▏", cursor_style()),
    ]);
    frame.render_widget(Paragraph::new(line).block(Block::bordered()), modal);
}

/// Renders the archive confirmation.
fn render_confirm_modal(frame: &mut Frame, area: Rect, name: &str) {
    let modal = centered(56, 3, area);
    frame.render_widget(Clear, modal);
    frame.render_widget(
        Paragraph::new(format!("Archive \"{name}\"?"))
            .block(Block::bordered().title("Confirm archive")),
        modal,
    );
}

/// Shrinks `area` to `width` × `height`, centered inside it.
///
/// The result never exceeds the available area.
fn centered(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

/// Formats a duration as `HH:MM:SS`.
fn format_elapsed(total: Duration) -> String {
    let seconds = total.as_secs();
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        (seconds % 3600) / 60,
        seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};
    use tracker_core::{Task, TaskId, TaskName, TrackerRepository};
    use tracker_storage::SqliteRepository;

    use super::*;
    use crate::command::Command;

    const WIDTH: u16 = 80;
    const HEIGHT: u16 = 24;

    fn app_with(names: &[&str]) -> App {
        let repository = SqliteRepository::open_in_memory().unwrap();
        for name in names {
            let task = Task::new(TaskId::generate(), TaskName::new(name).unwrap());
            repository.create_task(task).unwrap();
        }
        App::load(repository).unwrap()
    }

    fn draw(app: &App) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).unwrap();
        terminal.draw(|frame| render(frame, app)).unwrap();
        terminal
    }

    /// The visible text of every buffer row, top to bottom.
    fn rows(terminal: &Terminal<TestBackend>) -> Vec<String> {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    fn row(terminal: &Terminal<TestBackend>, y: u16) -> String {
        rows(terminal).remove(y as usize)
    }

    fn cell(terminal: &Terminal<TestBackend>, x: u16, y: u16) -> Style {
        terminal.backend().buffer()[(x, y)].style()
    }

    #[test]
    fn the_seeded_list_renders_with_title_selection_and_help() {
        let app = app_with(&["alpha", "beta"]);
        let terminal = draw(&app);
        let rows = rows(&terminal);

        assert!(rows[0].contains("Time Tracker"));
        assert!(rows[1].contains("Tasks"));
        assert!(rows[2].contains("alpha"));
        assert!(rows[3].contains("beta"));
        assert!(rows[22].contains("Ready"));
        assert!(rows[23].contains("a add"));
        assert!(rows[23].contains("ctrl+c quit"));
    }

    #[test]
    fn the_selected_row_is_highlighted_and_follows_the_selection() {
        let mut app = app_with(&["alpha", "beta", "gamma"]);
        let terminal = draw(&app);
        assert_eq!(cell(&terminal, 1, 2).bg, Some(Color::Blue));
        assert_eq!(cell(&terminal, 1, 3).bg, Some(Color::Reset));

        app.handle(crate::command::Command::MoveDown);
        let terminal = draw(&app);
        assert_eq!(cell(&terminal, 1, 3).bg, Some(Color::Blue));
        assert_eq!(cell(&terminal, 1, 2).bg, Some(Color::Reset));
    }

    #[test]
    fn the_active_task_shows_a_marker_and_the_live_elapsed_time() {
        let mut app = app_with(&["alpha", "beta"]);
        app.handle(Command::ToggleTracking);
        app.freeze_elapsed_for_tests(Duration::from_secs(125));
        let terminal = draw(&app);
        let rows = rows(&terminal);

        assert!(rows[2].contains("▶ alpha"), "got {:?}", rows[2]);
        assert!(rows[3].contains("beta"), "got {:?}", rows[3]);
        assert!(!rows[3].contains("▶"), "only the active task is marked");
        assert!(rows[0].contains("alpha"));
        assert!(rows[0].contains("00:02:05"), "got {:?}", rows[0]);
    }

    #[test]
    fn an_idle_header_shows_only_the_title() {
        let app = app_with(&["alpha"]);
        let terminal = draw(&app);
        let header = row(&terminal, 0);
        assert!(header.contains("Time Tracker"));
        assert!(!header.contains("▶"));
    }

    #[test]
    fn the_status_line_shows_errors_in_the_error_color() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::OpenAdd);
        app.handle(Command::Confirm);
        let terminal = draw(&app);
        assert!(row(&terminal, 22).contains("must not be empty"));
        assert_eq!(cell(&terminal, 0, 22).fg, Some(Color::Red));
    }

    #[test]
    fn the_status_line_shows_info_dimmed() {
        let app = app_with(&["alpha"]);
        let terminal = draw(&app);
        assert_eq!(cell(&terminal, 0, 22).fg, Some(Color::DarkGray));
    }

    #[test]
    fn the_footer_matches_each_mode() {
        let mut app = app_with(&["alpha"]);
        let terminal = draw(&app);
        assert!(row(&terminal, 23).contains("space start/stop"));

        app.handle(Command::OpenAdd);
        let terminal = draw(&app);
        assert!(row(&terminal, 23).contains("enter save"));

        app.handle(Command::Cancel);
        app.handle(Command::OpenArchiveConfirm);
        let terminal = draw(&app);
        assert!(row(&terminal, 23).contains("y/enter confirm"));
    }

    #[test]
    fn an_empty_list_renders_a_hint_and_stays_selectable() {
        let app = App::load(SqliteRepository::open_in_memory().unwrap()).unwrap();
        let terminal = draw(&app);
        assert!(row(&terminal, 2).contains("No tasks. Press a to add one."));
        assert!(row(&terminal, 1).contains("Tasks"));
    }

    #[test]
    fn the_input_modal_shows_the_prompt_buffer_and_cursor() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::OpenAdd);
        app.handle(Command::Insert('a'));
        app.handle(Command::Insert('b'));
        let terminal = draw(&app);
        let rows = rows(&terminal);
        let modal_row = rows
            .iter()
            .enumerate()
            .find(|(_, text)| text.contains("New task name:"))
            .map(|(y, _)| y)
            .expect("the add prompt is visible");
        let modal_row = modal_row as u16;
        assert!(rows[modal_row as usize].contains("ab▏"));
        // The cursor block is styled distinctly from the typed text.
        let cursor_x = rows[modal_row as usize]
            .char_indices()
            .position(|(_, character)| character == '▏')
            .unwrap() as u16;
        assert_eq!(cell(&terminal, cursor_x, modal_row).fg, Some(Color::Cyan));
    }

    #[test]
    fn the_rename_modal_shows_the_rename_prompt() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::OpenRename);
        let terminal = draw(&app);
        assert!(row(&terminal, 11).contains("Rename task: alpha"));
    }

    #[test]
    fn the_confirm_modal_shows_the_task_name() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::OpenArchiveConfirm);
        let terminal = draw(&app);
        let row = row(&terminal, 11);
        assert!(row.contains("Archive \"alpha\"?"), "got {row:?}");
    }

    #[test]
    fn centered_keeps_the_modal_inside_the_area() {
        let area = Rect::new(0, 0, 80, 24);
        assert_eq!(centered(56, 3, area), Rect::new(12, 10, 56, 3));
        // A modal larger than the area is clamped, never overflows.
        assert_eq!(centered(100, 50, area), area);
        // Off-center areas keep the modal centered inside them.
        let offset = Rect::new(10, 5, 20, 9);
        assert_eq!(centered(10, 3, offset), Rect::new(15, 8, 10, 3));
    }

    #[test]
    fn elapsed_times_format_as_hours_minutes_seconds() {
        assert_eq!(format_elapsed(Duration::from_secs(0)), "00:00:00");
        assert_eq!(format_elapsed(Duration::from_secs(59)), "00:00:59");
        assert_eq!(format_elapsed(Duration::from_secs(60)), "00:01:00");
        assert_eq!(format_elapsed(Duration::from_secs(3661)), "01:01:01");
        assert_eq!(format_elapsed(Duration::from_secs(86_399)), "23:59:59");
        assert_eq!(format_elapsed(Duration::from_secs(25 * 3600)), "25:00:00");
    }
}
