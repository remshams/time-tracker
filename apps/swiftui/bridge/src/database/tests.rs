use super::*;

fn open_database(path: &Path) -> *mut Bridge {
    let path = CString::new(path.to_str().unwrap()).unwrap();
    let mut error = ptr::dangling_mut();
    // SAFETY: The path and writable error storage remain live for the call.
    let bridge = unsafe { tt_bridge_open_path(path.as_ptr(), &mut error) };
    assert!(!bridge.is_null());
    assert!(error.is_null());
    bridge
}

fn error_message(path: *const c_char) -> String {
    let mut error = ptr::null_mut();
    // SAFETY: Callers supply a live C string or null, and error is writable.
    let bridge = unsafe { tt_bridge_open_path(path, &mut error) };
    assert!(bridge.is_null());
    assert!(!error.is_null());
    // SAFETY: The returned string belongs to the test and is released once.
    unsafe {
        let message = CStr::from_ptr(error).to_str().unwrap().to_owned();
        tt_bridge_string_free(error);
        message
    }
}

#[test]
fn explicit_databases_keep_their_records_separate_and_persist_after_reopening() {
    let directory = tempfile::tempdir().unwrap();
    let first_path = directory.path().join("first database.db");
    let second_path = directory.path().join("second database.db");
    let first = open_database(&first_path);
    let second = open_database(&second_path);
    // SAFETY: The two bridge handles are live and exclusively accessed.
    unsafe {
        (*first)
            .application
            .create_task(
                TaskName::new("Isolated task").unwrap(),
                DateTime::UNIX_EPOCH,
            )
            .unwrap();
        assert!(
            (*second)
                .application
                .tasks(TaskOrdering::default())
                .is_empty()
        );
        tt_bridge_close(first);
        tt_bridge_close(second);
    }
    let reopened = open_database(&first_path);
    // SAFETY: The reopened handle is live and exclusively accessed.
    unsafe {
        let tasks = (*reopened).application.tasks(TaskOrdering::default());
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].task.name().as_str(), "Isolated task");
        tt_bridge_close(reopened);
    }
}

#[test]
fn invalid_path_inputs_return_owned_errors_and_null_error_storage_does_not_open() {
    let invalid_utf8 = CString::from_vec_with_nul(vec![0xff, 0]).unwrap();
    for path in [ptr::null(), c"".as_ptr(), invalid_utf8.as_ptr()] {
        assert_eq!(error_message(path), "Invalid database path");
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("must-not-open.db");
    let string = CString::new(path.to_str().unwrap()).unwrap();
    // SAFETY: The path string is live; null error storage is explicitly rejected.
    assert!(unsafe { tt_bridge_open_path(string.as_ptr(), ptr::null_mut()) }.is_null());
    assert!(!path.exists());
}

#[test]
fn explicit_path_preserves_storage_file_and_parent_guards() {
    let directory = tempfile::tempdir().unwrap();
    for path in [
        directory.path().to_path_buf(),
        directory.path().join("missing/tt.db"),
    ] {
        let string = CString::new(path.to_str().unwrap()).unwrap();
        assert!(!error_message(string.as_ptr()).is_empty());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let file = directory.path().join("real.db");
        let bridge = open_database(&file);
        // SAFETY: The handle is live and released once.
        unsafe { tt_bridge_close(bridge) };
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let alias = directory.path().join("alias.db");
        symlink(&file, &alias).unwrap();
        let string = CString::new(alias.to_str().unwrap()).unwrap();
        assert!(error_message(string.as_ptr()).contains("symbolic link"));
    }
}
