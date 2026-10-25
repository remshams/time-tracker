use super::{
    ApplicationError, DeleteWorklogRequest, GlobalWorklogCursor, GlobalWorklogPage, HistoryRead,
    Method, MutationContext, RemoteSession, TaskId, Worklog, WorklogChangeRequest, WorklogCursor,
    WorklogId, WorklogPage, WorklogPageDto, WorklogTimes, decode_history, map_application_error,
    original_worklog_matches, protocol_failure, worklog_result,
};

/// Worklog protocol operations and response validation.
pub(super) struct WorklogsClient;

impl WorklogsClient {
    pub(super) async fn history(
        &mut self,
        session: &mut RemoteSession,
        task_id: Option<TaskId>,
        after: Option<(WorklogId, i64)>,
    ) -> Result<HistoryRead, ApplicationError> {
        session
            .ensure_version()
            .await
            .map_err(|error| map_application_error(&error, None, task_id))?;
        let mut url = session.url("v1/worklogs");
        if let Some(id) = task_id {
            url.query_pairs_mut()
                .append_pair("task_id", &id.to_string());
        }
        if let Some((id, revision)) = after {
            url.query_pairs_mut()
                .append_pair("after_id", &id.to_string())
                .append_pair("after_revision", &revision.to_string());
        }
        let dto: WorklogPageDto = session
            .get(url)
            .await
            .map_err(|error| map_application_error(&error, None, task_id))?;
        let decoded = decode_history(&dto, task_id).map_err(|error| {
            session.record_error(error.clone());
            map_application_error(&error, None, task_id)
        })?;
        session.confirmed();
        Ok(decoded)
    }

    pub(super) async fn worklogs_for_task(
        &mut self,
        session: &mut RemoteSession,
        task_id: TaskId,
        after: Option<&WorklogCursor>,
    ) -> Result<WorklogPage, ApplicationError> {
        let dto = self
            .history(
                session,
                Some(task_id),
                after.map(|cursor| (cursor.id, cursor.revision)),
            )
            .await?;
        Ok(WorklogPage {
            worklogs: dto.worklogs,
            next_cursor: dto.next_cursor.map(|cursor| WorklogCursor {
                id: cursor.id,
                revision: cursor.revision,
            }),
            adoption: None,
        })
    }

    pub(super) async fn all_worklogs(
        &mut self,
        session: &mut RemoteSession,
        after: Option<&GlobalWorklogCursor>,
    ) -> Result<GlobalWorklogPage, ApplicationError> {
        let dto = self
            .history(
                session,
                None,
                after.map(|cursor| (cursor.id, cursor.revision)),
            )
            .await?;
        Ok(GlobalWorklogPage {
            next_cursor: dto.next_cursor,
            task_items: vec![],
            tracking: None,
            worklogs: dto.worklogs,
        })
    }

    pub(super) async fn move_worklog(
        &mut self,
        session: &mut RemoteSession,
        id: WorklogId,
        expected: WorklogTimes,
        destination_task_id: TaskId,
        body: &WorklogChangeRequest,
    ) -> Result<Worklog, ApplicationError> {
        session
            .mutation(
                Method::PATCH,
                &format!("v1/worklogs/{id}"),
                body,
                body.guard(),
                MutationContext {
                    worklog_id: Some(id),
                    task_id: None,
                },
                |result| {
                    let worklog = worklog_result(result, id)?;
                    if !original_worklog_matches(&worklog, Some(destination_task_id), expected) {
                        return Err(protocol_failure("moved worklog contradicts command"));
                    }
                    Ok(worklog)
                },
            )
            .await
    }

    pub(super) async fn correct_worklog(
        &mut self,
        session: &mut RemoteSession,
        id: WorklogId,
        replacement: WorklogTimes,
        body: &WorklogChangeRequest,
    ) -> Result<Worklog, ApplicationError> {
        session
            .mutation(
                Method::PATCH,
                &format!("v1/worklogs/{id}"),
                body,
                body.guard(),
                MutationContext {
                    worklog_id: Some(id),
                    task_id: None,
                },
                |result| {
                    let worklog = worklog_result(result, id)?;
                    // The request names no task, so only the identity and new times are checked.
                    if !original_worklog_matches(&worklog, None, replacement) {
                        return Err(protocol_failure("corrected worklog contradicts command"));
                    }
                    Ok(worklog)
                },
            )
            .await
    }

    pub(super) async fn delete_completed_worklog(
        &mut self,
        session: &mut RemoteSession,
        id: WorklogId,
        expected_task_id: TaskId,
        expected: WorklogTimes,
        body: &DeleteWorklogRequest,
    ) -> Result<Worklog, ApplicationError> {
        session
            .mutation(
                Method::DELETE,
                &format!("v1/worklogs/{id}"),
                body,
                &body.guard,
                MutationContext {
                    worklog_id: Some(id),
                    task_id: None,
                },
                |result| {
                    let worklog = worklog_result(result, id)?;
                    if !original_worklog_matches(&worklog, Some(expected_task_id), expected) {
                        return Err(protocol_failure("deleted worklog contradicts command"));
                    }
                    Ok(worklog)
                },
            )
            .await
    }
}
