use std::ffi::{CStr, c_char};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracker_application::{ApplicationFailureCategory, WorklogCursor};
use tracker_domain::{TaskId, WorklogId, WorklogTimes};

use crate::Bridge;
use crate::backend::BridgeError;
use crate::ffi::{encode, read_identifier, read_optional_timestamp, timestamp};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WorklogJson {
    pub(super) id: String,
    pub(super) task_id: String,
    pub(super) start: String,
    pub(super) end: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryJson {
    worklogs: Vec<WorklogJson>,
    next_cursor: Option<String>,
    reset: bool,
}

pub(super) fn worklog_json(worklog: &tracker_domain::Worklog) -> WorklogJson {
    WorklogJson {
        id: worklog.id().to_string(),
        task_id: worklog.task_id().to_string(),
        start: timestamp(worklog.start()),
        end: worklog.end().map(timestamp),
    }
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
            "receipt": bridge.application.command_receipt()
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
        Ok(
            json!({ "worklog": worklog_json(&worklog), "receipt": bridge.application.command_receipt() }),
        )
    })();
    encode(result)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryCursor {
    id: String,
    revision: i64,
}

pub(super) fn parse_cursor(text: &str) -> Result<WorklogCursor, String> {
    let cursor: HistoryCursor =
        serde_json::from_str(text).map_err(|_| "Invalid history cursor".to_owned())?;
    if cursor.revision < 0 {
        return Err("Invalid history cursor".to_owned());
    }
    Ok(WorklogCursor {
        id: cursor
            .id
            .parse::<WorklogId>()
            .map_err(|_| "Invalid history cursor".to_owned())?,
        revision: cursor.revision,
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
            .and_then(|text| parse_cursor(text).map(Some))
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
