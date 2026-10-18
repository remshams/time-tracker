use super::{
    ApplicationError, ApplicationFailureCategory, HealthDto, Instant, Method, MutationContext,
    MutationDto, MutationResultDto, RemoteError, RemoteFailureKind, Transport, WriteGuard,
    classify_error, map_application_error, protocol_failure,
};

/// Shared connection state, HTTP transport and validated command receipts.
pub(super) struct RemoteSession {
    transport: Transport,
    last_failure: Option<RemoteFailureKind>,
    last_unavailable_at: Option<Instant>,
    version_checked: bool,
    last_command_receipt: Option<MutationDto>,
    last_write_attempted: bool,
}

impl RemoteSession {
    pub(super) fn new(endpoint: &str) -> Result<Self, RemoteError> {
        Ok(Self {
            transport: Transport::new(endpoint)?,
            last_failure: Some(RemoteFailureKind::Unavailable),
            last_unavailable_at: None,
            version_checked: false,
            last_command_receipt: None,
            last_write_attempted: false,
        })
    }
    pub(super) fn url(&self, path: &str) -> reqwest::Url {
        self.transport.url(path)
    }
    pub(super) fn last_failure(&self) -> Option<RemoteFailureKind> {
        self.last_failure
    }
    pub(super) fn last_unavailable_at(&self) -> Option<Instant> {
        self.last_unavailable_at
    }
    pub(super) fn last_write_attempted(&self) -> bool {
        self.last_write_attempted
    }
    pub(super) fn last_command_receipt(&self) -> Option<&MutationDto> {
        self.last_command_receipt.as_ref()
    }
    pub(super) fn begin_command(&mut self) {
        self.last_write_attempted = false;
        self.last_command_receipt = None;
    }
    pub(super) fn intent_changed(&mut self) -> ApplicationError {
        self.set_failure(RemoteFailureKind::Conflict);
        ApplicationError::semantic_failure(
            ApplicationFailureCategory::General,
            "Tracker state changed. Refresh and retry.",
        )
    }
    pub(super) fn set_failure(&mut self, failure: RemoteFailureKind) {
        self.last_failure = Some(failure);
    }
    pub(super) async fn check_connection(&mut self) -> Result<(), RemoteError> {
        let health: HealthDto = self.read("v1/health").await?;
        if health.protocol_version != tracker_protocol::VERSION || health.status != "ok" {
            return Err(self.record_error(RemoteError::Protocol(
                "server protocol version does not match".into(),
            )));
        }
        self.version_checked = true;
        self.confirmed();
        Ok(())
    }

    pub(super) async fn ensure_version(&mut self) -> Result<(), RemoteError> {
        if !self.version_checked {
            self.check_connection().await?;
        }
        Ok(())
    }

    pub(super) async fn read<T: serde::de::DeserializeOwned>(
        &mut self,
        path: &str,
    ) -> Result<T, RemoteError> {
        self.transport
            .send(Method::GET, self.transport.url(path), None::<&()>)
            .await
            .map_err(|error| self.record_error(error))
    }

    pub(super) fn confirmed(&mut self) {
        self.last_failure = None;
        self.last_unavailable_at = None;
    }

    pub(super) fn record_error(&mut self, error: RemoteError) -> RemoteError {
        self.last_failure = Some(classify_error(&error));
        if error.is_unavailable() {
            self.last_unavailable_at = Some(Instant::now());
        }
        error
    }

    pub(super) fn decode_receipt<T>(
        &mut self,
        response: Result<MutationDto, RemoteError>,
        guard: &WriteGuard,
        scope: MutationContext,
        decode: impl FnOnce(MutationResultDto) -> Result<T, ApplicationError>,
    ) -> Result<T, ApplicationError> {
        match response {
            Ok(dto) if dto.request_id == guard.request_id && !dto.applied_revision.is_empty() => {
                let result = decode(dto.result.clone())?;
                self.last_command_receipt = Some(dto);
                Ok(result)
            }
            Ok(_) => Err(protocol_failure("invalid command receipt")),
            Err(error) => {
                self.record_error(error.clone());
                Err(map_application_error(
                    &error,
                    scope.worklog_id,
                    scope.task_id,
                ))
            }
        }
    }

    pub(super) async fn send<T: serde::de::DeserializeOwned, B: serde::Serialize>(
        &mut self,
        method: Method,
        url: reqwest::Url,
        body: Option<&B>,
    ) -> Result<T, RemoteError> {
        self.transport.send(method, url, body).await
    }

    pub(super) async fn mutation<B: serde::Serialize, T>(
        &mut self,
        method: Method,
        path: &str,
        body: &B,
        guard: &WriteGuard,
        scope: MutationContext,
        decode: impl FnOnce(MutationResultDto) -> Result<T, ApplicationError>,
    ) -> Result<T, ApplicationError> {
        self.ensure_version()
            .await
            .map_err(|error| map_application_error(&error, scope.worklog_id, scope.task_id))?;
        // Stays set if the caller drops the command while the request is in flight.
        self.last_write_attempted = true;
        let delivery = self
            .transport
            .send_with_delivery(method, self.transport.url(path), Some(body))
            .await;
        // A refused connection proves the server never received the command.
        self.last_write_attempted = delivery.may_have_reached_server;
        self.decode_receipt(delivery.result, guard, scope, decode)
    }
}
