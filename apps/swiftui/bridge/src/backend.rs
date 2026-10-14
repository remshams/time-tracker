use chrono::{DateTime, Utc};
use tokio::runtime::{Builder, Runtime};
#[cfg(test)]
use tracker_application::TaskOrdering;
use tracker_application::{
    ApplicationError, ApplicationFailureCategory, ApplicationFailureSource, ClearActiveTaskOutcome,
    MoveCandidate, SetActiveTaskOutcome, TaskListItem, TrackerApplication, WorklogCursor,
    WorklogPage,
};
use tracker_client::{ClientReport, RefreshError, TrackerBackend};
use tracker_domain::{
    InactivityPeriod, Task, TaskId, TaskName, TrackingState, Worklog, WorklogId, WorklogTimes,
};
use tracker_remote::{RemoteApplication, RemoteError, ResourceSelection};
use tracker_storage::SqliteRepository;

pub(crate) use tracker_client::InactivePreview;
type ReportRows = (Vec<super::ReportRowJson>, Option<String>, DateTime<Utc>);

pub(crate) struct Backend {
    application: TrackerBackend,
    runtime: Runtime,
    requires_refresh: bool,
}

#[derive(Debug)]
pub(crate) struct BridgeError {
    pub message: String,
    pub kind: &'static str,
    pub uncertain: bool,
    pub requires_refresh: bool,
}

impl From<String> for BridgeError {
    fn from(message: String) -> Self {
        Self {
            message,
            kind: "general",
            uncertain: false,
            requires_refresh: false,
        }
    }
}

