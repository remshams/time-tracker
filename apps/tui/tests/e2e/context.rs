//! Test fixtures: one isolated temporary home per scenario.

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
}

impl TestContext {
    /// Creates the temporary home. The home directory itself must exist so
    /// `tt` can create the platform data directory below it.
    pub(crate) fn new() -> Self {
        let temp = tempfile::tempdir().expect("a temporary directory must be creatable");
        let home = temp.path().join("home");
        std::fs::create_dir(&home).expect("the temporary home must be creatable");
        let database_path = database_in(&home);
        Self {
            _temp: temp,
            home,
            database_path,
        }
    }

    /// Launches `tt` against this home in a fresh pseudo-terminal. Call
    /// again for a restart of the same context.
    pub(crate) fn launch(&self) -> TuiDriver {
        TuiDriver::spawn(&self.home)
    }

    /// Opens the context's database through the real SQLite adapter for
    /// postcondition probes. Each call opens a new connection and reruns
    /// migration setup. A scenario that probes while `tt` runs opens this
    /// once after the first frame, before its next action, and reuses the
    /// connection. This avoids opening a migration transaction alongside
    /// an application action.
    pub(crate) fn database(&self) -> Database {
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
