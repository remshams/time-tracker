use std::ffi::{CStr, CString, c_char};
use std::str::FromStr;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use serde_json::{Value, json};

use crate::Bridge;
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

/// Runs `body` against an open bridge and encodes its result as a JSON envelope.
/// A null handle returns the "Database is not open" error without running `body`.
///
/// # Safety
/// `bridge` must be null or point to a live, uniquely accessed bridge.
pub(super) unsafe fn with_bridge<T: Serialize>(
    bridge: *mut Bridge,
    body: impl FnOnce(&mut Bridge) -> Result<T, BridgeError>,
) -> *mut c_char {
    // SAFETY: The caller supplies exclusive access to a live bridge or null.
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    encode(
        body(bridge).and_then(|value| {
            serde_json::to_value(value).map_err(|error| error.to_string().into())
        }),
    )
}

/// Parses a required C string argument. Null, non-UTF-8 and unparsable input
/// all fail with `message`.
///
/// # Safety
/// A non-null `value` must point to a live, NUL-terminated C string.
pub(super) unsafe fn read_text<T: FromStr>(
    value: *const c_char,
    message: &str,
) -> Result<T, BridgeError> {
    // SAFETY: The caller supplies a valid C string or null.
    unsafe { read_optional_text(value, message) }?.ok_or_else(|| message.to_owned().into())
}

/// Parses an optional C string argument. Null means absent; non-UTF-8 and
/// unparsable input fail with `message`.
///
/// # Safety
/// A non-null `value` must point to a live, NUL-terminated C string.
pub(super) unsafe fn read_optional_text<T: FromStr>(
    value: *const c_char,
    message: &str,
) -> Result<Option<T>, BridgeError> {
    if value.is_null() {
        return Ok(None);
    }
    // SAFETY: The caller supplies a valid C string under this function's contract.
    unsafe { CStr::from_ptr(value) }
        .to_str()
        .ok()
        .and_then(|text| text.parse().ok())
        .map(Some)
        .ok_or_else(|| message.to_owned().into())
}

/// Same as `read_text` with a plain message error, for modules not yet moved to `read_text`.
///
/// # Safety
/// A non-null `identifier` must point to a live, NUL-terminated C string.
pub(super) unsafe fn read_identifier<T: FromStr>(
    identifier: *const c_char,
    message: &str,
) -> Result<T, String> {
    // SAFETY: The caller supplies a valid C string or null.
    unsafe { read_text(identifier, message) }.map_err(|error| error.message)
}
