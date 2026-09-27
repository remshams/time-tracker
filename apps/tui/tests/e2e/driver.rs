//! Process and terminal control for one live `tt` run.

use std::path::Path;
use std::time::Duration;

use termlens::{ExitStatus, Key, Screen, Signal, Terminal};

use crate::page::TimeTrackerPage;

/// The geometry every scenario starts at. Resize scenarios leave this
/// baseline behind; the page components derive their rows and columns from
/// the snapshot's own size, so no scenario pins to these numbers.
const COLS: u16 = 80;
const ROWS: u16 = 24;

/// How long `tt` may take to start up and render its first frame before
/// the test gives up. Startup also covers creating and seeding the
/// database; a healthy debug build renders in well under a second, so a
/// full allowance here still keeps every scenario inside the mutation
/// test's per-run timeout.
const STARTUP_LIMIT: Duration = Duration::from_secs(3);

/// How long `tt` may take to answer one key press after the first frame,
/// and to exit once quit was sent.
///
/// A healthy binary answers in milliseconds. Keeping this far below the
/// startup limit means a build that never receives input, for example when
/// raw mode was not enabled and the tty buffers the keys, fails in seconds
/// instead of burning the startup allowance in every staged wait.
const RESPONSE_LIMIT: Duration = Duration::from_secs(3);

/// How long the output must stay quiet after a wait matched, as a
/// best-effort sign that rendering finished. Quiet output is evidence,
/// not proof: it is not a frame-boundary guarantee. That is why every
/// wait predicate must already cover the regions the test asserts on;
/// the settle only lowers the odds of reading a half-finished repaint in
/// regions no predicate named.
const FRAME_QUIET: Duration = Duration::from_millis(50);

/// How long panic cleanup gives a terminated child to exit before termlens'
/// own bounded drop cleanup takes over.
const FAILURE_SHUTDOWN_LIMIT: Duration = Duration::from_millis(500);

/// Drives one `tt` process and reads its rendered screen.
pub(crate) struct TuiDriver {
    terminal: Terminal,
}

impl TuiDriver {
    /// Spawns `tt` with a cleared environment and UTC as its timezone.
    pub(crate) fn spawn(home: &Path) -> Self {
        Self::spawn_in_timezone(home, "UTC")
    }

    /// Spawns `tt` with the remote server selected and an isolated home.
    pub(crate) fn spawn_remote(home: &Path, endpoint: &str) -> Self {
        let terminal = Terminal::builder()
            .size(COLS, ROWS)
            .timeout(STARTUP_LIMIT)
            .env_clear()
            .env("HOME", home)
            .env("SHELL", "/bin/sh")
            .env("TZ", "UTC")
            .arg("--server")
            .arg(endpoint)
            .spawn(env!("CARGO_BIN_EXE_tt"))
            .expect("spawning remote tt in a pty must succeed");
        Self { terminal }
    }

    /// Spawns `tt` with a cleared environment and the given IANA timezone.
    ///
    /// The coverage profile file passes through when the suite runs under
    /// cargo-llvm-cov, so the child's execution counts toward the measured
    /// coverage.
    pub(crate) fn spawn_in_timezone(home: &Path, timezone: &str) -> Self {
        Self::spawn_with_report_clock(home, timezone, None, None, None)
    }

    /// Launches `tt` with deterministic report time and a private clipboard
    /// output file for reporting scenarios.
    pub(crate) fn spawn_with_report_clock(
        home: &Path,
        timezone: &str,
        now: Option<&str>,
        fake_bin: Option<&Path>,
        clipboard_file: Option<&Path>,
    ) -> Self {
        let mut builder = Terminal::builder()
            .size(COLS, ROWS)
            .timeout(STARTUP_LIMIT)
            .env_clear()
            .env("HOME", home)
            .env("SHELL", "/bin/sh")
            .env("TZ", timezone);
        if let Some(now) = now {
            builder = builder.env("TT_TEST_NOW", now);
        }
        if let Some(path) = fake_bin {
            builder = builder.env("PATH", path);
        }
        if let Some(path) = clipboard_file {
            builder = builder.env("TT_E2E_CLIPBOARD_FILE", path);
            builder = builder.env("TT_E2E_SHORT_COPY_NOTICE", "1");
        }
        if let Ok(profile) = std::env::var("LLVM_PROFILE_FILE") {
            builder = builder.env("LLVM_PROFILE_FILE", profile);
        }
        let terminal = builder
            .spawn(env!("CARGO_BIN_EXE_tt"))
            .expect("spawning tt in a pty must succeed");
        Self { terminal }
    }

