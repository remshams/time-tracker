use super::{
    ApplicationError, DateTime, DeleteWorklogRequest, GlobalWorklogCursor, GlobalWorklogPage,
    HistoryRead, Method, MutationScope, Recovery, RemoteApplication, RemoteError,
    RemoteFailureKind, TaskId, Utc, Worklog, WorklogChangeRequest, WorklogCursor, WorklogId,
    WorklogPage, WorklogPageDto, WorklogResourceDto, WorklogTimes, app_decode_worklog, canonical,
    decode_history, decode_worklog, map_application_error, original_worklog_matches,
    protocol_failure, worklog_result,
};

impl RemoteApplication {
    pub async fn read_worklog(&mut self, id: WorklogId) -> Result<WorklogResourceDto, RemoteError> {
        self.ensure_version().await?;
        let dto: WorklogResourceDto = self.read(&format!("v1/worklogs/{id}")).await?;
        if dto.worklog.id != id.to_string() || dto.revision.is_empty() {
            return Err(self.record_error(RemoteError::Protocol("invalid worklog resource".into())));
        }
        decode_worklog(dto.worklog.clone()).map_err(|error| self.record_error(error))?;
        self.worklog_cache.insert(id, dto.clone());
        self.confirmed();
        Ok(dto)
    }

    pub(super) async fn history(
        &mut self,
        task_id: Option<TaskId>,
        after: Option<(DateTime<Utc>, WorklogId, i64)>,
    ) -> Result<HistoryRead, ApplicationError> {
        self.ensure_version()
            .await
            .map_err(|error| map_application_error(&error, None, task_id))?;
        let mut url = self.transport.url("v1/worklogs");
        if let Some(id) = task_id {
            url.query_pairs_mut()
                .append_pair("task_id", &id.to_string());
        }
        if let Some((start, id, revision)) = after {
            url.query_pairs_mut()
                .append_pair("after_start", &start.to_rfc3339())
                .append_pair("after_id", &id.to_string())
                .append_pair("after_revision", &revision.to_string());
            if let Some(task_id) = task_id {
                url.query_pairs_mut()
                    .append_pair("after_task_id", &task_id.to_string());
            }
        }
        let dto: WorklogPageDto = self
            .transport
            .send(Method::GET, url, None::<&()>)
            .await
            .map_err(|error| {
                self.record_error(error.clone());
                map_application_error(&error, None, task_id)
            })?;
        let decoded = decode_history(&dto, task_id).map_err(|error| {
            self.record_error(error.clone());
            map_application_error(&error, None, task_id)
        })?;
        self.history_cache.insert(task_id, dto);
        self.confirmed();
        Ok(decoded)
    }

    pub async fn worklogs_for_task(
        &mut self,
        task_id: TaskId,
        after: Option<&WorklogCursor>,
    ) -> Result<WorklogPage, ApplicationError> {
        if after.is_some_and(|cursor| cursor.task_id != task_id) {
            return Err(ApplicationError::worklog_history_changed(task_id));
        }
        let dto = self
            .history(
                Some(task_id),
                after.map(|cursor| (cursor.start, cursor.id, cursor.revision)),
            )
            .await?;
        Ok(WorklogPage {
            worklogs: dto.worklogs,
            next_cursor: dto.next_cursor.map(|cursor| WorklogCursor {
                task_id,
                start: cursor.start,
                id: cursor.id,
                revision: cursor.revision,
            }),
            adoption: None,
        })
    }

    pub async fn all_worklogs(
        &mut self,
        after: Option<&GlobalWorklogCursor>,
    ) -> Result<GlobalWorklogPage, ApplicationError> {
        let dto = self
            .history(
                None,
                after.map(|cursor| (cursor.start, cursor.id, cursor.revision)),
            )
            .await?;
        Ok(GlobalWorklogPage {
            next_cursor: dto.next_cursor,
            task_items: vec![],
            tracking: None,
            worklogs: dto.worklogs,
        })
    }

    pub(super) async fn worklog_guard(
        &mut self,
        id: WorklogId,
        expected_task: Option<TaskId>,
        expected: WorklogTimes,
        destination: Option<TaskId>,
    ) -> Result<String, ApplicationError> {
        self.ensure_version()
            .await
            .map_err(|error| map_application_error(&error, Some(id), None))?;
        let reviewed_destination = destination.and_then(|id| self.task(id).cloned());
        for _ in 0..2 {
            let dto = self
                .read_worklog(id)
                .await
                .map_err(|error| map_application_error(&error, Some(id), None))?;
            let worklog = app_decode_worklog(dto.worklog)?;
            if !original_worklog_matches(&worklog, expected_task, expected) {
                self.last_failure = Some(RemoteFailureKind::Conflict);
                let primary = ApplicationError::worklog_changed(id);
                if let Err(recovery) = self.recover(Recovery::Worklog).await {
                    return Err(
                        primary.with_recovery_failure(map_application_error(&recovery, None, None))
                    );
                }
                return Err(primary);
            }
            if let Some(destination) = destination {
                let (item, revision) = self.read_task_item(destination).await?;
                if dto.revision != revision {
                    continue;
                }
                let changed = reviewed_destination
                    .as_ref()
                    .is_some_and(|old| old != &item.task);
                self.adopt_task(item, revision);
                if changed {
                    return Err(self.intent_changed());
                }
            }
            return Ok(dto.revision);
        }
        Err(self.intent_changed())
    }

