use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::components::text;
use crate::screens::reports::{ReportFocus, ReportMode, ReportPreset, ReportState};
use crate::screens::task_list::view::render_tabs;
use crate::styles;

use super::actions::format_exact;

pub(crate) fn render(frame: &mut Frame, area: Rect, state: &ReportState) {
    let total = state
        .totals
        .as_ref()
        .map_or_else(|| "loading".to_owned(), |totals| format_exact(totals.total));
    let block = Block::bordered()
        .title_bottom(Line::from(format!(" Total: {total} ")).right_aligned())
        .border_style(
            if state.mode == ReportMode::Normal && state.focus == ReportFocus::Rows {
                styles::focused_border()
            } else {
                Style::default()
            },
        );
    frame.render_widget(block, area);
    let content = report_content(area);
    let (show_title, preset_offset, list_offset) = match content.height {
        0 | 1 => (false, None, 0),
        2 => (false, Some(0), 1),
        3 => (true, Some(1), 2),
        _ => (true, Some(2), 3),
    };
    if show_title {
        frame.render_widget(Paragraph::new(report_title(state)), content);
    }
    if let Some(offset) = preset_offset {
        frame.render_widget(
            Paragraph::new(Line::from(preset_spans(state))),
            Rect {
                y: content.y.saturating_add(offset),
                ..content
            },
        );
    }
    let list_area = Rect {
        y: content.y.saturating_add(list_offset),
        height: content.height.saturating_sub(list_offset),
        ..content
    };
    render_rows(frame, area, list_area, state);
    render_tabs(frame, area, 3, state.focus == ReportFocus::TopTabs);
    match &state.mode {
        ReportMode::Custom { from, to, focus_to } => {
            render_custom(frame, area, from, to, *focus_to)
        }
        ReportMode::Normal => {}
    }
}

fn report_title(state: &ReportState) -> String {
    let date_text = if state.from == state.to {
        state.from.to_string()
    } else {
        format!("{} to {}", state.from, state.to)
    };
    format!(" Period: {} · {} ", state.period_label(), date_text)
}

fn report_content(area: Rect) -> Rect {
    let content = Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };
    if content.height >= 5 {
        Rect {
            y: content.y.saturating_add(1),
            height: content.height - 1,
            ..content
        }
    } else {
        content
    }
}

fn preset_spans(state: &ReportState) -> Vec<Span<'static>> {
    let mut preset_spans = Vec::new();
    for (index, preset) in ReportPreset::ALL.iter().enumerate() {
        if index > 0 {
            preset_spans.push(Span::raw(" │ "));
        }
        let mut style = if state.mode == ReportMode::Normal
            && state.focus == ReportFocus::Presets
            && index == state.preset_cursor
        {
            styles::selected()
        } else {
            Style::default()
        };
        if Some(*preset) == state.highlighted_preset() {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        preset_spans.push(Span::styled(preset.label(), style));
    }
    preset_spans
}

fn render_rows(frame: &mut Frame, area: Rect, list_area: Rect, state: &ReportState) {
    let rows = state
        .totals
        .as_ref()
        .map_or(&[][..], |totals| totals.rows.as_slice());
    if rows.is_empty() {
        frame.render_widget(Paragraph::new("No tracked time in this period."), list_area);
    } else {
        let duration_width = rows
            .iter()
            .map(|row| format_exact(row.duration).len())
            .max()
            .unwrap_or(0);
        let name_width = (area.width as usize).saturating_sub(duration_width + 6);
        let items: Vec<ListItem> = rows
            .iter()
            .map(|row| {
                let duration = format_exact(row.duration);
                let name = text::fit_prefix(row.task.name().as_str(), name_width);
                let padding = " ".repeat(name_width.saturating_sub(name.width()));
                ListItem::new(Line::from(vec![
                    Span::raw(name),
                    Span::raw(padding),
                    Span::raw("  "),
                    Span::raw(duration),
                ]))
            })
            .collect();
        let list = List::new(items).highlight_style(styles::selected());
        let mut selection = ListState::default().with_selected(
            (state.focus == ReportFocus::Rows)
                .then(|| state.selected_index())
                .flatten(),
        );
        frame.render_stateful_widget(list, list_area, &mut selection);
    }
}

