use super::*;

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
    if error.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: The caller supplies writable storage under this contract.
    unsafe { *error = ptr::null_mut() };
    // SAFETY: The caller supplies a live C string or null.
    let result =
        unsafe { read_identifier::<String>(path, "Invalid database path") }.and_then(|path| {
            if path.is_empty() {
                return Err("Invalid database path".to_owned());
            }
            open_at(Path::new(&path))
        });
    match result {
        Ok(bridge) => Box::into_raw(Box::new(bridge)),
        Err(message) => {
            let message =
                CString::new(message).unwrap_or_else(|_| c"Could not open database".to_owned());
            // SAFETY: The caller supplies writable storage under this contract.
            unsafe { *error = message.into_raw() };
            ptr::null_mut()
        }
    }
}

#[cfg(test)]
mod tests;
