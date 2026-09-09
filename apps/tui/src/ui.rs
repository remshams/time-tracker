//! Rendering for the task list interface.
//!
//! Everything here only reads state. Layout, styles, and widgets draw the
//! current [`App`]; mutating state happens in [`crate::app`].

use std::time::Duration;

use chrono::{DateTime, Offset, TimeZone, Utc};
use tracker_application::TrackerApplicationService;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{
    App, CorrectionDraft, CorrectionField, History, InputPurpose, Mode, Screen, Status, TaskView,
};
use crate::{keymap, styles};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph};

/// The narrowest terminal that renders the full interactive interface.
pub(crate) const MIN_TERMINAL_WIDTH: u16 = 60;

/// The display width of one character in terminal cells.
fn char_width(character: char) -> usize {
    character.width().unwrap_or(0)
}

/// The longest prefix of `text` that fits in `max_width` display cells.
///
/// Cutting always lands between whole characters, so wide characters and
/// combining marks are never split.
fn fit_prefix(text: &str, max_width: usize) -> &str {
    let mut end = 0;
    let mut width = 0;
    for (index, character) in text.char_indices() {
        let character_width = char_width(character);
        if width + character_width > max_width {
            break;
        }
        width += character_width;
        end = index + character.len_utf8();
    }
    &text[..end]
}

/// The longest suffix of `text` that fits in `max_width` display cells.
///
/// Cutting always lands before a whole base character, so a combining mark
/// is never separated from the base it follows.
fn fit_suffix(text: &str, max_width: usize) -> &str {
    let mut committed = text.len();
    let mut width = 0;
    for (index, character) in text.char_indices().rev() {
        let character_width = char_width(character);
        if character_width == 0 {
            // A combining mark travels with the base that follows in this
            // reverse pass; it joins the window only if that base fits.
            continue;
        }
        if width + character_width > max_width {
            break;
        }
        width += character_width;
        committed = index;
    }
    &text[committed..]
}

