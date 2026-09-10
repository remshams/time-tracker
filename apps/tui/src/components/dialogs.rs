use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::components::text::fit_suffix;
use crate::styles;

/// Shrinks `area` to `width` × `height`, centered inside it.
pub(crate) fn centered(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

/// Renders a fixed-width one-line input dialog.
pub(crate) fn render_input(frame: &mut Frame, area: Rect, prompt: &str, buffer: &str) {
    let modal = centered(56, 3, area);
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
    let modal = centered(56, 3, area);
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
    let modal = centered(width, height, area);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centered_keeps_the_modal_inside_the_area() {
        let area = Rect::new(0, 0, 80, 24);
        assert_eq!(centered(56, 3, area), Rect::new(12, 10, 56, 3));
        // A modal larger than the area is clamped, never overflows.
        assert_eq!(centered(100, 50, area), area);
        // Off-center areas keep the modal centered inside the area.
        let offset = Rect::new(10, 5, 20, 9);
        assert_eq!(centered(10, 3, offset), Rect::new(15, 8, 10, 3));
    }
}