fn render_custom(frame: &mut Frame, area: Rect, from: &str, to: &str, focus_to: bool) {
    let modal = area.centered(Constraint::Length(38), Constraint::Length(4));
    frame.render_widget(Clear, modal);
    let from_line = Line::from(vec![
        Span::styled(
            "From: ",
            if focus_to {
                Style::default()
            } else {
                styles::selected()
            },
        ),
        Span::raw(from.to_owned()),
        Span::raw(if focus_to { "" } else { "▏" }),
    ]);
    let to_line = Line::from(vec![
        Span::styled(
            "To:   ",
            if focus_to {
                styles::selected()
            } else {
                Style::default()
            },
        ),
        Span::raw(to.to_owned()),
        Span::raw(if focus_to { "▏" } else { "" }),
    ]);
    frame.render_widget(
        Paragraph::new(vec![from_line, to_line]).block(
            Block::bordered()
                .title("Custom period")
                .border_style(styles::focused_border()),
        ),
        modal,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::task;
    use chrono::{TimeDelta, Utc};
    use ratatui::style::{Color, Modifier};
    use ratatui::{Terminal, backend::TestBackend};
    use tracker_application::{ReportRow, ReportTotals};

    #[test]
    fn wide_task_names_keep_durations_in_one_display_column() {
        let mut state = ReportState::new(Utc::now(), chrono_tz::UTC);
        state.set_totals(ReportTotals {
            rows: vec![
                ReportRow {
                    task: task(1, "plain"),
                    duration: TimeDelta::minutes(15),
                },
                ReportRow {
                    task: task(2, "🕒 clock"),
                    duration: TimeDelta::minutes(15),
                },
            ],
            total: TimeDelta::minutes(30),
        });
        let mut terminal = Terminal::new(TestBackend::new(60, 10)).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), &state))
            .unwrap();
        let duration_x = |y| {
            (0..60)
                .find(|&x| terminal.backend().buffer()[(x, y)].symbol() == "1")
                .unwrap()
        };
        assert_eq!(duration_x(5), duration_x(6));
        assert!(duration_x(5) >= 50);
    }

    #[test]
    fn period_row_and_list_stay_inside_the_report_border() {
        let mut state = ReportState::new(Utc::now(), chrono_tz::UTC);
        state.set_totals(ReportTotals {
            rows: (1..=20)
                .map(|id| ReportRow {
                    task: task(id, &format!("task {id}")),
                    duration: TimeDelta::minutes(15),
                })
                .collect(),
            total: TimeDelta::minutes(300),
        });
        let mut terminal = Terminal::new(TestBackend::new(60, 10)).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), &state))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert!(buffer[(33, 0)].modifier.contains(Modifier::REVERSED));
        assert!(!buffer[(22, 0)].modifier.contains(Modifier::REVERSED));
        assert_eq!(buffer[(1, 1)].symbol(), " ");
        assert_eq!(buffer[(2, 2)].symbol(), "P");
        assert_eq!(buffer[(1, 3)].symbol(), " ");
        assert_eq!(buffer[(1, 4)].symbol(), "T");
        assert_eq!(
            (1..59).filter(|&x| buffer[(x, 4)].symbol() == "│").count(),
            5
        );
        assert!(buffer[(1, 4)].modifier.contains(Modifier::UNDERLINED));
        assert_ne!(buffer[(0, 3)].fg, Color::Blue);
        assert_eq!(buffer[(1, 9)].symbol(), "─");

        state.focus = ReportFocus::Presets;
        terminal
            .draw(|frame| render(frame, frame.area(), &state))
            .unwrap();
        assert!(
            terminal.backend().buffer()[(1, 4)]
                .modifier
                .contains(Modifier::REVERSED | Modifier::UNDERLINED)
        );
        state.preset_cursor = 1;
        terminal
            .draw(|frame| render(frame, frame.area(), &state))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert!(buffer[(1, 4)].modifier.contains(Modifier::UNDERLINED));
        assert!(!buffer[(1, 4)].modifier.contains(Modifier::REVERSED));
        assert!(buffer[(9, 4)].modifier.contains(Modifier::REVERSED));
        assert!(!buffer[(9, 4)].modifier.contains(Modifier::UNDERLINED));

        state.focus = ReportFocus::Rows;
        let mut compact = Terminal::new(TestBackend::new(60, 5)).unwrap();
        compact
            .draw(|frame| render(frame, frame.area(), &state))
            .unwrap();
        let compact_buffer = compact.backend().buffer();
        assert_eq!(compact_buffer[(1, 3)].symbol(), "t");
        assert!(compact_buffer[(1, 3)].modifier.contains(Modifier::REVERSED));
        assert_eq!(compact_buffer[(0, 3)].fg, Color::Blue);

        for (height, selected_y) in [(3, 1), (4, 2)] {
            let mut tiny = Terminal::new(TestBackend::new(60, height)).unwrap();
            tiny.draw(|frame| render(frame, frame.area(), &state))
                .unwrap();
            let cell = &tiny.backend().buffer()[(1, selected_y)];
            assert_eq!(cell.symbol(), "t");
            assert!(cell.modifier.contains(Modifier::REVERSED));
        }
        for (height, selected_y) in [(6, 4), (7, 5)] {
            let mut short = Terminal::new(TestBackend::new(60, height)).unwrap();
            short
                .draw(|frame| render(frame, frame.area(), &state))
                .unwrap();
            assert_eq!(short.backend().buffer()[(1, selected_y)].symbol(), "t");
        }
    }

    #[test]
    fn custom_date_dialog_owns_the_keyboard_focus() {
        let mut state = ReportState::new(Utc::now(), chrono_tz::UTC);
        state.focus = ReportFocus::Presets;
        state.mode = ReportMode::Custom {
            from: "2025-04-15".into(),
            to: "2025-04-16".into(),
            focus_to: false,
        };
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), &state))
            .unwrap();
        let today = &terminal.backend().buffer()[(1, 4)];
        assert!(today.modifier.contains(Modifier::UNDERLINED));
        assert!(!today.modifier.contains(Modifier::REVERSED));
    }
}
