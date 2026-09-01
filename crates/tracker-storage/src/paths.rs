//! Platform paths for the tracker's application data.
//!
//! The database lives in the platform application-data directory: Linux uses
//! `$XDG_DATA_HOME` or `~/.local/share`, macOS uses
//! `~/Library/Application Support`. The directory is named after the product,
//! and the default database file inside it is `tt.db`.
//!
//! The final directory and the database file are guarded: symlinks and
//! objects owned by another user are rejected, new objects are created with
//! owner-only permissions, and permissions of existing owner-owned objects
//! are repaired to owner-only.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::StorageError;

/// The directory name for Time Tracker data inside the platform data
/// directory.
const APP_DIR_NAME: &str = "Time Tracker";

/// The default database file name.
const DATABASE_FILE_NAME: &str = "tt.db";

/// The permissions of the application data directory.
#[cfg(unix)]
const DIR_MODE: u32 = 0o700;

/// The permissions of the database file.
#[cfg(unix)]
const FILE_MODE: u32 = 0o600;

/// The database path inside the given application data directory.
pub(crate) fn database_path_in(app_dir: &Path) -> PathBuf {
    app_dir.join(DATABASE_FILE_NAME)
}

/// Creates the given directory, and any missing parents, with owner-only
/// permissions on Unix.
///
/// An existing directory must be a real directory owned by the current user;
/// its permissions are repaired to owner-only. A symbolic link or a
/// non-directory at the path is rejected.
pub(crate) fn ensure_dir(path: &Path) -> Result<PathBuf, StorageError> {
    if let Err(error) = create_dir_owner_only(path) {
        // A missing path is created; a path that exists but cannot be
        // created in is validated below so the failure says what is wrong
        // with the object that is there.
        if fs::symlink_metadata(path).is_err() {
            return Err(error.into());
        }
    }
    validate_dir(path)?;
    Ok(path.to_path_buf())
}

/// Creates the directory, and any missing parents, with owner-only
/// permissions on Unix. Succeeds when the directory already exists.
fn create_dir_owner_only(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(DIR_MODE);
    }
    builder.create(path)
}

/// Checks that `path` is a real, current-user-owned directory with
/// owner-only permissions, repairing the permissions when needed.
fn validate_dir(path: &Path) -> Result<(), StorageError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(StorageError::UnsafeDataDir("is a symbolic link"));
    }
    if !metadata.is_dir() {
        return Err(StorageError::UnsafeDataDir("is not a directory"));
    }
    #[cfg(unix)]
    check_owner_and_mode(path, &metadata, DIR_MODE, StorageError::UnsafeDataDir)?;
    Ok(())
}

/// Prepares the database file at `path` for opening.
///
/// A missing file is created with owner-only permissions. An existing file
/// must be a regular file owned by the current user; its permissions are
/// repaired to owner-only. A symbolic link at the path is rejected.
pub(crate) fn prepare_database_file(path: &Path) -> Result<(), StorageError> {
    match create_exclusive(path) {
        Ok(()) => {
            // The mode given to the OS is filtered by the umask, so verify
            // and repair after creation.
            #[cfg(unix)]
            repair_mode(path, FILE_MODE)?;
        }
        // The file exists, or creating it failed for a reason that
        // validation reports more precisely.
        Err(_) => validate_file(path)?,
    }
    Ok(())
}

/// Creates the database file, failing if it already exists.
fn create_exclusive(path: &Path) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(FILE_MODE);
    }
    options.open(path).map(|_| ())
}

/// Checks that `path` is a real, current-user-owned regular file with
/// owner-only permissions, repairing the permissions when needed.
fn validate_file(path: &Path) -> Result<(), StorageError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(StorageError::UnsafeDatabase("is a symbolic link"));
    }
    if !metadata.is_file() {
        return Err(StorageError::UnsafeDatabase("is not a regular file"));
    }
    #[cfg(unix)]
    check_owner_and_mode(path, &metadata, FILE_MODE, StorageError::UnsafeDatabase)?;
    Ok(())
}

