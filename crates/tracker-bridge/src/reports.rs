use std::ffi::c_char;

use serde::Serialize;
use serde_json::json;

use crate::Bridge;
use crate::ffi::{read_text, with_bridge};

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
    // SAFETY: The caller supplies exclusive access to a live bridge or null,
    // and valid C strings or null.
    unsafe {
        with_bridge(bridge, |bridge| {
            let start = read_text(start, "Invalid report start")?;
            let end = read_text(end, "Invalid report end")?;
            let now = read_text(now, "Invalid report timestamp")?;
            let report = bridge.application.refresh_totals(start, end, now)?;
            bridge.report_observation = Some(report);
            Ok(json!({ "refreshed": true }))
        })
    }
}

/// Returns the report captured by the last successful totals refresh.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. Free the returned owned string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_report_observation(bridge: *mut Bridge) -> *mut c_char {
    // SAFETY: The caller supplies exclusive access to a live bridge or null.
    unsafe {
        with_bridge(bridge, |bridge| {
            serde_json::to_value(&bridge.report_observation)
                .map_err(|error| error.to_string().into())
        })
    }
}
