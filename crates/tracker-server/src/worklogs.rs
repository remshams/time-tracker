//! Worklog resource reads, history pagination and commands.

use axum::Json;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Path as RoutePath, Query, State};
use serde::Deserialize;
use tracker_application::{ApplicationError, GlobalWorklogCursor, WorklogCursor};
use tracker_domain::WorklogTimes;
use tracker_protocol::{
    DeleteWorklogRequest, MutationDto, MutationResultDto, WorklogChangeRequest, WorklogCursorError,
    WorklogDto, WorklogPageDto, WorklogResourceDto, parse_worklog_cursor,
};

use crate::core::{Shared, lock};
use crate::error::{ApiError, ApiResult, fingerprint, payload, query_payload, task_id, worklog_id};
use crate::tasks::task_resource;

pub(super) async fn worklog(
    State(shared): State<Shared>,
    RoutePath(raw_id): RoutePath<String>,
) -> ApiResult<WorklogResourceDto> {
    let id = worklog_id(&raw_id)?;
    let core = lock(&shared)?;
    let worklog = core.app.worklog(id)?.ok_or_else(|| {
        ApplicationError::from(tracker_application::RepositoryError::WorklogNotFound { id })
    })?;
    Ok(Json(WorklogResourceDto {
        worklog: WorklogDto::from(&worklog),
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PageQuery {
    task_id: Option<String>,
    after_id: Option<String>,
    after_revision: Option<i64>,
}

fn worklog_cursor(query: PageQuery) -> Result<Option<GlobalWorklogCursor>, ApiError> {
    match (query.after_id, query.after_revision) {
        (None, None) => Ok(None),
        (Some(id), Some(revision)) => match parse_worklog_cursor(&id, revision) {
            Ok((id, revision)) => Ok(Some(GlobalWorklogCursor { id, revision })),
            Err(error @ WorklogCursorError::InvalidId) => {
                Err(ApiError::invalid(&error.to_string()))
            }
            Err(WorklogCursorError::NegativeRevision) => Err(invalid_cursor()),
        },
        _ => Err(invalid_cursor()),
    }
}

fn invalid_cursor() -> ApiError {
    ApiError::invalid("Incomplete or invalid worklog cursor")
}

pub(super) async fn all_worklogs(
    State(shared): State<Shared>,
    input: Result<Query<PageQuery>, QueryRejection>,
) -> ApiResult<WorklogPageDto> {
    let query = query_payload(input)?;
    let task = query.task_id.as_deref().map(task_id).transpose()?;
    let cursor = worklog_cursor(query)?;
    let mut core = lock(&shared)?;
    let page = match task {
        Some(id) => {
            task_resource(&mut core, id)?;
            let task_cursor = cursor.map(|cursor| WorklogCursor {
                id: cursor.id,
                revision: cursor.revision,
            });
            let page = core.app.worklogs_for_task(id, task_cursor.as_ref())?;
            WorklogPageDto::from_page(&page)
        }
        None => {
            let page = core.app.all_worklogs(cursor.as_ref())?;
            WorklogPageDto::from_global_page(&page)
        }
    };
    Ok(Json(page))
}
pub(super) async fn change_worklog(
    State(shared): State<Shared>,
    RoutePath(raw_id): RoutePath<String>,
    input: Result<Json<WorklogChangeRequest>, JsonRejection>,
) -> ApiResult<MutationDto> {
    let request = payload(input)?;
    let id = worklog_id(&raw_id)?;
    let key = fingerprint(&format!("PATCH /v1/worklogs/{raw_id}"), &request)?;
    let guard = request.guard().clone();
    let mut core = lock(&shared)?;
    let response = core.execute(&guard, key, |app| {
        let worklog = match request {
            WorklogChangeRequest::Move {
                expected_task_id,
                expected_start,
                expected_end,
                destination_task_id,
                ..
            } => app.move_worklog(
                id,
                task_id(&expected_task_id)?,
                WorklogTimes::new(expected_start, expected_end),
                task_id(&destination_task_id)?,
            )?,
            WorklogChangeRequest::Correct {
                expected_start,
                expected_end,
                replacement_start,
                replacement_end,
                occurred_at,
                ..
            } => app.correct_worklog(
                id,
                WorklogTimes::new(expected_start, expected_end),
                WorklogTimes::new(replacement_start, replacement_end),
                occurred_at,
            )?,
        };
        Ok(MutationResultDto::Worklog(WorklogDto::from(&worklog)))
    })?;
    Ok(Json(response))
}

pub(super) async fn delete_worklog(
    State(shared): State<Shared>,
    RoutePath(raw_id): RoutePath<String>,
    input: Result<Json<DeleteWorklogRequest>, JsonRejection>,
) -> ApiResult<MutationDto> {
    let request = payload(input)?;
    let id = worklog_id(&raw_id)?;
    let key = fingerprint(&format!("DELETE /v1/worklogs/{raw_id}"), &request)?;
    let mut core = lock(&shared)?;
    let response = core.execute(&request.guard, key, |app| {
        app.delete_completed_worklog(
            id,
            task_id(&request.expected_task_id)?,
            WorklogTimes::new(request.expected_start, Some(request.expected_end)),
        )
        .map(|worklog| MutationResultDto::Worklog(WorklogDto::from(&worklog)))
        .map_err(Into::into)
    })?;
    Ok(Json(response))
}
