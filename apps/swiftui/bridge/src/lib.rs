//! JSON bridge between SwiftUI and the application service.

use std::ffi::{CStr, CString, c_char};
use std::path::Path;
use std::ptr;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use tracker_application::{
    ApplicationFailureCategory, TaskOrdering, TrackerApplication, WorklogCursor,
};

mod backend;
mod database;
mod inactive;
use backend::{Backend, BridgeError, InactivePreview};
pub use database::tt_bridge_open_path;
pub use inactive::{tt_bridge_archive_inactive_tasks_at, tt_bridge_preview_inactive_tasks_at};
use tracker_domain::{TaskId, TaskName, TrackingState, WorklogId, WorklogTimes};
use tracker_storage::{SqliteRepository, default_database_path, ensure_app_data_dir};

pub struct Bridge {
    application: Backend,
    inactive_preview: Option<InactivePreview>,
}

impl Bridge {
    fn new(application: Backend) -> Self {
        Self {
            application,
            inactive_preview: None,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskJson {
    id: String,
    name: String,
    archived: bool,
    latest_start: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorklogJson {
    id: String,
    task_id: String,
    start: String,
    end: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotJson {
    tasks: Vec<TaskJson>,
    active: Option<WorklogJson>,
    tasks_revision: Option<String>,
    tracking_revision: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryJson {
    worklogs: Vec<WorklogJson>,
    next_cursor: Option<String>,
    reset: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportRowJson {
    task_id: String,
    duration_microseconds: i64,
}

#[derive(Serialize)]
struct ReportJson {
    snapshot: SnapshotJson,
    rows: Vec<ReportRowJson>,
    revision: Option<String>,
    now: String,
}

fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Micros, true)
}

fn worklog_json(worklog: &tracker_domain::Worklog) -> WorklogJson {
    WorklogJson {
        id: worklog.id().to_string(),
        task_id: worklog.task_id().to_string(),
        start: timestamp(worklog.start()),
        end: worklog.end().map(timestamp),
    }
}

fn snapshot(application: &Backend) -> SnapshotJson {
    let tasks = application
        .tasks(TaskOrdering::default())
        .into_iter()
        .map(|item| TaskJson {
            id: item.task.id().to_string(),
            name: item.task.name().as_str().to_owned(),
            archived: item.task.is_archived(),
            latest_start: item.latest_work_start.map(timestamp),
        })
        .collect();
    let active = match application.current_tracking() {
        TrackingState::Idle => None,
        TrackingState::Running { worklog } => Some(WorklogJson {
            id: worklog.id().to_string(),
            task_id: worklog.task_id().to_string(),
            start: timestamp(worklog.start()),
            end: None,
        }),
    };
    let (tasks_revision, tracking_revision) = application.resource_revisions();
    SnapshotJson {
        tasks,
        active,
        tasks_revision,
        tracking_revision,
    }
}

fn encode(result: Result<Value, BridgeError>) -> *mut c_char {
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

fn open() -> Result<Bridge, String> {
    ensure_app_data_dir().map_err(|error| error.to_string())?;
    let path = default_database_path().map_err(|error| error.to_string())?;
    open_at(&path)
}

fn open_at(path: &Path) -> Result<Bridge, String> {
    let repository = SqliteRepository::open(path).map_err(|error| error.to_string())?;
    let application = TrackerApplication::load(repository).map_err(|error| error.to_string())?;
    Ok(Bridge::new(Backend::Local(application)))
}

#[cfg(test)]
fn open_fixture(path: &Path) -> Result<Bridge, String> {
    let mut bridge = open_at(path)?;
    if bridge.application.tasks(TaskOrdering::default()).is_empty() {
        for name in ["Review backlog", "Plan release", "Write documentation"] {
            bridge
                .application
                .create_task(TaskName::new(name).unwrap(), DateTime::UNIX_EPOCH)
                .map_err(|error| error.message)?;
        }
    }
    Ok(bridge)
}

/// Opens the same secured default database as the terminal client.
///
/// # Safety
/// `error` must point to writable storage for one C string pointer. On failure,
/// the caller owns that string and must release it with `tt_bridge_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_open(error: *mut *mut c_char) -> *mut Bridge {
    // SAFETY: The caller supplies writable error storage under this function's contract.
    unsafe { open_bridge(error, open, c"Could not open database") }
}

unsafe fn open_bridge(
    error: *mut *mut c_char,
    opener: impl FnOnce() -> Result<Bridge, String>,
    fallback: &CStr,
) -> *mut Bridge {
    if error.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: The caller supplies writable storage as required by this function.
    unsafe { *error = ptr::null_mut() };
    match opener() {
        Ok(bridge) => Box::into_raw(Box::new(bridge)),
        Err(message) => {
            let message = CString::new(message).unwrap_or_else(|_| fallback.to_owned());
            // SAFETY: The caller supplies writable storage as required by this function.
            unsafe { *error = message.into_raw() };
            ptr::null_mut()
        }
    }
}

/// Opens a disconnected remote client without creating a local database.
/// Call `tt_bridge_snapshot` with refresh enabled to test the connection.
///
/// # Safety
/// `endpoint` must be null or a valid C string. `error` must point to writable
/// storage. The caller must serialize all access and free returned strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_open_remote(
    endpoint: *const c_char,
    error: *mut *mut c_char,
) -> *mut Bridge {
    if error.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: The caller supplies writable storage under this contract.
    unsafe { *error = ptr::null_mut() };
    // SAFETY: The caller supplies a valid C string or null.
    let result = unsafe { read_identifier::<String>(endpoint, "Invalid server URL") }
        .and_then(|endpoint| Backend::remote(&endpoint));
    match result {
        Ok(application) => Box::into_raw(Box::new(Bridge::new(application))),
        Err(message) => {
            let message = CString::new(message)
                .unwrap_or_else(|_| c"Could not open remote client".to_owned());
            // SAFETY: The caller supplies writable storage under this contract.
            unsafe { *error = message.into_raw() };
            ptr::null_mut()
        }
    }
}

/// Releases a bridge returned by either opener.
///
/// # Safety
/// `bridge` must be null or a live pointer returned by either opener, used once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_close(bridge: *mut Bridge) {
    if !bridge.is_null() {
        // SAFETY: Ownership returns from the caller under this function's contract.
        drop(unsafe { Box::from_raw(bridge) });
    }
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

/// Tests connectivity and compatibility without adopting any resource state.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. Free the returned owned string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_check_connection(bridge: *mut Bridge) -> *mut c_char {
    // SAFETY: The caller supplies exclusive access to a live bridge or null.
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    encode(
        bridge
            .application
            .check_connection()
            .map(|_| json!({ "compatible": true })),
    )
}

/// Returns a JSON snapshot. A refresh rereads the authoritative backend state.
///
/// # Safety
/// `bridge` must point to a live bridge, and calls for a bridge must be serialized.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_snapshot(bridge: *mut Bridge, refresh: bool) -> *mut c_char {
    // SAFETY: The caller guarantees the bridge is live and uniquely accessed.
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    let result = if refresh {
        bridge.application.refresh()
    } else {
        Ok(())
    };
    encode(result.and_then(|_| {
        serde_json::to_value(snapshot(&bridge.application))
            .map_err(|error| error.to_string().into())
    }))
}

/// Returns task totals and separately composed task and tracking caches.
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
            snapshot: snapshot(&bridge.application),
            rows,
            revision,
            now: timestamp(cutoff),
        })
        .map_err(|error| error.to_string().into())
    })();
    encode(result)
}

