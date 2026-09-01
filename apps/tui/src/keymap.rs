//! Translation from raw key events to semantic commands.
//!
//! This is the only place where key codes meet commands. Every key a mode
//! accepts must appear in that mode's footer text, so the help strings live
//! here next to the mappings they document.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::app::Mode;
use crate::command::Command;

/// The quit chord, accepted in every mode.
const QUIT_MODIFIER: KeyModifiers = KeyModifiers::CONTROL;
const QUIT_KEY: KeyCode = KeyCode::Char('c');

/// Maps a key event to a command for the given mode.
///
/// Returns `None` for keys the mode does not handle. Ctrl+C quits in every
/// mode, including while typing.
pub fn map(mode: &Mode, key: KeyEvent) -> Option<Command> {
    if key.kind != KeyEventKind::Press {
        return None;
    }
    if key.modifiers.contains(QUIT_MODIFIER) && key.code == QUIT_KEY {
        return Some(Command::Quit);
    }
    match mode {
        Mode::Normal => map_normal(key),
        Mode::Input { .. } => map_input(key),
        Mode::ConfirmArchive { .. } => map_confirm(key),
    }
}

/// Keys for the task list.
fn map_normal(key: KeyEvent) -> Option<Command> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => Some(Command::MoveDown),
        KeyCode::Char('k') | KeyCode::Up => Some(Command::MoveUp),
        KeyCode::Char(' ') => Some(Command::ToggleTracking),
        KeyCode::Char('a') => Some(Command::OpenAdd),
        KeyCode::Char('e') => Some(Command::OpenRename),
        KeyCode::Char('d') => Some(Command::OpenArchiveConfirm),
        KeyCode::Char('q') | KeyCode::Esc => Some(Command::Quit),
        // `h` and `l` are deliberately unmapped here; see `Command::Reserved`.
        KeyCode::Char('h') | KeyCode::Char('l') => Some(Command::Reserved),
        _ => None,
    }
}

/// Keys for the one-line text input.
///
/// Printable characters and Backspace edit, Enter confirms, and Escape
/// cancels. Space is ordinary input.
fn map_input(key: KeyEvent) -> Option<Command> {
    match key.code {
        KeyCode::Enter => Some(Command::Confirm),
        KeyCode::Esc => Some(Command::Cancel),
        KeyCode::Backspace => Some(Command::Backspace),
        KeyCode::Char(character) if !is_chord(key) => Some(Command::Insert(character)),
        _ => None,
    }
}

/// Whether the key carries a command modifier rather than plain text.
fn is_chord(key: KeyEvent) -> bool {
    key.modifiers.intersects(KeyModifiers::CONTROL) || key.modifiers.intersects(KeyModifiers::ALT)
}

/// Keys for the archive confirmation.
fn map_confirm(key: KeyEvent) -> Option<Command> {
    match key.code {
        KeyCode::Enter | KeyCode::Char('y') => Some(Command::Confirm),
        KeyCode::Esc | KeyCode::Char('n') => Some(Command::Cancel),
        _ => None,
    }
}