/// Renders one frame of the interface.
pub fn render<S: TrackerApplicationService>(frame: &mut Frame, app: &App<S>) {
    if frame.area().width < MIN_TERMINAL_WIDTH {
        frame.render_widget(
            Paragraph::new("Time Tracker needs at least 60 columns."),
            frame.area(),
        );
        return;
    }

    let [header, body, status_area, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    render_header(frame, header, app);
    match app.screen() {
        Screen::TaskList => render_tasks(frame, body, app),
        Screen::WorklogHistory => render_history(frame, body, app),
    }
    render_status(frame, status_area, app);
    render_footer(frame, footer, app);
    render_modal(frame, body, app);
}

/// Renders the title line, with the live timer while tracking runs.
fn render_header<S: TrackerApplicationService>(frame: &mut Frame, area: Rect, app: &App<S>) {
    let mut spans = vec![Span::styled("Time Tracker", styles::title())];
    if let Some(name) = app.active_task_name() {
        let elapsed = app.elapsed().map(format_elapsed).unwrap_or_default();
        spans.push(Span::raw("  "));
        spans.push(Span::styled("▶", styles::active_marker()));
        spans.push(Span::raw(" "));
        spans.push(Span::raw(name));
        spans.push(Span::raw(format!("  {elapsed}")));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Renders the task list, its selection, and the active-task marker.
fn render_tasks<S: TrackerApplicationService>(frame: &mut Frame, area: Rect, app: &App<S>) {
    let (view_title, empty_text) = match app.view() {
        TaskView::Active => ("Active tasks", "No active tasks. Press a to add one."),
        TaskView::Archived => ("Archived tasks", "No archived tasks."),
    };
    let block = Block::bordered()
        .title(format!("{view_title} · {}", app.ordering_label()))
        .border_style(if app.mode() == &Mode::Normal {
            styles::focused_border()
        } else {
            Style::default()
        });
    if app.tasks().is_empty() {
        frame.render_widget(Paragraph::new(empty_text).block(block), area);
        return;
    }
    // Two border cells and the two-cell marker leave this much for a name.
    let name_budget = (area.width as usize).saturating_sub(4);
    let active_task_id = app.active_task_id();
    let items: Vec<ListItem> = app
        .tasks()
        .iter()
        .map(|task| {
            let active = active_task_id == Some(task.id);
            let marker = if active {
                Span::styled("▶ ", styles::active_marker())
            } else {
                Span::raw("  ")
            };
            let name = fit_prefix(task.name().as_str(), name_budget);
            ListItem::new(Line::from(vec![marker, Span::raw(name)]))
        })
        .collect();
    let list = List::new(items)
        .block(block)
        .highlight_style(styles::selected());
    let mut state = ListState::default().with_selected(app.selected());
    frame.render_stateful_widget(list, area, &mut state);
}

/// Renders the worklog history of one task.
///
/// Rows appear newest first. Each row takes two lines, with local timestamps
/// on the first line and the duration on the second. Each timestamp converts
/// to local time on its own, so rows on either side of a daylight-saving
/// transition show their own local minute.
fn render_history<S: TrackerApplicationService>(frame: &mut Frame, area: Rect, app: &App<S>) {
    let Some(history) = app.history() else {
        return;
    };
    let name = app.history_task_name().unwrap_or("unknown task");
    let block = Block::bordered()
        .title(format!("Worklog history · {name}"))
        .border_style(if app.mode() == &Mode::Normal {
            styles::focused_border()
        } else {
            Style::default()
        });
    if history.availability == crate::app::HistoryAvailability::Unavailable {
        frame.render_widget(
            Paragraph::new("History unavailable. Press r to retry.").block(block),
            area,
        );
        return;
    }
    if history.worklogs.is_empty() {
        frame.render_widget(Paragraph::new("No worklogs yet.").block(block), area);
        return;
    }
    let items: Vec<ListItem> = history
        .worklogs
        .iter()
        .map(|worklog| {
            let end = match worklog.end() {
                Some(end) => Span::raw(app.local_time(end)),
                None => Span::styled("Running", styles::active_marker()),
            };
            ListItem::new(vec![
                Line::from(vec![
                    Span::raw(app.local_time(worklog.start())),
                    Span::raw(" → "),
                    end,
                ]),
                Line::from(vec![
                    Span::raw("  "),
                    Span::raw(format_elapsed(app.history_row_duration(worklog))),
                ]),
            ])
        })
        .collect();
    let list = List::new(items)
        .block(block)
        .highlight_style(styles::selected());
    let mut state = ListState::default().with_selected(app.history_selected_index());
    frame.render_stateful_widget(list, area, &mut state);
}

/// Formats an instant in `tz` as a local minute timestamp.
pub(crate) fn local_time<Tz>(at: DateTime<Utc>, tz: &Tz) -> String
where
    Tz: TimeZone,
{
    let offset = tz.offset_from_utc_datetime(&at.naive_utc()).fix();
    at.naive_utc()
        .checked_add_offset(offset)
        .map(|local| local.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "outside local range".to_owned())
}

/// Renders the status or error line.
fn render_status<S: TrackerApplicationService>(frame: &mut Frame, area: Rect, app: &App<S>) {
    let paragraph = match app.status() {
        Status::Info(text) => Paragraph::new(text.as_str()),
        Status::Error(text) => Paragraph::new(Line::from(vec![
            Span::styled("Error: ", styles::error_label()),
            Span::raw(text),
        ])),
    };
    frame.render_widget(paragraph, area);
}

/// Renders the context-sensitive key help for the current mode.
fn render_footer<S: TrackerApplicationService>(frame: &mut Frame, area: Rect, app: &App<S>) {
    frame.render_widget(
        Paragraph::new(keymap::footer_hints(
            app.mode(),
            app.view(),
            app.screen(),
            app.history().is_some_and(History::is_available),
            area.width,
        )),
        area,
    );
}

/// Renders the modal dialog of the current mode, if any.
fn render_modal<S: TrackerApplicationService>(frame: &mut Frame, area: Rect, app: &App<S>) {
    match app.mode() {
        Mode::Input { purpose, buffer } => render_input_modal(frame, area, *purpose, buffer),
        Mode::ConfirmArchive { name, .. } => render_confirm_modal(frame, area, name),
        Mode::Correction(draft) => render_correction_modal(frame, area, draft),
        Mode::Normal => {}
    }
}

/// Renders the one-line text input with a visible cursor.
///
/// The modal is fixed-width, so a long text scrolls horizontally: the tail
/// of the buffer stays visible next to the cursor while the head moves out
/// of view. Wide characters are never cut in half.
fn render_input_modal(frame: &mut Frame, area: Rect, purpose: InputPurpose, buffer: &str) {
    let modal = centered(56, 3, area);
    frame.render_widget(Clear, modal);
    let prompt = match purpose {
        InputPurpose::Add => "New task name:",
        InputPurpose::Rename { .. } => "Rename task:",
    };
    // The border cells, the separating space, and the cursor block leave
    // this much display width for the visible part of the buffer.
    let budget = (modal.width as usize)
        .saturating_sub(2)
        .saturating_sub(prompt.width())
        .saturating_sub(2);
    let visible = fit_suffix(buffer, budget);
    let line = Line::from(vec![
        Span::raw(prompt),
        Span::raw(" "),
        Span::raw(visible),
        Span::styled("▏", styles::input_cursor()),
    ]);
    frame.render_widget(
        Paragraph::new(line).block(Block::bordered().border_style(styles::focused_border())),
        modal,
    );
}

fn render_correction_modal(frame: &mut Frame, area: Rect, draft: &CorrectionDraft) {
    let height = if draft.end().is_some() { 4 } else { 3 };
    let modal = centered(58, height, area);
    frame.render_widget(Clear, modal);
    let mut lines = vec![correction_field_line(
        "Start",
        draft.start(),
        draft.focused() == CorrectionField::Start,
    )];
    if let Some(end) = draft.end() {
        lines.push(correction_field_line(
            "End",
            end,
            draft.focused() == CorrectionField::End,
        ));
    }
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .title("Correct worklog")
                .border_style(styles::focused_border()),
        ),
        modal,
    );
}

fn correction_field_line<'a>(
    label: &'static str,
    input: &'a crate::app::TimestampInput,
    focused: bool,
) -> Line<'a> {
    let label = format!("{label:<5}: ");
    if !focused {
        return Line::from(vec![Span::raw(label), Span::raw(input.text())]);
    }
    let cursor_byte = input
        .text()
        .char_indices()
        .nth(input.cursor())
        .map_or(input.text().len(), |(index, _)| index);
    let (before, after) = input.text().split_at(cursor_byte);
    Line::from(vec![
        Span::styled(label, styles::selected()),
        Span::raw(before),
        Span::styled("▏", styles::input_cursor()),
        Span::raw(after),
    ])
}

