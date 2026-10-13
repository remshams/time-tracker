//! Task resource reads, commands and inactive-task confirmation.

use axum::Json;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Path as RoutePath, Query, State};
use chrono::{DateTime, TimeDelta, Utc};
use serde::Deserialize;
use tracker_application::{ApplicationError, TaskOrdering};
use tracker_domain::{InactivityPeriod, Task, TaskId};
use tracker_protocol::{
    ArchiveInactiveTasksRequest, CreateTaskRequest, InactiveTaskPreviewDto, MutationDto,
    MutationResultDto, TaskChangeRequest, TaskDto, TaskResourceDto, TasksDto,
};

use crate::core::{Core, Shared, lock};
use crate::error::{ApiError, ApiResult, fingerprint, payload, query_payload, task_id, task_name};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TasksQuery {
    archived: Option<bool>,
}

pub(super) async fn tasks(
    State(shared): State<Shared>,
    input: Result<Query<TasksQuery>, QueryRejection>,
) -> ApiResult<TasksDto> {
    let query = query_payload(input)?;
    let mut core = lock(&shared)?;
    core.app.refresh_task_catalog()?;
    let tasks = core
        .app
        .tasks(TaskOrdering::RecentlyCreated)
        .iter()
        .filter(|item| {
            query
                .archived
                .is_none_or(|archived| item.task.is_archived() == archived)
        })
        .map(TaskDto::from)
        .collect();
    Ok(Json(TasksDto {
        tasks,
        revision: core.revision(),
    }))
}

pub(super) fn task_resource(core: &mut Core, id: TaskId) -> Result<TaskDto, ApiError> {
    core.app
        .read_task_item(id)?
        .as_ref()
        .map(TaskDto::from)
        .ok_or_else(|| {
            ApplicationError::from(tracker_application::RepositoryError::TaskNotFound { id }).into()
        })
}

pub(super) async fn task(
    State(shared): State<Shared>,
    RoutePath(raw_id): RoutePath<String>,
) -> ApiResult<TaskResourceDto> {
    let id = task_id(&raw_id)?;
    let mut core = lock(&shared)?;
    Ok(Json(TaskResourceDto {
        task: task_resource(&mut core, id)?,
        revision: core.revision(),
    }))
}
const INACTIVE_AS_OF_TOLERANCE: TimeDelta = TimeDelta::minutes(15);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InactivePreviewQuery {
    as_of: DateTime<Utc>,
    #[serde(default = "default_inactive_days")]
    inactive_days: u32,
}

fn default_inactive_days() -> u32 {
    14
}

fn validate_inactive_as_of(as_of: DateTime<Utc>, now: DateTime<Utc>) -> Result<(), ApiError> {
    let difference = as_of.signed_duration_since(now);
    if difference < -INACTIVE_AS_OF_TOLERANCE || difference > INACTIVE_AS_OF_TOLERANCE {
        Err(ApiError::invalid(
            "Inactive task preview time is outside the allowed range",
        ))
    } else {
        Ok(())
    }
}

pub(super) async fn preview_inactive_tasks(
    State(shared): State<Shared>,
    input: Result<Query<InactivePreviewQuery>, QueryRejection>,
) -> ApiResult<InactiveTaskPreviewDto> {
    let query = query_payload(input)?;
    validate_inactive_as_of(query.as_of, Utc::now())?;
    let mut core = lock(&shared)?;
    let period = InactivityPeriod::new(query.inactive_days)
        .map_err(|error| ApiError::invalid(&error.to_string()))?;
    let tasks = core
        .app
        .preview_inactive_tasks_with_period(query.as_of, period)?;
    Ok(Json(InactiveTaskPreviewDto {
        as_of: query.as_of,
        inactive_days: period.days(),
        count: tasks.len(),
        candidate_task_ids: tasks.iter().map(|task| task.id().to_string()).collect(),
        revision: core.revision(),
    }))
}

pub(super) async fn archive_inactive_tasks(
    State(shared): State<Shared>,
    input: Result<Json<ArchiveInactiveTasksRequest>, JsonRejection>,
) -> ApiResult<MutationDto> {
    let request = payload(input)?;
    let key = fingerprint("POST /v1/tasks/archive-inactive", &request)?;
    let mut core = lock(&shared)?;
    let response = core.execute(&request.guard, key, |app| {
        validate_inactive_as_of(request.as_of, Utc::now())?;
        let period = InactivityPeriod::new(request.inactive_days)
            .map_err(|error| ApiError::invalid(&error.to_string()))?;
        let candidates = app.preview_inactive_tasks_with_period(request.as_of, period)?;
        let ids: Vec<TaskId> = candidates.iter().map(Task::id).collect();
        let archived = app.archive_inactive_tasks_with_period(&ids, request.as_of, period)?;
        Ok(MutationResultDto::ArchivedInactive {
            count: archived.len(),
        })
    })?;
    Ok(Json(response))
}
pub(super) async fn create_task(
    State(shared): State<Shared>,
    input: Result<Json<CreateTaskRequest>, JsonRejection>,
) -> ApiResult<MutationDto> {
    let request = payload(input)?;
    let id = task_id(&request.task_id)?;
    let name = task_name(&request.name)?;
    let key = fingerprint("POST /v1/tasks", &request)?;
    let mut core = lock(&shared)?;
    let response = core.execute(&request.guard, key, |app| {
        app.create_task_with_id(id, name, request.occurred_at)?;
        let item = app
            .tasks(TaskOrdering::RecentlyCreated)
            .into_iter()
            .find(|item| item.task.id() == id)
            .ok_or_else(ApiError::internal)?;
        Ok(MutationResultDto::Task(TaskDto::from(&item)))
    })?;
    Ok(Json(response))
}

pub(super) async fn change_task(
    State(shared): State<Shared>,
    RoutePath(raw_id): RoutePath<String>,
    input: Result<Json<TaskChangeRequest>, JsonRejection>,
) -> ApiResult<MutationDto> {
    let request = payload(input)?;
    let id = task_id(&raw_id)?;
    let key = fingerprint(&format!("PATCH /v1/tasks/{raw_id}"), &request)?;
    let guard = request.guard().clone();
    let mut core = lock(&shared)?;
    let response = core.execute(&guard, key, |app| {
        let task = match request {
            TaskChangeRequest::Rename {
                name, occurred_at, ..
            } => app.rename_task(id, task_name(&name)?, occurred_at)?,
            TaskChangeRequest::Archive { occurred_at, .. } => app.archive_task(id, occurred_at)?,
            TaskChangeRequest::Restore { occurred_at, .. } => {
                app.unarchive_task(id, occurred_at)?
            }
        };
        let item = app
            .tasks(TaskOrdering::RecentlyCreated)
            .into_iter()
            .find(|item| item.task.id() == task.id())
            .ok_or_else(ApiError::internal)?;
        Ok(MutationResultDto::Task(TaskDto::from(&item)))
    })?;
    Ok(Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inactive_preview_accepts_exact_time_limits() {
        let now = Utc::now();
        let tolerance = TimeDelta::minutes(15);
        let tick = TimeDelta::microseconds(1);
        assert!(validate_inactive_as_of(now - tolerance, now).is_ok());
        assert!(validate_inactive_as_of(now + tolerance, now).is_ok());
        assert!(validate_inactive_as_of(now - tolerance - tick, now).is_err());
        assert!(validate_inactive_as_of(now + tolerance + tick, now).is_err());
    }
}