// SAFETY: A non-null identifier must point to a live, NUL-terminated C string.
unsafe fn read_identifier<T: std::str::FromStr>(
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

/// Creates a task with a permanent Rust-generated ID at the submitted timestamp.
/// Returns its task ID and the committed snapshot without another database read.
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
        Ok(json!({
            "taskId": task.id().to_string(),
            "snapshot": snapshot(&bridge.application)
        }))
    })();
    encode(result)
}

/// Renames an existing task at the submitted timestamp and returns its snapshot.
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
        bridge.application.rename_task(task_id, name, occurred_at)?;
        serde_json::to_value(snapshot(&bridge.application))
            .map_err(|error| error.to_string().into())
    })();
    encode(result)
}

/// Archives a stopped task and returns the committed snapshot.
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

/// Restores a task and returns the committed snapshot.
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
        if archived {
            bridge.application.archive_task(task_id, occurred_at)?;
        } else {
            bridge.application.unarchive_task(task_id, occurred_at)?;
        }
        serde_json::to_value(snapshot(&bridge.application))
            .map_err(|error| error.to_string().into())
    })();
    encode(result)
}

/// Starts the requested task, or atomically switches from the running task.
/// Returns the committed snapshot without a second database read.
///
/// # Safety
/// `bridge` must point to a live, uniquely accessed bridge. `task_id` must be
/// null or a valid C string. Release the result with `tt_bridge_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_start_tracking(
    bridge: *mut Bridge,
    task_id: *const c_char,
) -> *mut c_char {
    // SAFETY: The caller guarantees the bridge is live and uniquely accessed.
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    // SAFETY: The caller supplies a valid C string or null.
    let result = unsafe { read_identifier(task_id, "Invalid task ID") }
        .map_err(BridgeError::from)
        .and_then(|task_id| bridge.application.set_active_task(task_id, Utc::now()));
    encode(result.and_then(|_| {
        serde_json::to_value(snapshot(&bridge.application))
            .map_err(|error| error.to_string().into())
    }))
}

