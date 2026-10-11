use super::{
    ApplicationError, DateTime, Instant, Method, RemoteApplication, RemoteError, ReportDto, Utc,
    canonical, map_application_error, report_cooldown_active, validate_report,
};

impl RemoteApplication {
    pub(super) async fn fetch_task_totals(
        &mut self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<ReportDto, RemoteError> {
        if end <= start {
            return Err(self.record_error(RemoteError::Protocol("invalid report range".into())));
        }
        if report_cooldown_active(self.last_unavailable_at, Instant::now()) {
            return Err(RemoteError::Unavailable(
                "tracker server is unavailable".into(),
            ));
        }
        self.ensure_version().await?;
        let (start, end, now) = (canonical(start), canonical(end), canonical(now));
        let mut url = self.transport.url("v1/reports/task-totals");
        url.query_pairs_mut()
            .append_pair("start", &start.to_rfc3339())
            .append_pair("end", &end.to_rfc3339())
            .append_pair("now", &now.to_rfc3339());
        let dto: ReportDto = self
            .transport
            .send(Method::GET, url, None::<&()>)
            .await
            .map_err(|error| self.record_error(error))?;
        validate_report(&dto)
            .map_err(|error| self.record_error(RemoteError::Protocol(error.to_string())))?;
        if dto.start != canonical(start) || dto.end != canonical(end) || dto.now != canonical(now) {
            return Err(self.record_error(RemoteError::Protocol(
                "report context does not match request".into(),
            )));
        }
        Ok(dto)
    }

    pub async fn task_totals(
        &mut self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<ReportDto, ApplicationError> {
        if end <= start {
            return Err(ApplicationError::InvalidReportRange);
        }
        let dto = self
            .fetch_task_totals(start, end, now)
            .await
            .map_err(|error| map_application_error(&error, None, None))?;
        self.report_cache = Some(dto.clone());
        self.confirmed();
        Ok(dto)
    }

    pub fn report_revision(&self) -> &str {
        self.report_cache
            .as_ref()
            .map_or("", |report| report.revision.as_str())
    }

    pub fn report_observation(&self) -> Option<&ReportDto> {
        self.report_cache.as_ref()
    }

    pub fn cached_task_totals(&self) -> Option<&ReportDto> {
        self.report_cache.as_ref()
    }
}
