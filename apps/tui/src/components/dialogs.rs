use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::components::text::fit_suffix;
use crate::styles;

/// Renders a fixed-width one-line input dialog.
pub(crate) fn render_input(frame: &mut Frame, area: Rect, prompt: &str, buffer: &str) {
    let modal = area.centered(Constraint::Length(56), Constraint::Length(3));
    frame.render_widget(Clear, modal);
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

/// Renders a confirmation dialog with one message line.
pub(crate) fn render_confirmation(frame: &mut Frame, area: Rect, title: &str, message: &str) {
    let modal = area.centered(Constraint::Length(56), Constraint::Length(3));
    frame.render_widget(Clear, modal);
    frame.render_widget(
        Paragraph::new(message.to_owned()).block(
            Block::bordered()
                .title(title)
                .border_style(styles::focused_border()),
        ),
        modal,
    );
}

/// Renders a cleared, centered dialog with the supplied lines.
pub(crate) fn render_lines<'a>(
    frame: &mut Frame,
    area: Rect,
    width: u16,
    height: u16,
    title: &str,
    lines: Vec<Line<'a>>,
) {
    let modal = area.centered(Constraint::Length(width), Constraint::Length(height));
    frame.render_widget(Clear, modal);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .title(title)
                .border_style(styles::focused_border()),
        ),
        modal,
    );
}
