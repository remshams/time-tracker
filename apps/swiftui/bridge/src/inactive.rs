use super::*;
use tracker_domain::InactivityPeriod;

fn canonical(as_of: DateTime<Utc>) -> DateTime<Utc> {
    DateTime::from_timestamp_micros(as_of.timestamp_micros())
        .expect("UTC timestamp fits microseconds")
}

fn preview_tasks(preview: &InactivePreview) -> Vec<TaskJson> {
    match preview {
        InactivePreview::Local { tasks, .. } => tasks
            .iter()
            .map(|task| TaskJson {
                id: task.id().to_string(),
                name: task.name().as_str().to_owned(),
                archived: task.is_archived(),
                latest_start: None,
            })
            .collect(),
        InactivePreview::Remote { tasks, .. } => tasks
            .iter()
            .map(|task| TaskJson {
                id: task.id.clone(),
                name: task.name.clone(),
                archived: task.archived,
                latest_start: None,
            })
            .collect(),
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
    // SAFETY: The caller supplies exclusive access to a live bridge or null.
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    bridge.inactive_preview = None;
    let result = (|| {
        let period = InactivityPeriod::new(inactive_days)
            .map_err(|error| BridgeError::from(error.to_string()))?;
        // SAFETY: The caller supplies a valid C string or null.
        let as_of = unsafe { read_identifier(as_of, "Invalid archive preview timestamp") }
            .map_err(BridgeError::from)?;
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
    })();
    encode(result)
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
    // SAFETY: The caller supplies exclusive access to a live bridge or null.
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return encode(Err("Database is not open".to_owned().into()));
    };
    let preview = bridge.inactive_preview.take();
    let result = (|| {
        let period = InactivityPeriod::new(inactive_days)
            .map_err(|error| BridgeError::from(error.to_string()))?;
        // SAFETY: The caller supplies a valid C string or null.
        let as_of = unsafe { read_identifier(as_of, "Invalid archive preview timestamp") }
            .map_err(BridgeError::from)?;
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
    })();
    encode(result)
}

#[cfg(test)]
mod tests;
