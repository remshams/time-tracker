//! Terminal lifecycle: raw mode, alternate screen, and panic safety.
//!
//! Terminal setup happens in stages. Every completed stage is recorded in a
//! [`Restoration`] shared between the [`TerminalGuard`] and the panic hook,
//! so whoever restores first — normal exit, an error, or a panic — restores
//! exactly the stages that were set up, exactly once. A setup step that
//! never completed is never restored: a raw-mode failure emits no terminal
//! escape sequence at all.

use std::io;
use std::panic;
use std::sync::{Arc, Mutex, MutexGuard};

use crossterm::cursor::Show;
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::CrosstermBackend;
use ratatui::{Frame, Terminal};

/// The terminal type the interface renders to.
type Tui = Terminal<CrosstermBackend<io::Stdout>>;

/// One step of terminal setup that may need restoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Raw mode was enabled.
    RawMode,
    /// The alternate screen was entered.
    AlternateScreen,
    /// A frame was drawn, from which point Ratatui keeps the cursor hidden.
    Cursor,
}

/// The setup stages that completed and have not been restored yet.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Stages {
    raw_mode: bool,
    alternate_screen: bool,
    cursor: bool,
}

impl Stages {
    /// Restores every completed stage, newest first, and clears itself.
    ///
    /// Returns the stages that were restored. Calling this again restores
    /// nothing, which makes restoration idempotent.
    fn restore(&mut self, effects: &dyn TerminalEffects) -> Stages {
        let restored = *self;
        if self.cursor {
            effects.show_cursor();
        }
        if self.alternate_screen {
            effects.leave_alternate_screen();
        }
        if self.raw_mode {
            effects.disable_raw_mode();
        }
        *self = Stages::default();
        restored
    }
}

/// The terminal operations restoration performs, injectable for tests.
pub(crate) trait TerminalEffects {
    fn show_cursor(&self);
    fn leave_alternate_screen(&self);
    fn disable_raw_mode(&self);
}

/// The real terminal operations.
struct RealEffects;

impl TerminalEffects for RealEffects {
    fn show_cursor(&self) {
        let _ = execute!(io::stdout(), Show);
    }

    fn leave_alternate_screen(&self) {
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }

    fn disable_raw_mode(&self) {
        let _ = disable_raw_mode();
    }
}

/// The terminal setup operations, injectable so tests can fail any stage.
pub(crate) trait TerminalSetup {
    fn enable_raw_mode(&self) -> io::Result<()>;
    fn enter_alternate_screen(&self) -> io::Result<()>;
    fn build_terminal(&self) -> io::Result<Tui>;
}

/// The real terminal setup.
struct RealSetup;

impl TerminalSetup for RealSetup {
    fn enable_raw_mode(&self) -> io::Result<()> {
        enable_raw_mode()
    }

    fn enter_alternate_screen(&self) -> io::Result<()> {
        execute!(io::stdout(), EnterAlternateScreen)
    }

    fn build_terminal(&self) -> io::Result<Tui> {
        Terminal::new(CrosstermBackend::new(io::stdout()))
    }
}

/// The restoration state shared between the guard and the panic hook.
///
/// Restoration runs at most once: the first caller clears the stages, so
/// later callers — the other of hook and guard, or a repeated cleanup —
/// find nothing left to do and write nothing.
#[derive(Debug, Clone, Default)]
pub struct Restoration {
    stages: Arc<Mutex<Stages>>,
}

impl Restoration {
    /// Creates an empty restoration state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one completed setup stage.
    fn record(&self, stage: Stage) {
        let mut stages = self.lock();
        match stage {
            Stage::RawMode => stages.raw_mode = true,
            Stage::AlternateScreen => stages.alternate_screen = true,
            Stage::Cursor => stages.cursor = true,
        }
    }

    /// Restores the stages that completed, exactly once.
    pub fn restore(&self) {
        self.restore_with(&RealEffects);
    }

    fn restore_with(&self, effects: &dyn TerminalEffects) -> Stages {
        let mut stages = self.lock();
        stages.restore(effects)
    }

    fn lock(&self) -> MutexGuard<'_, Stages> {
        // A panic while holding the lock must not break later restoration:
        // the state is plain flags, safe to keep using.
        self.stages
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[cfg(test)]
    fn is_clear(&self) -> bool {
        *self.lock() == Stages::default()
    }
}

/// Owns the terminal setup and restores it exactly once.
pub struct TerminalGuard {
    /// Present until the terminal has been restored or setup failed.
    terminal: Option<Tui>,
    restoration: Restoration,
}

