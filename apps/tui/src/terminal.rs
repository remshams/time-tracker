//! Terminal lifecycle: raw mode, alternate screen, and panic safety.
//!
//! [`TerminalGuard`] is the single owner of terminal setup. Its cleanup is
//! idempotent, runs on drop, and is deliberately independent of the panic
//! hook, which performs its own best-effort restore before delegating to the
//! previously installed hook.

use std::io;
use std::panic;

use crossterm::cursor::Show;
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::CrosstermBackend;
use ratatui::{Frame, Terminal};

/// The terminal type the interface renders to.
type Tui = Terminal<CrosstermBackend<io::Stdout>>;

/// Owns the terminal setup and restores it exactly once.
///
/// Cleanup runs at most once, whether triggered by [`TerminalGuard::restore`]
/// or by drop, and never leaves the terminal in raw mode or the alternate
/// screen, including after partial setup failures.
pub struct TerminalGuard {
    /// Present until the terminal has been restored.
    terminal: Option<Tui>,
}

impl TerminalGuard {
    /// Enables raw mode, enters the alternate screen, and creates the
    /// terminal.
    ///
    /// Every failure path restores whatever steps already succeeded before
    /// returning the error.
    pub fn new() -> io::Result<Self> {
        enable_raw_mode().map_err(Self::abort_setup)?;
        execute!(io::stdout(), EnterAlternateScreen).map_err(Self::abort_setup)?;
        let terminal =
            Terminal::new(CrosstermBackend::new(io::stdout())).map_err(Self::abort_setup)?;
        Ok(Self {
            terminal: Some(terminal),
        })
    }

    /// Restores the terminal after a failed setup step and hands back the
    /// error.
    fn abort_setup(error: io::Error) -> io::Error {
        force_restore();
        error
    }

    /// Draws one frame.
    pub fn draw(&mut self, render: impl FnOnce(&mut Frame)) -> io::Result<()> {
        match &mut self.terminal {
            Some(terminal) => terminal.draw(render).map(|_| ()),
            None => Ok(()),
        }
    }

    /// Restores raw mode, the previous screen, and the cursor.
    ///
    /// Calling this more than once is a no-op after the first call.
    fn restore(&mut self) {
        if self.terminal.take().is_some() {
            force_restore();
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.restore();
    }
}

/// Restores the terminal, ignoring any errors, and shows the cursor.
///
/// Safe to call repeatedly and from a panic hook: every step either does
/// nothing when there is nothing to restore or reports an error that is
/// deliberately dropped.
pub(crate) fn force_restore() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
    let _ = execute!(io::stdout(), Show);
}

/// Installs a panic hook that runs `restore` before delegating to the
/// previously installed hook.
///
/// The `restore` parameter is the terminal restoration to run; production
/// passes [`force_restore`], tests pass a spy.
pub fn install_panic_hook(restore: impl Fn() + Send + Sync + 'static) {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        restore();
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    #[test]
    fn restoring_twice_is_a_no_op_the_second_time() {
        let Ok(mut guard) = TerminalGuard::new() else {
            // Without a usable terminal the setup fails after having restored
            // everything it touched; there is nothing more to assert.
            return;
        };
        assert!(guard.terminal.is_some());
        guard.restore();
        assert!(guard.terminal.is_none());
        guard.restore();
        assert!(guard.terminal.is_none());
    }

    #[test]
    fn drawing_after_a_restore_is_a_harmless_no_op() {
        let Ok(mut guard) = TerminalGuard::new() else {
            return;
        };
        guard.restore();
        guard.draw(|_| {}).unwrap();
    }

    #[test]
    fn force_restore_is_safe_outside_a_terminal() {
        // Neither raw mode nor the alternate screen is active here; every
        // step must swallow its error and leave the process alive.
        force_restore();
        force_restore();
    }

    #[test]
    fn abort_setup_hands_back_the_error() {
        let error = io::Error::other("boom");
        let returned = TerminalGuard::abort_setup(error);
        assert_eq!(returned.to_string(), "boom");
    }

    #[test]
    fn the_panic_hook_restores_the_terminal_and_delegates() {
        let restored = Arc::new(AtomicBool::new(false));
        let delegated = Arc::new(AtomicBool::new(false));
        let restore_spy = restored.clone();
        let delegate_spy = delegated.clone();
        panic::set_hook(Box::new(move |_| {
            delegate_spy.store(true, Ordering::SeqCst)
        }));
        install_panic_hook(move || restore_spy.store(true, Ordering::SeqCst));

        let result = panic::catch_unwind(|| panic!("boom"));
        assert!(result.is_err());
        assert!(restored.load(Ordering::SeqCst), "the hook must restore");
        assert!(
            delegated.load(Ordering::SeqCst),
            "the previous hook must still run"
        );
        // Leave a sane hook state for the remaining tests.
        let _ = panic::take_hook();
    }
}
