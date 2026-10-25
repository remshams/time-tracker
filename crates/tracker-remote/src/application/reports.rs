use super::{
    ApplicationError, DailyOverviewDto, DateTime, Instant, RemoteError, RemoteSession, ReportDto,
    Utc, canonical, map_application_error, report_cooldown_active, validate_report,
};

struct ReportRequest {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    now: DateTime<Utc>,
}

impl ReportRequest {
    /// Rejects empty microsecond ranges before consulting connection or availability state.
    fn new(
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<Self, ApplicationError> {
        let (start, end, now) = (canonical(start), canonical(end), canonical(now));
        if end <= start {
            return Err(ApplicationError::InvalidReportRange);
        }
        Ok(Self { start, end, now })
    }

    /// Fetches and validates totals without publishing any response resources.
    async fn fetch<T: serde::de::DeserializeOwned>(
        &self,
        session: &mut RemoteSession,
        path: &str,
        report: impl FnOnce(&T) -> &ReportDto,
    ) -> Result<T, RemoteError> {
        if report_cooldown_active(session.last_unavailable_at(), Instant::now()) {
            return Err(RemoteError::Unavailable(
                "tracker server is unavailable".into(),
            ));
        }
        session.ensure_version().await?;
        let mut url = session.url(path);
        url.query_pairs_mut()
            .append_pair("start", &self.start.to_rfc3339())
            .append_pair("end", &self.end.to_rfc3339())
            .append_pair("now", &self.now.to_rfc3339());
        let dto = session.get(url).await?;
        let report = report(&dto);
        validate_report(report)
            .map_err(|error| session.record_error(RemoteError::Protocol(error.to_string())))?;
        if report.start != self.start || report.end != self.end || report.now != self.now {
            return Err(session.record_error(RemoteError::Protocol(
                "report context does not match request".into(),
            )));
        }
        Ok(dto)
    }
}

/// Report protocol operations and the latest confirmed totals response.
#[derive(Default)]
pub(super) struct ReportsClient {
    report_cache: Option<ReportDto>,
}

impl ReportsClient {
    /// Fetches validated totals without publishing any of the overview's resources.
    pub(super) async fn fetch_daily_overview(
        &mut self,
        session: &mut RemoteSession,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<DailyOverviewDto, RemoteError> {
        let request = ReportRequest::new(start, end, now)
            .map_err(|_| RemoteError::Protocol("invalid report range".into()))?;
        request
            .fetch(
                session,
                "v1/state/daily-overview",
                |dto: &DailyOverviewDto| &dto.totals,
            )
            .await
    }

    /// Publishes validated totals only after the echoed microsecond context matches the request.
    pub(super) async fn task_totals(
        &mut self,
        session: &mut RemoteSession,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<ReportDto, ApplicationError> {
        let request = ReportRequest::new(start, end, now)?;
        let dto = request
            .fetch(session, "v1/reports/task-totals", |dto: &ReportDto| dto)
            .await
            .map_err(|error| map_application_error(&error, None, None))?;
        self.report_cache = Some(dto.clone());
        session.confirmed();
        Ok(dto)
    }

    pub(super) fn report_observation(&self) -> Option<&ReportDto> {
        self.report_cache.as_ref()
    }

    pub(super) fn adopt(&mut self, report: ReportDto) {
        self.report_cache = Some(report);
    }
}