impl TerminalGuard {
    /// Enables raw mode, enters the alternate screen, and creates the
    /// terminal.
    ///
    /// The guard records every completed stage in `restoration`, which the
    /// panic hook must share. Every failure path restores the stages that
    /// already completed before returning the error.
    pub fn new(restoration: Restoration) -> io::Result<Self> {
        Self::setup(&RealSetup, &RealEffects, restoration)
    }

    fn setup(
        setup: &dyn TerminalSetup,
        effects: &dyn TerminalEffects,
        restoration: Restoration,
    ) -> io::Result<Self> {
        match Self::enter(setup, &restoration) {
            Ok(terminal) => Ok(Self {
                terminal: Some(terminal),
                restoration,
            }),
            Err(error) => {
                restoration.restore_with(effects);
                Err(error)
            }
        }
    }

    /// Runs the setup stages in order, recording each one as it completes.
    fn enter(setup: &dyn TerminalSetup, restoration: &Restoration) -> io::Result<Tui> {
        setup.enable_raw_mode()?;
        restoration.record(Stage::RawMode);
        setup.enter_alternate_screen()?;
        restoration.record(Stage::AlternateScreen);
        let terminal = setup.build_terminal()?;
        Ok(terminal)
    }

    /// Draws one frame.
    ///
    /// A successful draw makes cursor restoration part of the shared state,
    /// because Ratatui hides the cursor while drawing.
    pub fn draw(&mut self, render: impl FnOnce(&mut Frame)) -> io::Result<()> {
        let Some(terminal) = &mut self.terminal else {
            return Ok(());
        };
        let result = terminal.draw(render).map(|_| ());
        if result.is_ok() {
            self.restoration.record(Stage::Cursor);
        }
        result
    }

    /// Restores the terminal stages that completed.
    ///
    /// Calling this more than once is a no-op after the first call, whether
    /// triggered by this method, by drop, or by the panic hook sharing the
    /// same [`Restoration`].
    pub fn restore(&mut self) {
        self.terminal = None;
        self.restoration.restore();
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.restore();
    }
}