impl Backend {
    fn new(application: TrackerBackend) -> Result<Self, String> {
        let requires_refresh = application.is_remote();
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            application,
            runtime,
            requires_refresh,
        })
    }

    pub fn local(application: TrackerApplication<SqliteRepository>) -> Result<Self, String> {
        Self::new(TrackerBackend::Local(application))
    }

    pub fn remote(endpoint: &str) -> Result<Self, String> {
        let application = RemoteApplication::disconnected(endpoint)
            .map_err(|error| error.to_string())?
            .with_coherent_task_views();
        Self::new(TrackerBackend::Remote(Box::new(application)))
    }

    pub fn check_connection(&mut self) -> Result<(), BridgeError> {
        self.runtime
            .block_on(self.application.check_connection())
            .map_err(remote_read_error)
    }

    pub fn task_observation(&self) -> Option<(Vec<TaskListItem>, Option<String>)> {
        self.application.task_observation()
    }
    pub fn tracking_observation(&self) -> Option<(TrackingState, Option<String>)> {
        self.application.tracking_observation()
    }
    pub fn command_receipt(&self) -> Option<serde_json::Value> {
        self.application.command_receipt().map(|receipt| {
            serde_json::json!({ "requestId": receipt.request_id,
            "appliedRevision": receipt.applied_revision, "replayed": receipt.replayed })
        })
    }

    pub fn refresh_resources(&mut self, selection: u32) -> Result<(), BridgeError> {
        let selection = resource_selection(selection)?;
        let result = self
            .runtime
            .block_on(self.application.refresh_resources(selection));
        self.record_refresh(selection, &result);
        result.map_err(refresh_error)
    }

    /// Refreshes the selected report resources and returns their canonical cutoff.
    pub fn refresh_totals(
        &mut self,
        include_tasks: bool,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<super::ReportJson, BridgeError> {
        let selection = if include_tasks {
            ResourceSelection::TaskListWithTotals { start, end, now }
        } else {
            ResourceSelection::DailyTotals { start, end, now }
        };
        let result =
            self.runtime.block_on(
                self.application
                    .refresh_totals(include_tasks, start, end, now),
            );
        self.record_refresh(selection, &result);
        let report = result.map_err(refresh_error)?;
        Ok(super::ReportJson {
            rows: report_rows(&report)?,
            revision: report.revision,
            now: super::timestamp(report.now),
        })
    }

    fn record_refresh<T>(
        &mut self,
        selection: ResourceSelection,
        result: &Result<T, RefreshError>,
    ) {
        if !self.application.is_remote() {
            return;
        }
        if matches!(
            selection,
            ResourceSelection::TaskList | ResourceSelection::TaskListWithTotals { .. }
        ) {
            self.requires_refresh = result.is_err();
        } else {
            self.requires_refresh |= result.is_err();
        }
    }

    /// Returns report rows, their optional remote revision, and the canonical cutoff.
    pub fn report_rows(
        &mut self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<ReportRows, BridgeError> {
        let result = self
            .runtime
            .block_on(self.application.report(start, end, now));
        let report = result.map_err(|error| self.operation_error(error, false))?;
        Ok((report_rows(&report)?, report.revision, report.now))
    }

    #[cfg(test)]
    pub fn tasks(&self, ordering: TaskOrdering) -> Vec<TaskListItem> {
        self.application.tasks(ordering)
    }
    #[cfg(test)]
    pub fn current_tracking(&self) -> &TrackingState {
        self.application.current_tracking()
    }
    pub fn move_candidates(&self, source: TaskId, query: &str) -> Vec<MoveCandidate> {
        self.application.move_candidates(source, query)
    }

    fn check_write(&self) -> Result<(), BridgeError> {
        if self.application.is_remote()
            && (self.requires_refresh
                || self
                    .application
                    .remote_application()
                    .is_some_and(|application| application.last_failure().is_some()))
        {
            return Err(BridgeError {
                message: "Refresh server state before changing tracker state".into(),
                kind: "unavailable",
                uncertain: false,
                requires_refresh: true,
            });
        }
        Ok(())
    }

    fn record_write_completion(&mut self) {
        self.requires_refresh |= self
            .application
            .remote_application()
            .is_some_and(|application| application.last_failure().is_some());
    }

    fn operation_error(&mut self, error: ApplicationError, write: bool) -> BridgeError {
        if !self.application.is_remote() {
            return local_error(error);
        }
        let failure = error.failure();
        let kind = operation_error_kind(&error);
        let uncertain = write
            && self
                .application
                .remote_application()
                .is_some_and(|application| application.last_write_attempted())
            && matches!(kind, "unavailable" | "protocol");
        self.requires_refresh |= write || matches!(kind, "unavailable" | "protocol");
        BridgeError {
            message: failure.message().to_owned(),
            kind,
            uncertain,
            requires_refresh: self.requires_refresh,
        }
    }

    pub fn create_task(
        &mut self,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, BridgeError> {
        self.check_write()?;
        let result = self
            .runtime
            .block_on(self.application.create_task(name, occurred_at));
        self.record_write_completion();
        result.map_err(|error| self.operation_error(error, true))
    }

    pub fn rename_task(
        &mut self,
        task_id: TaskId,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, BridgeError> {
        self.check_write()?;
        let result =
            self.runtime
                .block_on(self.application.rename_task(task_id, name, occurred_at));
        self.record_write_completion();
        result.map_err(|error| self.operation_error(error, true))
    }

    pub fn set_active_task(
        &mut self,
        task_id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, BridgeError> {
        self.check_write()?;
        let result = self
            .runtime
            .block_on(self.application.set_active_task(task_id, occurred_at));
        self.record_write_completion();
        result.map_err(|error| self.operation_error(error, true))
    }

    pub fn clear_active_task(
        &mut self,
        worklog_id: WorklogId,
        occurred_at: DateTime<Utc>,
    ) -> Result<ClearActiveTaskOutcome, BridgeError> {
        self.check_write()?;
        let result = self
            .runtime
            .block_on(self.application.clear_active_task(worklog_id, occurred_at));
        self.record_write_completion();
        result.map_err(|error| self.operation_error(error, true))
    }

    pub fn preview_inactive_tasks(
        &mut self,
        as_of: DateTime<Utc>,
        period: InactivityPeriod,
    ) -> Result<InactivePreview, BridgeError> {
        let result = self
            .runtime
            .block_on(self.application.preview_inactive_tasks(as_of, period));
        result.map_err(|error| {
            if self.application.is_remote() {
                self.operation_error(error, false)
            } else {
                local_archive_error(error)
            }
        })
    }

    pub fn archive_inactive_tasks(
        &mut self,
        preview: InactivePreview,
    ) -> Result<usize, BridgeError> {
        self.check_write()?;
        let result = self
            .runtime
            .block_on(self.application.archive_inactive_tasks(preview));
        self.record_write_completion();
        result.map_err(|error| {
            if self.application.is_remote() {
                self.operation_error(error, true)
            } else {
                let mut mapped = local_archive_error(error);
                mapped.requires_refresh = true;
                mapped
            }
        })
    }

    pub fn correct_worklog(
        &mut self,
        id: WorklogId,
        expected: WorklogTimes,
        replacement: WorklogTimes,
        occurred_at: DateTime<Utc>,
    ) -> Result<Worklog, BridgeError> {
        let result = self.runtime.block_on(self.application.correct_worklog(
            id,
            expected,
            replacement,
            occurred_at,
        ));
        self.record_write_completion();
        result.map_err(|error| {
            if !self.application.is_remote() {
                return correction_error(error);
            }
            let kind = correction_error_kind(&error);
            let mut mapped = self.operation_error(error, true);
            if let Some(kind) = kind {
                mapped.kind = kind;
            }
            mapped
        })
    }

    pub fn move_worklog(
        &mut self,
        id: WorklogId,
        expected_source_task_id: TaskId,
        expected: WorklogTimes,
        destination_task_id: TaskId,
    ) -> Result<Worklog, BridgeError> {
        let result = self.runtime.block_on(self.application.move_worklog(
            id,
            expected_source_task_id,
            expected,
            destination_task_id,
        ));
        self.record_write_completion();
        result.map_err(|error| self.move_error(error, destination_task_id))
    }

    fn move_error(&mut self, error: ApplicationError, destination: TaskId) -> BridgeError {
        if !self.application.is_remote() {
            return local_move_error(&self.application, error, destination);
        }
        let recovery_failed = error.failure().recovery_failed();
        let kind = if error.failure().source() == ApplicationFailureSource::Operation {
            move_error_kind(&error)
        } else {
            None
        };
        let mut mapped = self.operation_error(error, true);
        if !recovery_failed
            && matches!(mapped.kind, "general" | "worklog_not_found" | "conflict")
            && matches!(
                kind,
                None | Some("worklog_not_found") | Some("destination_unavailable")
            )
            && !mapped.uncertain
            && self
                .application
                .task(destination)
                .is_none_or(|task| task.is_archived())
        {
            mapped.kind = "destination_unavailable";
        } else if let Some(kind) = kind {
            mapped.kind = kind;
        }
        mapped
    }

    pub fn resume_tracking(
        &mut self,
        task_id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, BridgeError> {
        if self.application.is_remote() {
            return self.set_active_task_if_active(task_id, None, occurred_at);
        }
        let result = self.runtime.block_on(
            self.application
                .start_tracking_if_idle(task_id, occurred_at),
        );
        result.map_err(local_error)
    }

    /// Rejects a tracking command when the reviewed active worklog no longer matches.
    pub fn set_active_task_if_active(
        &mut self,
        task_id: TaskId,
        expected: Option<WorklogId>,
        occurred_at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, BridgeError> {
        self.check_write()?;
        let result = self
            .runtime
            .block_on(self.application.set_active_task_with_expected_active(
                task_id,
                expected,
                occurred_at,
            ));
        self.record_write_completion();
        result.map_err(|error| {
            if !self.application.is_remote()
                && matches!(error, ApplicationError::TrackingStateChanged)
            {
                BridgeError {
                    message: "Tracking state changed. Refresh and retry.".into(),
                    kind: "conflict",
                    uncertain: false,
                    requires_refresh: true,
                }
            } else {
                self.operation_error(error, true)
            }
        })
    }

    pub fn worklogs_for_task(
        &mut self,
        task_id: TaskId,
        cursor: Option<&WorklogCursor>,
    ) -> Result<WorklogPage, ApplicationError> {
        self.runtime
            .block_on(self.application.worklogs_for_task(task_id, cursor))
    }
    pub fn history_error(&mut self, error: ApplicationError) -> BridgeError {
        self.operation_error(error, false)
    }

    pub fn archive_task(&mut self, id: TaskId, at: DateTime<Utc>) -> Result<Task, BridgeError> {
        self.change_task_archive(id, at, true)
    }
    pub fn unarchive_task(&mut self, id: TaskId, at: DateTime<Utc>) -> Result<Task, BridgeError> {
        self.change_task_archive(id, at, false)
    }

    fn change_task_archive(
        &mut self,
        id: TaskId,
        at: DateTime<Utc>,
        archived: bool,
    ) -> Result<Task, BridgeError> {
        self.check_write()?;
        let result = if archived {
            self.runtime.block_on(self.application.archive_task(id, at))
        } else {
            self.runtime
                .block_on(self.application.unarchive_task(id, at))
        };
        self.record_write_completion();
        result.map_err(|error| {
            if !self.application.is_remote() {
                return local_archive_error(error);
            }
            self.requires_refresh = true;
            let mut mapped = archive_error(&error);
            mapped.uncertain &= self
                .application
                .remote_application()
                .is_some_and(|application| application.last_write_attempted());
            mapped.requires_refresh = true;
            mapped
        })
    }
}

fn report_rows(report: &ClientReport) -> Result<Vec<super::ReportRowJson>, BridgeError> {
    report
        .rows
        .iter()
        .map(|row| {
            Ok(super::ReportRowJson {
                task_id: row.task_id.to_string(),
                duration_microseconds: row.duration.num_microseconds().ok_or_else(|| {
                    BridgeError::from("Report duration exceeds the supported range".to_owned())
                })?,
            })
        })
        .collect()
}

fn resource_selection(selection: u32) -> Result<ResourceSelection, BridgeError> {
    match selection {
        1 => Ok(ResourceSelection::Tasks),
        2 => Ok(ResourceSelection::Tracking),
        3 => Ok(ResourceSelection::TaskList),
        _ => Err("Invalid resource selection".to_owned().into()),
    }
}
fn remote_read_error(error: RemoteError) -> BridgeError {
    BridgeError {
        message: error.to_string(),
        kind: remote_error_kind(&error),
        uncertain: false,
        requires_refresh: true,
    }
}
fn refresh_error(error: RefreshError) -> BridgeError {
    match error {
        RefreshError::Local(error) => local_error(error),
        RefreshError::Remote(error) => remote_read_error(error),
    }
}

fn operation_error_kind(error: &ApplicationError) -> &'static str {
    let failure = error.failure();
    match failure.source() {
        ApplicationFailureSource::RemoteUnavailable => "unavailable",
        ApplicationFailureSource::RemoteProtocol => "protocol",
        ApplicationFailureSource::Operation
            if failure.category() != ApplicationFailureCategory::General
                || failure.message().contains("changed") =>
        {
            "conflict"
        }
        _ => "general",
    }
}

fn local_error(error: ApplicationError) -> BridgeError {
    error.failure().message().to_owned().into()
}

fn local_move_error(
    application: &TrackerBackend,
    error: ApplicationError,
    destination_task_id: TaskId,
) -> BridgeError {
    let mut mapped = move_error(error);
    if mapped.kind == "general"
        && !mapped.requires_refresh
        && application
            .task(destination_task_id)
            .is_none_or(|task| task.is_archived())
    {
        mapped.kind = "destination_unavailable";
        mapped.requires_refresh = true;
    }
    mapped
}

fn local_archive_error(error: ApplicationError) -> BridgeError {
    let committed = matches!(error, ApplicationError::TaskRecovery(_));
    let mut mapped = archive_error(&error);
    mapped.uncertain = committed;
    mapped
}

fn archive_error(error: &ApplicationError) -> BridgeError {
    let failure = error.failure();
    let kind = match failure.category() {
        ApplicationFailureCategory::ActiveTask => "task_active",
        ApplicationFailureCategory::TaskNotFound => "task_not_found",
        _ => match failure.source() {
            ApplicationFailureSource::RemoteUnavailable => "unavailable",
            ApplicationFailureSource::RemoteProtocol => "protocol",
            _ => "general",
        },
    };
    BridgeError {
        message: failure.message().to_owned(),
        kind,
        uncertain: matches!(
            failure.source(),
            ApplicationFailureSource::RemoteUnavailable | ApplicationFailureSource::RemoteProtocol
        ),
        requires_refresh: failure.recovery_failed()
            || matches!(kind, "task_active" | "task_not_found"),
    }
}

fn correction_error_kind(error: &ApplicationError) -> Option<&'static str> {
    match error.failure().category() {
        ApplicationFailureCategory::WorklogChanged => Some("worklog_changed"),
        ApplicationFailureCategory::WorklogOverlap => Some("worklog_overlap"),
        _ => None,
    }
}

fn correction_error(error: ApplicationError) -> BridgeError {
    let kind = correction_error_kind(&error).unwrap_or("general");
    BridgeError {
        message: error.failure().message().to_owned(),
        kind,
        uncertain: false,
        requires_refresh: kind == "worklog_changed",
    }
}

fn move_error_kind(error: &ApplicationError) -> Option<&'static str> {
    match error.failure().category() {
        ApplicationFailureCategory::WorklogNotFound => Some("worklog_not_found"),
        ApplicationFailureCategory::WorklogChanged => Some("worklog_changed"),
        ApplicationFailureCategory::WorklogOverlap => Some("worklog_overlap"),
        ApplicationFailureCategory::TaskNotFound => Some("destination_unavailable"),
        _ => None,
    }
}

fn move_error(error: ApplicationError) -> BridgeError {
    let failure = error.failure();
    BridgeError {
        message: failure.message().to_owned(),
        kind: move_error_kind(&error).unwrap_or("general"),
        uncertain: false,
        requires_refresh: failure.recovery_failed()
            || matches!(
                failure.category(),
                ApplicationFailureCategory::WorklogNotFound
                    | ApplicationFailureCategory::WorklogChanged
                    | ApplicationFailureCategory::TaskNotFound
            ),
    }
}

fn remote_error_kind(error: &RemoteError) -> &'static str {
    if error.is_unavailable() {
        "unavailable"
    } else if matches!(error, RemoteError::Http { status, .. } if status.as_u16() == 409) {
        "conflict"
    } else {
        "protocol"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovered_application_errors_keep_general_and_conflict_distinct() {
        for (category, message, expected) in [
            (
                ApplicationFailureCategory::General,
                "Task not found",
                "general",
            ),
            (
                ApplicationFailureCategory::General,
                "Tracker state changed. Refresh and retry.",
                "conflict",
            ),
            (
                ApplicationFailureCategory::WorklogHistoryChanged,
                "Reload worklog history",
                "conflict",
            ),
        ] {
            let error = ApplicationError::semantic_failure(category, message);
            assert_eq!(operation_error_kind(&error), expected);
        }
    }

    #[test]
    fn recovered_remote_failures_keep_their_source() {
        assert_eq!(
            operation_error_kind(&ApplicationError::RemoteUnavailable(
                "remote server error".into()
            )),
            "unavailable"
        );
        assert_eq!(
            operation_error_kind(&ApplicationError::RemoteProtocol("invalid response".into())),
            "protocol"
        );
        assert_eq!(
            operation_error_kind(&ApplicationError::storage_failure("local storage error")),
            "general"
        );
    }
}

#[cfg(test)]
mod archive_mapping_tests {
    use super::*;
    use tracker_application::RepositoryError;

    #[test]
    fn task_error_mapping_preserves_primary_categories_and_failed_recovery() {
        for (category, kind) in [
            (ApplicationFailureCategory::ActiveTask, "task_active"),
            (ApplicationFailureCategory::TaskNotFound, "task_not_found"),
        ] {
            let error =
                tracker_application::ApplicationError::semantic_failure(category, "Failure")
                    .with_recovery_failure(
                        tracker_application::ApplicationError::RemoteUnavailable("offline".into()),
                    );
            let mapped = archive_error(&error);
            assert_eq!(mapped.kind, kind);
            assert!(mapped.requires_refresh);
            assert!(mapped.uncertain);
        }
        let storage = ApplicationError::TaskRecovery(RepositoryError::Backend {
            message: "read failed".into(),
        });
        let mapped = local_archive_error(storage);
        assert_eq!(mapped.kind, "general");
        assert!(mapped.requires_refresh);
        assert!(mapped.uncertain);
        let write_failure =
            local_archive_error(ApplicationError::Repository(RepositoryError::Backend {
                message: "write failed".into(),
            }));
        assert_eq!(write_failure.kind, "general");
        assert!(!write_failure.requires_refresh);
        assert!(!write_failure.uncertain);
        for (error, kind) in [
            (
                ApplicationError::RemoteUnavailable("offline".into()),
                "unavailable",
            ),
            (
                ApplicationError::RemoteProtocol("malformed".into()),
                "protocol",
            ),
        ] {
            let mapped = archive_error(&error);
            assert_eq!(mapped.kind, kind);
            assert!(mapped.uncertain);
        }
    }
}
