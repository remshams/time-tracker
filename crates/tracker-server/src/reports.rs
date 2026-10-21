//! Report queries with canonical microsecond timestamps.

use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tracker_application::timestamps::canonical_timestamp;
use tracker_protocol::ReportDto;

use crate::core::{Shared, lock};
use crate::error::{ApiResult, query_payload};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReportQuery {
    pub(super) start: DateTime<Utc>,
    pub(super) end: DateTime<Utc>,
    pub(super) now: DateTime<Utc>,
}

/// Echoes the same microsecond bounds and cutoff used to compute report totals.
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
    Ok(Json(ReportDto::from_totals(&totals, start, end, now)))
}
