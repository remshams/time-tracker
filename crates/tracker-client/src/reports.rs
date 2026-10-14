use crate::{ClientReport, ClientReportRow, RefreshError, TrackerBackend};
use chrono::{DateTime, Utc};
use tracker_application::timestamps::canonical_timestamp;
use tracker_application::{ApplicationError, ReportRow, ReportTotals};
use tracker_remote::ResourceSelection;

impl TrackerBackend {
    pub async fn report(
        &mut self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<ClientReport, ApplicationError> {
        let now = canonical_timestamp(now);
        match self {
            Self::Local(application) => {
                let totals = application.report_totals(start, end, now)?;
                Ok(ClientReport {
                    rows: totals
                        .rows
                        .into_iter()
                        .map(|row| ClientReportRow {
                            task_id: row.task.id(),
                            duration: row.duration,
                        })
                        .collect(),
                    total: totals.total,
                    now,
                    revision: None,
                })
            }
            Self::Remote(application) => {
                let report = application.task_totals(start, end, now).await?;
                remote_report(report)
            }
        }
    }

    pub async fn report_totals(
        &mut self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<ReportTotals, ApplicationError> {
        let report = self.report(start, end, now).await?;
        let mut rows = Vec::with_capacity(report.rows.len());
        for row in report.rows {
            let task = match self.task(row.task_id) {
                Some(task) => task.clone(),
                None => self.read_task(row.task_id).await?.task,
            };
            rows.push(ReportRow {
                task,
                duration: row.duration,
            });
        }
        Ok(ReportTotals {
            rows,
            total: report.total,
        })
    }

    pub async fn refresh_totals(
        &mut self,
        include_tasks: bool,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<ClientReport, RefreshError> {
        let now = canonical_timestamp(now);
        match self {
            Self::Local(application) => {
                let totals = if include_tasks {
                    application.task_list_report_totals(start, end, now)
                } else {
                    application.report_totals(start, end, now)
                }?;
                Ok(ClientReport {
                    rows: totals
                        .rows
                        .into_iter()
                        .map(|row| ClientReportRow {
                            task_id: row.task.id(),
                            duration: row.duration,
                        })
                        .collect(),
                    total: totals.total,
                    now,
                    revision: None,
                })
            }
            Self::Remote(application) => {
                let selection = if include_tasks {
                    ResourceSelection::TaskListWithTotals { start, end, now }
                } else {
                    ResourceSelection::DailyTotals { start, end, now }
                };
                application.refresh_resources(selection).await?;
                let report = application.report_observation().cloned().ok_or_else(|| {
                    ApplicationError::RemoteProtocol("Report resource was not loaded".into())
                })?;
                remote_report(report).map_err(RefreshError::Local)
            }
        }
    }
}

fn remote_report(report: tracker_protocol::ReportDto) -> Result<ClientReport, ApplicationError> {
    let rows = report
        .rows
        .into_iter()
        .map(|row| {
            Ok(ClientReportRow {
                task_id: row.task_id.parse().map_err(|_| {
                    ApplicationError::RemoteProtocol("Invalid report task identifier".into())
                })?,
                duration: chrono::TimeDelta::microseconds(row.duration_us),
            })
        })
        .collect::<Result<Vec<_>, ApplicationError>>()?;
    Ok(ClientReport {
        rows,
        total: chrono::TimeDelta::microseconds(report.total_us),
        now: report.now,
        revision: Some(report.revision),
    })
}
