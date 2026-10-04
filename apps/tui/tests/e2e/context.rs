//! Test fixtures: one isolated temporary home per scenario.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use crate::database::Database;
use crate::driver::TuiDriver;

/// One test's private environment: a temporary home directory whose
/// platform data directory `tt` uses, so a test never touches the user's
/// real data, and parallel tests never see each other's database.
pub(crate) struct TestContext {
    _temp: TempDir,
    home: PathBuf,
    database_path: PathBuf,
    clipboard_path: PathBuf,
    fake_bin: PathBuf,
}

impl TestContext {
    /// Creates the temporary home. The home directory itself must exist so
    /// `tt` can create the platform data directory below it. Nothing below
    /// it is created here: a fresh-start scenario lets `tt` build its own
    /// data path, and a scenario that seeds first calls [`TestContext::database`],
    /// which prepares the directory then.
    pub(crate) fn new() -> Self {
        let temp = tempfile::tempdir().expect("a temporary directory must be creatable");
        let home = temp.path().join("home");
        std::fs::create_dir(&home).expect("the temporary home must be creatable");
        let database_path = database_in(&home);
        let clipboard_path = temp.path().join("clipboard.txt");
        let fake_bin = temp.path().join("bin");
        std::fs::create_dir(&fake_bin).expect("the private command directory must be creatable");
        let clipboard_command = if cfg!(target_os = "macos") {
            "pbcopy"
        } else {
            "wl-copy"
        };
        let fake_clipboard = fake_bin.join(clipboard_command);
        std::fs::write(
            &fake_clipboard,
            "#!/bin/sh\n/bin/cat > \"$TT_E2E_CLIPBOARD_FILE\"\n",
        )
        .expect("the fake clipboard command must be writable");
        std::fs::set_permissions(&fake_clipboard, std::fs::Permissions::from_mode(0o700))
            .expect("the fake clipboard command must be executable");
        Self {
            _temp: temp,
            home,
            database_path,
            clipboard_path,
            fake_bin,
        }
    }

    /// Creates the task fixture used by scenarios that need existing rows.
    pub(crate) fn new_with_tasks() -> Self {
        let context = Self::new();
        context.database().create_fixture_tasks();
        context
    }

    /// Launches `tt` against this home in a fresh pseudo-terminal. Call
    /// again for a restart of the same context.
    pub(crate) fn launch(&self) -> TuiDriver {
        TuiDriver::spawn(&self.home)
    }

    pub(crate) fn launch_remote(&self, endpoint: &str) -> TuiDriver {
        TuiDriver::spawn_remote(&self.home, endpoint)
    }

    pub(crate) fn home(&self) -> &Path {
        &self.home
    }

    pub(crate) fn local_database_path(&self) -> &Path {
        &self.database_path
    }

    /// Launches `tt` with a specific IANA timezone for display assertions.
    pub(crate) fn launch_in_timezone(&self, timezone: &str) -> TuiDriver {
        TuiDriver::spawn_in_timezone(&self.home, timezone)
    }

    /// Launches `tt` with a fixed report clock and a clipboard file inside
    /// this scenario's private temporary directory.
    pub(crate) fn launch_for_reports(&self, now: &str) -> TuiDriver {
        TuiDriver::spawn_with_report_clock(
            &self.home,
            "UTC",
            Some(now),
            Some(&self.fake_bin),
            Some(&self.clipboard_path),
        )
    }

    /// The private file where report copy actions record their value.
    pub(crate) fn clipboard_text(&self) -> String {
        std::fs::read_to_string(&self.clipboard_path)
            .expect("the report copy action must write the private clipboard file")
    }

    /// Opens the context's database through the real SQLite adapter, as a
    /// fixture for seeding and injected failures and as a probe for
    /// postconditions.
    ///
    /// The database's parent directory is created lazily, only when it is
    /// absent, so a scenario that seeds before launch can create it here
    /// while a post-launch probe uses the directory `tt` itself created.
    /// A directory created here gets the mode production uses, so the
    /// adapter's own validation accepts it. Each call opens a new
    /// connection and reruns migration setup. A scenario that probes
    /// while `tt` runs opens this once after the first frame, before its
    /// next action, and reuses the connection. This avoids opening a
    /// migration transaction alongside an application action.
    pub(crate) fn database(&self) -> Database {
        let database_dir = self
            .database_path
            .parent()
            .expect("the database path has a parent directory");
        if !database_dir.exists() {
            std::fs::create_dir_all(database_dir).expect("the data directory must be creatable");
            std::fs::set_permissions(database_dir, std::fs::Permissions::from_mode(0o700))
                .expect("the data directory permissions must be settable");
        }
        Database::open(&self.database_path)
    }
}

/// The database path inside a temporary home directory.
///
/// On Linux the platform data directory is `$HOME/.local/share`; on macOS
/// it is `$HOME/Library/Application Support`.
fn database_in(home: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home.join("Library")
            .join("Application Support")
            .join("Time Tracker")
            .join("tt.db")
    }
    #[cfg(target_os = "linux")]
    {
        home.join(".local")
            .join("share")
            .join("Time Tracker")
            .join("tt.db")
    }
}
