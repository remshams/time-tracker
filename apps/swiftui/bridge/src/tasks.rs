use std::ffi::c_char;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use tracker_domain::TaskName;

use crate::Bridge;
use crate::backend::{Backend, BridgeError};
use crate::ffi::{ResourceJson, encode, read_identifier, timestamp};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TaskJson {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) archived: bool,
    pub(super) latest_start: Option<String>,
}

fn task_json(task: &tracker_domain::Task, latest_start: Option<DateTime<Utc>>) -> TaskJson {
    TaskJson {
        id: task.id().to_string(),
        name: task.name().as_str().to_owned(),
        archived: task.is_archived(),
        latest_start: latest_start.map(timestamp),
    }
}

pub(super) fn tasks_json(application: &Backend) -> Option<ResourceJson<Vec<TaskJson>>> {
    application
        .task_observation()
        .map(|(items, revision)| ResourceJson {
            value: items
                .iter()
                .map(|item| task_json(&item.task, item.latest_work_start))
                .collect(),
            revision,
        })
}

fn task_result(application: &Backend, task: &tracker_domain::Task) -> Value {
    json!({ "task": task_json(task, None), "receipt": application.command_receipt() })
}

/// Returns the last confirmed task resource, or null before its first read.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. Free the returned owned string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_tasks(bridge: *mut Bridge) -> *mut c_char {
    // SAFETY: The caller supplies exclusive access to a live bridge or null.
    let Some(bridge) = (unsafe { bridge.as_ref() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    encode(
        serde_json::to_value(tasks_json(&bridge.application))
            .map_err(|error| error.to_string().into()),
    )
}

/// Creates a task with a permanent Rust-generated ID at the submitted timestamp.
/// Returns its task ID and the committed outcome without another database read.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. Strings must be null or valid
/// C strings. Free the result with `tt_bridge_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_create_task_at(
    bridge: *mut Bridge,
    name: *const c_char,
    occurred_at: *const c_char,
) -> *mut c_char {
    // SAFETY: The caller guarantees exclusive access to the live bridge.
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    let result = (|| {
        // SAFETY: The caller supplies valid C strings or null.
        let name = unsafe { read_identifier::<String>(name, "Invalid task name") }
            .map_err(BridgeError::from)?;
        let name = TaskName::new(&name).map_err(|error| BridgeError::from(error.to_string()))?;
        // SAFETY: The caller supplies a valid C string or null.
        let occurred_at = unsafe { read_identifier(occurred_at, "Invalid creation timestamp") }
            .map_err(BridgeError::from)?;
        let task = bridge.application.create_task(name, occurred_at)?;
        Ok(task_result(&bridge.application, &task))
    })();
    encode(result)
}

/// Renames an existing task at the submitted timestamp and returns the committed task.
/// Task identity, archive state, and worklogs remain intact.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. Strings must be null or valid
/// C strings. Free the result with `tt_bridge_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_rename_task_at(
    bridge: *mut Bridge,
    task_id: *const c_char,
    name: *const c_char,
    occurred_at: *const c_char,
) -> *mut c_char {
    // SAFETY: The caller guarantees exclusive access to the live bridge.
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    let result = (|| {
        // SAFETY: The caller supplies valid C strings or null.
        let task_id =
            unsafe { read_identifier(task_id, "Invalid task ID") }.map_err(BridgeError::from)?;
        // SAFETY: The caller supplies a valid C string or null.
        let name = unsafe { read_identifier::<String>(name, "Invalid task name") }
            .map_err(BridgeError::from)?;
        let name = TaskName::new(&name).map_err(|error| BridgeError::from(error.to_string()))?;
        // SAFETY: The caller supplies a valid C string or null.
        let occurred_at = unsafe { read_identifier(occurred_at, "Invalid rename timestamp") }
            .map_err(BridgeError::from)?;
        let task = bridge.application.rename_task(task_id, name, occurred_at)?;
        Ok(task_result(&bridge.application, &task))
    })();
    encode(result)
}

