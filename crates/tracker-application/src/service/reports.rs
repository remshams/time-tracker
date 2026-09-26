//! Period report queries.

use chrono::{DateTime, TimeDelta, Utc};

use super::{ReportQueries, TrackerApplication};
use crate::{ApplicationError, ReportTotals, RepositoryError, TrackerRepository};

impl<R: TrackerRepository> ReportQueries for TrackerApplication<R> {
    fn report_totals(
        &mut self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<ReportTotals, ApplicationError> {
        if end <= start {
            return Err(ApplicationError::InvalidReportRange);
        }
        let read = self
            .repository
            .report_read(start, end, now)
            .map_err(|error| match error {
                RepositoryError::ReportDurationOverflow => ApplicationError::ReportDurationOverflow,
                other => ApplicationError::Repository(other),
            })?;
        let mut rows = read.rows;
        let mut total_us = 0_i64;
        for row in &rows {
            let duration_us = row
                .duration
                .num_microseconds()
                .ok_or(ApplicationError::ReportDurationOverflow)?;
            if duration_us <= 0 {
                return Err(ApplicationError::ReportDurationOverflow);
            }
            total_us = total_us
                .checked_add(duration_us)
                .ok_or(ApplicationError::ReportDurationOverflow)?;
        }
        rows.sort_by(|a, b| {
            b.duration
                .cmp(&a.duration)
                .then_with(|| a.task.id().cmp(&b.task.id()))
        });
        self.adopt_snapshot(read.snapshot)?;
        Ok(ReportTotals {
            rows,
            total: TimeDelta::microseconds(total_us),
        })
    }
}
