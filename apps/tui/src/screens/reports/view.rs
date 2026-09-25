use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::components::text;
use crate::screens::reports::{ReportMode, ReportPreset, ReportState};
use crate::screens::task_list::view::render_tabs;
use crate::styles;

use super::actions::format_exact;

pub(crate) fn render(frame: &mut Frame, area: Rect, state: &ReportState) {
    let date_text = if state.from == state.to {
        state.from.to_string()
    } else {
        format!("{} to {}", state.from, state.to)
    };
    let title = format!(" Period: {} · {} ", state.period_label(), date_text);
    let total = state
        .totals
        .as_ref()
        .map_or_else(|| "loading".to_owned(), |totals| format_exact(totals.total));
    let block = Block::bordered()
        .title_bottom(Line::from(format!(" Total: {total} ")).right_aligned())
        .border_style(if state.mode == ReportMode::Normal {
            styles::focused_border()
        } else {
            Style::default()
        });
    let rows = state
        .totals
        .as_ref()
        .map_or(&[][..], |totals| totals.rows.as_slice());
    frame.render_widget(block, area);
    let content = Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };
    frame.render_widget(Paragraph::new(title), content);
    let mut preset_spans = Vec::new();
    for (index, preset) in ReportPreset::ALL.iter().enumerate() {
        if index > 0 {
            preset_spans.push(Span::raw(" │ "));
        }
        preset_spans.push(Span::styled(
            preset.label(),
            if Some(*preset) == state.highlighted_preset() {
                styles::selected()
            } else {
                Style::default()
            },
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(preset_spans)),
        Rect {
            y: content.y.saturating_add(1),
            ..content
        },
    );
    let list_area = Rect {
        y: content.y.saturating_add(2),
        height: content.height.saturating_sub(2),
        ..content
    };
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
        let mut selection = ListState::default().with_selected(state.selected_index());
        frame.render_stateful_widget(list, list_area, &mut selection);
    }
    render_tabs(frame, area, 2);
    match &state.mode {
        ReportMode::Presets { selected } => render_presets(frame, area, *selected),
        ReportMode::Custom { from, to, focus_to } => {
            render_custom(frame, area, from, to, *focus_to)
        }
        ReportMode::Normal => {}
    }
}

fn render_presets(frame: &mut Frame, area: Rect, selected: usize) {
    let modal = area.centered(Constraint::Length(32), Constraint::Length(8));
    frame.render_widget(Clear, modal);
    let items: Vec<ListItem> = ReportPreset::ALL
        .iter()
        .map(|preset| ListItem::new(preset.label()))
        .collect();
    let list = List::new(items)
        .block(
            Block::bordered()
                .title("Choose period")
                .border_style(styles::focused_border()),
        )
        .highlight_style(styles::selected());
    let mut selection = ListState::default().with_selected(Some(selected));
    frame.render_stateful_widget(list, modal, &mut selection);
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
        assert_eq!(duration_x(3), duration_x(4));
        assert!(duration_x(3) >= 50);
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
        assert_eq!(buffer[(1, 2)].symbol(), "T");
        assert_eq!(
            (1..59).filter(|&x| buffer[(x, 2)].symbol() == "│").count(),
            5
        );
        assert!(buffer[(1, 2)].modifier.contains(Modifier::REVERSED));
        assert_eq!(buffer[(0, 3)].fg, Color::Blue);
        assert_eq!(buffer[(1, 9)].symbol(), "─");

        state.mode = ReportMode::Presets { selected: 0 };
        terminal
            .draw(|frame| render(frame, frame.area(), &state))
            .unwrap();
        assert_ne!(terminal.backend().buffer()[(0, 3)].fg, Color::Blue);
    }
}
