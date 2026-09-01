//! Shared styles for the terminal interface.
//!
//! Styles use the terminal's default palette rather than fixed colors. This
//! keeps text readable when a terminal switches between light and dark themes.

use ratatui::style::{Modifier, Style};

/// The application title.
pub(crate) fn title() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

/// The selected task row.
///
/// Reversing the terminal's normal foreground and background preserves their
/// contrast instead of assuming an ANSI accent works as a text background.
pub(crate) fn selected() -> Style {
    Style::default()
        .add_modifier(Modifier::REVERSED)
        .add_modifier(Modifier::BOLD)
}

/// An error status.
///
/// The text also carries an `Error:` prefix, so color is not needed to convey
/// its meaning.
pub(crate) fn error() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

/// The text-input cursor glyph.
pub(crate) fn input_cursor() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_no_fixed_colors(style: Style) -> bool {
        style.fg.is_none() && style.bg.is_none() && style.underline_color.is_none()
    }

    #[test]
    fn every_style_uses_the_terminal_palette() {
        for style in [title(), selected(), error(), input_cursor()] {
            assert!(has_no_fixed_colors(style));
        }
    }

    #[test]
    fn selection_reverses_the_terminals_contrasting_pair() {
        let style = selected();
        assert!(style.add_modifier.contains(Modifier::REVERSED));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn important_status_and_cursor_roles_remain_prominent() {
        assert!(error().add_modifier.contains(Modifier::BOLD));
        assert!(input_cursor().add_modifier.contains(Modifier::BOLD));
    }
}
