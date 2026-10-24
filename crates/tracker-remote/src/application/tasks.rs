use super::{
    ApplicationError, ArchiveInactiveTasksRequest, CreateTaskRequest, DateTime, HashMap,
    InactiveTaskPreview, InactiveTaskPreviewDto, InactivityPeriod, Method, MoveCandidate,
    MutationContext, MutationResultDto, RemoteError, RemoteFailureKind, RemoteSession, Task,
    TaskChangeRequest, TaskId, TaskListItem, TaskOrdering, TaskResourceDto, TasksDto, Utc,
    canonical, decode_task, decode_task_item, decode_tasks, map_application_error,
    protocol_failure, task_result,
};

/// Task protocol operations and confirmed task observations.
#[derive(Default)]
pub(super) struct TasksClient {
    task_observation: Option<Vec<TaskListItem>>,
    task_cache: HashMap<TaskId, TaskListItem>,
}

impl TasksClient {
    pub(super) async fn refresh_tasks(
        &mut self,
        session: &mut RemoteSession,
    ) -> Result<(), RemoteError> {
        session.ensure_version().await?;
        let items = self.fetch_task_catalog(session).await?;
        self.adopt_catalog(items);
        session.confirmed();
        Ok(())
    }

    async fn fetch_task_catalog(
        &mut self,
        session: &mut RemoteSession,
    ) -> Result<Vec<TaskListItem>, RemoteError> {
        let dto: TasksDto = session.read("v1/tasks").await?;
        decode_tasks(dto).map_err(|error| session.record_error(error))
    }

    pub(super) fn tasks(&self, ordering: TaskOrdering) -> Vec<TaskListItem> {
        let mut items = self.task_items().to_vec();
        ordering.sort_items(&mut items);
        items
    }

    pub(super) fn move_candidates(
        &self,
        source_task_id: TaskId,
        query: &str,
    ) -> Vec<MoveCandidate> {
        tracker_application::move_candidates_for_tasks(self.task_items(), source_task_id, query)
    }

    pub(super) fn task_item(&self, id: TaskId) -> Option<&TaskListItem> {
        self.task_cache
            .get(&id)
            .or_else(|| self.task_items().iter().find(|item| item.task.id() == id))
    }

    pub(super) fn task(&self, id: TaskId) -> Option<&Task> {
        self.task_item(id).map(|item| &item.task)
    }

    pub(super) async fn fetch_task(
        &mut self,
        session: &mut RemoteSession,
        id: TaskId,
    ) -> Result<(TaskResourceDto, TaskListItem), RemoteError> {
        session.ensure_version().await?;
        let dto: TaskResourceDto = session.read(&format!("v1/tasks/{id}")).await?;
        if dto.task.id != id.to_string() {
            return Err(session.record_error(RemoteError::Protocol("invalid task resource".into())));
        }
        let item =
            decode_task_item(dto.task.clone()).map_err(|error| session.record_error(error))?;
        Ok((dto, item))
    }

    /// Reads one task and preserves application error categories.
    pub(super) async fn read_task_item(
        &mut self,
        session: &mut RemoteSession,
        id: TaskId,
    ) -> Result<TaskListItem, ApplicationError> {
        let (_, item) = self
            .fetch_task(session, id)
            .await
            .map_err(|error| map_application_error(&error, None, Some(id)))?;
        self.adopt_task(item.clone());
        session.confirmed();
        Ok(item)
    }

    pub(super) async fn read_task(
        &mut self,
        session: &mut RemoteSession,
        id: TaskId,
    ) -> Result<TaskResourceDto, RemoteError> {
        let (dto, item) = self.fetch_task(session, id).await?;
        self.adopt_task(item);
        session.confirmed();
        Ok(dto)
    }

    /// Loads domain tasks only while every candidate remains unarchived.
    pub(super) async fn resolve_preview_tasks(
        &mut self,
        session: &mut RemoteSession,
        preview: &InactiveTaskPreview,
    ) -> Result<Vec<Task>, ApplicationError> {
        let mut tasks = Vec::with_capacity(preview.candidate_ids.len());
        for &id in &preview.candidate_ids {
            let dto = self
                .read_task(session, id)
                .await
                .map_err(|error| map_application_error(&error, None, Some(id)))?;
            if dto.task.archived {
                return Err(session.intent_changed());
            }
            tasks.push(
                decode_task(dto.task)
                    .map_err(|error| map_application_error(&error, None, Some(id)))?,
            );
        }
        Ok(tasks)
    }

