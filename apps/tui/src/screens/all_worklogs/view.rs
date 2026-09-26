use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::app::AppView;
use crate::components::text;
use crate::screens::task_list::view::render_tabs;
use crate::screens::worklog_history::view::render_move_modal;
use crate::styles;

use super::{AllWorklogsFocus, AllWorklogsState};

pub(crate) fn render(frame: &mut Frame, area: Rect, app: AppView<'_>, state: &AllWorklogsState) {
    let block = Block::bordered().border_style(
        if state.focus == AllWorklogsFocus::Rows && state.move_draft.is_none() {
            styles::focused_border()
        } else {
            Style::default()
        },
    );
    if !state.available {
        frame.render_widget(
            Paragraph::new("Worklogs unavailable. Press r to retry.").block(block),
            area,
        );
    } else if state.worklogs().is_empty() {
        frame.render_widget(Paragraph::new("No worklogs yet.").block(block), area);
    } else {
        let items = state
            .worklogs()
            .iter()
            .map(|worklog| {
                let name = app.task_name(worklog.task_id()).unwrap_or("unknown task");
                let start = app.local_time(worklog.start());
                let end_text = worklog
                    .end()
                    .map_or_else(|| "Running".to_owned(), |end| app.local_time(end));
                let name_width =
                    (area.width as usize).saturating_sub(2 + start.width() + end_text.width() + 6);
                let visible_name = if name.width() > name_width {
                    format!("{}…", text::fit_prefix(name, name_width.saturating_sub(1)))
                } else {
                    name.to_owned()
                };
                let end = if worklog.end().is_some() {
                    Span::raw(end_text)
                } else {
                    Span::styled(end_text, styles::active_marker())
                };
                ListItem::new(vec![
                    Line::from(vec![
                        Span::raw(visible_name),
                        Span::raw(" · "),
                        Span::raw(start),
                        Span::raw(" → "),
                        end,
                    ]),
                    Line::from(format!(
                        "  {}",
                        text::format_elapsed(app.history_row_duration(worklog))
                    )),
                ])
            })
            .collect::<Vec<_>>();
        let list = List::new(items)
            .block(block)
            .highlight_style(styles::selected());
        let mut selection = ListState::default().with_selected(
            (state.focus == AllWorklogsFocus::Rows && state.move_draft.is_none())
                .then(|| state.selected_index())
                .flatten(),
        );
        frame.render_stateful_widget(list, area, &mut selection);
    }
    render_tabs(
        frame,
        area,
        2,
        state.focus == AllWorklogsFocus::Tabs && state.move_draft.is_none(),
    );
    if let Some(draft) = &state.move_draft {
        render_move_modal(frame, area, app, draft);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::{Color, Modifier};
    use ratatui::{Terminal, backend::TestBackend};
    use tracker_domain::{Worklog, WorklogId};

    use crate::command::Command;
    use crate::screens::{AllWorklogsCommand, TaskListCommand};
    use crate::test_support::{TestService, app_in_timezone, at, task};

    fn draw(app: &crate::app::App<TestService>) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        terminal
            .draw(|frame| crate::ui::render(frame, app.app_view()))
            .unwrap();
        terminal
    }

    fn row(terminal: &Terminal<TestBackend>, y: u16) -> String {
        terminal
            .backend()
            .buffer()
            .content
            .chunks(80)
            .nth(y as usize)
            .unwrap()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn an_empty_feed_has_the_global_tab_and_empty_hint() {
        let mut app = app_in_timezone(TestService::with_tasks(vec![]), chrono_tz::UTC);
        app.handle(Command::TaskList(TaskListCommand::ShowAllWorklogs));
        let terminal = draw(&app);
        assert!(row(&terminal, 1).contains(" Active │ Archived │ Worklogs │ Reports "));
        assert!(row(&terminal, 2).contains("No worklogs yet."));
    }

    #[test]
    fn archived_and_running_rows_include_their_task_names() {
        let mut archived = task(1, "archived source");
        archived.archive(at(100));
        let active = task(2, "active source");
        let mut service = TestService::with_tasks(vec![archived.clone(), active.clone()]);
        service.authoritative_worklogs = vec![
            Worklog::begin(WorklogId::generate(), active.id(), at(20)),
            Worklog::new(WorklogId::generate(), archived.id(), at(10), Some(at(15))).unwrap(),
        ];
        let mut app = app_in_timezone(service, chrono_tz::UTC);
        app.handle(Command::TaskList(TaskListCommand::ShowAllWorklogs));
        let terminal = draw(&app);
        assert!(row(&terminal, 2).contains("active source"));
        assert!(row(&terminal, 2).contains("Running"));
        assert!(row(&terminal, 4).contains("archived source"));
    }

    #[test]
    fn the_selected_row_is_highlighted_only_when_rows_have_focus() {
        let source = task(1, "source");
        let mut service = TestService::with_tasks(vec![source.clone()]);
        service.authoritative_worklogs =
            vec![Worklog::new(WorklogId::generate(), source.id(), at(10), Some(at(15))).unwrap()];
        let mut app = app_in_timezone(service, chrono_tz::UTC);
        app.handle(Command::TaskList(TaskListCommand::ShowAllWorklogs));
        let terminal = draw(&app);
        assert!(
            !terminal.backend().buffer()[(2, 2)]
                .style()
                .add_modifier
                .contains(Modifier::REVERSED)
        );

        app.handle(Command::AllWorklogs(AllWorklogsCommand::FocusRows));
        let terminal = draw(&app);
        assert!(
            terminal.backend().buffer()[(2, 2)]
                .style()
                .add_modifier
                .contains(Modifier::REVERSED)
        );
    }

    #[test]
    fn the_border_marks_row_focus_and_the_move_dialog_takes_focus() {
        let source = task(1, "source");
        let destination = task(2, "destination");
        let mut service = TestService::with_tasks(vec![source.clone(), destination]);
        service.authoritative_worklogs =
            vec![Worklog::new(WorklogId::generate(), source.id(), at(10), Some(at(15))).unwrap()];
        let mut app = app_in_timezone(service, chrono_tz::UTC);
        app.handle(Command::TaskList(TaskListCommand::ShowAllWorklogs));
        assert_ne!(draw(&app).backend().buffer()[(0, 3)].fg, Color::Blue);

        app.handle(Command::AllWorklogs(AllWorklogsCommand::FocusRows));
        assert_eq!(draw(&app).backend().buffer()[(0, 3)].fg, Color::Blue);

        app.handle(Command::AllWorklogs(AllWorklogsCommand::OpenMove));
        assert_ne!(draw(&app).backend().buffer()[(0, 3)].fg, Color::Blue);
    }

    #[test]
    fn a_task_name_that_exactly_fits_does_not_gain_an_ellipsis() {
        let probe = app_in_timezone(TestService::with_tasks(vec![]), chrono_tz::UTC);
        let start = probe.app_view().local_time(at(10));
        let end = probe.app_view().local_time(at(15));
        let name_width = 80 - 2 - start.len() - end.len() - 6;
        let source = task(1, &"N".repeat(name_width));
        let mut service = TestService::with_tasks(vec![source.clone()]);
        service.authoritative_worklogs =
            vec![Worklog::new(WorklogId::generate(), source.id(), at(10), Some(at(15))).unwrap()];
        let mut app = app_in_timezone(service, chrono_tz::UTC);
        app.handle(Command::TaskList(TaskListCommand::ShowAllWorklogs));
        let terminal = draw(&app);
        assert!(row(&terminal, 2).contains(&"N".repeat(name_width)));
        assert!(!row(&terminal, 2).contains('…'));
    }
}