    /// The screen as it looks right now, wrapped in page components.
    ///
    /// A page is a snapshot of one moment. The snapshot never changes, and
    /// the application keeps repainting, so the page goes stale as soon as
    /// the terminal draws again. Every wait method on this driver
    /// recaptures; never carry a page across a key press.
    pub(crate) fn page(&mut self) -> TimeTrackerPage {
        TimeTrackerPage::new(self.terminal.screen())
    }

    /// Sends one key press.
    pub(crate) fn press(&mut self, key: Key) {
        self.terminal
            .send(key)
            .expect("the tt process must accept key input");
    }

    /// Sends a run of printable characters.
    pub(crate) fn type_text(&mut self, text: &str) {
        self.terminal
            .send_str(text)
            .expect("the tt process must accept typed text");
    }

    /// Waits for the first frame after spawning, using the startup
    /// allowance, and returns a fresh page.
    pub(crate) fn wait_for_first_frame(
        &mut self,
        waiting_for: &str,
        mut condition: impl FnMut(&Screen) -> bool,
    ) -> TimeTrackerPage {
        self.terminal
            .wait_until(|screen| condition(screen))
            .unwrap_or_else(|error| panic!("tt never rendered {waiting_for}: {error}"));
        self.settle();
        self.page()
    }

    /// Waits until `condition` holds on the rendered screen, with one
    /// key-press response allowance, and returns a fresh page.
    pub(crate) fn wait_for(
        &mut self,
        waiting_for: &str,
        mut condition: impl FnMut(&Screen) -> bool,
    ) -> TimeTrackerPage {
        self.terminal
            .wait_until_for(|screen| condition(screen), RESPONSE_LIMIT)
            .unwrap_or_else(|error| panic!("timed out waiting for {waiting_for}: {error}"));
        self.settle();
        self.page()
    }

    /// Waits out the output burst after a wait matched, so a page taken
    /// next is less likely to describe a half-finished repaint. Best
    /// effort only: quiet output is evidence, not a frame-boundary
    /// guarantee. Every wait predicate must cover the regions the test
    /// asserts on.
    fn settle(&mut self) {
        self.terminal
            .wait_idle_for(FRAME_QUIET, RESPONSE_LIMIT)
            .expect("tt must go quiet after rendering");
    }

    /// Sends `key` and waits for the post-action state.
    ///
    /// The condition must describe the state that follows the key press,
    /// and must not hold before it: a predicate that is already true
    /// describes a stale frame, and the wait below could never prove the
    /// key did anything. Asserting the pre-state here rejects stale
    /// predicates instead of letting them pass against a frame the key
    /// never produced.
    pub(crate) fn press_and_wait(
        &mut self,
        key: Key,
        waiting_for: &str,
        mut condition: impl FnMut(&Screen) -> bool,
    ) -> TimeTrackerPage {
        let before = self.terminal.screen();
        assert!(
            !condition(&before),
            "{waiting_for} already holds before {key:?} was sent; the wait \
             predicate must describe the state after the key press:\n{before}"
        );
        self.press(key);
        self.wait_for(waiting_for, condition)
    }

