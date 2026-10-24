use std::ffi::c_char;

#[cfg(test)]
use chrono::{DateTime, Utc};
use serde_json::json;
use tracker_application::timestamps::canonical_timestamp as canonical;
use tracker_domain::InactivityPeriod;

use crate::Bridge;
use crate::backend::{BridgeError, InactivePreview};
use crate::ffi::{read_text, timestamp, with_bridge};
use crate::tasks::{TaskJson, task_json};

#[cfg(test)]
use crate::*;

/// Maps captured candidates into bridge task values without exposing confirmation guards.
fn preview_tasks(preview: &InactivePreview) -> Vec<TaskJson> {
    match preview {
        InactivePreview::Local { tasks, .. } | InactivePreview::Remote { tasks, .. } => {
            tasks.iter().map(|task| task_json(task, None)).collect()
        }
    }
}

/// Previews inactive tasks and retains their confirmation context in this bridge.
/// A new attempt invalidates any earlier preview, including failed attempts.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. `as_of` must be null or a valid
/// C string. Free the result with `tt_bridge_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_preview_inactive_tasks_at(
    bridge: *mut Bridge,
    inactive_days: u32,
    as_of: *const c_char,
) -> *mut c_char {
    // SAFETY: The caller supplies exclusive access to a live bridge or null,
    // and a valid C string or null.
    unsafe {
        with_bridge(bridge, |bridge| {
            bridge.inactive_preview = None;
            let period = InactivityPeriod::new(inactive_days)
                .map_err(|error| BridgeError::from(error.to_string()))?;
            let as_of = read_text(as_of, "Invalid archive preview timestamp")?;
            let preview = bridge
                .application
                .preview_inactive_tasks(canonical(as_of), period)?;
            let value = json!({
                "asOf": timestamp(preview.as_of()),
                "inactiveDays": preview.days(),
                "tasks": preview_tasks(&preview),
            });
            bridge.inactive_preview = Some(preview);
            Ok(value)
        })
    }
}

/// Confirms the retained preview once and returns the actual archived count.
/// Confirmation consumes the context before validating or sending the request.
///
/// # Safety
/// `bridge` must be live and uniquely accessed. `as_of` must be null or a valid
/// C string. Free the result with `tt_bridge_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tt_bridge_archive_inactive_tasks_at(
    bridge: *mut Bridge,
    inactive_days: u32,
    as_of: *const c_char,
) -> *mut c_char {
    // SAFETY: The caller supplies exclusive access to a live bridge or null,
    // and a valid C string or null.
    unsafe {
        with_bridge(bridge, |bridge| {
            let preview = bridge.inactive_preview.take();
            let period = InactivityPeriod::new(inactive_days)
                .map_err(|error| BridgeError::from(error.to_string()))?;
            let as_of = read_text(as_of, "Invalid archive preview timestamp")?;
            let preview = preview.ok_or_else(|| {
                BridgeError::from("Load an archive preview before confirming".to_owned())
            })?;
            if preview.as_of() != canonical(as_of) || preview.days() != period.days() {
                return Err(
                    "Archive preview changed. Load a new preview before confirming"
                        .to_owned()
                        .into(),
                );
            }
            let archived_count = bridge.application.archive_inactive_tasks(preview)?;
            Ok(
                json!({ "archivedCount": archived_count, "receipt": bridge.application.command_receipt() }),
            )
        })
    }
}

#[cfg(test)]
mod tests;
