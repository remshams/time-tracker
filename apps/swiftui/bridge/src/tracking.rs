use std::ffi::c_char;

use serde_json::{Value, json};
use tracker_domain::TrackingState;

use crate::Bridge;
use crate::backend::{Backend, BridgeError};
use crate::ffi::{ResourceJson, encode, read_identifier, timestamp};
use crate::worklogs::{WorklogJson, worklog_json};

pub(super) fn tracking_json(application: &Backend) -> Option<ResourceJson<Option<WorklogJson>>> {
    application
        .tracking_observation()
        .map(|(tracking, revision)| ResourceJson {
            value: match tracking {
                TrackingState::Idle => None,
                TrackingState::Running { worklog } => Some(WorklogJson {
                    id: worklog.id().to_string(),
                    task_id: worklog.task_id().to_string(),
                    start: timestamp(worklog.start()),
                    end: None,
                }),
            },
            revision,
        })
}

fn start_result(
    application: &Backend,
    outcome: tracker_application::SetActiveTaskOutcome,
) -> Value {
    use tracker_application::SetActiveTaskOutcome;
    let (active, stopped) = match outcome {
        SetActiveTaskOutcome::Started { worklog } => (worklog_json(&worklog), None),
        SetActiveTaskOutcome::AlreadyActive { worklog } => (
            WorklogJson {
                id: worklog.id().to_string(),
                task_id: worklog.task_id().to_string(),
                start: timestamp(worklog.start()),
                end: None,
            },
            None,
        ),
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
    let Some(bridge) = (unsafe { bridge.as_ref() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    encode(
        serde_json::to_value(tracking_json(&bridge.application))
            .map_err(|error| error.to_string().into()),
    )
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
    // SAFETY: The caller guarantees exclusive access to the live bridge.
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    // SAFETY: The caller supplies valid C strings or null.
    let result = unsafe { read_identifier(worklog_id, "Invalid worklog ID") }
        .map_err(BridgeError::from)
        .and_then(|worklog_id| {
            // SAFETY: The caller supplies a valid C string or null.
            let occurred_at = unsafe { read_identifier(occurred_at, "Invalid tracking timestamp") }
                .map_err(BridgeError::from)?;
            bridge
                .application
                .clear_active_task(worklog_id, occurred_at)
        });
    encode(result.map(|outcome| stop_result(&bridge.application, outcome)))
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
    // SAFETY: The caller guarantees exclusive access to the live bridge.
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    // SAFETY: The caller supplies valid C strings or null.
    let result = unsafe { read_identifier(task_id, "Invalid task ID") }
        .map_err(BridgeError::from)
        .and_then(|task_id| {
            let expected_active = if expected_active_id.is_null() {
                None
            } else {
                // SAFETY: The caller supplies a valid C string or null.
                Some(
                    unsafe { read_identifier(expected_active_id, "Invalid worklog ID") }
                        .map_err(BridgeError::from)?,
                )
            };
            // SAFETY: The caller supplies a valid C string or null.
            let occurred_at = unsafe { read_identifier(occurred_at, "Invalid tracking timestamp") }
                .map_err(BridgeError::from)?;
            bridge
                .application
                .set_active_task_if_active(task_id, expected_active, occurred_at)
        });
    encode(result.map(|outcome| start_result(&bridge.application, outcome)))
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
    // SAFETY: The caller guarantees exclusive access to the live bridge.
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    // SAFETY: The caller supplies valid C strings or null.
    let result = unsafe { read_identifier(worklog_id, "Invalid worklog ID") }
        .map_err(BridgeError::from)
        .and_then(|worklog_id| {
            // SAFETY: The caller supplies a valid C string or null.
            let occurred_at = unsafe { read_identifier(occurred_at, "Invalid tracking timestamp") }
                .map_err(BridgeError::from)?;
            bridge
                .application
                .clear_active_task(worklog_id, occurred_at)
        });
    encode(result.map(|outcome| stop_result(&bridge.application, outcome)))
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
    // SAFETY: The caller guarantees exclusive access to the live bridge.
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    // SAFETY: The caller supplies valid C strings or null.
    let result = unsafe { read_identifier(task_id, "Invalid task ID") }
        .map_err(BridgeError::from)
        .and_then(|task_id| {
            // SAFETY: The caller supplies a valid C string or null.
            let occurred_at = unsafe { read_identifier(occurred_at, "Invalid tracking timestamp") }
                .map_err(BridgeError::from)?;
            bridge.application.resume_tracking(task_id, occurred_at)
        });
    encode(result.map(|outcome| start_result(&bridge.application, outcome)))
}
