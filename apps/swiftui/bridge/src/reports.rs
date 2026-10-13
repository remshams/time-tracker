use std::ffi::c_char;

use serde::Serialize;
use serde_json::json;

use crate::Bridge;
use crate::backend::BridgeError;
use crate::ffi::{encode, read_identifier, timestamp};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ReportRowJson {
    pub(super) task_id: String,
    pub(super) duration_microseconds: i64,
}

#[derive(Serialize)]
pub(super) struct ReportJson {
    pub(super) rows: Vec<ReportRowJson>,
    pub(super) revision: Option<String>,
    pub(super) now: String,
}

/// Refreshes totals with tracking and, when requested, the task list.
/// The coordinator publishes the requested remote observations together.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. Strings must be valid C strings
/// or null. Free the returned owned string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_refresh_totals(
    bridge: *mut Bridge,
    include_tasks: bool,
    start: *const c_char,
    end: *const c_char,
    now: *const c_char,
) -> *mut c_char {
    // SAFETY: The caller supplies exclusive access to a live bridge or null.
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    let result = (|| {
        // SAFETY: The caller supplies valid C strings or null.
        let (start, end, now) = unsafe {
            (
                read_identifier(start, "Invalid report start")?,
                read_identifier(end, "Invalid report end")?,
                read_identifier(now, "Invalid report timestamp")?,
            )
        };
        let report = bridge
            .application
            .refresh_totals(include_tasks, start, end, now)?;
        bridge.report_observation = Some(report);
        Ok(json!({ "refreshed": true }))
    })();
    encode(result)
}

/// Returns the report captured by the last successful selected totals refresh.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. Free the returned owned string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_report_observation(bridge: *mut Bridge) -> *mut c_char {
    // SAFETY: The caller supplies exclusive access to a live bridge or null.
    let Some(bridge) = (unsafe { bridge.as_ref() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    encode(
        serde_json::to_value(&bridge.report_observation).map_err(|error| error.to_string().into()),
    )
}

/// Returns task totals without reading task or tracking resources.
/// The caller supplies the UTC interval and the instant used to cap active work.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. Strings must be null or valid
/// C strings. Free the result with `tt_bridge_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_report(
    bridge: *mut Bridge,
    start: *const c_char,
    end: *const c_char,
    now: *const c_char,
) -> *mut c_char {
    // SAFETY: The caller guarantees exclusive access to the live bridge.
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    let result = (|| {
        // SAFETY: The caller supplies valid C strings or null.
        let start =
            unsafe { read_identifier(start, "Invalid report start") }.map_err(BridgeError::from)?;
        // SAFETY: The caller supplies valid C strings or null.
        let end =
            unsafe { read_identifier(end, "Invalid report end") }.map_err(BridgeError::from)?;
        // SAFETY: The caller supplies valid C strings or null.
        let now = unsafe { read_identifier(now, "Invalid report timestamp") }
            .map_err(BridgeError::from)?;
        let (rows, revision, cutoff) = bridge.application.report_rows(start, end, now)?;
        serde_json::to_value(ReportJson {
            rows,
            revision,
            now: timestamp(cutoff),
        })
        .map_err(|error| error.to_string().into())
    })();
    encode(result)
}