/// Stops the expected worklog without stopping a different client's new timer.
/// Returns the committed snapshot without a second database read.
///
/// # Safety
/// `bridge` must point to a live, uniquely accessed bridge. `worklog_id` must be
/// null or a valid C string. Release the result with `tt_bridge_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_stop_tracking(
    bridge: *mut Bridge,
    worklog_id: *const c_char,
) -> *mut c_char {
    // SAFETY: The caller guarantees the bridge is live and uniquely accessed.
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    // SAFETY: The caller supplies a valid C string or null.
    let result = unsafe { read_identifier(worklog_id, "Invalid worklog ID") }
        .map_err(BridgeError::from)
        .and_then(|id| bridge.application.clear_active_task(id, Utc::now()));
    encode(result.and_then(|_| {
        serde_json::to_value(snapshot(&bridge.application))
            .map_err(|error| error.to_string().into())
    }))
}

/// Starts or switches tracking at the timestamp captured when the user clicked.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. Both strings must be null or
/// valid C strings. Free the result with `tt_bridge_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_start_tracking_at(
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
            bridge.application.set_active_task(task_id, occurred_at)
        });
    encode(result.and_then(|_| {
        serde_json::to_value(snapshot(&bridge.application))
            .map_err(|error| error.to_string().into())
    }))
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
    encode(result.and_then(|_| {
        serde_json::to_value(snapshot(&bridge.application))
            .map_err(|error| error.to_string().into())
    }))
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
    encode(result.and_then(|_| {
        serde_json::to_value(snapshot(&bridge.application))
            .map_err(|error| error.to_string().into())
    }))
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
    encode(result.map(|outcome| {
        json!({
            "snapshot": snapshot(&bridge.application),
            "didStop": matches!(outcome, tracker_application::ClearActiveTaskOutcome::Stopped { .. })
        })
    }))
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
    encode(result.and_then(|_| {
        serde_json::to_value(snapshot(&bridge.application))
            .map_err(|error| error.to_string().into())
    }))
}

/// Corrects timestamps only when the stored worklog still matches the editor.
/// Null end timestamps describe a running worklog.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. Strings must be null or valid
/// C strings. Free the result with `tt_bridge_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_correct_worklog_at(
    bridge: *mut Bridge,
    worklog_id: *const c_char,
    expected_start: *const c_char,
    expected_end: *const c_char,
    replacement_start: *const c_char,
    replacement_end: *const c_char,
    occurred_at: *const c_char,
) -> *mut c_char {
    // SAFETY: The caller guarantees exclusive access to the live bridge.
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    let result = (|| {
        // SAFETY: The caller supplies valid C strings or null for all inputs.
        let (id, expected_start, expected_end, replacement_start, replacement_end, occurred_at) = unsafe {
            (
                read_identifier(worklog_id, "Invalid worklog ID")?,
                read_identifier(expected_start, "Invalid original start timestamp")?,
                read_optional_timestamp(expected_end, "Invalid original end timestamp")?,
                read_identifier(replacement_start, "Invalid replacement start timestamp")?,
                read_optional_timestamp(replacement_end, "Invalid replacement end timestamp")?,
                read_identifier(occurred_at, "Invalid correction timestamp")?,
            )
        };
        let worklog = bridge.application.correct_worklog(
            id,
            WorklogTimes::new(expected_start, expected_end),
            WorklogTimes::new(replacement_start, replacement_end),
            occurred_at,
        )?;
        Ok(json!({
            "worklog": worklog_json(&worklog),
            "snapshot": snapshot(&bridge.application)
        }))
    })();
    encode(result)
}

