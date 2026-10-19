use std::ffi::{CStr, CString, c_char};

use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use serde_json::{Value, json};

use crate::backend::BridgeError;

#[derive(Serialize)]
pub(super) struct ResourceJson<T> {
    pub(super) value: T,
}

pub(super) fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Micros, true)
}

pub(super) fn encode(result: Result<Value, BridgeError>) -> *mut c_char {
    let value = match result {
        Ok(data) => json!({ "data": data }),
        Err(error) => {
            json!({ "error": error.message, "kind": error.kind, "uncertain": error.uncertain, "requiresRefresh": error.requires_refresh })
        }
    };
    let bytes = serde_json::to_string(&value)
        .unwrap_or_else(|_| "{\"error\":\"JSON encoding failed\"}".to_owned());
    CString::new(bytes)
        .expect("JSON has no raw NUL bytes")
        .into_raw()
}

/// Releases a string returned by this bridge.
///
/// # Safety
/// `value` must be null or a live string pointer returned by this bridge, used once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_string_free(value: *mut c_char) {
    if !value.is_null() {
        // SAFETY: Ownership returns from the caller under this function's contract.
        drop(unsafe { CString::from_raw(value) });
    }
}

// SAFETY: A non-null identifier must point to a live, NUL-terminated C string.
pub(super) unsafe fn read_identifier<T: std::str::FromStr>(
    identifier: *const c_char,
    message: &str,
) -> Result<T, String> {
    if identifier.is_null() {
        return Err(message.to_owned());
    }
    // SAFETY: The caller supplies a valid C string under this function's contract.
    unsafe { CStr::from_ptr(identifier) }
        .to_str()
        .ok()
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| message.to_owned())
}

pub(super) unsafe fn read_optional_timestamp(
    value: *const c_char,
    message: &str,
) -> Result<Option<DateTime<Utc>>, String> {
    if value.is_null() {
        return Ok(None);
    }
    // SAFETY: The caller supplies a valid C string.
    unsafe { read_identifier(value, message) }.map(Some)
}
