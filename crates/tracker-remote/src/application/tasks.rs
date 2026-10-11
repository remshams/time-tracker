use super::{
    ApplicationError, ApplicationFailureCategory, ArchiveInactiveTasksRequest, CreateTaskRequest,
    DateTime, InactiveTaskPreviewDto, InactivityPeriod, Method, MoveCandidate, MutationResultDto,
    MutationScope, Recovery, RemoteApplication, RemoteError, RemoteFailureKind,
    ResourceObservation, Task, TaskChangeRequest, TaskDto, TaskId, TaskListItem, TaskName,
    TaskOrdering, TaskResourceDto, TasksDto, Utc, canonical, creation_needs_recovery,
    decode_task_item, decode_tasks, map_application_error, protocol_failure, task_result,
    validate_inactive_preview,
};

impl RemoteApplication {
    pub async fn refresh_tasks(&mut self) -> Result<(), RemoteError> {
        self.ensure_version().await?;
        let (items, revision) = self.fetch_task_catalog().await?;
        self.task_cache.clear();
        self.task_observation = Some(ResourceObservation {
            value: items,
            revision,
        });
        self.confirmed();
        Ok(())
    }

    pub(super) async fn fetch_task_catalog(
        &mut self,
    ) -> Result<(Vec<TaskListItem>, String), RemoteError> {
        let dto: TasksDto = self.read("v1/tasks").await?;
        decode_tasks(dto).map_err(|error| self.record_error(error))
    }

    pub fn tasks(&self, ordering: TaskOrdering) -> Vec<TaskListItem> {
        let mut items = self.task_items().to_vec();
        ordering.sort_items(&mut items);
        items
    }

    pub fn move_candidates(&self, source_task_id: TaskId, query: &str) -> Vec<MoveCandidate> {
        tracker_application::move_candidates_for_tasks(self.task_items(), source_task_id, query)
    }

    pub fn task_item(&self, id: TaskId) -> Option<&TaskListItem> {
        self.task_cache
            .get(&id)
            .map(|(item, _)| item)
            .or_else(|| self.task_items().iter().find(|item| item.task.id() == id))
    }

    pub fn task(&self, id: TaskId) -> Option<&Task> {
        self.task_item(id).map(|item| &item.task)
    }

    pub(super) async fn fetch_task(
        &mut self,
        id: TaskId,
    ) -> Result<(TaskResourceDto, TaskListItem), RemoteError> {
        self.ensure_version().await?;
        let dto: TaskResourceDto = self.read(&format!("v1/tasks/{id}")).await?;
        if dto.task.id != id.to_string() || dto.revision.is_empty() {
            return Err(self.record_error(RemoteError::Protocol("invalid task resource".into())));
        }
        let item = decode_task_item(dto.task.clone()).map_err(|error| self.record_error(error))?;
        Ok((dto, item))
    }

    pub(super) async fn read_task_item(
        &mut self,
        id: TaskId,
    ) -> Result<(TaskListItem, String), ApplicationError> {
        let (dto, item) = self
            .fetch_task(id)
            .await
            .map_err(|error| map_application_error(&error, None, Some(id)))?;
        Ok((item, dto.revision))
    }

    /// Reads one task with its own revision and preserves application error categories.
    pub async fn read_task_observation(
        &mut self,
        id: TaskId,
    ) -> Result<ResourceObservation<TaskListItem>, ApplicationError> {
        let (value, revision) = self.read_task_item(id).await?;
        self.adopt_task(value.clone(), revision.clone());
        self.confirmed();
        Ok(ResourceObservation { value, revision })
    }

    pub async fn read_task(&mut self, id: TaskId) -> Result<TaskResourceDto, RemoteError> {
        let (dto, item) = self.fetch_task(id).await?;
        self.adopt_task(item, dto.revision.clone());
        self.confirmed();
        Ok(dto)
    }

    pub async fn resolve_preview_tasks(
        &mut self,
        preview: &InactiveTaskPreviewDto,
    ) -> Result<Vec<TaskDto>, ApplicationError> {
        validate_inactive_preview(preview)?;
        let mut tasks = Vec::with_capacity(preview.count);
        for raw_id in &preview.candidate_task_ids {
            let id = raw_id
                .parse()
                .map_err(|_| protocol_failure("invalid candidate task id"))?;
            let dto = self
                .read_task(id)
                .await
                .map_err(|error| map_application_error(&error, None, Some(id)))?;
            if dto.revision != preview.revision || dto.task.archived {
                return Err(self.intent_changed());
            }
            tasks.push(dto.task);
        }
        Ok(tasks)
    }