/// Returns destination identifiers and names from the last confirmed task cache.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. Strings must be null or valid
/// C strings. Free the result with `tt_bridge_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_move_candidates(
    bridge: *mut Bridge,
    source_task_id: *const c_char,
    query: *const c_char,
) -> *mut c_char {
    // SAFETY: The caller guarantees exclusive access to the live bridge.
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    let result = (|| {
        // SAFETY: The caller supplies valid C strings or null.
        let source = unsafe { read_identifier(source_task_id, "Invalid source task ID") }?;
        if query.is_null() {
            return Err("Invalid move search query".to_owned().into());
        }
        // SAFETY: The non-null query is a valid C string under this contract.
        let query = unsafe { CStr::from_ptr(query) }
            .to_str()
            .map_err(|_| "Invalid move search query".to_owned())?;
        let candidates: Vec<Value> = bridge
            .application
            .move_candidates(source, query)
            .into_iter()
            .map(|candidate| json!({ "id": candidate.id.to_string(), "name": candidate.name }))
            .collect();
        Ok(json!(candidates))
    })();
    encode(result)
}

/// Moves a worklog only when its stored task and timestamps match the selection.
/// A null expected end describes a running worklog.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. Strings must be null or valid
/// C strings. Free the result with `tt_bridge_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_move_worklog(
    bridge: *mut Bridge,
    worklog_id: *const c_char,
    expected_task_id: *const c_char,
    expected_start: *const c_char,
    expected_end: *const c_char,
    destination_task_id: *const c_char,
) -> *mut c_char {
    // SAFETY: The caller guarantees exclusive access to the live bridge.
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    let result = (|| {
        // SAFETY: The caller supplies valid C strings or null for all inputs.
        let (id, source, start, end, destination) = unsafe {
            (
                read_identifier(worklog_id, "Invalid worklog ID")?,
                read_identifier(expected_task_id, "Invalid source task ID")?,
                read_identifier(expected_start, "Invalid original start timestamp")?,
                read_optional_timestamp(expected_end, "Invalid original end timestamp")?,
                read_identifier(destination_task_id, "Invalid destination task ID")?,
            )
        };
        let worklog = bridge.application.move_worklog(
            id,
            source,
            WorklogTimes::new(start, end),
            destination,
        )?;
        Ok(json!({ "worklog": worklog_json(&worklog), "snapshot": snapshot(&bridge.application) }))
    })();
    encode(result)
}

unsafe fn read_optional_timestamp(
    value: *const c_char,
    message: &str,
) -> Result<Option<DateTime<Utc>>, String> {
    if value.is_null() {
        return Ok(None);
    }
    // SAFETY: The caller supplies a valid C string.
    unsafe { read_identifier(value, message) }.map(Some)
}

fn parse_cursor(task_id: TaskId, text: &str) -> Result<WorklogCursor, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|_| "Invalid history cursor".to_owned())?;
    let start = value["start"]
        .as_str()
        .ok_or("Invalid history cursor")?
        .parse::<DateTime<Utc>>()
        .map_err(|_| "Invalid history cursor")?;
    let id = value["id"]
        .as_str()
        .ok_or("Invalid history cursor")?
        .parse::<WorklogId>()
        .map_err(|_| "Invalid history cursor")?;
    let revision = value["revision"].as_i64().ok_or("Invalid history cursor")?;
    Ok(WorklogCursor {
        task_id,
        start,
        id,
        revision,
    })
}

