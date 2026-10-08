use chrono::{DateTime, Utc};
use tokio::runtime::{Builder, Runtime};
use tracker_application::{
    ApplicationError, ApplicationFailureCategory, ApplicationFailureSource, ClearActiveTaskOutcome,
    InactiveTaskOperations, MoveCandidate, ReportQueries, ReportTotals, SetActiveTaskOutcome,
    TaskListItem, TaskOperations, TaskOrdering, TaskQueries, TrackerApplication,
    TrackingOperations, WorklogCursor, WorklogOperations, WorklogPage, WorklogQueries,
};
use tracker_domain::{
    InactivityPeriod, Task, TaskId, TaskName, TrackingState, Worklog, WorklogId, WorklogTimes,
};
use tracker_remote::{InactiveTaskCandidatesDto, RemoteApplication, RemoteError};
use tracker_storage::SqliteRepository;

pub(crate) enum Backend {
    Local(TrackerApplication<SqliteRepository>),
    Remote(Box<RemoteBackend>),
}

pub(crate) struct RemoteBackend {
    application: RemoteApplication,
    runtime: Runtime,
    requires_refresh: bool,
}

pub(crate) enum InactivePreview {
    Local {
        as_of: DateTime<Utc>,
        period: InactivityPeriod,
        tasks: Vec<Task>,
    },
    Remote(InactiveTaskCandidatesDto),
}

impl InactivePreview {
    pub fn as_of(&self) -> DateTime<Utc> {
        match self {
            Self::Local { as_of, .. } => *as_of,
            Self::Remote(preview) => preview.as_of,
        }
    }

    pub fn days(&self) -> u32 {
        match self {
            Self::Local { period, .. } => period.days(),
            Self::Remote(preview) => preview.inactive_days,
        }
    }
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
    pub fn preview_inactive_tasks(
        &mut self,
        as_of: DateTime<Utc>,
        period: InactivityPeriod,
    ) -> Result<InactivePreview, BridgeError> {
        match self {
            Self::Local(application) => application
                .preview_inactive_tasks_with_period(as_of, period)
                .map(|tasks| InactivePreview::Local {
                    as_of,
                    period,
                    tasks,
                })
                .map_err(local_archive_error),
            Self::Remote(remote) => {
                let result = remote.runtime.block_on(
                    remote
                        .application
                        .preview_inactive_tasks_with_period(as_of, period),
                );
                result
                    .map(InactivePreview::Remote)
                    .map_err(|error| remote.operation_error(error, false))
            }
        }
    }

    pub fn archive_inactive_tasks(
        &mut self,
        preview: InactivePreview,
    ) -> Result<usize, BridgeError> {
        match (self, preview) {
            (
                Self::Local(application),
                InactivePreview::Local {
                    as_of,
                    period,
                    tasks,
                },
            ) => {
                let ids: Vec<_> = tasks.iter().map(Task::id).collect();
                application
                    .archive_inactive_tasks_with_period(&ids, as_of, period)
                    .map(|archived| archived.len())
                    .map_err(|error| {
                        let mut mapped = local_archive_error(error);
                        mapped.requires_refresh = true;
                        mapped
                    })
            }
            (Self::Remote(remote), InactivePreview::Remote(preview)) => {
                remote.check_write()?;
                let result = remote.runtime.block_on(
                    remote
                        .application
                        .archive_inactive_tasks_with_period(&preview),
                );
                result.map_err(|error| remote.operation_error(error, true))
            }
            _ => Err("Archive preview does not match the current data source"
                .to_owned()
                .into()),
        }
    }

