use std::cmp::Reverse;
use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracker_application::{TaskOperations, TaskOrdering};
use tracker_domain::{TaskId, TaskName};
use tracker_protocol::{InactiveTaskPreviewDto, TaskDto, TaskItemDto};

use crate::args::{TaskSort, TaskState, Tasks};
use crate::backend::{Backend, BackendKind};
use crate::{CliError, read_json};

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
        preview: InactiveTaskPreviewDto,
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
            let mut seen = HashSet::new();
            for id in task_ids {
                let id: TaskId = id.parse().map_err(CliError::input)?;
                if !seen.insert(id) {
                    return Err(CliError::input("preview contains duplicate task IDs"));
                }
            }
            if !identity.starts_with("local:") {
                return Err(CliError::input("local preview requires a local database"));
            }
            backend
        }
        PreviewToken::Remote { backend, preview } => {
            if preview.revision.is_empty()
                || preview.candidate_fingerprint.len() != 64
                || !preview
                    .candidate_fingerprint
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(CliError::input("invalid remote preview guard"));
            }
            if preview.sample_names.len() > 5
                || preview.sample_names.len() > preview.count
                || preview
                    .sample_names
                    .iter()
                    .any(|name| TaskName::new(name).is_err())
            {
                return Err(CliError::input("invalid remote preview candidates"));
            }
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
        } => list(backend, state, sort, search.as_deref()),
        Tasks::Get { task_id } => {
            let task = backend.task(task_id).ok_or_else(|| {
                CliError::application(
                    tracker_application::RepositoryError::TaskNotFound { id: task_id }.into(),
                    None,
                )
            })?;
            crate::json(TaskDto::from(task))
        }
        Tasks::Create { name } => crate::json(TaskDto::from(
            &backend
                .create_task(TaskName::new(&name).map_err(CliError::input)?, now)
                .await?,
        )),
        Tasks::Rename { task_id, name } => crate::json(TaskDto::from(
            &backend
                .rename_task(task_id, TaskName::new(&name).map_err(CliError::input)?, now)
                .await?,
        )),
        Tasks::Archive { task_id } => {
            crate::json(TaskDto::from(&backend.archive_task(task_id, now).await?))
        }
        Tasks::Restore { task_id } => {
            crate::json(TaskDto::from(&backend.unarchive_task(task_id, now).await?))
        }
        Tasks::PreviewInactive => preview(backend, now).await,
        Tasks::ArchiveInactive { preview, .. } => archive(backend, read_json(&preview)?, now).await,
    }
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
            (
                Reverse(
                    item.latest_work_start
                        .map_or(item.task.updated_at(), |worked| {
                            worked.max(item.task.updated_at())
                        }),
                ),
                Reverse(item.task.created_at()),
                item.task.id(),
            )
        });
    }
    Ok(json!({"tasks": items.iter().map(TaskItemDto::from).collect::<Vec<_>>()}))
}

pub(crate) fn fuzzy_match(name: &str, query: &str) -> bool {
    let mut name = name.chars().flat_map(char::to_lowercase);
    query
        .chars()
        .flat_map(char::to_lowercase)
        .all(|wanted| name.by_ref().any(|ch| ch == wanted))
}

async fn preview(backend: &mut Backend, as_of: DateTime<Utc>) -> Result<Value, CliError> {
    match &mut backend.kind {
        BackendKind::Local(app) => {
            let tasks = app
                .preview_inactive_tasks(as_of)
                .map_err(|error| CliError::application(error, None))?;
            let token = PreviewToken::Local {
                backend: backend.identity.clone(),
                as_of,
                task_ids: tasks.iter().map(|task| task.id().to_string()).collect(),
            };
            Ok(
                json!({"count": tasks.len(), "tasks": tasks.iter().map(TaskDto::from).collect::<Vec<_>>(), "preview": token}),
            )
        }
        BackendKind::Remote(app) => {
            let preview = app
                .preview_inactive_tasks(as_of)
                .await
                .map_err(|error| CliError::remote_application(error, app.last_failure()))?;
            let token = PreviewToken::Remote {
                backend: backend.identity.clone(),
                preview: preview.clone(),
            };
            Ok(
                json!({"count": preview.count, "sample_names": preview.sample_names, "preview": token}),
            )
        }
    }
}

async fn archive(
    backend: &mut Backend,
    token: PreviewToken,
    now: DateTime<Utc>,
) -> Result<Value, CliError> {
    validate_preview(&token, &backend.identity)?;
    let count = match (&mut backend.kind, token) {
        (BackendKind::Local(app), PreviewToken::Local { task_ids, .. }) => {
            let ids = task_ids
                .iter()
                .map(|id| id.parse())
                .collect::<Result<Vec<TaskId>, _>>()
                .map_err(CliError::input)?;
            app.archive_inactive_tasks(&ids, now)
                .map_err(|error| CliError::application(error, None))?
                .len()
        }
        (BackendKind::Remote(app), PreviewToken::Remote { preview, .. }) => app
            .archive_inactive_tasks(&preview)
            .await
            .map_err(|error| CliError::remote_application(error, app.last_failure()))?,
        _ => return Err(CliError::input("preview mode does not match backend")),
    };
    Ok(json!({"archived_count": count}))
}
