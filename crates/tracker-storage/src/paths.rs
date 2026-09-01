//! Platform paths for the tracker's application data.
//!
//! The database lives in the platform application-data directory: Linux uses
//! `$XDG_DATA_HOME` or `~/.local/share`, macOS uses
//! `~/Library/Application Support`. The directory is named after the product,
//! and the default database file inside it is `tt.db`.

use std::fs;
use std::path::PathBuf;

use crate::error::StorageError;

/// The directory name for Time Tracker data inside the platform data
/// directory.
const APP_DIR_NAME: &str = "Time Tracker";

/// The default database file name.
const DATABASE_FILE_NAME: &str = "tt.db";

/// The database path inside the given application data directory.
pub(crate) fn database_path_in(app_dir: &std::path::Path) -> PathBuf {
    app_dir.join(DATABASE_FILE_NAME)
}

/// Creates the given directory, and any missing parents, with owner-only
/// permissions on Unix.
///
/// An existing directory is left untouched, including its permissions. Only
/// call this with the tracker's own application data directory.
pub(crate) fn ensure_dir(path: &std::path::Path) -> Result<PathBuf, StorageError> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    Ok(path.to_path_buf())
}

/// Returns the tracker's application data directory.
pub fn app_data_dir() -> Result<PathBuf, StorageError> {
    let data_dir = dirs::data_dir().ok_or(StorageError::NoDataDir)?;
    Ok(data_dir.join(APP_DIR_NAME))
}

/// Returns the standard database path without touching the filesystem.
pub fn default_database_path() -> Result<PathBuf, StorageError> {
    Ok(database_path_in(&app_data_dir()?))
}

/// Creates the tracker's application data directory with owner-only
/// permissions on Unix and returns its path.
pub fn ensure_app_data_dir() -> Result<PathBuf, StorageError> {
    ensure_dir(&app_data_dir()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_path_sits_inside_the_app_directory() {
        let base = PathBuf::from("/data");
        let app_dir = base.join(APP_DIR_NAME);
        assert_eq!(database_path_in(&app_dir), app_dir.join("tt.db"));
    }

    #[test]
    fn ensure_dir_creates_missing_parents_owner_only() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("one").join("two").join(APP_DIR_NAME);
        let created = ensure_dir(&target).unwrap();
        assert_eq!(created, target);
        assert!(target.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = target.metadata().unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }
        // A second call succeeds and keeps the directory usable.
        assert_eq!(ensure_dir(&target).unwrap(), target);
    }

    #[test]
    fn app_data_dir_ends_with_the_product_directory() {
        let dir = app_data_dir().unwrap();
        assert!(dir.ends_with(APP_DIR_NAME));
    }

    #[test]
    fn default_database_path_ends_with_the_database_file() {
        let path = default_database_path().unwrap();
        assert_eq!(path.file_name().unwrap(), DATABASE_FILE_NAME);
        assert!(path.parent().unwrap().ends_with(APP_DIR_NAME));
    }

    #[test]
    fn ensure_app_data_dir_creates_the_platform_directory() {
        let dir = app_data_dir().unwrap();
        let existed_before = dir.is_dir();
        let created = ensure_app_data_dir().unwrap();
        assert_eq!(created, dir);
        assert!(dir.is_dir());
        if !existed_before {
            // remove_dir only removes empty directories, so a directory with
            // real data is never lost here.
            let _ = fs::remove_dir(&dir);
        }
    }
}