    pub(super) fn adopt_task(&mut self, item: TaskListItem) {
        self.task_cache.insert(item.task.id(), item);
    }

    /// Captures candidates in the default inactivity window.
    pub(super) async fn preview_inactive_tasks(
        &mut self,
        session: &mut RemoteSession,
        as_of: DateTime<Utc>,
    ) -> Result<InactiveTaskPreview, ApplicationError> {
        self.preview_inactive_tasks_with_period(session, as_of, InactivityPeriod::default())
            .await
    }

    /// Captures candidate identities for the chosen cutoff and threshold.
    pub(super) async fn preview_inactive_tasks_with_period(
        &mut self,
        session: &mut RemoteSession,
        as_of: DateTime<Utc>,
        period: InactivityPeriod,
    ) -> Result<InactiveTaskPreview, ApplicationError> {
        session
            .ensure_version()
            .await
            .map_err(|error| map_application_error(&error, None, None))?;
        let as_of = canonical(as_of);
        let mut url = session.url("v1/tasks/inactive-preview");
        url.query_pairs_mut()
            .append_pair("as_of", &as_of.to_rfc3339())
            .append_pair("inactive_days", &period.days().to_string());
        let preview: InactiveTaskPreviewDto = session
            .get(url)
            .await
            .map_err(|error| map_application_error(&error, None, None))?;
        let preview = InactiveTaskPreview::validated(preview)
            .inspect_err(|_| session.set_failure(RemoteFailureKind::Protocol))?;
        if preview.as_of() != as_of || preview.inactive_days() != period.days() {
            session.set_failure(RemoteFailureKind::Protocol);
            return Err(protocol_failure("invalid inactive task preview"));
        }
        session.confirmed();
        Ok(preview)
    }

    pub(super) fn task_observation(&self) -> Option<&[TaskListItem]> {
        self.task_observation.as_deref()
    }

    pub(super) fn task_items(&self) -> &[TaskListItem] {
        self.task_observation().unwrap_or(&[])
    }

    pub(super) fn adopt_catalog(&mut self, items: Vec<TaskListItem>) {
        self.task_cache.clear();
        self.task_observation = Some(items);
    }

    pub(super) async fn create_task(
        &mut self,
        session: &mut RemoteSession,
        id: TaskId,
        body: &CreateTaskRequest,
    ) -> Result<Task, ApplicationError> {
        session
            .mutation(
                Method::POST,
                "v1/tasks",
                body,
                &body.guard,
                MutationContext {
                    worklog_id: None,
                    task_id: Some(id),
                },
                |result| {
                    let task = task_result(result, id)?;
                    if task.name().as_str() != body.name
                        || task.is_archived()
                        || task.created_at() != body.occurred_at
                        || task.updated_at() != body.occurred_at
                    {
                        return Err(protocol_failure("created task contradicts command"));
                    }
                    Ok(task)
                },
            )
            .await
    }

    pub(super) async fn task_mutation(
        &mut self,
        session: &mut RemoteSession,
        id: TaskId,
        body: &TaskChangeRequest,
    ) -> Result<Task, ApplicationError> {
        session
            .mutation(
                Method::PATCH,
                &format!("v1/tasks/{id}"),
                body,
                body.guard(),
                MutationContext {
                    worklog_id: None,
                    task_id: Some(id),
                },
                |result| {
                    let task = task_result(result, id)?;
                    let valid = match body {
                        TaskChangeRequest::Rename { name, .. } => task.name().as_str() == name,
                        TaskChangeRequest::Archive { .. } => task.is_archived(),
                        TaskChangeRequest::Restore { .. } => !task.is_archived(),
                    };
                    if !valid {
                        return Err(protocol_failure("task result contradicts command"));
                    }
                    Ok(task)
                },
            )
            .await
    }

    /// Archives the tasks that match the preview's period when the command runs.
    pub(super) async fn archive_inactive_tasks(
        &mut self,
        session: &mut RemoteSession,
        body: &ArchiveInactiveTasksRequest,
    ) -> Result<usize, ApplicationError> {
        session
            .mutation(
                Method::POST,
                "v1/tasks/archive-inactive",
                body,
                &body.guard,
                MutationContext {
                    worklog_id: None,
                    task_id: None,
                },
                |result| match result {
                    MutationResultDto::ArchivedInactive { count } => Ok(count),
                    _ => Err(protocol_failure("invalid inactive task archive result")),
                },
            )
            .await
    }
}