/// Returns one bounded history page and an opaque cursor for the next page.
///
/// # Safety
/// `bridge` must point to a live, uniquely accessed bridge. `task_id` must be a
/// valid C string. `cursor` may be null or a valid C string from a prior page.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_history(
    bridge: *mut Bridge,
    task_id: *const c_char,
    cursor: *const c_char,
) -> *mut c_char {
    // SAFETY: The caller guarantees the bridge is live and uniquely accessed.
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    // SAFETY: The caller supplies a valid C string or null.
    let task_id = unsafe { read_identifier::<TaskId>(task_id, "Invalid task ID") };
    let task_id = match task_id {
        Ok(task_id) => task_id,
        Err(error) => return encode(Err(error.into())),
    };
    let cursor = if cursor.is_null() {
        Ok(None)
    } else {
        // SAFETY: The caller supplies a valid C string under this function's contract.
        unsafe { CStr::from_ptr(cursor) }
            .to_str()
            .map_err(|_| "Invalid history cursor".to_owned())
            .and_then(|text| parse_cursor(task_id, text).map(Some))
    };
    let result = cursor.map_err(BridgeError::from).and_then(|cursor| {
        let (page, reset) = match bridge
            .application
            .worklogs_for_task(task_id, cursor.as_ref())
        {
            Ok(page) => (page, false),
            Err(error)
                if error.failure().category()
                    == ApplicationFailureCategory::WorklogHistoryChanged =>
            {
                (
                    bridge
                        .application
                        .worklogs_for_task(task_id, None)
                        .map_err(|error| bridge.application.history_error(error))?,
                    true,
                )
            }
            Err(error) => return Err(bridge.application.history_error(error)),
        };
        let next_cursor = page.next_cursor.map(|next| {
            json!({
                "start": timestamp(next.start),
                "id": next.id.to_string(),
                "revision": next.revision
            })
            .to_string()
        });
        Ok(HistoryJson {
            worklogs: page.worklogs.iter().map(worklog_json).collect(),
            next_cursor,
            reset,
        })
    });
    encode(
        result
            .and_then(|page| serde_json::to_value(page).map_err(|error| error.to_string().into())),
    )
}

#[cfg(test)]
mod automation_tests;

#[cfg(test)]
mod report_tests;

#[cfg(test)]
mod creation_tests;

#[cfg(test)]
mod rename_tests;

#[cfg(test)]
mod correction_tests;

#[cfg(test)]
mod move_tests;

