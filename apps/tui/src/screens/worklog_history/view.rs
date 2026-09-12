use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph};

use crate::components::dialogs;
use crate::screens::worklog_history::{
    CorrectionDraft, CorrectionField, HistoryAvailability, WorklogHistoryMode, WorklogHistoryState,
};
use crate::styles;
use crate::support::timestamps::TimestampInput;

/// The immutable text needed to render one history row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) start: String,
    pub(crate) end: Option<String>,
    pub(crate) duration: String,
}

/// The immutable text needed by the deletion dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Deletion {
    pub(crate) start: String,
    pub(crate) end: String,
}

/// Renders history rows and the modal owned by its current mode.
pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    state: &WorklogHistoryState,
    task_name: &str,
    rows: &[Row],
    deletion: Option<&Deletion>,
) {
    render_body(frame, area, state, task_name, rows);
    match state.mode() {
        WorklogHistoryMode::Correction(draft) => render_correction_modal(frame, area, draft),
        WorklogHistoryMode::ConfirmDeletion { .. } => {
            if let Some(deletion) = deletion {
                render_delete_modal(frame, area, deletion);
            }
        }
        WorklogHistoryMode::Normal => {}
    }
}

fn render_body(
    frame: &mut Frame,
    area: Rect,
    state: &WorklogHistoryState,
    task_name: &str,
    rows: &[Row],
) {
    let history = state.history();
    let block = Block::bordered()
        .title(format!("Worklog history · {task_name}"))
        .border_style(if matches!(state.mode(), WorklogHistoryMode::Normal) {
            styles::focused_border()
        } else {
            Style::default()
        });
    if history.availability() == HistoryAvailability::Unavailable {
        frame.render_widget(
            Paragraph::new("History unavailable. Press r to retry.").block(block),
            area,
        );
        return;
    }
    if history.worklogs().is_empty() {
        let message = if history.next_cursor().is_some() {
            "No loaded worklogs. Older worklogs remain. Press o to load them."
        } else {
            "No worklogs yet."
        };
        frame.render_widget(Paragraph::new(message).block(block), area);
        return;
    }

    let items: Vec<ListItem> = rows
        .iter()
        .map(|row| {
            let end = match &row.end {
                Some(end) => Span::raw(end),
                None => Span::styled("Running", styles::active_marker()),
            };
            ListItem::new(vec![
                Line::from(vec![Span::raw(row.start.as_str()), Span::raw(" → "), end]),
                Line::from(vec![Span::raw("  "), Span::raw(row.duration.as_str())]),
            ])
        })
        .collect();
    let list = List::new(items)
        .block(block)
        .highlight_style(styles::selected());
    let mut list_state = ListState::default().with_selected(history.selected_index());
    frame.render_stateful_widget(list, area, &mut list_state);
}

fn render_correction_modal(frame: &mut Frame, area: Rect, draft: &CorrectionDraft) {
    let height = if draft.end().is_some() { 4 } else { 3 };
    let modal = dialogs::centered(58, height, area);
    frame.render_widget(ratatui::widgets::Clear, modal);
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
    input: &'a TimestampInput,
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

fn render_delete_modal(frame: &mut Frame, area: Rect, deletion: &Deletion) {
    let lines = vec![
        Line::from("Delete this worklog permanently?"),
        Line::from(format!("{} → {}", deletion.start, deletion.end)),
        Line::from("This cannot be undone."),
    ];
    dialogs::render_lines(frame, area, 58, 6, "Delete worklog", lines);
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chrono::{DateTime, Utc};
    use ratatui::style::{Modifier, Style};
    use ratatui::{Terminal, backend::TestBackend};
    use tracker_application::TrackerApplication;
    use tracker_domain::{Task, TaskId, TaskName, Worklog, WorklogId};
    use tracker_storage::SqliteRepository;

    use crate::app::App;
    use crate::command::Command;
    use crate::test_support::{app_in_timezone, app_with_test_clock};

    const WIDTH: u16 = 80;
    const HEIGHT: u16 = 24;

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
                task.id(),
                DateTime::<Utc>::from_timestamp(*start, 0).unwrap(),
                Some(DateTime::<Utc>::from_timestamp(*end, 0).unwrap()),
            )
            .unwrap();
            repository.insert_worklog(&worklog).unwrap();
        }
        let mut app = app_in_timezone(
            TrackerApplication::load(repository).unwrap(),
            chrono_tz::Africa::Johannesburg,
        );
        app.handle(Command::OpenHistory);
        app
    }

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

    fn app_with_test_clock_for_view(
        names: &[&str],
    ) -> (
        App<TrackerApplication<SqliteRepository>>,
        crate::app::TestClock,
    ) {
        let repository = SqliteRepository::open_in_memory().unwrap();
        for name in names {
            repository
                .create_task(Task::create(
                    TaskId::generate(),
                    TaskName::new(name).unwrap(),
                    DateTime::<Utc>::from_timestamp(100, 0).unwrap(),
                ))
                .unwrap();
        }
        app_with_test_clock(
            TrackerApplication::load(repository).unwrap(),
            chrono_tz::UTC,
            Utc::now(),
        )
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
        terminal
            .draw(|frame| crate::ui::render(frame, app.app_view()))
            .unwrap();
        terminal
    }

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
    fn the_deletion_modal_shows_local_times_and_permanence_at_sixty_columns() {
        let mut app = history_app(&[(3_600, 3_615)]);
        app.handle(Command::OpenDeletion);
        let terminal = draw_at(&app, 60, 20);
        let screen = rows(&terminal).join("\n");
        assert!(screen.contains("Delete worklog"), "got {screen:?}");
        assert!(
            screen.contains("Delete this worklog permanently?"),
            "got {screen:?}"
        );
        assert!(
            screen.contains("1970-01-01 03:00 → 1970-01-01 03:00"),
            "got {screen:?}"
        );
        assert!(screen.contains("This cannot be undone."), "got {screen:?}");
        assert!(screen.contains("d/y/enter delete"), "got {screen:?}");
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
        assert!(rows[23].contains("d delete"), "got {:?}", rows[23]);
        assert!(rows[23].contains("r refresh"), "got {:?}", rows[23]);
        assert!(rows[23].contains("esc back"), "got {:?}", rows[23]);
        assert!(
            !rows[23].contains("space track"),
            "no task-list hint leaks in"
        );
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
        let (mut app, clock) = app_with_test_clock_for_view(&["alpha"]);
        app.handle(Command::ToggleTracking);
        clock.advance_monotonic(Duration::from_secs(125));
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
    fn an_empty_loaded_history_prompts_for_older_worklogs() {
        let entries: Vec<(i64, i64)> = (0..51)
            .map(|index| (3_600 + index * 60, 3_615 + index * 60))
            .collect();
        let mut app = history_app(&entries);
        for _ in 0..50 {
            app.handle(Command::OpenDeletion);
            app.handle(Command::Confirm);
        }

        assert!(app.app_view().history().unwrap().worklogs().is_empty());
        assert!(app.app_view().history().unwrap().next_cursor().is_some());
        let terminal = draw(&app);
        assert_eq!(
            row(&terminal, 2).trim_matches(['│', ' ']),
            "No loaded worklogs. Older worklogs remain. Press o to load them."
        );
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
