//! Shared styles for the terminal interface.
//!
//! Styles use named ANSI colors and terminal defaults rather than fixed RGB
//! values. Terminal themes can therefore supply their own accents while
//! important text retains the terminal's contrasting foreground and background.

use ratatui::style::{Color, Modifier, Style};

/// The application title.
pub(crate) fn title() -> Style {
    Style::default()
        .fg(Color::Blue)
        .add_modifier(Modifier::BOLD)
}

/// The border of the currently focused panel or dialog.
pub(crate) fn focused_border() -> Style {
    Style::default().fg(Color::Blue)
}

/// The selected task row.
///
/// Reversing explicit terminal defaults prevents a child accent, such as the
/// active marker, from becoming the selected row's background.
pub(crate) fn selected() -> Style {
    Style::default()
        .fg(Color::Reset)
        .bg(Color::Reset)
        .add_modifier(Modifier::REVERSED)
        .add_modifier(Modifier::BOLD)
}

/// The marker that identifies the running task.
pub(crate) fn active_marker() -> Style {
    Style::default()
        .fg(Color::Green)
        .add_modifier(Modifier::BOLD)
}

/// The label introducing an error status.
///
/// Only the redundant `Error:` label receives the accent. The error message
/// itself keeps the terminal's normal foreground for reliable contrast.
pub(crate) fn error_label() -> Style {
    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
}

/// The text-input cursor glyph.
pub(crate) fn input_cursor() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_terminal_color(color: Option<Color>) -> bool {
        !matches!(color, Some(Color::Rgb(..) | Color::Indexed(..)))
    }

    #[test]
    fn every_style_uses_terminal_palette_colors() {
        for style in [
            title(),
            focused_border(),
            selected(),
            active_marker(),
            error_label(),
            input_cursor(),
        ] {
            assert!(is_terminal_color(style.fg));
            assert!(is_terminal_color(style.bg));
            assert!(is_terminal_color(style.underline_color));
        }
    }

    #[test]
    fn selection_resets_child_accents_and_reverses_the_terminal_pair() {
        let style = selected();
        assert_eq!(style.fg, Some(Color::Reset));
        assert_eq!(style.bg, Some(Color::Reset));
        assert!(style.add_modifier.contains(Modifier::REVERSED));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn decorative_roles_use_named_ansi_accents() {
        assert_eq!(title().fg, Some(Color::Blue));
        assert_eq!(focused_border().fg, Some(Color::Blue));
        assert_eq!(active_marker().fg, Some(Color::Green));
        assert_eq!(error_label().fg, Some(Color::Red));
    }

    #[test]
    fn important_roles_remain_prominent_without_fixed_colors() {
        assert!(title().add_modifier.contains(Modifier::BOLD));
        assert!(active_marker().add_modifier.contains(Modifier::BOLD));
        assert!(error_label().add_modifier.contains(Modifier::BOLD));
        assert!(input_cursor().add_modifier.contains(Modifier::BOLD));
        assert_eq!(input_cursor().fg, None);
    }
}
