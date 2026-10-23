use std::ffi::c_char;

use serde::Serialize;
use serde_json::json;

use crate::Bridge;
use crate::ffi::{encode, read_identifier};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ReportRowJson {
    pub(super) task_id: String,
    pub(super) duration_microseconds: i64,
}

#[derive(Serialize)]
pub(super) struct ReportJson {
    pub(super) rows: Vec<ReportRowJson>,
    pub(super) now: String,
}

/// Refreshes totals together with the task list and tracking state.
/// The coordinator publishes the remote observations together.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. Strings must be valid C strings
/// or null. Free the returned owned string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_refresh_totals(
    bridge: *mut Bridge,
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
        let report = bridge.application.refresh_totals(start, end, now)?;
        bridge.report_observation = Some(report);
        Ok(json!({ "refreshed": true }))
    })();
    encode(result)
}

/// Returns the report captured by the last successful totals refresh.
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
