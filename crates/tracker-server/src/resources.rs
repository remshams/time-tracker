//! Aggregate resource reads captured under one server operation lock.

use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use tracker_application::TaskOrdering;
use tracker_protocol::{DailyOverviewDto, ReportDto, TaskDto, TaskListStateDto};

use crate::core::{Shared, lock};
use crate::error::{ApiResult, query_payload};
use crate::reports::ReportQuery;
use tracker_application::timestamps::canonical_timestamp;

/// Captures the full catalog and active tracking from one SQLite read version.
pub(super) async fn task_list_state(State(shared): State<Shared>) -> ApiResult<TaskListStateDto> {
    let mut core = lock(&shared)?;
    core.app.refresh_task_list()?;
    Ok(Json(TaskListStateDto {
        tasks: core
            .app
            .tasks(TaskOrdering::RecentlyCreated)
            .iter()
            .map(TaskDto::from)
            .collect(),
        active_worklog: core.active_worklog(),
        revision: core.revision(),
    }))
}

/// Includes archived tasks and preserves the independent report's interval semantics.
pub(super) async fn daily_overview(
    State(shared): State<Shared>,
    input: Result<Query<ReportQuery>, QueryRejection>,
) -> ApiResult<DailyOverviewDto> {
    let query = query_payload(input)?;
    let start = canonical_timestamp(query.start);
    let end = canonical_timestamp(query.end);
    let now = canonical_timestamp(query.now);
    let mut core = lock(&shared)?;
    let totals = core.app.task_list_report_totals(start, end, now)?;
    Ok(Json(DailyOverviewDto {
        tasks: core
            .app
            .tasks(TaskOrdering::RecentlyCreated)
            .iter()
            .map(TaskDto::from)
            .collect(),
        active_worklog: core.active_worklog(),
        totals: ReportDto::from_totals(&totals, start, end, now, core.revision()),
    }))
}