    pub async fn move_worklog(
        &mut self,
        id: WorklogId,
        expected_source_task_id: TaskId,
        expected: WorklogTimes,
        destination_task_id: TaskId,
    ) -> Result<Worklog, ApplicationError> {
        self.last_write_attempted = false;
        self.last_command_receipt = None;
        let revision = self
            .worklog_guard(
                id,
                Some(expected_source_task_id),
                expected,
                Some(destination_task_id),
            )
            .await?;
        let body = WorklogChangeRequest::Move {
            expected_task_id: expected_source_task_id.to_string(),
            expected_start: canonical(expected.start()),
            expected_end: expected.end().map(canonical),
            destination_task_id: destination_task_id.to_string(),
            guard: Self::guard(&revision),
        };
        self.mutation(
            Method::PATCH,
            &format!("v1/worklogs/{id}"),
            &body,
            body.guard(),
            MutationScope {
                recovery: Recovery::Worklog,
                worklog_id: Some(id),
                task_id: None,
            },
            |result| {
                let worklog = worklog_result(result, id)?;
                if worklog.task_id() != destination_task_id
                    || worklog.start() != canonical(expected.start())
                    || worklog.end() != expected.end().map(canonical)
                {
                    return Err(protocol_failure("moved worklog contradicts command"));
                }
                Ok(worklog)
            },
        )
        .await
    }

    pub async fn correct_worklog(
        &mut self,
        id: WorklogId,
        expected: WorklogTimes,
        replacement: WorklogTimes,
        occurred_at: DateTime<Utc>,
    ) -> Result<Worklog, ApplicationError> {
        self.last_write_attempted = false;
        self.last_command_receipt = None;
        let revision = self.worklog_guard(id, None, expected, None).await?;
        let expected_task_id = self
            .worklog_cache
            .get(&id)
            .expect("successful preflight caches the worklog")
            .worklog
            .task_id
            .clone();
        let body = WorklogChangeRequest::Correct {
            expected_start: canonical(expected.start()),
            expected_end: expected.end().map(canonical),
            replacement_start: canonical(replacement.start()),
            replacement_end: replacement.end().map(canonical),
            occurred_at: canonical(occurred_at),
            guard: Self::guard(&revision),
        };
        self.mutation(
            Method::PATCH,
            &format!("v1/worklogs/{id}"),
            &body,
            body.guard(),
            MutationScope {
                recovery: Recovery::Worklog,
                worklog_id: Some(id),
                task_id: None,
            },
            |result| {
                let worklog = worklog_result(result, id)?;
                if worklog.task_id().to_string() != expected_task_id
                    || worklog.start() != canonical(replacement.start())
                    || worklog.end() != replacement.end().map(canonical)
                {
                    return Err(protocol_failure("corrected worklog contradicts command"));
                }
                Ok(worklog)
            },
        )
        .await
    }

    pub async fn delete_completed_worklog(
        &mut self,
        id: WorklogId,
        expected_task_id: TaskId,
        expected: WorklogTimes,
    ) -> Result<Worklog, ApplicationError> {
        self.last_write_attempted = false;
        self.last_command_receipt = None;
        let end = expected
            .end()
            .ok_or_else(|| ApplicationError::active_worklog(id))?;
        let revision = self
            .worklog_guard(id, Some(expected_task_id), expected, None)
            .await?;
        let body = DeleteWorklogRequest {
            expected_task_id: expected_task_id.to_string(),
            expected_start: canonical(expected.start()),
            expected_end: canonical(end),
            guard: Self::guard(&revision),
        };
        self.mutation(
            Method::DELETE,
            &format!("v1/worklogs/{id}"),
            &body,
            &body.guard,
            MutationScope {
                recovery: Recovery::Worklog,
                worklog_id: Some(id),
                task_id: None,
            },
            |result| {
                let worklog = worklog_result(result, id)?;
                if worklog.task_id() != expected_task_id
                    || worklog.start() != canonical(expected.start())
                    || worklog.end() != expected.end().map(canonical)
                {
                    return Err(protocol_failure("deleted worklog contradicts command"));
                }
                Ok(worklog)
            },
        )
        .await
    }

    pub fn cached_history(&self, task_id: Option<TaskId>) -> Option<&WorklogPageDto> {
        self.history_cache.get(&task_id)
    }

    pub fn cached_worklog(&self, id: WorklogId) -> Option<&WorklogResourceDto> {
        self.worklog_cache.get(&id)
    }
}