/// Renames a task only if its stored name still matches the reviewed name.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. Strings must be null or valid
/// C strings. Free the result with `tt_bridge_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_rename_task_with_expected_name_at(
    bridge: *mut Bridge,
    task_id: *const c_char,
    expected_name: *const c_char,
    name: *const c_char,
    occurred_at: *const c_char,
) -> *mut c_char {
    // SAFETY: The caller supplies exclusive access to a live bridge or null.
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    let result = (|| {
        // SAFETY: The caller supplies valid C strings or null.
        let task_id =
            unsafe { read_identifier(task_id, "Invalid task ID") }.map_err(BridgeError::from)?;
        // SAFETY: The caller supplies valid C strings or null.
        let expected_name = unsafe { read_task_name(expected_name, "Invalid reviewed task name") }?;
        // SAFETY: The caller supplies valid C strings or null.
        let name = unsafe { read_task_name(name, "Invalid task name") }?;
        // SAFETY: The caller supplies valid C strings or null.
        let occurred_at = unsafe { read_identifier(occurred_at, "Invalid rename timestamp") }
            .map_err(BridgeError::from)?;
        let task = bridge.application.rename_task_with_expected_name(
            task_id,
            expected_name,
            name,
            occurred_at,
        )?;
        Ok(task_result(&bridge.application, &task))
    })();
    encode(result)
}

/// Archives a stopped task only if its stored name still matches the reviewed name.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. Strings must be null or valid
/// C strings. Free the result with `tt_bridge_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_archive_task_with_expected_name_at(
    bridge: *mut Bridge,
    task_id: *const c_char,
    expected_name: *const c_char,
    occurred_at: *const c_char,
) -> *mut c_char {
    // SAFETY: The caller supplies exclusive access to a live bridge or null.
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    let result = (|| {
        // SAFETY: The caller supplies valid C strings or null.
        let task_id =
            unsafe { read_identifier(task_id, "Invalid task ID") }.map_err(BridgeError::from)?;
        // SAFETY: The caller supplies valid C strings or null.
        let expected_name = unsafe { read_task_name(expected_name, "Invalid reviewed task name") }?;
        // SAFETY: The caller supplies valid C strings or null.
        let occurred_at = unsafe { read_identifier(occurred_at, "Invalid archive timestamp") }
            .map_err(BridgeError::from)?;
        let task = bridge.application.archive_task_with_expected_name(
            task_id,
            expected_name,
            occurred_at,
        )?;
        Ok(task_result(&bridge.application, &task))
    })();
    encode(result)
}

unsafe fn read_task_name(pointer: *const c_char, message: &str) -> Result<TaskName, BridgeError> {
    // SAFETY: The caller supplies a valid C string or null.
    let name = unsafe { read_identifier::<String>(pointer, message) }.map_err(BridgeError::from)?;
    TaskName::new(&name).map_err(|error| BridgeError::from(error.to_string()))
}

/// Archives a stopped task and returns the committed outcome.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. Strings must be null or valid
/// C strings. Free the result with `tt_bridge_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_archive_task_at(
    bridge: *mut Bridge,
    task_id: *const c_char,
    occurred_at: *const c_char,
) -> *mut c_char {
    // SAFETY: The caller guarantees the bridge and strings meet this contract.
    unsafe { change_task_archive_at(bridge, task_id, occurred_at, true) }
}

/// Restores a task and returns the committed outcome.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. Strings must be null or valid
/// C strings. Free the result with `tt_bridge_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_unarchive_task_at(
    bridge: *mut Bridge,
    task_id: *const c_char,
    occurred_at: *const c_char,
) -> *mut c_char {
    // SAFETY: The caller guarantees the bridge and strings meet this contract.
    unsafe { change_task_archive_at(bridge, task_id, occurred_at, false) }
}

unsafe fn change_task_archive_at(
    bridge: *mut Bridge,
    task_id: *const c_char,
    occurred_at: *const c_char,
    archived: bool,
) -> *mut c_char {
    // SAFETY: The caller supplies exclusive access to a live bridge or null.
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    let result = (|| {
        // SAFETY: The caller supplies valid C strings or null.
        let task_id =
            unsafe { read_identifier(task_id, "Invalid task ID") }.map_err(BridgeError::from)?;
        // SAFETY: The caller supplies valid C strings or null.
        let occurred_at = unsafe { read_identifier(occurred_at, "Invalid archive timestamp") }
            .map_err(BridgeError::from)?;
        let task = if archived {
            bridge.application.archive_task(task_id, occurred_at)?
        } else {
            bridge.application.unarchive_task(task_id, occurred_at)?
        };
        Ok(task_result(&bridge.application, &task))
    })();
    encode(result)
}
