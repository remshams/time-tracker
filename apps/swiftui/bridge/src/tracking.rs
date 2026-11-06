use std::ffi::c_char;

use serde_json::{Value, json};
use tracker_domain::{ActiveWorklog, TrackingState};

use crate::Bridge;
use crate::backend::Backend;
use crate::ffi::{ResourceJson, read_optional_text, read_text, timestamp, with_bridge};
use crate::worklogs::{WorklogJson, worklog_json};

pub(super) fn tracking_json(application: &Backend) -> Option<ResourceJson<Option<WorklogJson>>> {
    application
        .tracking_observation()
        .map(|tracking| ResourceJson {
            value: match tracking {
                TrackingState::Idle => None,
                TrackingState::Running { worklog } => Some(active_worklog_json(&worklog)),
            },
        })
}

fn active_worklog_json(worklog: &ActiveWorklog) -> WorklogJson {
    WorklogJson {
        id: worklog.id().to_string(),
        task_id: worklog.task_id().to_string(),
        start: timestamp(worklog.start()),
        end: None,
    }
}

fn start_result(
    application: &Backend,
    outcome: tracker_application::SetActiveTaskOutcome,
) -> Value {
    use tracker_application::SetActiveTaskOutcome;
    let (active, stopped) = match outcome {
        SetActiveTaskOutcome::Started { worklog } => (worklog_json(&worklog), None),
        SetActiveTaskOutcome::AlreadyActive { worklog } => (active_worklog_json(&worklog), None),
        SetActiveTaskOutcome::Switched { stopped, started } => {
            (worklog_json(&started), Some(worklog_json(&stopped)))
        }
    };
    json!({ "active": active, "stopped": stopped, "didStop": false,
        "receipt": application.command_receipt() })
}

fn stop_result(
    application: &Backend,
    outcome: tracker_application::ClearActiveTaskOutcome,
) -> Value {
    use tracker_application::ClearActiveTaskOutcome;
    let (stopped, did_stop) = match outcome {
        ClearActiveTaskOutcome::Stopped { worklog } => (Some(worklog_json(&worklog)), true),
        ClearActiveTaskOutcome::AlreadyIdle => (None, false),
    };
    json!({ "active": null, "stopped": stopped, "didStop": did_stop,
        "receipt": application.command_receipt() })
}

/// Returns confirmed tracking, or null before its first read.
/// A confirmed idle resource has a null value inside the resource observation.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. Free the returned owned string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_tracking(bridge: *mut Bridge) -> *mut c_char {
    // SAFETY: The caller supplies exclusive access to a live bridge or null.
    unsafe { with_bridge(bridge, |bridge| Ok(tracking_json(&bridge.application))) }
}

/// Stops the expected worklog at the timestamp captured when the user clicked.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. Both strings must be null or
/// valid C strings. Free the result with `tt_bridge_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_stop_tracking_at(
    bridge: *mut Bridge,
    worklog_id: *const c_char,
    occurred_at: *const c_char,
) -> *mut c_char {
    // SAFETY: The caller supplies exclusive access to a live bridge or null,
    // and valid C strings or null.
    unsafe {
        with_bridge(bridge, |bridge| {
            let worklog_id = read_text(worklog_id, "Invalid worklog ID")?;
            let occurred_at = read_text(occurred_at, "Invalid tracking timestamp")?;
            let outcome = bridge
                .application
                .clear_active_task(worklog_id, occurred_at)?;
            Ok(stop_result(&bridge.application, outcome))
        })
    }
}

/// Starts or switches only if cached tracking still matches the rendered timer.
/// A null expected active ID means the user saw an idle timer.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. Strings must be null or valid
/// C strings. Free the result with `tt_bridge_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_start_tracking_if_active_at(
    bridge: *mut Bridge,
    task_id: *const c_char,
    expected_active_id: *const c_char,
    occurred_at: *const c_char,
) -> *mut c_char {
    // SAFETY: The caller supplies exclusive access to a live bridge or null,
    // and valid C strings or null.
    unsafe {
        with_bridge(bridge, |bridge| {
            let task_id = read_text(task_id, "Invalid task ID")?;
            let expected_active = read_optional_text(expected_active_id, "Invalid worklog ID")?;
            let occurred_at = read_text(occurred_at, "Invalid tracking timestamp")?;
            let outcome = bridge.application.set_active_task_if_active(
                task_id,
                expected_active,
                occurred_at,
            )?;
            Ok(start_result(&bridge.application, outcome))
        })
    }
}

/// Pauses the expected worklog and reports whether this command stopped it.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. Strings must be null or valid
/// C strings. Free the result with `tt_bridge_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_pause_tracking_at(
    bridge: *mut Bridge,
    worklog_id: *const c_char,
    occurred_at: *const c_char,
) -> *mut c_char {
    // SAFETY: The caller supplies exclusive access to a live bridge or null,
    // and valid C strings or null.
    unsafe {
        with_bridge(bridge, |bridge| {
            let worklog_id = read_text(worklog_id, "Invalid worklog ID")?;
            let occurred_at = read_text(occurred_at, "Invalid tracking timestamp")?;
            let outcome = bridge
                .application
                .clear_active_task(worklog_id, occurred_at)?;
            Ok(stop_result(&bridge.application, outcome))
        })
    }
}

/// Resumes a task only when the authoritative tracker remains idle.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. Strings must be null or valid
/// C strings. Free the result with `tt_bridge_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_resume_tracking_at(
    bridge: *mut Bridge,
    task_id: *const c_char,
    occurred_at: *const c_char,
) -> *mut c_char {
    // SAFETY: The caller supplies exclusive access to a live bridge or null,
    // and valid C strings or null.
    unsafe {
        with_bridge(bridge, |bridge| {
            let task_id = read_text(task_id, "Invalid task ID")?;
            let occurred_at = read_text(occurred_at, "Invalid tracking timestamp")?;
            let outcome = bridge.application.resume_tracking(task_id, occurred_at)?;
            Ok(start_result(&bridge.application, outcome))
        })
    }
}
