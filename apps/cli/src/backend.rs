use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use tracker_application::{
    ClearActiveTaskOutcome, GlobalWorklogCursor, GlobalWorklogPage, ReportQueries, ReportTotals,
    SetActiveTaskOutcome, TaskListItem, TaskOperations, TaskOrdering, TaskQueries,
    TrackerApplication, TrackingOperations, WorklogCursor, WorklogOperations, WorklogPage,
    WorklogQueries,
};
use tracker_domain::{Task, TaskId, TaskName, TrackingState, Worklog, WorklogId, WorklogTimes};
use tracker_remote::RemoteApplication;
use tracker_storage::{SqliteRepository, default_database_path, ensure_app_data_dir};

use crate::CliError;

pub(crate) struct Backend {
    pub identity: String,
    pub kind: BackendKind,
}

pub(crate) enum BackendKind {
    Local(TrackerApplication<SqliteRepository>),
    Remote(Box<RemoteApplication>),
}

pub(crate) fn identity(database: Option<&Path>, server: Option<&str>) -> Result<String, CliError> {
    if let Some(server) = server {
        RemoteApplication::disconnected(server).map_err(CliError::remote)?;
        let url = reqwest::Url::parse(server).map_err(CliError::input)?;
        return Ok(format!("remote:{url}"));
    }
    let path = database
        .map(Path::to_path_buf)
        .map(Ok)
        .unwrap_or_else(default_database_path)
        .map_err(CliError::storage)?;
    if path.to_str().is_none() {
        return Err(CliError::input("database path must be valid UTF-8"));
    }
    let path = canonical_database_path(&path).map_err(CliError::storage)?;
    let path = path
        .to_str()
        .ok_or_else(|| CliError::input("canonical database path must be valid UTF-8"))?;
    Ok(format!("local:{path}"))
}

fn canonical_database_path(path: &Path) -> std::io::Result<PathBuf> {
    let mut ancestor = std::path::absolute(path)?;
    let mut suffix = Vec::new();
    loop {
        if let Ok(mut resolved) = std::fs::canonicalize(&ancestor) {
            for component in suffix.into_iter().rev() {
                resolved.push(component);
            }
            return Ok(resolved);
        }
        let name = ancestor
            .file_name()
            .ok_or_else(|| std::io::Error::other("database ancestor cannot be resolved"))?
            .to_os_string();
        suffix.push(name);
        if !ancestor.pop() {
            return Err(std::io::Error::other(
                "database ancestor cannot be resolved",
            ));
        }
    }
}

impl Backend {
    pub async fn open(database: Option<PathBuf>, server: Option<String>) -> Result<Self, CliError> {
        let identity = identity(database.as_deref(), server.as_deref())?;
        let kind = if let Some(server) = server {
            BackendKind::Remote(Box::new(
                RemoteApplication::connect(&server)
                    .await
                    .map_err(CliError::remote)?,
            ))
        } else {
            let path = match database {
                Some(path) => path,
                None => {
                    ensure_app_data_dir().map_err(CliError::storage)?;
                    default_database_path().map_err(CliError::storage)?
                }
            };
            let repository = SqliteRepository::open(path).map_err(CliError::storage)?;
            BackendKind::Local(TrackerApplication::load(repository).map_err(CliError::application)?)
        };
        Ok(Self { identity, kind })
    }

    pub fn tasks(&self, ordering: TaskOrdering) -> Vec<TaskListItem> {
        match &self.kind {
            BackendKind::Local(app) => app.tasks(ordering),
            BackendKind::Remote(app) => app.tasks(ordering),
        }
    }

    pub fn task(&self, id: TaskId) -> Option<&Task> {
        match &self.kind {
            BackendKind::Local(app) => app.task(id),
            BackendKind::Remote(app) => app.task(id),
        }
    }

    pub fn current_tracking(&self) -> &TrackingState {
        match &self.kind {
            BackendKind::Local(app) => app.current_tracking(),
            BackendKind::Remote(app) => app.current_tracking(),
        }
    }
}

macro_rules! operation {
    ($name:ident($($arg:ident: $ty:ty),*) -> $result:ty) => {
        impl Backend {
            pub async fn $name(&mut self, $($arg: $ty),*) -> Result<$result, CliError> {
                match &mut self.kind {
                    BackendKind::Local(app) => app.$name($($arg),*)
                        .map_err(CliError::application),
                    BackendKind::Remote(app) => app.$name($($arg),*).await
                        .map_err(CliError::application),
                }
            }
        }
    };
}

operation!(create_task(name: TaskName, now: DateTime<Utc>) -> Task);
operation!(rename_task(id: TaskId, name: TaskName, now: DateTime<Utc>) -> Task);
operation!(archive_task(id: TaskId, now: DateTime<Utc>) -> Task);
operation!(unarchive_task(id: TaskId, now: DateTime<Utc>) -> Task);
operation!(set_active_task(id: TaskId, now: DateTime<Utc>) -> SetActiveTaskOutcome);
operation!(clear_active_task(expected: WorklogId, now: DateTime<Utc>) -> ClearActiveTaskOutcome);
operation!(worklogs_for_task(id: TaskId, after: Option<&WorklogCursor>) -> WorklogPage);
operation!(all_worklogs(after: Option<&GlobalWorklogCursor>) -> GlobalWorklogPage);
operation!(move_worklog(id: WorklogId, source: TaskId, expected: WorklogTimes, destination: TaskId) -> Worklog);
operation!(correct_worklog(id: WorklogId, expected: WorklogTimes, replacement: WorklogTimes, now: DateTime<Utc>) -> Worklog);
operation!(delete_completed_worklog(id: WorklogId, task: TaskId, expected: WorklogTimes) -> Worklog);
operation!(report_totals(start: DateTime<Utc>, end: DateTime<Utc>, now: DateTime<Utc>) -> ReportTotals);
