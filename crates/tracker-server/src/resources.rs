//! Independent resource reads captured under the server operation lock.

use super::*;

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
    let core = lock(&shared)?;
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

fn task_resource(core: &Core, id: TaskId) -> Result<TaskDto, ApiError> {
    core.app
        .tasks(TaskOrdering::RecentlyCreated)
        .iter()
        .find(|item| item.task.id() == id)
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
    let core = lock(&shared)?;
    Ok(Json(TaskResourceDto {
        task: task_resource(&core, id)?,
        revision: core.revision(),
    }))
}

pub(super) async fn tracking(State(shared): State<Shared>) -> ApiResult<TrackingDto> {
    let core = lock(&shared)?;
    Ok(Json(TrackingDto {
        active_worklog: core.active_worklog(),
        revision: core.revision(),
    }))
}

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
        revision: core.revision(),
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PageQuery {
    task_id: Option<String>,
    after_task_id: Option<String>,
    after_start: Option<DateTime<Utc>>,
    after_id: Option<String>,
    after_revision: Option<i64>,
}

pub(super) async fn all_worklogs(
    State(shared): State<Shared>,
    input: Result<Query<PageQuery>, QueryRejection>,
) -> ApiResult<WorklogPageDto> {
    let query = query_payload(input)?;
    let task = query.task_id.as_deref().map(task_id).transpose()?;
    let continuation_scope = query.after_task_id.as_deref().map(task_id).transpose()?;
    let cursor = match (query.after_start, query.after_id, query.after_revision) {
        (None, None, None) if continuation_scope.is_none() => None,
        (Some(start), Some(id), Some(revision)) if revision >= 0 && task == continuation_scope => {
            Some(GlobalWorklogCursor {
                start,
                id: worklog_id(&id)?,
                revision,
            })
        }
        _ => {
            return Err(ApiError::invalid(
                "Incomplete or mismatched worklog cursor scope",
            ));
        }
    };
    let mut core = lock(&shared)?;
    let page = match task {
        Some(id) => {
            task_resource(&core, id)?;
            let task_cursor = cursor.map(|cursor| WorklogCursor {
                task_id: id,
                start: cursor.start,
                id: cursor.id,
                revision: cursor.revision,
            });
            let page = core.app.worklogs_for_task(id, task_cursor.as_ref())?;
            WorklogPageDto::from_page(&page, core.revision())
        }
        None => {
            let page = core.app.all_worklogs(cursor.as_ref())?;
            WorklogPageDto::from_global_page(&page, core.revision())
        }
    };
    Ok(Json(page))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReportQuery {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    now: DateTime<Utc>,
}

pub(super) async fn report(
    State(shared): State<Shared>,
    input: Result<Query<ReportQuery>, QueryRejection>,
) -> ApiResult<ReportDto> {
    let query = query_payload(input)?;
    let start = canonical_timestamp(query.start);
    let end = canonical_timestamp(query.end);
    let now = canonical_timestamp(query.now);
    let mut core = lock(&shared)?;
    let totals = core.app.report_totals(start, end, now)?;
    Ok(Json(ReportDto::from_totals(
        &totals,
        start,
        end,
        now,
        core.revision(),
    )))
}

fn canonical_timestamp(value: DateTime<Utc>) -> DateTime<Utc> {
    DateTime::from_timestamp_micros(value.timestamp_micros())
        .expect("UTC timestamps fit the microsecond range")
}
