use std::ffi::c_char;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use tracker_domain::TaskName;

use crate::Bridge;
use crate::backend::{Backend, BridgeError};
use crate::ffi::{ResourceJson, read_text, timestamp, with_bridge};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TaskJson {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) archived: bool,
    pub(super) latest_start: Option<String>,
}

pub(super) fn task_json(
    task: &tracker_domain::Task,
    latest_start: Option<DateTime<Utc>>,
) -> TaskJson {
    TaskJson {
        id: task.id().to_string(),
        name: task.name().as_str().to_owned(),
        archived: task.is_archived(),
        latest_start: latest_start.map(timestamp),
    }
}

pub(super) fn tasks_json(application: &Backend) -> Option<ResourceJson<Vec<TaskJson>>> {
    application.task_observation().map(|items| ResourceJson {
        value: items
            .iter()
            .map(|item| task_json(&item.task, item.latest_work_start))
            .collect(),
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
    unsafe { with_bridge(bridge, |bridge| Ok(tasks_json(&bridge.application))) }
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
    // SAFETY: The caller supplies exclusive access to a live bridge or null,
    // and valid C strings or null.
    unsafe {
        with_bridge(bridge, |bridge| {
            let name = read_task_name(name, "Invalid task name")?;
            let occurred_at = read_text(occurred_at, "Invalid creation timestamp")?;
            let task = bridge.application.create_task(name, occurred_at)?;
            Ok(task_result(&bridge.application, &task))
        })
    }
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
    // SAFETY: The caller supplies exclusive access to a live bridge or null,
    // and valid C strings or null.
    unsafe {
        with_bridge(bridge, |bridge| {
            let task_id = read_text(task_id, "Invalid task ID")?;
            let expected_name = read_task_name(expected_name, "Invalid reviewed task name")?;
            let name = read_task_name(name, "Invalid task name")?;
            let occurred_at = read_text(occurred_at, "Invalid rename timestamp")?;
            let task = bridge.application.rename_task_with_expected_name(
                task_id,
                expected_name,
                name,
                occurred_at,
            )?;
            Ok(task_result(&bridge.application, &task))
        })
    }
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
    // SAFETY: The caller supplies exclusive access to a live bridge or null,
    // and valid C strings or null.
    unsafe {
        with_bridge(bridge, |bridge| {
            let task_id = read_text(task_id, "Invalid task ID")?;
            let expected_name = read_task_name(expected_name, "Invalid reviewed task name")?;
            let occurred_at = read_text(occurred_at, "Invalid archive timestamp")?;
            let task = bridge.application.archive_task_with_expected_name(
                task_id,
                expected_name,
                occurred_at,
            )?;
            Ok(task_result(&bridge.application, &task))
        })
    }
}

unsafe fn read_task_name(pointer: *const c_char, message: &str) -> Result<TaskName, BridgeError> {
    // SAFETY: The caller supplies a valid C string or null.
    let name = unsafe { read_text::<String>(pointer, message) }?;
    TaskName::new(&name).map_err(|error| BridgeError::from(error.to_string()))
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
    // SAFETY: The caller supplies exclusive access to a live bridge or null,
    // and valid C strings or null.
    unsafe {
        with_bridge(bridge, |bridge| {
            let task_id = read_text(task_id, "Invalid task ID")?;
            let occurred_at = read_text(occurred_at, "Invalid restore timestamp")?;
            let task = bridge.application.unarchive_task(task_id, occurred_at)?;
            Ok(task_result(&bridge.application, &task))
        })
    }
}
