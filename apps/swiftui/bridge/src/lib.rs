//! JSON bridge between SwiftUI and the application service.

mod backend;
mod database;
mod ffi;
mod inactive;
mod lifecycle;
mod reports;
mod tasks;
mod tracking;
mod worklogs;

use backend::{Backend, InactivePreview};
use ffi::timestamp;
use reports::{ReportJson, ReportRowJson};

pub use database::tt_bridge_open_path;
pub use ffi::tt_bridge_string_free;
pub use inactive::{tt_bridge_archive_inactive_tasks_at, tt_bridge_preview_inactive_tasks_at};
pub use lifecycle::{
    tt_bridge_check_connection, tt_bridge_close, tt_bridge_open, tt_bridge_open_remote,
    tt_bridge_refresh_resources,
};
pub use reports::{tt_bridge_refresh_totals, tt_bridge_report_observation};
pub use tasks::{
    tt_bridge_archive_task_with_expected_name_at, tt_bridge_create_task_at,
    tt_bridge_rename_task_with_expected_name_at, tt_bridge_tasks, tt_bridge_unarchive_task_at,
};
pub use tracking::{
    tt_bridge_pause_tracking_at, tt_bridge_resume_tracking_at,
    tt_bridge_start_tracking_if_active_at, tt_bridge_stop_tracking_at, tt_bridge_tracking,
};
pub use worklogs::{
    tt_bridge_correct_worklog_at, tt_bridge_delete_worklog, tt_bridge_history,
    tt_bridge_move_candidates, tt_bridge_move_worklog,
};

pub struct Bridge {
    application: Backend,
    inactive_preview: Option<InactivePreview>,
    report_observation: Option<ReportJson>,
}

impl Bridge {
    fn new(application: Backend) -> Self {
        Self {
            application,
            inactive_preview: None,
            report_observation: None,
        }
    }
}

#[cfg(test)]
use chrono::{DateTime, Utc};
#[cfg(test)]
use lifecycle::{open_at, open_bridge, open_fixture};
#[cfg(test)]
use serde_json::{Value, json};
#[cfg(test)]
use std::ffi::{CStr, CString, c_char};
#[cfg(test)]
use std::ptr;
#[cfg(test)]
use tasks::tasks_json;
#[cfg(test)]
use tracker_application::{TaskOrdering, TrackerApplication};
#[cfg(test)]
use tracker_domain::{TaskId, TaskName, TrackingState, WorklogId, WorklogTimes};
#[cfg(test)]
use tracker_storage::SqliteRepository;
#[cfg(test)]
use tracking::tracking_json;
#[cfg(test)]
use worklogs::worklog_json;

#[cfg(test)]
mod test_support;

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
mod deletion_tests;

#[cfg(test)]
mod archive_tests;

#[cfg(test)]
mod tests;
