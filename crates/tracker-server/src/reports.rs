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
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    now: DateTime<Utc>,
}

impl ReportQuery {
    /// Returns the start, end, and cutoff at canonical microsecond precision.
    pub(super) fn canonical(&self) -> (DateTime<Utc>, DateTime<Utc>, DateTime<Utc>) {
        (
            canonical_timestamp(self.start),
            canonical_timestamp(self.end),
            canonical_timestamp(self.now),
        )
    }
}

/// Echoes the same microsecond bounds and cutoff used to compute report totals.
pub(super) async fn report(
    State(shared): State<Shared>,
    input: Result<Query<ReportQuery>, QueryRejection>,
) -> ApiResult<ReportDto> {
    let query = query_payload(input)?;
    let (start, end, now) = query.canonical();
    let mut core = lock(&shared)?;
    let totals = core.app.report_totals(start, end, now)?;
    Ok(Json(ReportDto::from_totals(&totals, start, end, now)))
}