/// Installs a panic hook that runs `restore` before delegating to the
/// previously installed hook.
///
/// The `restore` parameter is the terminal restoration to run; production
/// passes a clone of the guard's [`Restoration`], tests pass a spy.
pub fn install_panic_hook(restore: impl Fn() + Send + Sync + 'static) {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        restore();
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    /// Which setup stage fails.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum FailAt {
        RawMode,
        AlternateScreen,
        Terminal,
    }

    /// Records every effect call and fails setup at a chosen stage.
    #[derive(Default)]
    struct Spy {
        fail_at: Option<FailAt>,
        calls: Rc<RefCell<Vec<&'static str>>>,
    }

    impl Spy {
        fn failing_at(fail_at: FailAt) -> Self {
            Self {
                fail_at: Some(fail_at),
                ..Self::default()
            }
        }

        fn calls(&self) -> Vec<&'static str> {
            self.calls.borrow().clone()
        }
    }

    impl TerminalSetup for Spy {
        fn enable_raw_mode(&self) -> io::Result<()> {
            if self.fail_at == Some(FailAt::RawMode) {
                return Err(io::Error::other("no raw mode"));
            }
            Ok(())
        }

        fn enter_alternate_screen(&self) -> io::Result<()> {
            if self.fail_at == Some(FailAt::AlternateScreen) {
                return Err(io::Error::other("no alternate screen"));
            }
            Ok(())
        }

        fn build_terminal(&self) -> io::Result<Tui> {
            if self.fail_at == Some(FailAt::Terminal) {
                return Err(io::Error::other("no terminal"));
            }
            Terminal::new(CrosstermBackend::new(io::stdout()))
        }
    }

    impl TerminalEffects for Spy {
        fn show_cursor(&self) {
            self.calls.borrow_mut().push("show_cursor");
        }

        fn leave_alternate_screen(&self) {
            self.calls.borrow_mut().push("leave_screen");
        }

        fn disable_raw_mode(&self) {
            self.calls.borrow_mut().push("disable_raw");
        }
    }

    #[test]
    fn stages_restore_newest_first_and_clear_themselves() {
        let spy = Spy::default();
        let mut stages = Stages {
            raw_mode: true,
            alternate_screen: true,
            cursor: true,
        };
        let restored = stages.restore(&spy);
        assert_eq!(
            restored,
            Stages {
                raw_mode: true,
                alternate_screen: true,
                cursor: true,
            }
        );
        assert_eq!(spy.calls(), ["show_cursor", "leave_screen", "disable_raw"]);
        // A second call restores nothing.
        assert_eq!(stages.restore(&spy), Stages::default());
        assert_eq!(spy.calls(), ["show_cursor", "leave_screen", "disable_raw"]);
    }

    #[test]
    fn a_raw_mode_failure_restores_nothing_and_writes_no_escape() {
        let restoration = Restoration::new();
        let spy = Spy::failing_at(FailAt::RawMode);
        let Err(error) = TerminalGuard::setup(&spy, &spy, restoration.clone()) else {
            panic!("raw mode must fail");
        };
        assert_eq!(error.to_string(), "no raw mode");
        // No stage completed, so no restore action — and no escape
        // sequence — ran.
        assert_eq!(spy.calls(), Vec::<&'static str>::new());
        assert!(restoration.is_clear());
    }

    #[test]
    fn an_alternate_screen_failure_restores_only_raw_mode() {
        let restoration = Restoration::new();
        let spy = Spy::failing_at(FailAt::AlternateScreen);
        let Err(error) = TerminalGuard::setup(&spy, &spy, restoration.clone()) else {
            panic!("the alternate screen must fail");
        };
        assert_eq!(error.to_string(), "no alternate screen");
        // Raw mode completed, so it is restored; no escape sequence ran.
        assert_eq!(spy.calls(), ["disable_raw"]);
        assert!(restoration.is_clear());
    }

    #[test]
    fn a_terminal_failure_restores_the_screen_and_raw_mode() {
        let restoration = Restoration::new();
        let spy = Spy::failing_at(FailAt::Terminal);
        let Err(error) = TerminalGuard::setup(&spy, &spy, restoration.clone()) else {
            panic!("building the terminal must fail");
        };
        assert_eq!(error.to_string(), "no terminal");
        assert_eq!(spy.calls(), ["leave_screen", "disable_raw"]);
        assert!(restoration.is_clear());
    }

    #[test]
    fn restoration_runs_exactly_once_across_hook_and_drop() {
        let restoration = Restoration::new();
        restoration.record(Stage::RawMode);
        restoration.record(Stage::AlternateScreen);
        let spy = Spy::default();
        let restored = restoration.restore_with(&spy);
        assert_eq!(
            restored,
            Stages {
                raw_mode: true,
                alternate_screen: true,
                cursor: false,
            }
        );
        assert_eq!(spy.calls(), ["leave_screen", "disable_raw"]);
        // The second restoration — the other of hook and guard drop — finds
        // the state empty and writes nothing.
        assert_eq!(restoration.restore_with(&spy), Stages::default());
        assert_eq!(spy.calls(), ["leave_screen", "disable_raw"]);
        assert!(restoration.is_clear());
    }

    #[test]
    fn a_successful_draw_records_the_cursor_stage() {
        let restoration = Restoration::new();
        let Ok(terminal) = TerminalGuard::enter(&RealSetup, &restoration) else {
            // Without a usable terminal the setup fails after having
            // restored everything it touched; there is nothing to assert.
            return;
        };
        let mut guard = TerminalGuard {
            terminal: Some(terminal),
            restoration: restoration.clone(),
        };
        guard.draw(|_| {}).expect("a bare draw works");
        // The drawn terminal hides the cursor, so restoring reports the
        // cursor stage.
        let spy = Spy::default();
        let restored = restoration.restore_with(&spy);
        assert!(restored.cursor, "a drawn terminal restores the cursor");
        assert!(restoration.is_clear());
        // The guard's own cleanup after a manual restore is a no-op.
        drop(guard);
    }

    #[test]
    fn restoring_twice_is_a_no_op_the_second_time() {
        let Ok(mut guard) = TerminalGuard::new(Restoration::new()) else {
            // Without a usable terminal the setup fails after having restored
            // everything it touched; there is nothing more to assert.
            return;
        };
        guard.restore();
        guard.restore();
    }

    #[test]
    fn drawing_after_a_restore_is_a_harmless_no_op() {
        let Ok(mut guard) = TerminalGuard::new(Restoration::new()) else {
            return;
        };
        guard.restore();
        guard.draw(|_| {}).unwrap();
    }

    #[test]
    fn restoring_an_empty_state_is_safe_outside_a_terminal() {
        // Neither raw mode nor the alternate screen is active here; every
        // step must swallow its error and leave the process alive.
        Restoration::new().restore();
        Restoration::new().restore();
    }

    #[test]
    fn the_panic_hook_restores_the_terminal_and_delegates() {
        use std::sync::atomic::{AtomicBool, Ordering};

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
