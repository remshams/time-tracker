use std::ffi::c_char;

use serde::Serialize;
use serde_json::{Value, json};
use tracker_application::{ApplicationFailureCategory, WorklogCursor};
use tracker_domain::{TaskId, WorklogTimes};
use tracker_protocol::{WorklogCursorDto, parse_worklog_cursor};

use crate::Bridge;
use crate::backend::BridgeError;
use crate::ffi::{read_optional_text, read_text, timestamp, with_bridge};

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
    // SAFETY: The caller supplies exclusive access to a live bridge or null,
    // and valid C strings or null for all inputs.
    unsafe {
        with_bridge(bridge, |bridge| {
            let id = read_text(worklog_id, "Invalid worklog ID")?;
            let expected_start = read_text(expected_start, "Invalid original start timestamp")?;
            let expected_end = read_optional_text(expected_end, "Invalid original end timestamp")?;
            let replacement_start =
                read_text(replacement_start, "Invalid replacement start timestamp")?;
            let replacement_end =
                read_optional_text(replacement_end, "Invalid replacement end timestamp")?;
            let occurred_at = read_text(occurred_at, "Invalid correction timestamp")?;
            let worklog = bridge.application.correct_worklog(
                id,
                WorklogTimes::new(expected_start, expected_end),
                WorklogTimes::new(replacement_start, replacement_end),
                occurred_at,
            )?;
            Ok(worklog_result(bridge, &worklog))
        })
    }
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
    // SAFETY: The caller supplies exclusive access to a live bridge or null,
    // and valid C strings or null.
    unsafe {
        with_bridge(bridge, |bridge| {
            let source = read_text(source_task_id, "Invalid source task ID")?;
            let query: String = read_text(query, "Invalid move search query")?;
            let candidates: Vec<Value> = bridge
                .application
                .move_candidates(source, &query)
                .into_iter()
                .map(|candidate| json!({ "id": candidate.id.to_string(), "name": candidate.name }))
                .collect();
            Ok(json!(candidates))
        })
    }
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
    // SAFETY: The caller supplies exclusive access to a live bridge or null,
    // and valid C strings or null for all inputs.
    unsafe {
        with_bridge(bridge, |bridge| {
            let id = read_text(worklog_id, "Invalid worklog ID")?;
            let source = read_text(expected_task_id, "Invalid source task ID")?;
            let start = read_text(expected_start, "Invalid original start timestamp")?;
            let end = read_optional_text(expected_end, "Invalid original end timestamp")?;
            let destination = read_text(destination_task_id, "Invalid destination task ID")?;
            let worklog = bridge.application.move_worklog(
                id,
                source,
                WorklogTimes::new(start, end),
                destination,
            )?;
            Ok(worklog_result(bridge, &worklog))
        })
    }
}

/// Permanently deletes a completed worklog only when its stored task and
/// timestamps match the selection. Running worklogs cannot be deleted, so the
/// expected end is required.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. Strings must be null or valid
/// C strings. Free the result with `tt_bridge_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_delete_worklog(
    bridge: *mut Bridge,
    worklog_id: *const c_char,
    expected_task_id: *const c_char,
    expected_start: *const c_char,
    expected_end: *const c_char,
) -> *mut c_char {
    // SAFETY: The caller supplies exclusive access to a live bridge or null,
    // and valid C strings or null for all inputs.
    unsafe {
        with_bridge(bridge, |bridge| {
            let id = read_text(worklog_id, "Invalid worklog ID")?;
            let task = read_text(expected_task_id, "Invalid task ID")?;
            let start = read_text(expected_start, "Invalid original start timestamp")?;
            let end = read_text(expected_end, "Invalid original end timestamp")?;
            let worklog = bridge.application.delete_completed_worklog(
                id,
                task,
                WorklogTimes::new(start, Some(end)),
            )?;
            Ok(worklog_result(bridge, &worklog))
        })
    }
}

fn worklog_result(bridge: &Bridge, worklog: &tracker_domain::Worklog) -> Value {
    json!({ "worklog": worklog_json(worklog), "receipt": bridge.application.command_receipt() })
}

pub(super) fn parse_cursor(text: &str) -> Result<WorklogCursor, String> {
    let cursor: WorklogCursorDto =
        serde_json::from_str(text).map_err(|_| "Invalid history cursor".to_owned())?;
    let (id, revision) = parse_worklog_cursor(&cursor.id, cursor.revision)
        .map_err(|_| "Invalid history cursor".to_owned())?;
    Ok(WorklogCursor { id, revision })
}

fn encode_cursor(cursor: WorklogCursor) -> Result<String, BridgeError> {
    serde_json::to_string(&WorklogCursorDto::from(cursor)).map_err(|error| error.to_string().into())
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
    // SAFETY: The caller supplies exclusive access to a live bridge or null,
    // and valid C strings or null.
    unsafe {
        with_bridge(bridge, |bridge| {
            let task_id: TaskId = read_text(task_id, "Invalid task ID")?;
            let cursor = read_optional_text::<String>(cursor, "Invalid history cursor")?
                .map(|text| parse_cursor(&text))
                .transpose()?;
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
            let next_cursor = page.next_cursor.map(encode_cursor).transpose()?;
            Ok(HistoryJson {
                worklogs: page.worklogs.iter().map(worklog_json).collect(),
                next_cursor,
                reset,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_cursors_keep_their_token_bytes() {
        let id = "00000000-0000-7000-8000-000000000001".parse().unwrap();
        for revision in [0, 7] {
            let token = encode_cursor(WorklogCursor { id, revision }).unwrap();
            assert_eq!(
                token,
                format!(r#"{{"id":"00000000-0000-7000-8000-000000000001","revision":{revision}}}"#)
            );
            let parsed = parse_cursor(&token).unwrap();
            assert_eq!((parsed.id, parsed.revision), (id, revision));
        }
    }

    #[test]
    fn history_cursor_rejects_invalid_and_legacy_fields() {
        let id = tracker_domain::WorklogId::generate();
        assert!(parse_cursor("not json").is_err());
        for value in [
            json!({"id": id.to_string()}),
            json!({"revision": 0}),
            json!({"id": "invalid", "revision": 0}),
            json!({"id": id.to_string(), "revision": -1}),
            json!({"id": id.to_string(), "revision": 0, "start": "2026-01-01T00:00:00Z"}),
            json!({"id": id.to_string(), "revision": 0, "task_id": TaskId::generate().to_string()}),
        ] {
            assert!(parse_cursor(&value.to_string()).is_err(), "{value}");
        }
    }
}