/// One-line key help for the given mode.
///
/// Every key the mode accepts appears here.
pub fn footer_hints(mode: &Mode) -> &'static str {
    match mode {
        Mode::Normal => {
            "j/k move · space start/stop · a add · e rename · d archive · q/esc/ctrl+c quit"
        }
        Mode::Input { .. } => "type · backspace delete · enter save · esc cancel · ctrl+c quit",
        Mode::ConfirmArchive { .. } => "y/enter confirm · n/esc cancel · ctrl+c quit",
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    use super::*;
    use crate::app::InputPurpose;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(character: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL)
    }

    fn normal() -> Mode {
        Mode::Normal
    }

    fn input() -> Mode {
        Mode::Input {
            purpose: InputPurpose::Add,
            buffer: String::new(),
        }
    }

    fn confirm() -> Mode {
        Mode::ConfirmArchive {
            task_id: tracker_core::TaskId::generate(),
            name: "task".to_owned(),
        }
    }

    #[test]
    fn key_releases_produce_no_command() {
        let release = KeyEvent::new_with_kind(
            KeyCode::Char('j'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );
        for mode in [normal(), input(), confirm()] {
            assert_eq!(map(&mode, release), None);
        }
    }

    #[test]
    fn normal_mode_maps_movement_and_actions() {
        assert_eq!(
            map(&normal(), key(KeyCode::Char('j'))),
            Some(Command::MoveDown)
        );
        assert_eq!(map(&normal(), key(KeyCode::Down)), Some(Command::MoveDown));
        assert_eq!(
            map(&normal(), key(KeyCode::Char('k'))),
            Some(Command::MoveUp)
        );
        assert_eq!(map(&normal(), key(KeyCode::Up)), Some(Command::MoveUp));
        assert_eq!(
            map(&normal(), key(KeyCode::Char(' '))),
            Some(Command::ToggleTracking)
        );
        assert_eq!(
            map(&normal(), key(KeyCode::Char('a'))),
            Some(Command::OpenAdd)
        );
        assert_eq!(
            map(&normal(), key(KeyCode::Char('e'))),
            Some(Command::OpenRename)
        );
        assert_eq!(
            map(&normal(), key(KeyCode::Char('d'))),
            Some(Command::OpenArchiveConfirm)
        );
        assert_eq!(map(&normal(), key(KeyCode::Char('q'))), Some(Command::Quit));
        assert_eq!(map(&normal(), key(KeyCode::Esc)), Some(Command::Quit));
    }

    #[test]
    fn normal_mode_reserves_h_and_l() {
        assert_eq!(
            map(&normal(), key(KeyCode::Char('h'))),
            Some(Command::Reserved)
        );
        assert_eq!(
            map(&normal(), key(KeyCode::Char('l'))),
            Some(Command::Reserved)
        );
    }

    #[test]
    fn normal_mode_ignores_unmapped_keys() {
        assert_eq!(map(&normal(), key(KeyCode::Char('x'))), None);
        assert_eq!(map(&normal(), key(KeyCode::Enter)), None);
        assert_eq!(map(&normal(), key(KeyCode::Backspace)), None);
        assert_eq!(map(&normal(), key(KeyCode::Left)), None);
    }

    #[test]
    fn ctrl_c_quits_in_every_mode() {
        for mode in [normal(), input(), confirm()] {
            assert_eq!(map(&mode, ctrl('c')), Some(Command::Quit));
        }
    }

    #[test]
    fn input_mode_edits_confirms_and_cancels() {
        assert_eq!(
            map(&input(), key(KeyCode::Char('x'))),
            Some(Command::Insert('x'))
        );
        // Space is ordinary input while typing.
        assert_eq!(
            map(&input(), key(KeyCode::Char(' '))),
            Some(Command::Insert(' '))
        );
        assert_eq!(
            map(&input(), key(KeyCode::Backspace)),
            Some(Command::Backspace)
        );
        assert_eq!(map(&input(), key(KeyCode::Enter)), Some(Command::Confirm));
        assert_eq!(map(&input(), key(KeyCode::Esc)), Some(Command::Cancel));
    }

    #[test]
    fn input_mode_ignores_modifier_chords_and_other_keys() {
        assert_eq!(map(&input(), ctrl('a')), None);
        let alt = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT);
        assert_eq!(map(&input(), alt), None);
        assert_eq!(map(&input(), key(KeyCode::Up)), None);
        assert_eq!(map(&input(), key(KeyCode::Tab)), None);
    }

    #[test]
    fn confirm_mode_accepts_and_cancels() {
        assert_eq!(map(&confirm(), key(KeyCode::Enter)), Some(Command::Confirm));
        assert_eq!(
            map(&confirm(), key(KeyCode::Char('y'))),
            Some(Command::Confirm)
        );
        assert_eq!(
            map(&confirm(), key(KeyCode::Char('n'))),
            Some(Command::Cancel)
        );
        assert_eq!(map(&confirm(), key(KeyCode::Esc)), Some(Command::Cancel));
        assert_eq!(map(&confirm(), key(KeyCode::Char('x'))), None);
    }

    #[test]
    fn every_accepted_key_is_listed_in_the_footer() {
        let normal_keys = footer_hints(&normal());
        for hint in ["j/k", "space", "a ", "e ", "d ", "q/esc", "ctrl+c"] {
            assert!(normal_keys.contains(hint), "normal footer misses {hint:?}");
        }
        // Reserved keys are not listed: they perform no action.
        assert!(!normal_keys.contains("h/l"));
        let input_keys = footer_hints(&input());
        for hint in ["type", "backspace", "enter", "esc", "ctrl+c"] {
            assert!(input_keys.contains(hint), "input footer misses {hint:?}");
        }
        let confirm_keys = footer_hints(&confirm());
        for hint in ["y/enter", "n/esc", "ctrl+c"] {
            assert!(
                confirm_keys.contains(hint),
                "confirm footer misses {hint:?}"
            );
        }
        // Every footer fits a standard 80-column terminal.
        for footer in [
            footer_hints(&normal()),
            footer_hints(&input()),
            footer_hints(&confirm()),
        ] {
            assert!(footer.chars().count() <= 80, "footer too wide: {footer:?}");
        }
    }
}