    pub(super) fn adopt_task(&mut self, item: TaskListItem, revision: String) {
        self.task_cache.insert(item.task.id(), (item, revision));
    }

    pub fn cached_task_revision(&self, id: TaskId) -> Option<&str> {
        self.task_cache
            .get(&id)
            .map(|(_, revision)| revision.as_str())
    }

    pub(super) async fn task_guard(&mut self, id: TaskId) -> Result<String, ApplicationError> {
        let reviewed = self.task(id).cloned();
        let (item, revision) = self.read_task_item(id).await?;
        let changed = reviewed.is_some_and(|old| old != item.task);
        self.adopt_task(item, revision.clone());
        if changed {
            return Err(self.intent_changed());
        }
        Ok(revision)
    }

    pub async fn create_task(
        &mut self,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        self.last_write_attempted = false;
        self.last_command_receipt = None;
        self.refresh_task_command_view()
            .await
            .map_err(|error| map_application_error(&error, None, None))?;
        if let Some(pending) = self.pending_create.clone() {
            if pending.name != name.as_str() {
                return Err(ApplicationError::semantic_failure(
                    ApplicationFailureCategory::General,
                    "Recover the pending task creation before creating another task",
                ));
            }
            let id = pending
                .task_id
                .parse()
                .map_err(|_| protocol_failure("invalid pending task id"))?;
            if let Some(task) = self.task(id).cloned() {
                self.pending_create = None;
                return Ok(task);
            }
        }
        let mut body = self
            .pending_create
            .clone()
            .unwrap_or_else(|| CreateTaskRequest {
                task_id: TaskId::generate().to_string(),
                name: name.as_str().to_owned(),
                occurred_at: canonical(occurred_at),
                guard: Self::guard(self.task_revision()),
            });
        if body.guard.expected_revision != self.task_revision() {
            body.guard = Self::guard(self.task_revision());
        }
        let id = body
            .task_id
            .parse()
            .map_err(|_| protocol_failure("invalid pending task id"))?;
        self.pending_create = Some(body.clone());
        let mut receipt_confirmed = false;
        let result = self
            .mutation(
                Method::POST,
                "v1/tasks",
                &body,
                &body.guard,
                MutationScope {
                    recovery: Recovery::Tasks,
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
                    receipt_confirmed = true;
                    Ok(task)
                },
            )
            .await;
        if !result
            .as_ref()
            .is_err_and(|error| creation_needs_recovery(error, receipt_confirmed))
        {
            self.pending_create = None;
        }
        result
    }

    pub async fn rename_task(
        &mut self,
        id: TaskId,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        self.last_write_attempted = false;
        self.last_command_receipt = None;
        if let Some(expected_name) = self.task(id).map(|task| task.name().clone()) {
            return self
                .rename_task_with_expected_name(id, &expected_name, name, occurred_at)
                .await;
        }
        let revision = self.task_guard(id).await?;
        let body = TaskChangeRequest::Rename {
            name: name.as_str().to_owned(),
            occurred_at: canonical(occurred_at),
            guard: Self::guard(&revision),
        };
        self.task_mutation(id, body).await
    }

    pub async fn archive_task(
        &mut self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        self.last_write_attempted = false;
        self.last_command_receipt = None;
        let revision = self.task_guard(id).await?;
        self.task_mutation(
            id,
            TaskChangeRequest::Archive {
                occurred_at: canonical(occurred_at),
                guard: Self::guard(&revision),
            },
        )
        .await
    }

    pub async fn unarchive_task(
        &mut self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        self.last_write_attempted = false;
        self.last_command_receipt = None;
        let revision = self.task_guard(id).await?;
        self.task_mutation(
            id,
            TaskChangeRequest::Restore {
                occurred_at: canonical(occurred_at),
                guard: Self::guard(&revision),
            },
        )
        .await
    }

