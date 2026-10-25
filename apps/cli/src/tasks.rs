use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracker_application::task_search::{SearchRank, fuzzy_match};
use tracker_application::{TaskListItem, TaskOrdering};
use tracker_domain::{TaskId, TaskName};
use tracker_protocol::TaskDto;
use tracker_remote::InactiveTaskPreview;

use crate::args::{TaskSort, TaskState, Tasks};
use crate::backend::{Backend, BackendKind};
use crate::{CliError, read_json};

/// One task with its latest work start, as `tasks list` prints it.
#[derive(Serialize)]
struct TaskItemOutput {
    task: TaskDto,
    latest_work_start: Option<DateTime<Utc>>,
}

impl From<&TaskListItem> for TaskItemOutput {
    fn from(item: &TaskListItem) -> Self {
        Self {
            task: TaskDto::from(&item.task),
            latest_work_start: item.latest_work_start,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum PreviewToken {
    Local {
        backend: String,
        as_of: DateTime<Utc>,
        task_ids: Vec<String>,
    },
    Remote {
        backend: String,
        preview: InactiveTaskPreview,
    },
}

pub(crate) fn validate(
    command: &mut Tasks,
    identity: &str,
    now: DateTime<Utc>,
) -> Result<(), CliError> {
    match command {
        Tasks::Create { name } | Tasks::Rename { name, .. } => {
            TaskName::new(name.as_str()).map_err(CliError::input)?;
        }
        Tasks::ArchiveInactive { preview, yes } => {
            if !*yes {
                return Err(CliError::input("archive-inactive requires --yes"));
            }
            let token: PreviewToken = read_json(preview)?;
            validate_preview(&token, identity)?;
            if let PreviewToken::Local { as_of, .. } = &token
                && now < *as_of
            {
                return Err(CliError::input(
                    "archive time must not precede preview time",
                ));
            }
            *preview = serde_json::to_string(&token).map_err(CliError::input)?;
        }
        _ => {}
    }
    Ok(())
}

fn validate_preview(token: &PreviewToken, identity: &str) -> Result<(), CliError> {
    let backend = match token {
        PreviewToken::Local {
            backend, task_ids, ..
        } => {
            validate_local_candidates(task_ids)?;
            if !identity.starts_with("local:") {
                return Err(CliError::input("local preview requires a local database"));
            }
            backend
        }
        PreviewToken::Remote { backend, preview } => {
            validate_remote_period(preview)?;
            if !identity.starts_with("remote:") {
                return Err(CliError::input("remote preview requires a server"));
            }
            backend
        }
    };
    if backend != identity {
        return Err(CliError::input("preview belongs to a different backend"));
    }
    Ok(())
}

fn validate_local_candidates(task_ids: &[String]) -> Result<(), CliError> {
    let mut seen = HashSet::new();
    for id in task_ids {
        let id: TaskId = id.parse().map_err(CliError::input)?;
        if !seen.insert(id) {
            return Err(CliError::input("preview contains duplicate task IDs"));
        }
    }
    Ok(())
}

/// Rejects remote previews outside the CLI's fixed period before opening a connection.
///
/// `tracker-remote` validates the rest of the token while deserializing it.
fn validate_remote_period(preview: &InactiveTaskPreview) -> Result<(), CliError> {
    if preview.inactive_days() != 14 {
        return Err(CliError::input("remote preview must use the 14-day period"));
    }
    Ok(())
}

pub(crate) async fn execute(
    backend: &mut Backend,
    command: Tasks,
    now: DateTime<Utc>,
) -> Result<Value, CliError> {
    match command {
        Tasks::List {
            state,
            sort,
            search,
        } => {
            if let BackendKind::Remote(app) = &mut backend.kind {
                app.refresh_tasks().await.map_err(CliError::remote)?;
            }
            list(backend, state, sort, search.as_deref())
        }
        Tasks::Get { task_id } => {
            if let BackendKind::Remote(app) = &mut backend.kind {
                let resource = app.read_task(task_id).await.map_err(CliError::remote)?;
                return crate::json(resource.task);
            }
            get(backend, task_id)
        }
        Tasks::PreviewInactive => preview(backend, now).await,
        Tasks::ArchiveInactive { preview, .. } => archive(backend, read_json(&preview)?).await,
        change @ (Tasks::Create { .. }
        | Tasks::Rename { .. }
        | Tasks::Archive { .. }
        | Tasks::Restore { .. }) => change_task(backend, change, now).await,
    }
}

fn get(backend: &Backend, task_id: TaskId) -> Result<Value, CliError> {
    let item = backend
        .tasks(TaskOrdering::RecentlyCreated)
        .into_iter()
        .find(|item| item.task.id() == task_id)
        .ok_or_else(|| {
            CliError::application(
                tracker_application::RepositoryError::TaskNotFound { id: task_id }.into(),
            )
        })?;
    crate::json(TaskDto::from(&item))
}

async fn change_task(
    backend: &mut Backend,
    command: Tasks,
    now: DateTime<Utc>,
) -> Result<Value, CliError> {
    let task = match command {
        Tasks::Create { name } => {
            backend
                .create_task(TaskName::new(&name).map_err(CliError::input)?, now)
                .await
        }
        Tasks::Rename { task_id, name } => {
            backend
                .rename_task(task_id, TaskName::new(&name).map_err(CliError::input)?, now)
                .await
        }
        Tasks::Archive { task_id } => backend.archive_task(task_id, now).await,
        Tasks::Restore { task_id } => backend.unarchive_task(task_id, now).await,
        _ => unreachable!("task reads and bulk archive are handled before task changes"),
    }?;
    crate::json(TaskDto::from(&task))
}

fn list(
    backend: &Backend,
    state: TaskState,
    sort: TaskSort,
    search: Option<&str>,
) -> Result<Value, CliError> {
    let ordering = match sort {
        TaskSort::Worked => TaskOrdering::RecentlyWorked,
        TaskSort::Updated => TaskOrdering::RecentlyUpdated,
        TaskSort::Created => TaskOrdering::RecentlyCreated,
    };
    let mut items = backend.tasks(ordering);
    items.retain(|item| match state {
        TaskState::Active => !item.task.is_archived(),
        TaskState::Archived => item.task.is_archived(),
        TaskState::All => true,
    });
    if let Some(query) = search {
        items.retain(|item| fuzzy_match(item.task.name().as_str(), query));
        items.sort_by_key(|item| {
            SearchRank::from_task_activity(
                item.task.id(),
                item.task.created_at(),
                item.task.updated_at(),
                item.latest_work_start,
            )
        });
    }
    Ok(json!({"tasks": items.iter().map(TaskItemOutput::from).collect::<Vec<_>>()}))
}

/// Captures the eligible candidate set and backend identity for later confirmation.
async fn preview(backend: &mut Backend, as_of: DateTime<Utc>) -> Result<Value, CliError> {
    let (count, tasks, token) = match backend
        .preview_inactive_tasks(
            as_of,
            tracker_domain::InactivityPeriod::new(14).expect("default inactivity period is valid"),
        )
        .await?
    {
        tracker_client::InactivePreview::Local { tasks, .. } => {
            let token = PreviewToken::Local {
                backend: backend.identity.clone(),
                as_of,
                task_ids: tasks.iter().map(|task| task.id().to_string()).collect(),
            };
            (tasks.len(), tasks, token)
        }
        tracker_client::InactivePreview::Remote { preview, tasks } => {
            let count = preview.count();
            let token = PreviewToken::Remote {
                backend: backend.identity.clone(),
                preview,
            };
            (count, tasks, token)
        }
    };
    Ok(
        json!({"count": count, "tasks": tasks.iter().map(TaskDto::from).collect::<Vec<_>>(), "preview": token}),
    )
}

/// Archives the candidates of a preview that `validate` already matched to this backend.
async fn archive(backend: &mut Backend, token: PreviewToken) -> Result<Value, CliError> {
    let count = match (&mut backend.kind, token) {
        (
            BackendKind::Local(app),
            PreviewToken::Local {
                task_ids, as_of, ..
            },
        ) => {
            let ids = task_ids
                .iter()
                .map(|id| id.parse())
                .collect::<Result<Vec<TaskId>, _>>()
                .map_err(CliError::input)?;
            app.archive_inactive_tasks(&ids, as_of)
                .map_err(CliError::application)?
                .len()
        }
        (BackendKind::Remote(app), PreviewToken::Remote { preview, .. }) => app
            .archive_inactive_tasks(&preview)
            .await
            .map_err(CliError::application)?,
        _ => unreachable!("validate rejects previews for the other backend kind"),
    };
    Ok(json!({"archived_count": count}))
}