#[cfg(test)]
mod archive_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use tracker_application::{TaskOperations, TrackingOperations};

    #[test]
    fn opening_without_error_storage_does_not_open_a_database() {
        // SAFETY: Null storage is explicitly accepted and prevents the opener from running.
        let bridge = unsafe {
            open_bridge(
                ptr::null_mut(),
                || panic!("opener must not run"),
                c"Fallback",
            )
        };
        assert!(bridge.is_null());
        // SAFETY: The public opener accepts null error storage.
        assert!(unsafe { tt_bridge_open(ptr::null_mut()) }.is_null());
    }

    #[test]
    fn opening_clears_error_storage_and_returns_an_owned_bridge() {
        let directory = tempfile::tempdir().unwrap();
        let sentinel = c"Old error".as_ptr().cast_mut();
        let mut error = sentinel;
        // SAFETY: Error storage is writable and the returned bridge is released once.
        let bridge = unsafe {
            open_bridge(
                &mut error,
                || open_at(&directory.path().join("tt.db")),
                c"Fallback",
            )
        };
        assert!(!bridge.is_null());
        assert!(error.is_null());
        // SAFETY: The helper returns ownership of a live bridge.
        unsafe { tt_bridge_close(bridge) };
    }

    #[test]
    fn opening_failure_returns_an_owned_error_string() {
        for (message, expected) in [
            ("Database failure", "Database failure"),
            ("Bad\0message", "Fallback"),
        ] {
            let mut error = ptr::null_mut();
            // SAFETY: Error storage is writable and the returned string is released once.
            let bridge =
                unsafe { open_bridge(&mut error, || Err(message.to_owned()), c"Fallback") };
            assert!(bridge.is_null());
            assert!(!error.is_null());
            // SAFETY: A failed opener returns a live C string.
            assert_eq!(unsafe { CStr::from_ptr(error) }.to_str().unwrap(), expected);
            // SAFETY: The caller owns this string and releases it once.
            unsafe { tt_bridge_string_free(error) };
        }
    }

    fn response(pointer: *mut c_char) -> Value {
        assert!(!pointer.is_null());
        // SAFETY: Tests pass strings returned by the bridge and release each once.
        unsafe {
            let value = serde_json::from_str(CStr::from_ptr(pointer).to_str().unwrap()).unwrap();
            tt_bridge_string_free(pointer);
            value
        }
    }

    fn start_tracking(bridge: &mut Bridge, task_id: TaskId) -> Value {
        let task_id = CString::new(task_id.to_string()).unwrap();
        // SAFETY: Both the bridge and task ID are live and uniquely accessed.
        response(unsafe { tt_bridge_start_tracking(bridge, task_id.as_ptr()) })
    }

    fn stop_tracking(bridge: &mut Bridge, worklog_id: &str) -> Value {
        let worklog_id = CString::new(worklog_id).unwrap();
        // SAFETY: Both the bridge and worklog ID are live and uniquely accessed.
        response(unsafe { tt_bridge_stop_tracking(bridge, worklog_id.as_ptr()) })
    }

    #[test]
    fn tracking_commands_persist_one_worklog_and_repeated_commands_are_noops() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tt.db");
        let mut bridge = open_fixture(&path).unwrap();
        let task_id = bridge.application.tasks(TaskOrdering::default())[0]
            .task
            .id();

        let started = start_tracking(&mut bridge, task_id);
        assert!(started.get("error").is_none(), "{started}");
        assert_eq!(started["data"]["active"]["taskId"], task_id.to_string());
        assert!(started["data"]["active"]["end"].is_null());
        let worklog_id = started["data"]["active"]["id"].as_str().unwrap();
        let start = started["data"]["active"]["start"].as_str().unwrap();
        let task = started["data"]["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["id"] == task_id.to_string())
            .unwrap();
        assert_eq!(task["latestStart"], start);

        let repeated = start_tracking(&mut bridge, task_id);
        assert_eq!(repeated, started);

        let stopped = stop_tracking(&mut bridge, worklog_id);
        assert!(stopped.get("error").is_none(), "{stopped}");
        assert!(stopped["data"]["active"].is_null());
        assert_eq!(stop_tracking(&mut bridge, worklog_id), stopped);
        drop(bridge);

        let mut reopened = open_fixture(&path).unwrap();
        assert!(matches!(
            reopened.application.current_tracking(),
            TrackingState::Idle
        ));
        let page = reopened
            .application
            .worklogs_for_task(task_id, None)
            .unwrap();
        assert_eq!(page.worklogs.len(), 1);
        assert_eq!(page.worklogs[0].id().to_string(), worklog_id);
        assert_eq!(timestamp(page.worklogs[0].start()), start);
        assert!(page.worklogs[0].end().unwrap() >= page.worklogs[0].start());
    }

    #[test]
    fn switching_through_the_bridge_stops_and_starts_at_the_same_instant() {
        let directory = tempfile::tempdir().unwrap();
        let mut bridge = open_fixture(&directory.path().join("tt.db")).unwrap();
        let tasks = bridge.application.tasks(TaskOrdering::default());
        let first = tasks[0].task.id();
        let second = tasks[1].task.id();
        let started = start_tracking(&mut bridge, first);
        let switched = start_tracking(&mut bridge, second);
        assert!(switched.get("error").is_none(), "{switched}");
        assert_eq!(switched["data"]["active"]["taskId"], second.to_string());
        assert_ne!(
            switched["data"]["active"]["id"],
            started["data"]["active"]["id"]
        );

        let old_page = bridge.application.worklogs_for_task(first, None).unwrap();
        assert_eq!(old_page.worklogs.len(), 1);
        assert_eq!(
            timestamp(old_page.worklogs[0].end().unwrap()),
            switched["data"]["active"]["start"].as_str().unwrap()
        );
        let new_page = bridge.application.worklogs_for_task(second, None).unwrap();
        assert_eq!(new_page.worklogs.len(), 1);
        assert!(new_page.worklogs[0].end().is_none());
    }

    #[test]
    fn stale_stop_does_not_stop_another_clients_new_timer() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tt.db");
        let mut first = open_fixture(&path).unwrap();
        let tasks = first.application.tasks(TaskOrdering::default());
        let started = start_tracking(&mut first, tasks[0].task.id());
        let expected_id = started["data"]["active"]["id"].as_str().unwrap();
        let mut second = open_fixture(&path).unwrap();
        let switched = start_tracking(&mut second, tasks[1].task.id());

        let rejected = stop_tracking(&mut first, expected_id);
        assert_eq!(
            rejected["error"],
            "Tracking state changed in another client. Refreshed state."
        );
        assert_eq!(
            serde_json::to_value(snapshot(&first.application)).unwrap()["active"],
            switched["data"]["active"]
        );
        let page = second
            .application
            .worklogs_for_task(tasks[1].task.id(), None)
            .unwrap();
        assert_eq!(page.worklogs.len(), 1);
        assert!(page.worklogs[0].end().is_none());
    }

    #[test]
    fn start_rejects_missing_and_newly_archived_tasks_without_switching() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tt.db");
        let mut bridge = open_fixture(&path).unwrap();
        let tasks = bridge.application.tasks(TaskOrdering::default());
        let started = start_tracking(&mut bridge, tasks[0].task.id());
        let mut other = open_fixture(&path).unwrap();
        other
            .application
            .archive_task(tasks[1].task.id(), Utc::now())
            .unwrap();

        let archived = start_tracking(&mut bridge, tasks[1].task.id());
        assert_eq!(
            archived["error"],
            format!("task {} is archived", tasks[1].task.id())
        );
        assert_eq!(
            start_tracking(&mut bridge, TaskId::generate())["error"],
            "Task not found"
        );
        assert_eq!(
            serde_json::to_value(snapshot(&bridge.application)).unwrap()["active"],
            started["data"]["active"]
        );
    }

    #[test]
    fn tracking_commands_reject_null_bridges_and_invalid_identifiers() {
        let directory = tempfile::tempdir().unwrap();
        let mut bridge = open_fixture(&directory.path().join("tt.db")).unwrap();
        let malformed = CString::new("not an ID").unwrap();
        let non_utf8 = CString::new(vec![0xff]).unwrap();
        // SAFETY: Null bridges are supported, and no identifiers are dereferenced.
        unsafe {
            assert_eq!(
                response(tt_bridge_start_tracking(ptr::null_mut(), ptr::null()))["error"],
                "Database is not open"
            );
            assert_eq!(
                response(tt_bridge_stop_tracking(ptr::null_mut(), ptr::null()))["error"],
                "Database is not open"
            );
        }
        for identifier in [ptr::null(), malformed.as_ptr(), non_utf8.as_ptr()] {
            // SAFETY: The bridge is uniquely accessed; inputs are null or live C strings.
            unsafe {
                assert_eq!(
                    response(tt_bridge_start_tracking(&mut bridge, identifier))["error"],
                    "Invalid task ID"
                );
                assert_eq!(
                    response(tt_bridge_stop_tracking(&mut bridge, identifier))["error"],
                    "Invalid worklog ID"
                );
            }
        }
        assert!(matches!(
            bridge.application.current_tracking(),
            TrackingState::Idle
        ));
    }

    #[test]
    fn history_cursor_rejects_incomplete_input() {
        let task_id = TaskId::generate();
        assert!(parse_cursor(task_id, "{}").is_err());
        assert!(parse_cursor(task_id, "not json").is_err());
    }

    #[test]
    fn history_cursor_round_trips_page_position() {
        let task_id = TaskId::generate();
        let id = WorklogId::generate();
        let start = DateTime::from_timestamp_micros(123_456_789).unwrap();
        let text = json!({
            "start": timestamp(start),
            "id": id.to_string(),
            "revision": 7
        })
        .to_string();

        let cursor = parse_cursor(task_id, &text).unwrap();
        assert_eq!(cursor.task_id, task_id);
        assert_eq!(cursor.start, start);
        assert_eq!(cursor.id, id);
        assert_eq!(cursor.revision, 7);
    }

    #[test]
    fn snapshot_exposes_explicit_tasks_in_the_swift_schema() {
        let directory = tempfile::tempdir().unwrap();
        let bridge = open_fixture(&directory.path().join("tt.db")).unwrap();

        let value = serde_json::to_value(snapshot(&bridge.application)).unwrap();
        let tasks = value["tasks"].as_array().unwrap();
        assert_eq!(tasks.len(), 3);
        assert_eq!(value["active"], Value::Null);
        assert!(tasks.iter().all(|task| {
            task["id"].is_string()
                && task["name"].is_string()
                && task["archived"] == false
                && task["latestStart"].is_null()
        }));
    }

    #[test]
    fn opening_a_database_starts_empty_and_preserves_existing_tasks() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tt.db");
        let mut first = open_at(&path).unwrap();
        assert!(snapshot(&first.application).tasks.is_empty());
        let task = first
            .application
            .create_task(TaskName::new("Saved project").unwrap(), Utc::now())
            .unwrap_or_else(|error| panic!("{}", error.message));
        let before = serde_json::to_value(snapshot(&first.application)).unwrap();
        drop(first);

        let second = open_at(&path).unwrap();
        assert_eq!(
            serde_json::to_value(snapshot(&second.application)).unwrap(),
            before
        );
        assert_eq!(
            snapshot(&second.application).tasks[0].id,
            task.id().to_string()
        );
    }

    #[test]
    fn history_bridge_returns_bounded_pages_without_repeating_rows() {
        let repository = SqliteRepository::open_in_memory().unwrap();
        let mut application = TrackerApplication::load(repository).unwrap();
        let start = DateTime::from_timestamp(1_000, 0).unwrap();
        let task = application
            .create_task(TaskName::new("Review backlog").unwrap(), start)
            .unwrap();
        for index in 0..51 {
            let at = start + chrono::Duration::seconds(index * 10);
            application.set_active_task(task.id(), at).unwrap();
            let active_id = match application.current_tracking() {
                TrackingState::Running { worklog } => worklog.id(),
                TrackingState::Idle => panic!("tracking should be running"),
            };
            application
                .clear_active_task(active_id, at + chrono::Duration::seconds(1))
                .unwrap();
        }

        let bridge = Box::into_raw(Box::new(Bridge::new(Backend::Local(application))));
        let task_id = CString::new(task.id().to_string()).unwrap();
        // SAFETY: The bridge and input string are live for both calls.
        let first_ptr = unsafe { tt_bridge_history(bridge, task_id.as_ptr(), ptr::null()) };
        // SAFETY: The bridge returns a valid C string.
        let first: Value =
            serde_json::from_str(unsafe { CStr::from_ptr(first_ptr) }.to_str().unwrap()).unwrap();
        let first_rows = first["data"]["worklogs"].as_array().unwrap();
        assert_eq!(first_rows.len(), 50);
        let cursor = CString::new(first["data"]["nextCursor"].as_str().unwrap()).unwrap();
        // SAFETY: The bridge and both input strings are live for this call.
        let second_ptr = unsafe { tt_bridge_history(bridge, task_id.as_ptr(), cursor.as_ptr()) };
        // SAFETY: The bridge returns a valid C string.
        let second: Value =
            serde_json::from_str(unsafe { CStr::from_ptr(second_ptr) }.to_str().unwrap()).unwrap();
        let second_rows = second["data"]["worklogs"].as_array().unwrap();
        assert_eq!(second_rows.len(), 1);
        assert_ne!(first_rows[49]["id"], second_rows[0]["id"]);
        assert!(second["data"]["nextCursor"].is_null());
        assert_eq!(second["data"]["reset"], false);
        // SAFETY: Each pointer is released once after its final use.
        unsafe {
            tt_bridge_string_free(first_ptr);
            tt_bridge_string_free(second_ptr);
            tt_bridge_close(bridge);
        }
    }

    #[test]
    fn null_bridge_returns_an_error_envelope() {
        // SAFETY: The null pointer is explicitly supported by the function.
        let response = unsafe { tt_bridge_snapshot(ptr::null_mut(), true) };
        // SAFETY: The bridge returns a valid C string.
        let text = unsafe { CStr::from_ptr(response) }.to_str().unwrap();
        assert!(text.contains("Database is not open"));
        // SAFETY: The bridge owns the string until it is freed once.
        unsafe { tt_bridge_string_free(response) };
    }
}
