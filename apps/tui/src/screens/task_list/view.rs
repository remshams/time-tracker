use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph, Tabs};
use tracker_domain::{Task, TaskId};

use crate::components::{dialogs, text};
use crate::screens::task_list::{InputPurpose, TaskListMode, TaskListState, TaskView};
use crate::styles;

/// Renders the task list and the modal owned by its current mode.
pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    state: &TaskListState,
    tasks: &[&Task],
    ordering_label: &str,
    active_task_id: Option<TaskId>,
) {
    render_body(frame, area, state, tasks, ordering_label, active_task_id);
    render_tabs(
        frame,
        area,
        usize::from(state.view() == TaskView::Archived),
        matches!(state.mode(), TaskListMode::Normal),
    );
    match state.mode() {
        TaskListMode::Input { purpose, buffer } => {
            let prompt = match purpose {
                InputPurpose::Add => "New task name:",
                InputPurpose::Rename { .. } => "Rename task:",
            };
            dialogs::render_input(frame, area, prompt, buffer);
        }
        TaskListMode::ConfirmArchive { name, .. } => {
            dialogs::render_confirmation(
                frame,
                area,
                "Confirm archive",
                &format!("Archive \"{name}\"?"),
            );
        }
        TaskListMode::Normal | TaskListMode::Search => {}
    }
}

pub(crate) fn render_tabs(frame: &mut Frame, area: Rect, selected: usize, focused: bool) {
    const LABELS: [&str; 4] = ["Active", "Archived", "Worklogs", "Reports"];
    let tabs = Tabs::new(LABELS)
        .select(selected)
        .highlight_style(if focused {
            styles::selected()
        } else {
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED)
        })
        .divider("│");
    frame.render_widget(
        tabs,
        Rect {
            x: area.x.saturating_add(1),
            y: area.y,
            width: area.width.saturating_sub(2),
            height: 1,
        },
    );
}

