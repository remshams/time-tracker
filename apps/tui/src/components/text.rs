use std::time::Duration;

use unicode_width::UnicodeWidthChar;

fn char_width(character: char) -> usize {
    character.width().unwrap_or(0)
}

/// The longest prefix of `text` that fits in `max_width` display cells.
pub(crate) fn fit_prefix(text: &str, max_width: usize) -> &str {
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
pub(crate) fn fit_suffix(text: &str, max_width: usize) -> &str {
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

/// Formats a duration as `HH:MM:SS`.
pub(crate) fn format_elapsed(total: Duration) -> String {
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
    use super::*;

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
    fn elapsed_times_format_as_hours_minutes_seconds() {
        assert_eq!(format_elapsed(Duration::from_secs(0)), "00:00:00");
        assert_eq!(format_elapsed(Duration::from_secs(59)), "00:00:59");
        assert_eq!(format_elapsed(Duration::from_secs(60)), "00:01:00");
        assert_eq!(format_elapsed(Duration::from_secs(3661)), "01:01:01");
        assert_eq!(format_elapsed(Duration::from_secs(86_399)), "23:59:59");
        assert_eq!(format_elapsed(Duration::from_secs(25 * 3600)), "25:00:00");
    }
}