/// Renders the archive confirmation.
fn render_confirm_modal(frame: &mut Frame, area: Rect, name: &str) {
    let modal = centered(56, 3, area);
    frame.render_widget(Clear, modal);
    frame.render_widget(
        Paragraph::new(format!("Archive \"{name}\"?")).block(
            Block::bordered()
                .title("Confirm archive")
                .border_style(styles::focused_border()),
        ),
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
    use chrono::{DateTime, FixedOffset, MappedLocalTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::{Terminal, backend::TestBackend};
    use tracker_application::TrackerApplication;
    use tracker_domain::{Task, TaskId, TaskName, Worklog, WorklogId};
    use tracker_storage::SqliteRepository;

    use super::*;
    use crate::command::Command;

    const WIDTH: u16 = 80;
    const HEIGHT: u16 = 24;

    fn app_with(names: &[&str]) -> App<TrackerApplication<SqliteRepository>> {
        let repository = SqliteRepository::open_in_memory().unwrap();
        for name in names {
            let task = Task::create(
                TaskId::generate(),
                TaskName::new(name).unwrap(),
                DateTime::<Utc>::from_timestamp(100, 0).unwrap(),
            );
            repository.create_task(task).unwrap();
        }
        App::load(TrackerApplication::load(repository).unwrap())
    }

    /// An app whose "alpha" history is open, holding one stopped worklog
    /// per `(start, end)` pair, newest first on screen.
    ///
    /// The display offset is frozen at UTC+02:00, so rendered local times
    /// are deterministic on every host.
    fn history_app(entries: &[(i64, i64)]) -> App<TrackerApplication<SqliteRepository>> {
        let repository = SqliteRepository::open_in_memory().unwrap();
        let task = Task::create(
            TaskId::from_uuid(uuid::Uuid::from_u128(1)),
            TaskName::new("alpha").unwrap(),
            DateTime::<Utc>::from_timestamp(100, 0).unwrap(),
        );
        repository.create_task(task.clone()).unwrap();
        for (index, (start, end)) in entries.iter().enumerate() {
            let worklog = Worklog::new(
                WorklogId::from_uuid(uuid::Uuid::from_u128(index as u128 + 1)),
                task.id,
                DateTime::<Utc>::from_timestamp(*start, 0).unwrap(),
                Some(DateTime::<Utc>::from_timestamp(*end, 0).unwrap()),
            )
            .unwrap();
            repository.insert_worklog(&worklog).unwrap();
        }
        let mut app = App::load(TrackerApplication::load(repository).unwrap());
        app.freeze_offset_for_tests(FixedOffset::east_opt(2 * 3600).unwrap());
        app.handle(Command::OpenHistory);
        app
    }

    fn draw(app: &App<TrackerApplication<SqliteRepository>>) -> Terminal<TestBackend> {
        draw_at(app, WIDTH, HEIGHT)
    }

    fn draw_at(
        app: &App<TrackerApplication<SqliteRepository>>,
        width: u16,
        height: u16,
    ) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
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
    fn a_terminal_below_the_minimum_width_shows_a_resize_message() {
        let app = app_with(&["alpha"]);
        let terminal = draw_at(&app, 59, HEIGHT);
        assert!(row(&terminal, 0).contains("Time Tracker needs at least 60 columns."));
        assert!(!row(&terminal, 1).contains("Active tasks"));
    }

    #[test]
    fn the_seeded_list_renders_with_title_selection_and_help() {
        let app = app_with(&["alpha", "beta"]);
        let terminal = draw(&app);
        let rows = rows(&terminal);

        assert!(rows[0].contains("Time Tracker"));
        assert!(rows[1].contains("Active tasks · recently worked"));
        assert!(rows[2].contains("alpha"));
        assert!(rows[3].contains("beta"));
        assert!(rows[22].contains("Ready"));
        assert!(rows[23].contains("enter history"));
        assert!(rows[23].contains("s sort"));
        assert!(rows[23].contains("a/e/d edit"));
        assert!(rows[23].contains("ctrl+c quit"));
        assert_eq!(cell(&terminal, 0, 0).fg, Some(Color::Blue));
        assert_eq!(cell(&terminal, 0, 1).fg, Some(Color::Blue));
    }

    #[test]
    fn the_selected_row_is_reversed_and_follows_the_selection() {
        let mut app = app_with(&["alpha", "beta", "gamma"]);
        let terminal = draw(&app);
        let selected = cell(&terminal, 1, 2);
        assert_eq!(selected.fg, Some(Color::Reset));
        assert_eq!(selected.bg, Some(Color::Reset));
        assert!(selected.add_modifier.contains(Modifier::REVERSED));
        assert!(cell(&terminal, 1, 3).add_modifier.is_empty());

        app.handle(crate::command::Command::MoveDown);
        let terminal = draw(&app);
        assert!(
            cell(&terminal, 1, 3)
                .add_modifier
                .contains(Modifier::REVERSED)
        );
        assert!(cell(&terminal, 1, 2).add_modifier.is_empty());
    }

    #[test]
    fn the_active_task_shows_a_marker_and_the_live_elapsed_time() {
        let mut app = app_with(&["alpha", "beta"]);
        app.handle(Command::ToggleTracking);
        app.freeze_elapsed_for_tests(Duration::from_secs(125));
        app.handle(Command::MoveDown);
        let terminal = draw(&app);
        let rows = rows(&terminal);

        assert!(rows[2].contains("▶ alpha"), "got {:?}", rows[2]);
        assert!(rows[3].contains("beta"), "got {:?}", rows[3]);
        assert!(!rows[3].contains("▶"), "only the active task is marked");
        assert!(rows[0].contains("alpha"));
        assert!(rows[0].contains("00:02:05"), "got {:?}", rows[0]);
        assert_eq!(cell(&terminal, 14, 0).fg, Some(Color::Green));
        assert_eq!(cell(&terminal, 1, 2).fg, Some(Color::Green));
        assert_eq!(cell(&terminal, 3, 2).fg, Some(Color::Reset));
    }

    #[test]
    fn a_selected_running_task_keeps_selection_colors_over_its_marker() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::ToggleTracking);
        let terminal = draw(&app);

        let marker = cell(&terminal, 1, 2);
        assert_eq!(marker.fg, Some(Color::Reset));
        assert_eq!(marker.bg, Some(Color::Reset));
        assert!(marker.add_modifier.contains(Modifier::REVERSED));
        assert_eq!(cell(&terminal, 14, 0).fg, Some(Color::Green));
    }

    #[test]
    fn only_normal_mode_focuses_the_task_list_border() {
        let mut app = app_with(&["alpha"]);
        let terminal = draw(&app);
        assert_eq!(cell(&terminal, 0, 1).fg, Some(Color::Blue));

        app.handle(Command::OpenAdd);
        let terminal = draw(&app);
        assert_eq!(cell(&terminal, 0, 1).fg, Some(Color::Reset));
        assert_eq!(cell(&terminal, 12, 10).fg, Some(Color::Blue));

        app.handle(Command::Cancel);
        app.handle(Command::OpenArchiveConfirm);
        let terminal = draw(&app);
        assert_eq!(cell(&terminal, 0, 1).fg, Some(Color::Reset));
        assert_eq!(cell(&terminal, 12, 10).fg, Some(Color::Blue));
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
    fn the_status_line_labels_errors_and_uses_default_colors() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::OpenAdd);
        app.handle(Command::Confirm);
        let terminal = draw(&app);
        assert!(row(&terminal, 22).contains("Error: The task name must not be empty"));
        let label = cell(&terminal, 0, 22);
        assert_eq!(label.fg, Some(Color::Red));
        assert!(label.add_modifier.contains(Modifier::BOLD));
        let message = cell(&terminal, 7, 22);
        assert_eq!(message.fg, Some(Color::Reset));
        assert!(message.add_modifier.is_empty());
    }

    #[test]
    fn the_status_line_shows_info_in_the_terminal_foreground() {
        let app = app_with(&["alpha"]);
        let terminal = draw(&app);
        let style = cell(&terminal, 0, 22);
        assert_eq!(style.fg, Some(Color::Reset));
        assert!(style.add_modifier.is_empty());
    }

    #[test]
    fn the_footer_matches_each_mode() {
        let mut app = app_with(&["alpha"]);
        let terminal = draw(&app);
        assert!(row(&terminal, 23).contains("h/l view"));
        assert!(row(&terminal, 23).contains("space track"));
        assert!(row(&terminal, 23).contains("enter history"));

        app.handle(Command::OpenAdd);
        let terminal = draw(&app);
        assert!(row(&terminal, 23).contains("enter save"));

        app.handle(Command::Cancel);
        app.handle(Command::OpenArchiveConfirm);
        let terminal = draw(&app);
        assert!(row(&terminal, 23).contains("y/enter confirm"));
    }

    #[test]
    fn narrow_views_keep_ordering_titles_and_complete_compact_footers() {
        let mut app = app_with(&["alpha"]);
        let terminal = draw_at(&app, 60, 20);
        assert!(row(&terminal, 1).contains("Active tasks · recently worked"));
        let footer = row(&terminal, 19);
        assert!(footer.contains("enter history"), "got {footer:?}");
        assert!(footer.contains("s sort"), "got {footer:?}");
        assert!(footer.contains("q/esc/ctrl+c quit"), "got {footer:?}");

        app.handle(Command::ShowArchivedTasks);
        app.handle(Command::CycleOrdering);
        let terminal = draw_at(&app, 60, 20);
        assert!(row(&terminal, 1).contains("Archived tasks · recently updated"));
        let footer = row(&terminal, 19);
        assert!(footer.contains("enter history"), "got {footer:?}");
        assert!(footer.contains("u restore"), "got {footer:?}");
        assert!(footer.contains("s sort"), "got {footer:?}");
        assert!(footer.contains("q/esc/ctrl+c quit"), "got {footer:?}");
    }

    #[test]
    fn task_panel_titles_show_the_shared_ordering_in_both_views() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::CycleOrdering);
        let terminal = draw(&app);
        assert!(
            row(&terminal, 1).contains("Active tasks · recently updated"),
            "got {:?}",
            row(&terminal, 1)
        );

        app.handle(Command::ShowArchivedTasks);
        let terminal = draw(&app);
        assert!(
            row(&terminal, 1).contains("Archived tasks · recently updated"),
            "got {:?}",
            row(&terminal, 1)
        );
    }

    #[test]
    fn an_empty_active_list_renders_a_hint_and_stays_selectable() {
        let app = App::load(
            TrackerApplication::load(SqliteRepository::open_in_memory().unwrap()).unwrap(),
        );
        let terminal = draw(&app);
        assert!(row(&terminal, 2).contains("No active tasks. Press a to add one."));
        assert!(row(&terminal, 1).contains("Active tasks"));
    }

    #[test]
    fn the_archived_view_renders_its_title_rows_and_footer() {
        let mut app = app_with(&["alpha", "beta"]);
        app.handle(Command::OpenArchiveConfirm);
        app.handle(Command::Confirm);
        app.handle(Command::ShowArchivedTasks);
        let terminal = draw(&app);
        let rows = rows(&terminal);

        assert!(rows[1].contains("Archived tasks"), "got {:?}", rows[1]);
        assert!(rows[2].contains("alpha"), "got {:?}", rows[2]);
        assert!(!rows[3].contains("beta"), "beta is still active");
        assert!(rows[23].contains("u unarchive"), "got {:?}", rows[23]);
        assert!(rows[23].contains("h/l view"));
        assert!(rows[23].contains("enter history"));
        assert!(
            !rows[23].contains("a add"),
            "archived view must not hint add"
        );
    }

    #[test]
    fn an_empty_archived_view_renders_its_own_empty_text() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::ShowArchivedTasks);
        let terminal = draw(&app);
        assert!(row(&terminal, 2).contains("No archived tasks."));
        assert!(row(&terminal, 1).contains("Archived tasks"));
    }

    #[test]
    fn the_archived_view_keeps_the_timer_header_and_selection() {
        let mut app = app_with(&["alpha", "beta"]);
        app.handle(Command::ToggleTracking);
        app.handle(Command::MoveDown);
        app.handle(Command::OpenArchiveConfirm);
        app.handle(Command::Confirm);
        app.handle(Command::ShowArchivedTasks);
        app.freeze_elapsed_for_tests(Duration::from_secs(61));
        let terminal = draw(&app);
        let rows = rows(&terminal);

        // The active timer outlives the view switch.
        assert!(rows[0].contains("▶ alpha"), "got {:?}", rows[0]);
        assert!(rows[0].contains("00:01:01"), "got {:?}", rows[0]);
        assert!(rows[1].contains("Archived tasks"));
        assert!(rows[2].contains("beta"), "got {:?}", rows[2]);
        // The archived row carries the selection highlight.
        assert!(
            cell(&terminal, 1, 2)
                .add_modifier
                .contains(Modifier::REVERSED)
        );
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
        // The cursor remains prominent without assuming a palette color.
        let cursor_x = rows[modal_row as usize]
            .char_indices()
            .position(|(_, character)| character == '▏')
            .unwrap() as u16;
        let style = cell(&terminal, cursor_x, modal_row);
        assert_eq!(style.fg, Some(Color::Reset));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn a_long_input_scrolls_so_the_tail_and_cursor_stay_visible() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::OpenAdd);
        // Longer than the visible budget: the head scrolls out of view.
        for character in "a".repeat(80).chars() {
            app.handle(Command::Insert(character));
        }
        let terminal = draw(&app);
        let row_text = row(&terminal, 11);
        // The modal shows the last 38 columns of the buffer plus the cursor.
        assert!(row_text.contains(&"a".repeat(38)), "got {row_text:?}");
        assert!(!row_text.contains(&"a".repeat(39)), "got {row_text:?}");
        assert!(row_text.contains('▏'), "the cursor must stay visible");
    }

    #[test]
    fn wide_characters_scroll_on_whole_characters() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::OpenAdd);
        // 30 clock symbols are 60 columns, wider than the budget of 38.
        for character in "🕒".repeat(30).chars() {
            app.handle(Command::Insert(character));
        }
        let terminal = draw(&app);
        let row_text = row(&terminal, 11);
        // 19 symbols fill the 38-column budget exactly; the wide glyphs
        // occupy their own cells, so count them.
        assert_eq!(row_text.matches('🕒').count(), 19, "got {row_text:?}");
        assert!(row_text.contains('▏'), "the cursor must stay visible");
    }

    #[test]
    fn prefix_and_suffix_windows_cut_on_whole_characters() {
        assert_eq!(fit_prefix("hello", 3), "hel");
        assert_eq!(fit_prefix("hello", 0), "");
        assert_eq!(fit_prefix("hello", 99), "hello");
        assert_eq!(fit_prefix("🕒🕒🕒", 3), "🕒");
        assert_eq!(fit_suffix("hello", 3), "llo");
        assert_eq!(fit_suffix("hello", 0), "");
        assert_eq!(fit_suffix("🕒🕒🕒", 3), "🕒");
        // Combining marks travel with their base character.
        let acute = "e\u{301}";
        let text: String = acute.repeat(3);
        assert_eq!(fit_prefix(&text, 2), "e\u{301}e\u{301}");
        assert_eq!(fit_suffix(&text, 1), "e\u{301}");
    }

    #[test]
    fn a_very_long_task_name_renders_single_line_and_truncated() {
        // 256 characters is the longest storable name, and far wider than
        // the 80-column test terminal.
        let long = "x".repeat(256);
        let app = app_with(&[&long, "beta"]);
        let terminal = draw(&app);
        let rows = rows(&terminal);
        // Row 2 holds the long name and row 3 the next task: nothing wrapped.
        assert!(rows[2].contains("xxx"), "got {:?}", rows[2]);
        assert!(rows[3].contains("beta"), "got {:?}", rows[3]);
        // The right border survived the truncation.
        assert!(rows[2].ends_with('│'), "got {:?}", rows[2]);
        assert!(rows[3].ends_with('│'), "got {:?}", rows[3]);
    }

    #[test]
    fn a_wide_character_task_name_truncates_without_splitting_a_character() {
        // 128 clock symbols occupy 256 terminal columns.
        let wide = "🕒".repeat(128);
        let app = app_with(&[&wide]);
        let terminal = draw(&app);
        let row_text = row(&terminal, 2);
        // The 76-cell budget fits 38 wide symbols.
        assert_eq!(row_text.matches('🕒').count(), 38, "got {row_text:?}");
        assert!(row_text.ends_with('│'), "got {row_text:?}");
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

    #[test]
    fn local_times_render_as_local_minutes() {
        let plus_two = FixedOffset::east_opt(2 * 3600).unwrap();
        assert_eq!(
            local_time(DateTime::<Utc>::from_timestamp(0, 0).unwrap(), &plus_two),
            "1970-01-01 02:00"
        );
        assert_eq!(
            local_time(
                DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
                &FixedOffset::west_opt(5 * 3600 + 1800).unwrap()
            ),
            "1969-12-31 18:30"
        );
        assert_eq!(
            local_time(
                DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
                &FixedOffset::east_opt(0).unwrap()
            ),
            "1970-01-01 00:00"
        );
    }

    #[test]
    fn local_times_render_chrono_signed_expanded_years() {
        for (year, expected) in [
            (-1, "-0001-01-02 03:04"),
            (0, "0000-01-02 03:04"),
            (9999, "9999-01-02 03:04"),
            (10000, "+10000-01-02 03:04"),
        ] {
            let instant = Utc.with_ymd_and_hms(year, 1, 2, 3, 4, 0).single().unwrap();
            assert_eq!(local_time(instant, &Utc), expected);
        }
    }

    #[test]
    fn local_times_outside_chrono_range_render_a_safe_marker() {
        assert_eq!(
            local_time(DateTime::<Utc>::MIN_UTC, &FixedOffset::west_opt(1).unwrap()),
            "outside local range"
        );
        assert_eq!(
            local_time(DateTime::<Utc>::MAX_UTC, &FixedOffset::east_opt(1).unwrap()),
            "outside local range"
        );
    }

    /// A zone that jumps from UTC+01:00 to UTC+02:00 at a fixed UTC
    /// instant, standing in for a daylight-saving transition.
    #[derive(Clone, Copy, Debug)]
    struct SwitchingZone {
        /// The UTC second the jump happens at; before it the offset is
        /// +01:00, from it on +02:00.
        switch: i64,
    }

    impl SwitchingZone {
        fn pick(&self, utc_seconds: i64) -> FixedOffset {
            if utc_seconds < self.switch {
                FixedOffset::east_opt(3600).unwrap()
            } else {
                FixedOffset::east_opt(2 * 3600).unwrap()
            }
        }
    }

    impl TimeZone for SwitchingZone {
        type Offset = FixedOffset;

        fn from_offset(_offset: &FixedOffset) -> Self {
            // The zone is a plain value, so it cannot be reconstructed from
            // an offset; the formatter never asks.
            unimplemented!("the formatter never recovers the zone")
        }

        fn offset_from_local_date(&self, local: &NaiveDate) -> MappedLocalTime<FixedOffset> {
            self.offset_from_local_datetime(
                &local
                    .and_hms_opt(0, 0, 0)
                    .expect("midnight exists on every date"),
            )
        }

        fn offset_from_local_datetime(
            &self,
            local: &NaiveDateTime,
        ) -> MappedLocalTime<FixedOffset> {
            // The formatter only asks about UTC instants; reading the naive
            // value as UTC is enough to pick a side of the jump.
            MappedLocalTime::Single(self.pick(local.and_utc().timestamp()))
        }

        fn offset_from_utc_date(&self, utc: &NaiveDate) -> FixedOffset {
            self.pick(
                utc.and_hms_opt(0, 0, 0)
                    .expect("midnight exists on every date")
                    .and_utc()
                    .timestamp(),
            )
        }

        fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> FixedOffset {
            self.pick(utc.and_utc().timestamp())
        }
    }

    #[test]
    fn local_times_use_the_offset_valid_at_each_instant() {
        // The zone jumps from +01:00 to +02:00 one hour after the epoch, so
        // the second before and the second of the jump render through
        // different offsets, exactly like a worklog on each side of a
        // daylight-saving transition.
        let zone = SwitchingZone { switch: 3600 };
        assert_eq!(
            local_time(DateTime::<Utc>::from_timestamp(3599, 0).unwrap(), &zone),
            "1970-01-01 01:59"
        );
        assert_eq!(
            local_time(DateTime::<Utc>::from_timestamp(3600, 0).unwrap(), &zone),
            "1970-01-01 03:00"
        );
    }

    #[test]
    fn the_history_renders_title_rows_and_durations() {
        // 01:00:00Z to 01:00:15Z, then a whole day later, newest first.
        let app = history_app(&[(3600, 3615), (86_400, 86_400 + 90)]);
        let terminal = draw(&app);
        let rows = rows(&terminal);

        assert!(
            rows[1].contains("Worklog history · alpha"),
            "got {:?}",
            rows[1]
        );
        assert!(
            rows[2].contains("1970-01-02 02:00 → 1970-01-02 02:01"),
            "the newest row leads: {:?}",
            rows[2]
        );
        assert!(rows[3].contains("00:01:30"), "got {:?}", rows[3]);
        assert!(
            rows[4].contains("1970-01-01 03:00 → 1970-01-01 03:00"),
            "got {:?}",
            rows[4]
        );
        assert!(rows[5].contains("00:00:15"), "got {:?}", rows[5]);
        assert!(rows[23].contains("o older"), "got {:?}", rows[23]);
        assert!(rows[23].contains("r refresh"), "got {:?}", rows[23]);
        assert!(rows[23].contains("esc back"), "got {:?}", rows[23]);
        assert!(
            !rows[23].contains("space track"),
            "no task-list hint leaks in"
        );
        // The newest row carries the selection highlight.
        assert!(
            cell(&terminal, 1, 2)
                .add_modifier
                .contains(Modifier::REVERSED),
            "the first history row is selected"
        );
        assert!(
            !cell(&terminal, 1, 4)
                .add_modifier
                .contains(Modifier::REVERSED),
            "the second row is not selected"
        );
    }

    #[test]
    fn history_rows_stay_complete_at_sixty_columns() {
        let app = history_app(&[(3600, 3615)]);
        let terminal = draw_at(&app, 60, 20);
        let row_text = row(&terminal, 2);
        assert!(
            row_text.contains("1970-01-01 03:00 → 1970-01-01 03:00"),
            "both timestamps stayed visible: {row_text:?}"
        );
        assert!(row_text.ends_with('│'), "the border survived: {row_text:?}");
        assert!(row(&terminal, 3).contains("00:00:15"));
        let footer = row(&terminal, 19);
        assert!(footer.contains("o older"), "got {footer:?}");
        assert!(footer.contains("esc back"), "got {footer:?}");
        assert!(footer.contains("ctrl+c"), "got {footer:?}");
    }

    #[test]
    fn the_running_row_shows_running_and_the_headers_elapsed_time() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::ToggleTracking);
        app.freeze_elapsed_for_tests(Duration::from_secs(125));
        app.handle(Command::OpenHistory);
        let terminal = draw(&app);
        let rows = rows(&terminal);

        // The header keeps the timer while the history is open.
        assert!(rows[0].contains("▶ alpha"), "got {:?}", rows[0]);
        assert!(rows[0].contains("00:02:05"), "got {:?}", rows[0]);
        assert!(rows[2].contains("Running"), "got {:?}", rows[2]);
        assert!(
            rows[3].contains("00:02:05"),
            "the running row shares the header's clock: {:?}",
            rows[3]
        );
    }

    #[test]
    fn an_empty_history_renders_its_own_message() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::OpenHistory);
        let terminal = draw(&app);
        assert!(
            row(&terminal, 2).contains("No worklogs yet."),
            "got {:?}",
            row(&terminal, 2)
        );
        assert!(row(&terminal, 1).contains("Worklog history · alpha"));
    }

    #[test]
    fn correction_modal_marks_focus_shows_the_cursor_and_fits_sixty_columns() {
        let mut app = history_app(&[(3600, 3615)]);
        app.handle(Command::OpenCorrection);
        let terminal = draw_at(&app, 60, 20);
        let screen_rows = rows(&terminal);
        let start_row = screen_rows
            .iter()
            .position(|row| row.contains("Start: "))
            .expect("the start field is visible") as u16;
        let end_row = screen_rows
            .iter()
            .position(|row| row.contains("End  : "))
            .expect("the end field is visible") as u16;
        assert!(screen_rows[start_row as usize].contains("1970-01-01 03:00▏"));
        assert!(screen_rows[end_row as usize].contains("1970-01-01 03:00"));
        assert!(
            cell(&terminal, 2, start_row)
                .add_modifier
                .contains(Modifier::REVERSED),
            "the focused label uses the selection style"
        );
        let footer = &screen_rows[19];
        for hint in [
            "←/→", "bs/del", "tab", "j/k", "J/K", "enter", "esc", "ctrl+c",
        ] {
            assert!(footer.contains(hint), "footer misses {hint:?}: {footer:?}");
        }

        app.handle(Command::SwitchCorrectionField);
        let terminal = draw_at(&app, 60, 20);
        let rows = rows(&terminal);
        let end_row = rows.iter().position(|row| row.contains("End  : ")).unwrap() as u16;
        assert!(rows[end_row as usize].contains("1970-01-01 03:00▏"));
        assert!(
            cell(&terminal, 2, end_row)
                .add_modifier
                .contains(Modifier::REVERSED)
        );
    }
}