fn render_body(
    frame: &mut Frame,
    area: Rect,
    state: &TaskListState,
    tasks: &[&Task],
    ordering_label: &str,
    active_task_id: Option<TaskId>,
) {
    let empty_text = match state.view() {
        TaskView::Active => "No active tasks. Press a to add one.",
        TaskView::Archived => "No archived tasks.",
    };
    let displayed_order = if state.search_query().is_some() {
        "latest activity"
    } else {
        ordering_label
    };
    let block = Block::bordered()
        .title_bottom(Line::from(format!(" Sort: {displayed_order} ")).right_aligned())
        .border_style(
            if matches!(state.mode(), TaskListMode::Normal | TaskListMode::Search) {
                styles::focused_border()
            } else {
                Style::default()
            },
        );
    let list_area = if let Some(query) = state.search_query() {
        frame.render_widget(&block, area);
        let inner = area.inner(Margin {
            horizontal: 1,
            vertical: 1,
        });
        let query_width = (inner.width as usize).saturating_sub(9);
        let visible_query = text::fit_suffix(query, query_width);
        let mut spans = vec![Span::raw("Search: "), Span::raw(visible_query.to_owned())];
        if matches!(state.mode(), TaskListMode::Search) {
            spans.push(Span::styled("▏", styles::input_cursor()));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), inner);
        Rect {
            y: inner.y.saturating_add(1),
            height: inner.height.saturating_sub(1),
            ..inner
        }
    } else {
        area
    };
    if tasks.is_empty() {
        let message = if state.search_query().is_some() {
            "No matching tasks."
        } else {
            empty_text
        };
        if state.search_query().is_some() {
            frame.render_widget(Paragraph::new(message), list_area);
        } else {
            frame.render_widget(Paragraph::new(message).block(block), list_area);
        }
        return;
    }

    // Two border cells and the two-cell marker leave this much for a name.
    let name_budget = (list_area.width as usize)
        .saturating_sub(if state.search_query().is_some() { 2 } else { 4 });
    let items: Vec<ListItem> = tasks
        .iter()
        .map(|task| {
            let active = active_task_id == Some(task.id());
            let marker = if active {
                Span::styled("▶ ", styles::active_marker())
            } else {
                Span::raw("  ")
            };
            let name = text::fit_prefix(task.name().as_str(), name_budget);
            ListItem::new(Line::from(vec![marker, Span::raw(name)]))
        })
        .collect();
    let mut list = List::new(items).highlight_style(styles::selected());
    if state.search_query().is_none() {
        list = list.block(block);
    }
    let selected = state
        .selection()
        .and_then(|id| tasks.iter().position(|task| task.id() == id));
    let mut list_state = ListState::default().with_selected(selected);
    frame.render_stateful_widget(list, list_area, &mut list_state);
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chrono::{DateTime, Utc};
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::{Terminal, backend::TestBackend};
    use tracker_application::TrackerApplication;
    use tracker_domain::{Task, TaskId, TaskName};
    use tracker_storage::SqliteRepository;

    use crate::app::App;
    use crate::command::Command;
    use crate::screens::TaskListCommand;
    use crate::test_support::app_with_test_clock;

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
    fn the_seeded_list_renders_with_title_selection_and_help() {
        let app = app_with(&["alpha", "beta"]);
        let terminal = draw(&app);
        let rows = rows(&terminal);

        assert!(rows[0].contains("Time Tracker"));
        assert!(rows[1].contains(" Active │ Archived │ Worklogs │ Reports "));
        assert!(rows[21].contains("Sort: recently worked"));
        assert!(rows[2].contains("alpha"));
        assert!(rows[3].contains("beta"));
        assert!(rows[22].trim().is_empty());
        assert!(rows[23].contains("enter history"));
        assert!(rows[23].contains("s sort"));
        assert!(rows[23].contains("a/e/d edit"));
        assert!(rows[23].contains("ctrl+c quit"));
        assert_eq!(cell(&terminal, 0, 0).fg, Some(Color::Blue));
        assert_eq!(cell(&terminal, 0, 1).fg, Some(Color::Blue));
    }

    #[test]
    fn the_selected_task_tab_is_marked_in_both_views() {
        let mut app = app_with(&["alpha"]);
        let terminal = draw_at(&app, 60, 20);
        assert!(row(&terminal, 1).contains(" Active │ Archived "));
        assert_eq!(terminal.backend().buffer()[(45, 1)].symbol(), "─");
        assert!(
            cell(&terminal, 2, 1)
                .add_modifier
                .contains(Modifier::REVERSED)
        );
        assert!(
            !cell(&terminal, 11, 1)
                .add_modifier
                .contains(Modifier::REVERSED)
        );

        app.handle(Command::TaskList(TaskListCommand::ShowArchivedTasks));
        let terminal = draw_at(&app, 60, 20);
        assert!(row(&terminal, 1).contains(" Active │ Archived "));
        assert!(
            !cell(&terminal, 2, 1)
                .add_modifier
                .contains(Modifier::REVERSED)
        );
        assert!(
            cell(&terminal, 11, 1)
                .add_modifier
                .contains(Modifier::REVERSED)
        );
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

        app.handle(crate::command::Command::TaskList(TaskListCommand::MoveDown));
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
        let (mut app, clock) = app_with_test_clock_for_view(&["alpha", "beta"]);
        app.handle(Command::TaskList(TaskListCommand::ToggleTracking));
        clock.advance_monotonic(Duration::from_secs(125));
        app.handle(Command::TaskList(TaskListCommand::MoveDown));
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
        app.handle(Command::TaskList(TaskListCommand::ToggleTracking));
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

        app.handle(Command::TaskList(TaskListCommand::OpenAdd));
        let terminal = draw(&app);
        assert_eq!(cell(&terminal, 0, 1).fg, Some(Color::Reset));
        assert_eq!(cell(&terminal, 12, 10).fg, Some(Color::Blue));

        app.handle(Command::TaskList(TaskListCommand::Cancel));
        app.handle(Command::TaskList(TaskListCommand::OpenArchiveConfirm));
        let terminal = draw(&app);
        assert_eq!(cell(&terminal, 0, 1).fg, Some(Color::Reset));
        assert_eq!(cell(&terminal, 12, 10).fg, Some(Color::Blue));
    }

    #[test]
    fn the_footer_matches_each_mode() {
        let mut app = app_with(&["alpha"]);
        let terminal = draw(&app);
        assert!(row(&terminal, 23).contains("tab/⇧tab"));
        assert!(row(&terminal, 23).contains("space track"));
        assert!(row(&terminal, 23).contains("enter history"));

        app.handle(Command::TaskList(TaskListCommand::OpenAdd));
        let terminal = draw(&app);
        assert!(row(&terminal, 23).contains("enter save"));

        app.handle(Command::TaskList(TaskListCommand::Cancel));
        app.handle(Command::TaskList(TaskListCommand::OpenArchiveConfirm));
        let terminal = draw(&app);
        assert!(row(&terminal, 23).contains("y/enter confirm"));
    }

    #[test]
    fn narrow_views_keep_sort_labels_and_complete_compact_footers() {
        let mut app = app_with(&["alpha"]);
        let terminal = draw_at(&app, 60, 20);
        assert!(row(&terminal, 1).contains(" Active │ Archived "));
        assert!(row(&terminal, 17).contains("Sort: recently worked"));
        let footer = row(&terminal, 19);
        assert!(footer.contains("enter history"), "got {footer:?}");
        assert!(footer.contains("s sort"), "got {footer:?}");
        assert!(footer.contains("q/esc/ctrl+c"), "got {footer:?}");

        app.handle(Command::TaskList(TaskListCommand::ShowArchivedTasks));
        app.handle(Command::TaskList(TaskListCommand::CycleOrdering));
        let terminal = draw_at(&app, 60, 20);
        assert!(row(&terminal, 1).contains(" Active │ Archived "));
        assert!(row(&terminal, 17).contains("Sort: recently updated"));
        let footer = row(&terminal, 19);
        assert!(footer.contains("enter history"), "got {footer:?}");
        assert!(footer.contains("u restore"), "got {footer:?}");
        assert!(footer.contains("s sort"), "got {footer:?}");
        assert!(footer.contains("q/esc/ctrl+c"), "got {footer:?}");
    }

    #[test]
    fn search_results_leave_the_bottom_border_intact_in_a_short_terminal() {
        let mut app = app_with(&["First task", "Second task", "Third task", "Fourth task"]);
        app.handle(Command::TaskList(TaskListCommand::OpenSearch));
        let terminal = draw_at(&app, 60, 8);
        assert!(row(&terminal, 2).contains("Search: "));
        assert_eq!(
            terminal.backend().buffer()[(3, 5)].symbol(),
            "─",
            "search results must stay inside the panel"
        );
        assert!(row(&terminal, 5).ends_with(" Sort: latest activity ┘"));
    }

    #[test]
    fn task_panel_bottom_border_shows_the_shared_ordering_in_both_views() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::TaskList(TaskListCommand::CycleOrdering));
        let terminal = draw(&app);
        assert!(
            row(&terminal, 21).ends_with(" Sort: recently updated ┘"),
            "got {:?}",
            row(&terminal, 21)
        );

        app.handle(Command::TaskList(TaskListCommand::ShowArchivedTasks));
        let terminal = draw(&app);
        assert!(
            row(&terminal, 21).ends_with(" Sort: recently updated ┘"),
            "got {:?}",
            row(&terminal, 21)
        );
    }

    #[test]
    fn an_empty_active_list_renders_a_hint_and_stays_selectable() {
        let app = App::load(
            TrackerApplication::load(SqliteRepository::open_in_memory().unwrap()).unwrap(),
        );
        let terminal = draw(&app);
        assert!(row(&terminal, 2).contains("No active tasks. Press a to add one."));
        assert!(row(&terminal, 1).contains(" Active │ Archived "));
    }

    #[test]
    fn the_archived_view_renders_its_title_rows_and_footer() {
        let mut app = app_with(&["alpha", "beta"]);
        app.handle(Command::TaskList(TaskListCommand::OpenArchiveConfirm));
        app.handle(Command::TaskList(TaskListCommand::Confirm));
        app.handle(Command::TaskList(TaskListCommand::ShowArchivedTasks));
        let terminal = draw(&app);
        let rows = rows(&terminal);

        assert!(rows[1].contains(" Active │ Archived "), "got {:?}", rows[1]);
        assert!(rows[2].contains("alpha"), "got {:?}", rows[2]);
        assert!(!rows[3].contains("beta"), "beta is still active");
        assert!(rows[23].contains("u unarchive"), "got {:?}", rows[23]);
        assert!(rows[23].contains("tab/⇧tab"));
        assert!(rows[23].contains("enter history"));
        assert!(
            !rows[23].contains("a add"),
            "archived view must not hint add"
        );
    }

    #[test]
    fn an_empty_archived_view_renders_its_own_empty_text() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::TaskList(TaskListCommand::ShowArchivedTasks));
        let terminal = draw(&app);
        assert!(row(&terminal, 2).contains("No archived tasks."));
        assert!(row(&terminal, 1).contains(" Active │ Archived "));
    }

    #[test]
    fn the_archived_view_keeps_the_timer_header_and_selection() {
        let (mut app, clock) = app_with_test_clock_for_view(&["alpha", "beta"]);
        app.handle(Command::TaskList(TaskListCommand::ToggleTracking));
        app.handle(Command::TaskList(TaskListCommand::MoveDown));
        app.handle(Command::TaskList(TaskListCommand::OpenArchiveConfirm));
        app.handle(Command::TaskList(TaskListCommand::Confirm));
        app.handle(Command::TaskList(TaskListCommand::ShowArchivedTasks));
        clock.advance_monotonic(Duration::from_secs(61));
        let terminal = draw(&app);
        let rows = rows(&terminal);

        // The active timer outlives the view switch.
        assert!(rows[0].contains("▶ alpha"), "got {:?}", rows[0]);
        assert!(rows[0].contains("00:01:01"), "got {:?}", rows[0]);
        assert!(rows[1].contains(" Active │ Archived "));
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
        app.handle(Command::TaskList(TaskListCommand::OpenAdd));
        app.handle(Command::TaskList(TaskListCommand::Insert('a')));
        app.handle(Command::TaskList(TaskListCommand::Insert('b')));
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
        app.handle(Command::TaskList(TaskListCommand::OpenAdd));
        // Longer than the visible budget: the head scrolls out of view.
        for character in "a".repeat(80).chars() {
            app.handle(Command::TaskList(TaskListCommand::Insert(character)));
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
        app.handle(Command::TaskList(TaskListCommand::OpenAdd));
        // 30 clock symbols are 60 columns, wider than the budget of 38.
        for character in "🕒".repeat(30).chars() {
            app.handle(Command::TaskList(TaskListCommand::Insert(character)));
        }
        let terminal = draw(&app);
        let row_text = row(&terminal, 11);
        // 19 symbols fill the 38-column budget exactly; the wide glyphs
        // occupy their own cells, so count them.
        assert_eq!(row_text.matches('🕒').count(), 19, "got {row_text:?}");
        assert!(row_text.contains('▏'), "the cursor must stay visible");
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
        app.handle(Command::TaskList(TaskListCommand::OpenRename));
        let terminal = draw(&app);
        assert!(row(&terminal, 11).contains("Rename task: alpha"));
    }

    #[test]
    fn the_confirm_modal_shows_the_task_name() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::TaskList(TaskListCommand::OpenArchiveConfirm));
        let terminal = draw(&app);
        let row = row(&terminal, 11);
        assert!(row.contains("Archive \"alpha\"?"), "got {row:?}");
    }
}
