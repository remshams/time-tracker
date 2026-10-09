//! Period report queries.

use chrono::{DateTime, TimeDelta, Utc};

use super::{ReportQueries, TrackerApplication};
use crate::{ApplicationError, ReportRow, ReportTotals, RepositoryError, TrackerRepository};

impl<R: TrackerRepository> ReportQueries for TrackerApplication<R> {
    fn report_totals(
        &mut self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<ReportTotals, ApplicationError> {
        validate_range(start, end)?;
        let read = self
            .repository
            .report_read(start, end, now)
            .map_err(report_error)?;
        let totals = totals_from_rows(read.rows)?;
        self.adopt_tracking_read(read.tracking)?;
        for row in &totals.rows {
            self.replace_task(row.task.clone());
        }
        Ok(totals)
    }
}

impl<R: TrackerRepository> TrackerApplication<R> {
    /// Refreshes task-list totals and the list's tracking indication together.
    /// Clients capture their selected presentation values after this read.
    pub fn task_list_report_totals(
        &mut self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<ReportTotals, ApplicationError> {
        validate_range(start, end)?;
        let (items, read) = self
            .repository
            .task_list_report_read(start, end, now)
            .map_err(report_error)?;
        let totals = totals_from_rows(read.rows)?;
        self.adopt_task_tracking_resources(items, read.tracking.active_worklog)?;
        Ok(totals)
    }
}

fn validate_range(start: DateTime<Utc>, end: DateTime<Utc>) -> Result<(), ApplicationError> {
    if end <= start {
        Err(ApplicationError::InvalidReportRange)
    } else {
        Ok(())
    }
}

fn report_error(error: RepositoryError) -> ApplicationError {
    match error {
        RepositoryError::ReportDurationOverflow => ApplicationError::ReportDurationOverflow,
        other => ApplicationError::Repository(other),
    }
}

fn totals_from_rows(mut rows: Vec<ReportRow>) -> Result<ReportTotals, ApplicationError> {
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
    Ok(ReportTotals {
        rows,
        total: TimeDelta::microseconds(total_us),
    })
}
