use std::ffi::c_char;
use std::path::Path;

use crate::Bridge;
use crate::ffi::read_identifier;
use crate::lifecycle::{open_at, open_bridge};

#[cfg(test)]
use crate::*;

/// Opens an explicit local database with the same filesystem guards as the default database.
/// The parent directory must already exist.
///
/// # Safety
/// `path` must be null or a live, NUL-terminated C string. `error` must point to
/// writable storage for one C string pointer. The caller owns any returned
/// error string and must release it with `tt_bridge_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_open_path(
    path: *const c_char,
    error: *mut *mut c_char,
) -> *mut Bridge {
    let opener = || {
        // SAFETY: The caller supplies a live C string or null.
        unsafe { read_identifier::<String>(path, "Invalid database path") }.and_then(|path| {
            if path.is_empty() {
                return Err("Invalid database path".to_owned());
            }
            open_at(Path::new(&path))
        })
    };
    // SAFETY: The caller supplies writable error storage under this function's contract.
    unsafe { open_bridge(error, opener, c"Could not open database") }
}

#[cfg(test)]
mod tests;