    pub fn remote(endpoint: &str) -> Result<Self, String> {
        let application =
            RemoteApplication::disconnected(endpoint).map_err(|error| error.to_string())?;
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self::Remote(Box::new(RemoteBackend {
            application,
            runtime,
            requires_refresh: true,
        })))
    }

    pub fn tasks(&self, ordering: TaskOrdering) -> Vec<TaskListItem> {
        match self {
            Self::Local(application) => application.tasks(ordering),
            Self::Remote(remote) => remote.application.tasks(ordering),
        }
    }

    pub fn current_tracking(&self) -> &TrackingState {
        match self {
            Self::Local(application) => application.current_tracking(),
            Self::Remote(remote) => remote.application.current_tracking(),
        }
    }

    pub fn refresh(&mut self) -> Result<(), BridgeError> {
        match self {
            Self::Local(application) => application
                .refresh_authoritative_state()
                .map_err(local_error),
            Self::Remote(remote) => match remote.runtime.block_on(remote.application.refresh()) {
                Ok(()) => {
                    remote.requires_refresh = false;
                    Ok(())
                }
                Err(error) => {
                    remote.requires_refresh = true;
                    Err(BridgeError {
                        message: error.to_string(),
                        kind: remote_error_kind(&error),
                        uncertain: false,
                        requires_refresh: true,
                    })
                }
            },
        }
    }

    pub fn set_active_task(
        &mut self,
        task_id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, BridgeError> {
        match self {
            Self::Local(application) => application
                .set_active_task(task_id, occurred_at)
                .map_err(local_error),
            Self::Remote(remote) => {
                remote.check_write()?;
                let result = remote
                    .runtime
                    .block_on(remote.application.set_active_task(task_id, occurred_at));
                result.map_err(|error| remote.operation_error(error, true))
            }
        }
    }

    pub fn create_task(
        &mut self,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, BridgeError> {
        match self {
            Self::Local(application) => application
                .create_task(name, occurred_at)
                .map_err(local_error),
            Self::Remote(remote) => {
                remote.check_write()?;
                let result = remote
                    .runtime
                    .block_on(remote.application.create_task(name, occurred_at));
                result.map_err(|error| remote.operation_error(error, true))
            }
        }
    }

    pub fn rename_task(
        &mut self,
        task_id: TaskId,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, BridgeError> {
        match self {
            Self::Local(application) => application
                .rename_task(task_id, name, occurred_at)
                .map_err(local_error),
            Self::Remote(remote) => {
                remote.check_write()?;
                let expected_name = name.clone();
                let result = remote.runtime.block_on(remote.application.rename_task(
                    task_id,
                    name,
                    occurred_at,
                ));
                match result {
                    Ok(task)
                        if task.name() == &expected_name
                            && remote.application.task(task_id) == Some(&task) =>
                    {
                        Ok(task)
                    }
                    Ok(_) => {
                        remote.requires_refresh = true;
                        Err(BridgeError {
                            message: "Renamed task does not match returned server state".into(),
                            kind: "protocol",
                            uncertain: true,
                            requires_refresh: true,
                        })
                    }
                    Err(error) => Err(remote.operation_error(error, true)),
                }
            }
        }
    }

    pub fn clear_active_task(
        &mut self,
        worklog_id: WorklogId,
        occurred_at: DateTime<Utc>,
    ) -> Result<ClearActiveTaskOutcome, BridgeError> {
        match self {
            Self::Local(application) => application
                .clear_active_task(worklog_id, occurred_at)
                .map_err(local_error),
            Self::Remote(remote) => {
                remote.check_write()?;
                let result = remote.runtime.block_on(
                    remote
                        .application
                        .clear_active_task(worklog_id, occurred_at),
                );
                result.map_err(|error| remote.operation_error(error, true))
            }
        }
    }

    pub fn correct_worklog(
        &mut self,
        id: WorklogId,
        expected: WorklogTimes,
        replacement: WorklogTimes,
        occurred_at: DateTime<Utc>,
    ) -> Result<Worklog, BridgeError> {
        // Refresh the remote revision without replacing the editor's original timestamps.
        if matches!(self, Self::Remote(_)) {
            self.refresh()?;
        }
        match self {
            Self::Local(application) => application
                .correct_worklog(id, expected, replacement, occurred_at)
                .map_err(correction_error),
            Self::Remote(remote) => {
                let result = remote.runtime.block_on(remote.application.correct_worklog(
                    id,
                    expected,
                    replacement,
                    occurred_at,
                ));
                result.map_err(|error| {
                    let kind = correction_error_kind(&error);
                    let mut mapped = remote.operation_error(error, true);
                    if let Some(kind) = kind {
                        mapped.kind = kind;
                    }
                    mapped
                })
            }
        }
    }

    pub fn move_candidates(&self, source_task_id: TaskId, query: &str) -> Vec<MoveCandidate> {
        match self {
            Self::Local(application) => application.move_candidates(source_task_id, query),
            Self::Remote(remote) => remote.application.move_candidates(source_task_id, query),
        }
    }

    pub fn move_worklog(
        &mut self,
        id: WorklogId,
        expected_source_task_id: TaskId,
        expected: WorklogTimes,
        destination_task_id: TaskId,
    ) -> Result<Worklog, BridgeError> {
        // Refresh the revision while preserving the selected row's task and timestamps.
        if matches!(self, Self::Remote(_)) {
            self.refresh()?;
        }
        match self {
            Self::Local(application) => application
                .move_worklog(id, expected_source_task_id, expected, destination_task_id)
                .map_err(|error| local_move_error(application, error, destination_task_id)),
            Self::Remote(remote) => {
                remote.move_worklog(id, expected_source_task_id, expected, destination_task_id)
            }
        }
    }

    pub fn resume_tracking(
        &mut self,
        task_id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, BridgeError> {
        match self {
            Self::Local(application) => application
                .start_tracking_if_idle(task_id, occurred_at)
                .map_err(local_error),
            Self::Remote(_) => self.set_active_task_if_active(task_id, None, occurred_at),
        }
    }

    pub fn set_active_task_if_active(
        &mut self,
        task_id: TaskId,
        expected_active: Option<WorklogId>,
        occurred_at: DateTime<Utc>,
    ) -> Result<SetActiveTaskOutcome, BridgeError> {
        let current_active = match self.current_tracking() {
            TrackingState::Idle => None,
            TrackingState::Running { worklog } => Some(worklog.id()),
        };
        if current_active != expected_active {
            if let Self::Remote(remote) = self {
                remote.requires_refresh = true;
            }
            return Err(BridgeError {
                message: "Tracking state changed. Refresh and retry.".into(),
                kind: "conflict",
                uncertain: false,
                requires_refresh: true,
            });
        }
        self.set_active_task(task_id, occurred_at)
    }

    pub fn worklogs_for_task(
        &mut self,
        task_id: TaskId,
        cursor: Option<&WorklogCursor>,
    ) -> Result<WorklogPage, ApplicationError> {
        match self {
            Self::Local(application) => application.worklogs_for_task(task_id, cursor),
            Self::Remote(remote) => remote
                .runtime
                .block_on(remote.application.worklogs_for_task(task_id, cursor)),
        }
    }

    pub fn report_totals(
        &mut self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<ReportTotals, BridgeError> {
        if end <= start {
            return Err(local_error(ApplicationError::InvalidReportRange));
        }
        match self {
            Self::Local(application) => application
                .report_totals(start, end, now)
                .map_err(local_error),
            Self::Remote(remote) => {
                match remote
                    .runtime
                    .block_on(remote.application.report_totals(start, end, now))
                {
                    Ok(totals) => {
                        remote.requires_refresh = false;
                        Ok(totals)
                    }
                    Err(error) => {
                        // A failed remote query can refresh Rust's cache without returning it to Swift.
                        remote.requires_refresh = true;
                        Err(remote.operation_error(error, false))
                    }
                }
            }
        }
    }

    pub fn history_error(&mut self, error: ApplicationError) -> BridgeError {
        match self {
            Self::Local(_) => local_error(error),
            Self::Remote(remote) => remote.operation_error(error, false),
        }
    }

    pub fn archive_task(
        &mut self,
        task_id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, BridgeError> {
        self.change_task_archive(task_id, occurred_at, true)
    }

    pub fn unarchive_task(
        &mut self,
        task_id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, BridgeError> {
        self.change_task_archive(task_id, occurred_at, false)
    }

    fn change_task_archive(
        &mut self,
        task_id: TaskId,
        occurred_at: DateTime<Utc>,
        archived: bool,
    ) -> Result<Task, BridgeError> {
        match self {
            Self::Local(application) => {
                let result = if archived {
                    application.archive_task(task_id, occurred_at)
                } else {
                    application.unarchive_task(task_id, occurred_at)
                };
                result.map_err(local_archive_error)
            }
            Self::Remote(remote) => {
                remote.check_write()?;
                let previous_tracking = remote.application.current_tracking().clone();
                let result = if archived {
                    remote
                        .runtime
                        .block_on(remote.application.archive_task(task_id, occurred_at))
                } else {
                    remote
                        .runtime
                        .block_on(remote.application.unarchive_task(task_id, occurred_at))
                };
                match result {
                    Ok(task)
                        if task.is_archived() == archived
                            && remote.application.task(task_id) == Some(&task)
                            && remote.application.current_tracking() == &previous_tracking =>
                    {
                        Ok(task)
                    }
                    Ok(_) => {
                        remote.requires_refresh = true;
                        Err(BridgeError {
                            message: "Task archive result does not match returned server state"
                                .into(),
                            kind: "protocol",
                            uncertain: true,
                            requires_refresh: true,
                        })
                    }
                    Err(error) => {
                        remote.requires_refresh = true;
                        let mut mapped = archive_error(&error);
                        mapped.requires_refresh = true;
                        Err(mapped)
                    }
                }
            }
        }
    }
}

impl RemoteBackend {
    fn move_worklog(
        &mut self,
        id: WorklogId,
        expected_source_task_id: TaskId,
        expected: WorklogTimes,
        destination_task_id: TaskId,
    ) -> Result<Worklog, BridgeError> {
        let previous_tracking = self.application.current_tracking().clone();
        let result = self.runtime.block_on(self.application.move_worklog(
            id,
            expected_source_task_id,
            expected,
            destination_task_id,
        ));
        match result {
            Ok(worklog)
                if self
                    .application
                    .task(destination_task_id)
                    .is_some_and(|task| !task.is_archived())
                    && moved_worklog_matches(
                        &worklog,
                        id,
                        expected,
                        destination_task_id,
                        &previous_tracking,
                        self.application.current_tracking(),
                    ) =>
            {
                Ok(worklog)
            }
            Ok(_) => {
                self.requires_refresh = true;
                Err(BridgeError {
                    message: "Moved worklog does not match returned server state".into(),
                    kind: "protocol",
                    uncertain: true,
                    requires_refresh: true,
                })
            }
            Err(error) => Err(self.move_error(error, destination_task_id)),
        }
    }

    fn move_error(&mut self, error: ApplicationError, destination_task_id: TaskId) -> BridgeError {
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
                .task(destination_task_id)
                .is_none_or(|task| task.is_archived())
        {
            mapped.kind = "destination_unavailable";
        } else if let Some(kind) = kind {
            mapped.kind = kind;
        }
        mapped
    }

    fn check_write(&self) -> Result<(), BridgeError> {
        if self.requires_refresh {
            return Err(BridgeError {
                message: "Refresh server state before changing tracker state".into(),
                kind: "unavailable",
                uncertain: false,
                requires_refresh: true,
            });
        }
        Ok(())
    }

    fn operation_error(&mut self, error: ApplicationError, write: bool) -> BridgeError {
        let failure = error.failure();
        let kind = operation_error_kind(&error);
        let uncertain = write && matches!(kind, "unavailable" | "protocol");
        self.requires_refresh |= write || matches!(kind, "unavailable" | "protocol");
        BridgeError {
            message: failure.message().to_owned(),
            kind,
            uncertain,
            requires_refresh: self.requires_refresh,
        }
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
    application: &TrackerApplication<SqliteRepository>,
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

fn moved_worklog_matches(
    worklog: &Worklog,
    id: WorklogId,
    expected: WorklogTimes,
    destination_task_id: TaskId,
    previous: &TrackingState,
    current: &TrackingState,
) -> bool {
    let canonical = |value: DateTime<Utc>| value.timestamp_micros();
    if worklog.id() != id
        || worklog.task_id() != destination_task_id
        || canonical(worklog.start()) != canonical(expected.start())
        || worklog.end().map(canonical) != expected.end().map(canonical)
    {
        return false;
    }
    if expected.is_active() {
        matches!(current, TrackingState::Running { worklog: active }
            if active.id() == worklog.id()
                && active.task_id() == worklog.task_id()
                && active.start() == worklog.start())
    } else {
        current == previous
            && !matches!(current, TrackingState::Running { worklog: active } if active.id() == id)
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