/// Verifies current-user ownership and repairs the permissions of an
/// existing object to `wanted`.
#[cfg(unix)]
fn check_owner_and_mode(
    path: &Path,
    metadata: &fs::Metadata,
    wanted: u32,
    unsafe_error: fn(&'static str) -> StorageError,
) -> Result<(), StorageError> {
    use std::os::unix::fs::MetadataExt;
    if !uid_is_current(metadata.uid()) {
        return Err(unsafe_error("is not owned by the current user"));
    }
    repair_mode(path, wanted)
}

/// Whether a stored owner id belongs to the current user.
#[cfg(unix)]
fn uid_is_current(uid: u32) -> bool {
    uid == current_uid()
}

/// The effective user id of this process.
#[cfg(unix)]
fn current_uid() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

/// Sets the permission bits of the file at `path` to `wanted` unless they
/// already match.
#[cfg(unix)]
fn repair_mode(path: &Path, wanted: u32) -> Result<(), StorageError> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = fs::symlink_metadata(path)?;
    if permission_bits(&metadata) != wanted {
        let mut permissions = metadata.permissions();
        permissions.set_mode(wanted);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

/// The permission bits of a file, without type or special bits.
#[cfg(unix)]
fn permission_bits(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o777
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

    #[cfg(unix)]
    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[cfg(unix)]
    #[test]
    fn ensure_dir_repairs_permissions_of_an_existing_directory() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join(APP_DIR_NAME);
        fs::create_dir(&target).unwrap();
        let mut permissions = fs::metadata(&target).unwrap().permissions();
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
        fs::set_permissions(&target, permissions).unwrap();
        ensure_dir(&target).unwrap();
        assert_eq!(mode_of(&target), 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn ensure_dir_rejects_a_symlinked_directory() {
        let temp = tempfile::tempdir().unwrap();
        let real = temp.path().join("real");
        fs::create_dir(&real).unwrap();
        let link = temp.path().join(APP_DIR_NAME);
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let mode_before = mode_of(&real);
        let error = ensure_dir(&link).expect_err("a symlink must be rejected");
        assert_eq!(
            error.to_string(),
            "unsafe application data directory: is a symbolic link"
        );
        // The link itself was not modified or followed.
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(mode_of(&real), mode_before);
    }

    #[cfg(unix)]
    #[test]
    fn ensure_dir_rejects_a_file_at_the_directory_path() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join(APP_DIR_NAME);
        fs::write(&target, b"not a directory").unwrap();
        let error = ensure_dir(&target).expect_err("a file must be rejected");
        assert_eq!(
            error.to_string(),
            "unsafe application data directory: is not a directory"
        );
    }

    #[cfg(unix)]
    #[test]
    fn ownership_checks_accept_current_user_and_reject_others() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("file");
        fs::write(&file, b"x").unwrap();
        let metadata = fs::symlink_metadata(&file).unwrap();
        use std::os::unix::fs::MetadataExt;
        assert!(uid_is_current(metadata.uid()));
        assert!(!uid_is_current(metadata.uid() + 1));
        // The current process's own effective uid always matches.
        assert_eq!(current_uid(), current_uid());
        assert!(!uid_is_current(current_uid() + 1));
    }

    #[cfg(unix)]
    #[test]
    fn permission_bits_report_the_mode_without_type_bits() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("file");
        fs::write(&file, b"x").unwrap();
        let mut permissions = fs::metadata(&file).unwrap().permissions();
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o644);
        fs::set_permissions(&file, permissions).unwrap();
        assert_eq!(
            permission_bits(&fs::symlink_metadata(&file).unwrap()),
            0o644
        );
    }

    #[cfg(unix)]
    #[test]
    fn prepare_database_file_creates_the_file_owner_only() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tt.db");
        prepare_database_file(&path).unwrap();
        assert!(path.is_file());
        assert_eq!(mode_of(&path), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn prepare_database_file_repairs_an_existing_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tt.db");
        fs::write(&path, b"existing").unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o644);
        fs::set_permissions(&path, permissions).unwrap();
        prepare_database_file(&path).unwrap();
        assert_eq!(mode_of(&path), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn prepare_database_file_rejects_a_symlink() {
        let temp = tempfile::tempdir().unwrap();
        let real = temp.path().join("real.db");
        fs::write(&real, b"real").unwrap();
        let link = temp.path().join("tt.db");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let error = prepare_database_file(&link).expect_err("a symlink must be rejected");
        assert_eq!(
            error.to_string(),
            "unsafe database file: is a symbolic link"
        );
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(&real).unwrap(), b"real");
    }

    #[cfg(unix)]
    #[test]
    fn prepare_database_file_rejects_a_directory() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tt.db");
        fs::create_dir(&path).unwrap();
        let error = prepare_database_file(&path).expect_err("a directory must be rejected");
        assert_eq!(
            error.to_string(),
            "unsafe database file: is not a regular file"
        );
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