    /// Sends one key and measures how long the matching screen state takes
    /// to appear. This deliberately skips [`Self::settle`], so the duration
    /// reflects input-to-render latency rather than the quiet-output window.
    pub(crate) fn press_and_measure(
        &mut self,
        key: Key,
        waiting_for: &str,
        mut condition: impl FnMut(&Screen) -> bool,
    ) -> Duration {
        let before = self.terminal.screen();
        assert!(
            !condition(&before),
            "{waiting_for} already holds before {key:?} was sent; the wait predicate must describe the state after the key press:\n{before}"
        );
        let started = std::time::Instant::now();
        self.press(key);
        self.terminal
            .wait_until_for(|screen| condition(screen), RESPONSE_LIMIT)
            .unwrap_or_else(|error| panic!("timed out waiting for {waiting_for}: {error}"));
        started.elapsed()
    }

    /// Resizes the terminal and waits for the repaint that answers it.
    ///
    /// `tt` emits no DEC 2026 synchronized-update markers, so there is no
    /// formal frame boundary to wait for and no `wait_frame_for` here.
    /// What makes the wait sound instead is the predicate: termlens clips
    /// or pads the visible grid to the new size the moment the resize
    /// lands, so the pre-repaint grid already *reports* the new geometry.
    /// Only postconditions that name the relocated components, such as
    /// status and footer text on the new bottom rows, the panel corners
    /// on the new edges, or a dialog at its new center, can separate that
    /// stale grid from the post-SIGWINCH repaint, and the caller's
    /// predicate must cover exactly those. A match therefore proves the
    /// post-SIGWINCH visible state; the settle afterwards stays best
    /// effort, exactly as after every other wait.
    pub(crate) fn resize_and_wait(
        &mut self,
        cols: u16,
        rows: u16,
        waiting_for: &str,
        mut condition: impl FnMut(&Screen) -> bool,
    ) -> TimeTrackerPage {
        let before = self.terminal.screen();
        assert!(
            !condition(&before),
            "{waiting_for} already holds before the resize to {cols}x{rows}; the wait \
             predicate must describe the state after the repaint:\n{before}"
        );
        self.terminal
            .resize(cols, rows)
            .expect("the terminal must accept the resize");
        self.wait_for(waiting_for, condition)
    }

    /// Quits with ctrl+c and waits for the process to exit, bounded by the
    /// response allowance.
    pub(crate) fn quit(self) -> ExitOutcome {
        self.exit_with(Key::Ctrl('c'))
    }

    /// Sends `key` and waits for the process to exit, bounded by the
    /// response allowance.
    pub(crate) fn exit_with(mut self, key: Key) -> ExitOutcome {
        self.press(key);
        let status = self
            .terminal
            .wait_exit_for(RESPONSE_LIMIT)
            .expect("tt must exit after quit");
        ExitOutcome {
            status,
            screen: self.terminal.screen(),
        }
    }
}

impl Drop for TuiDriver {
    fn drop(&mut self) {
        // Normal exits are already cached, making both calls harmless. On a
        // scenario panic, terminate and reap before termlens enters its
        // process-wide serialized fallback cleanup. This keeps a shared
        // rendering failure from paying that fallback once per test.
        let _ = self.terminal.signal(Signal::Term);
        let _ = self.terminal.wait_exit_for(FAILURE_SHUTDOWN_LIMIT);
    }
}

/// The end of one `tt` run: its exit status and the screen it left behind.
pub(crate) struct ExitOutcome {
    status: ExitStatus,
    screen: Screen,
}

impl ExitOutcome {
    /// Asserts the terminal lifecycle invariants of a clean run: exit code
    /// 0, no alternate screen left up, cursor shown again.
    pub(crate) fn assert_clean_exit(&self) {
        assert_eq!(
            self.status.code(),
            Some(0),
            "tt exited abnormally:\n{}",
            self.screen
        );
        assert!(
            !self.screen.alternate_screen(),
            "the alternate screen was not left:\n{}",
            self.screen
        );
        assert!(
            self.screen.cursor().2,
            "the cursor was not shown again:\n{}",
            self.screen
        );
    }
}