    pub(super) async fn task_mutation(
        &mut self,
        id: TaskId,
        body: TaskChangeRequest,
    ) -> Result<Task, ApplicationError> {
        self.mutation(
            Method::PATCH,
            &format!("v1/tasks/{id}"),
            &body,
            body.guard(),
            MutationScope {
                recovery: Recovery::Tasks,
                worklog_id: None,
                task_id: Some(id),
            },
            |result| {
                let task = task_result(result, id)?;
                let valid = match &body {
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

    pub async fn preview_inactive_tasks(
        &mut self,
        as_of: DateTime<Utc>,
    ) -> Result<InactiveTaskPreviewDto, ApplicationError> {
        self.preview_inactive_tasks_with_period(as_of, InactivityPeriod::default())
            .await
    }

    pub async fn preview_inactive_tasks_with_period(
        &mut self,
        as_of: DateTime<Utc>,
        period: InactivityPeriod,
    ) -> Result<InactiveTaskPreviewDto, ApplicationError> {
        self.ensure_version()
            .await
            .map_err(|error| map_application_error(&error, None, None))?;
        let as_of = canonical(as_of);
        let path = format!(
            "v1/tasks/inactive-preview?as_of={}&inactive_days={}",
            as_of.to_rfc3339().replace('+', "%2B"),
            period.days()
        );
        let preview: InactiveTaskPreviewDto = self
            .read(&path)
            .await
            .map_err(|error| map_application_error(&error, None, None))?;
        validate_inactive_preview(&preview)
            .inspect_err(|_| self.last_failure = Some(RemoteFailureKind::Protocol))?;
        if preview.as_of != as_of || preview.inactive_days != period.days() {
            self.last_failure = Some(RemoteFailureKind::Protocol);
            return Err(protocol_failure("invalid inactive task preview"));
        }
        self.confirmed();
        Ok(preview)
    }

    pub async fn archive_inactive_tasks(
        &mut self,
        preview: &InactiveTaskPreviewDto,
    ) -> Result<usize, ApplicationError> {
        self.last_write_attempted = false;
        self.last_command_receipt = None;
        validate_inactive_preview(preview)?;
        let body = ArchiveInactiveTasksRequest {
            as_of: preview.as_of,
            inactive_days: preview.inactive_days,
            guard: Self::guard(&preview.revision),
        };
        self.mutation(
            Method::POST,
            "v1/tasks/archive-inactive",
            &body,
            &body.guard,
            MutationScope {
                recovery: Recovery::Tasks,
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

    pub async fn archive_inactive_tasks_with_period(
        &mut self,
        preview: &InactiveTaskPreviewDto,
    ) -> Result<usize, ApplicationError> {
        self.last_write_attempted = false;
        self.last_command_receipt = None;
        self.archive_inactive_tasks(preview).await
    }

    pub fn task_observation(&self) -> Option<&ResourceObservation<Vec<TaskListItem>>> {
        self.task_observation.as_ref()
    }

    pub fn task_items(&self) -> &[TaskListItem] {
        self.task_observation
            .as_ref()
            .map_or(&[], |observation| observation.value.as_slice())
    }

    pub fn task_revision(&self) -> &str {
        self.task_observation
            .as_ref()
            .map_or("", |observation| observation.revision.as_str())
    }

    /// Keeps the name the user reviewed even when background reads update the cache.
    pub async fn rename_task_with_expected_name(
        &mut self,
        id: TaskId,
        expected_name: &TaskName,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        self.last_write_attempted = false;
        self.last_command_receipt = None;
        let revision = self.task_guard(id).await?;
        if self
            .task(id)
            .is_none_or(|task| task.name() != expected_name)
        {
            return Err(self.intent_changed());
        }
        let body = TaskChangeRequest::Rename {
            name: name.as_str().to_owned(),
            occurred_at: canonical(occurred_at),
            guard: Self::guard(&revision),
        };
        self.task_mutation(id, body).await
    }

    /// Keeps the name the user reviewed even when background reads update the cache.
    pub async fn archive_task_with_expected_name(
        &mut self,
        id: TaskId,
        expected_name: &TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        self.last_write_attempted = false;
        self.last_command_receipt = None;
        let revision = self.task_guard(id).await?;
        if self
            .task(id)
            .is_none_or(|task| task.name() != expected_name)
        {
            return Err(self.intent_changed());
        }
        self.task_mutation(
            id,
            TaskChangeRequest::Archive {
                occurred_at: canonical(occurred_at),
                guard: Self::guard(&revision),
            },
        )
        .await
    }
}
