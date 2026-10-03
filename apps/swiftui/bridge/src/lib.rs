//! JSON bridge between SwiftUI and the application service.

use std::ffi::{CStr, CString, c_char};
use std::path::Path;
use std::ptr;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use tracker_application::{
    ApplicationFailureCategory, DEFAULT_TASK_NAMES, TaskOrdering, TaskQueries, TrackerApplication,
    TrackingOperations, WorklogCursor, WorklogQueries,
};
use tracker_domain::{TaskId, TaskName, TrackingState, WorklogId};
use tracker_storage::{SqliteRepository, default_database_path, ensure_app_data_dir};

pub struct Bridge {
    application: TrackerApplication<SqliteRepository>,
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
struct SnapshotJson {
    tasks: Vec<TaskJson>,
    active: Option<WorklogJson>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryJson {
    worklogs: Vec<WorklogJson>,
    next_cursor: Option<String>,
    reset: bool,
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

fn snapshot(application: &TrackerApplication<SqliteRepository>) -> SnapshotJson {
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
    SnapshotJson { tasks, active }
}

fn encode(result: Result<Value, String>) -> *mut c_char {
    let value = match result {
        Ok(data) => json!({ "data": data }),
        Err(error) => json!({ "error": error }),
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
    let names: Vec<TaskName> = DEFAULT_TASK_NAMES
        .iter()
        .map(|name| TaskName::new(name).expect("default task name is valid"))
        .collect();
    repository
        .seed_default_tasks(&names, Utc::now())
        .map_err(|error| error.to_string())?;
    let application = TrackerApplication::load(repository).map_err(|error| error.to_string())?;
    Ok(Bridge { application })
}

/// Opens the same secured default database as the terminal client.
///
/// # Safety
/// `error` must point to writable storage for one C string pointer. On failure,
/// the caller owns that string and must release it with `tt_bridge_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_open(error: *mut *mut c_char) -> *mut Bridge {
    if error.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: The caller supplies writable storage as required by this function.
    unsafe { *error = ptr::null_mut() };
    match open() {
        Ok(bridge) => Box::into_raw(Box::new(bridge)),
        Err(message) => {
            let message =
                CString::new(message).unwrap_or_else(|_| c"Could not open database".to_owned());
            // SAFETY: The caller supplies writable storage as required by this function.
            unsafe { *error = message.into_raw() };
            ptr::null_mut()
        }
    }
}

/// Releases a bridge returned by `tt_bridge_open`.
///
/// # Safety
/// `bridge` must be null or a live pointer returned by `tt_bridge_open`, used once.
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

/// Returns a JSON snapshot. A refresh rereads the authoritative database state.
///
/// # Safety
/// `bridge` must point to a live bridge, and calls for a bridge must be serialized.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_snapshot(bridge: *mut Bridge, refresh: bool) -> *mut c_char {
    // SAFETY: The caller guarantees the bridge is live and uniquely accessed.
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return encode(Err("Database is not open".to_owned()));
    };
    let result = if refresh {
        bridge
            .application
            .refresh_authoritative_state()
            .map_err(|error| error.failure().message().to_owned())
    } else {
        Ok(())
    };
    encode(result.and_then(|_| {
        serde_json::to_value(snapshot(&bridge.application)).map_err(|error| error.to_string())
    }))
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
        return encode(Err("Database is not open".to_owned()));
    };
    // SAFETY: The caller supplies a valid C string or null.
    let result = unsafe { read_identifier(task_id, "Invalid task ID") }.and_then(|task_id| {
        bridge
            .application
            .set_active_task(task_id, Utc::now())
            .map_err(|error| error.failure().message().to_owned())
    });
    encode(result.and_then(|_| {
        serde_json::to_value(snapshot(&bridge.application)).map_err(|error| error.to_string())
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
        return encode(Err("Database is not open".to_owned()));
    };
    // SAFETY: The caller supplies a valid C string or null.
    let result = unsafe { read_identifier(worklog_id, "Invalid worklog ID") }.and_then(|id| {
        bridge
            .application
            .clear_active_task(id, Utc::now())
            .map_err(|error| error.failure().message().to_owned())
    });
    encode(result.and_then(|_| {
        serde_json::to_value(snapshot(&bridge.application)).map_err(|error| error.to_string())
    }))
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
        return encode(Err("Database is not open".to_owned()));
    };
    // SAFETY: The caller supplies a valid C string or null.
    let task_id = unsafe { read_identifier::<TaskId>(task_id, "Invalid task ID") };
    let task_id = match task_id {
        Ok(task_id) => task_id,
        Err(error) => return encode(Err(error)),
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
    let result = cursor.and_then(|cursor| {
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
                        .map_err(|error| error.failure().message().to_owned())?,
                    true,
                )
            }
            Err(error) => return Err(error.failure().message().to_owned()),
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
    encode(result.and_then(|page| serde_json::to_value(page).map_err(|error| error.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracker_application::{TaskOperations, TrackingOperations};

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
        let mut bridge = open_at(&path).unwrap();
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

        let mut reopened = open_at(&path).unwrap();
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
        let mut bridge = open_at(&directory.path().join("tt.db")).unwrap();
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
        let mut first = open_at(&path).unwrap();
        let tasks = first.application.tasks(TaskOrdering::default());
        let started = start_tracking(&mut first, tasks[0].task.id());
        let expected_id = started["data"]["active"]["id"].as_str().unwrap();
        let mut second = open_at(&path).unwrap();
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
        let mut bridge = open_at(&path).unwrap();
        let tasks = bridge.application.tasks(TaskOrdering::default());
        let started = start_tracking(&mut bridge, tasks[0].task.id());
        let mut other = open_at(&path).unwrap();
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
        let mut bridge = open_at(&directory.path().join("tt.db")).unwrap();
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
    fn snapshot_exposes_seeded_tasks_in_the_swift_schema() {
        let repository = SqliteRepository::open_in_memory().unwrap();
        let names: Vec<TaskName> = DEFAULT_TASK_NAMES
            .iter()
            .map(|name| TaskName::new(name).unwrap())
            .collect();
        repository.seed_default_tasks(&names, Utc::now()).unwrap();
        let application = TrackerApplication::load(repository).unwrap();

        let value = serde_json::to_value(snapshot(&application)).unwrap();
        let tasks = value["tasks"].as_array().unwrap();
        assert_eq!(tasks.len(), DEFAULT_TASK_NAMES.len());
        assert_eq!(value["active"], Value::Null);
        assert!(tasks.iter().all(|task| {
            task["id"].is_string()
                && task["name"].is_string()
                && task["archived"] == false
                && task["latestStart"].is_null()
        }));
    }

    #[test]
    fn opening_a_database_seeds_once_and_preserves_existing_tasks() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tt.db");
        let first = open_at(&path).unwrap();
        assert_eq!(
            snapshot(&first.application).tasks.len(),
            DEFAULT_TASK_NAMES.len()
        );
        drop(first);

        let second = open_at(&path).unwrap();
        assert_eq!(
            snapshot(&second.application).tasks.len(),
            DEFAULT_TASK_NAMES.len()
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

        let bridge = Box::into_raw(Box::new(Bridge { application }));
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
