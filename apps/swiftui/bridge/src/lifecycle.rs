use std::ffi::{CStr, CString, c_char};
use std::path::Path;
use std::ptr;

use serde_json::json;
use tracker_application::TrackerApplication;
use tracker_storage::{SqliteRepository, default_database_path, ensure_app_data_dir};

use crate::Bridge;
use crate::backend::Backend;
use crate::ffi::{encode, read_identifier};

#[cfg(test)]
use chrono::DateTime;
#[cfg(test)]
use tracker_application::TaskOrdering;
#[cfg(test)]
use tracker_domain::TaskName;

fn open() -> Result<Bridge, String> {
    ensure_app_data_dir().map_err(|error| error.to_string())?;
    let path = default_database_path().map_err(|error| error.to_string())?;
    open_at(&path)
}

pub(super) fn open_at(path: &Path) -> Result<Bridge, String> {
    let repository = SqliteRepository::open(path).map_err(|error| error.to_string())?;
    let application = TrackerApplication::load(repository).map_err(|error| error.to_string())?;
    Ok(Bridge::new(Backend::local(application)?))
}

#[cfg(test)]
pub(super) fn open_fixture(path: &Path) -> Result<Bridge, String> {
    let mut bridge = open_at(path)?;
    if bridge.application.tasks(TaskOrdering::default()).is_empty() {
        for name in ["Review backlog", "Plan release", "Write documentation"] {
            bridge
                .application
                .create_task(TaskName::new(name).unwrap(), DateTime::UNIX_EPOCH)
                .map_err(|error| error.message)?;
        }
    }
    Ok(bridge)
}

/// Opens the same secured default database as the terminal client.
///
/// # Safety
/// `error` must point to writable storage for one C string pointer. On failure,
/// the caller owns that string and must release it with `tt_bridge_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_open(error: *mut *mut c_char) -> *mut Bridge {
    // SAFETY: The caller supplies writable error storage under this function's contract.
    unsafe { open_bridge(error, open, c"Could not open database") }
}

pub(super) unsafe fn open_bridge(
    error: *mut *mut c_char,
    opener: impl FnOnce() -> Result<Bridge, String>,
    fallback: &CStr,
) -> *mut Bridge {
    if error.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: The caller supplies writable storage as required by this function.
    unsafe { *error = ptr::null_mut() };
    match opener() {
        Ok(bridge) => Box::into_raw(Box::new(bridge)),
        Err(message) => {
            let message = CString::new(message).unwrap_or_else(|_| fallback.to_owned());
            // SAFETY: The caller supplies writable storage as required by this function.
            unsafe { *error = message.into_raw() };
            ptr::null_mut()
        }
    }
}

/// Opens a disconnected remote client without creating a local database.
/// Call `tt_bridge_check_connection` to test the connection.
///
/// # Safety
/// `endpoint` must be null or a valid C string. `error` must point to writable
/// storage. The caller must serialize all access and free returned strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_open_remote(
    endpoint: *const c_char,
    error: *mut *mut c_char,
) -> *mut Bridge {
    if error.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: The caller supplies writable storage under this contract.
    unsafe { *error = ptr::null_mut() };
    // SAFETY: The caller supplies a valid C string or null.
    let result = unsafe { read_identifier::<String>(endpoint, "Invalid server URL") }
        .and_then(|endpoint| Backend::remote(&endpoint));
    match result {
        Ok(application) => Box::into_raw(Box::new(Bridge::new(application))),
        Err(message) => {
            let message = CString::new(message)
                .unwrap_or_else(|_| c"Could not open remote client".to_owned());
            // SAFETY: The caller supplies writable storage under this contract.
            unsafe { *error = message.into_raw() };
            ptr::null_mut()
        }
    }
}

/// Releases a bridge returned by either opener.
///
/// # Safety
/// `bridge` must be null or a live pointer returned by either opener, used once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_close(bridge: *mut Bridge) {
    if !bridge.is_null() {
        // SAFETY: Ownership returns from the caller under this function's contract.
        drop(unsafe { Box::from_raw(bridge) });
    }
}

/// Tests connectivity and compatibility without adopting any resource state.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. Free the returned owned string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_check_connection(bridge: *mut Bridge) -> *mut c_char {
    // SAFETY: The caller supplies exclusive access to a live bridge or null.
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    encode(
        bridge
            .application
            .check_connection()
            .map(|_| json!({ "compatible": true })),
    )
}

/// Refreshes the task catalog and tracking state as one coherent task list.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. Free the returned owned string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_refresh_resources(bridge: *mut Bridge) -> *mut c_char {
    // SAFETY: The caller supplies exclusive access to a live bridge or null.
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    encode(
        bridge
            .application
            .refresh_resources()
            .map(|_| json!({ "refreshed": true })),
    )
}
