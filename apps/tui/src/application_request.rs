//! Application use cases requested by the TUI, independent of the selected store.

use chrono::{DateTime, Utc};
use tracker_application::{
    ApplicationError, ClearActiveTaskOutcome, GlobalWorklogCursor, GlobalWorklogPage, ReportTotals,
    SetActiveTaskOutcome, TrackerApplicationService, WorklogCursor, WorklogPage,
};
use tracker_domain::{Task, TaskId, TaskName, TrackingState, Worklog, WorklogId, WorklogTimes};
use tracker_remote::RemoteApplication;

use crate::screens::reports::{ReportPresentation, ReportPresentationRow};
use crate::screens::task_list::InactiveTaskPreview;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ApplicationRequest {
    RefreshTaskList,
    CreateTask {
        name: TaskName,
        occurred_at: DateTime<Utc>,
    },
    RenameTask {
        id: TaskId,
        expected_name: TaskName,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    },
    ArchiveTask {
        id: TaskId,
        expected_name: TaskName,
        occurred_at: DateTime<Utc>,
    },
    PreviewInactiveTasks {
        as_of: DateTime<Utc>,
    },
    ArchiveInactiveTasks {
        preview: InactiveTaskPreview,
    },
    UnarchiveTask {
        id: TaskId,
        occurred_at: DateTime<Utc>,
    },
    SetActiveTask {
        task_id: TaskId,
        expected_active: Option<WorklogId>,
        occurred_at: DateTime<Utc>,
    },
    ClearActiveTask {
        expected_active: WorklogId,
        occurred_at: DateTime<Utc>,
    },
    WorklogsForTask {
        task_id: TaskId,
        after: Option<WorklogCursor>,
    },
    AllWorklogs {
        after: Option<GlobalWorklogCursor>,
    },
    MoveWorklog {
        id: WorklogId,
        expected_source_task_id: TaskId,
        expected: WorklogTimes,
        destination_task_id: TaskId,
    },
    CorrectWorklog {
        id: WorklogId,
        expected: WorklogTimes,
        replacement: WorklogTimes,
        occurred_at: DateTime<Utc>,
    },
    DeleteCompletedWorklog {
        id: WorklogId,
        expected_task_id: TaskId,
        expected: WorklogTimes,
    },
    ReportTotals {
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    },
}

impl ApplicationRequest {
    pub(crate) fn is_write(&self) -> bool {
        matches!(
            self,
            Self::CreateTask { .. }
                | Self::RenameTask { .. }
                | Self::ArchiveTask { .. }
                | Self::ArchiveInactiveTasks { .. }
                | Self::UnarchiveTask { .. }
                | Self::SetActiveTask { .. }
                | Self::ClearActiveTask { .. }
                | Self::MoveWorklog { .. }
                | Self::CorrectWorklog { .. }
                | Self::DeleteCompletedWorklog { .. }
        )
    }

    pub(crate) fn same_write_intent(&self, other: &Self) -> bool {
        self.write_intent()
            .is_some_and(|intent| Some(intent) == other.write_intent())
    }

    fn write_intent(&self) -> Option<WriteIntent<'_>> {
        let intent = match self {
            Self::CreateTask { name, .. } => WriteIntent::CreateTask(name),
            Self::RenameTask {
                id,
                expected_name,
                name,
                ..
            } => WriteIntent::RenameTask(id, expected_name, name),
            Self::ArchiveTask {
                id, expected_name, ..
            } => WriteIntent::ArchiveTask(id, expected_name),
            Self::UnarchiveTask { id, .. } => WriteIntent::UnarchiveTask(id),
            Self::ArchiveInactiveTasks { preview } => WriteIntent::ArchiveInactiveTasks(preview),
            Self::SetActiveTask {
                task_id,
                expected_active,
                ..
            } => WriteIntent::SetActiveTask(task_id, expected_active),
            Self::ClearActiveTask {
                expected_active, ..
            } => WriteIntent::ClearActiveTask(expected_active),
            Self::MoveWorklog { .. }
            | Self::CorrectWorklog { .. }
            | Self::DeleteCompletedWorklog { .. } => return self.worklog_write_intent(),
            _ => return None,
        };
        Some(intent)
    }

    fn worklog_write_intent(&self) -> Option<WriteIntent<'_>> {
        let intent = match self {
            Self::MoveWorklog {
                id,
                expected_source_task_id,
                expected,
                destination_task_id,
            } => {
                WriteIntent::MoveWorklog(id, expected_source_task_id, expected, destination_task_id)
            }
            Self::CorrectWorklog {
                id,
                expected,
                replacement,
                ..
            } => WriteIntent::CorrectWorklog(id, expected, replacement),
            Self::DeleteCompletedWorklog {
                id,
                expected_task_id,
                expected,
            } => WriteIntent::DeleteCompletedWorklog(id, expected_task_id, expected),
            _ => return None,
        };
        Some(intent)
    }
}

#[derive(PartialEq, Eq)]
enum WriteIntent<'a> {
    CreateTask(&'a TaskName),
    RenameTask(&'a TaskId, &'a TaskName, &'a TaskName),
    ArchiveTask(&'a TaskId, &'a TaskName),
    UnarchiveTask(&'a TaskId),
    ArchiveInactiveTasks(&'a InactiveTaskPreview),
    SetActiveTask(&'a TaskId, &'a Option<WorklogId>),
    ClearActiveTask(&'a WorklogId),
    MoveWorklog(&'a WorklogId, &'a TaskId, &'a WorklogTimes, &'a TaskId),
    CorrectWorklog(&'a WorklogId, &'a WorklogTimes, &'a WorklogTimes),
    DeleteCompletedWorklog(&'a WorklogId, &'a TaskId, &'a WorklogTimes),
}

#[derive(Debug)]
pub(crate) enum ApplicationOutcome {
    TaskListRefresh(Result<(), ApplicationError>),
    Task(Result<Task, ApplicationError>),
    InactiveTaskPreview(Result<InactiveTaskPreview, ApplicationError>),
    ArchivedInactiveTasks(Result<usize, ApplicationError>),
    SetActiveTask(Result<SetActiveTaskOutcome, ApplicationError>),
    ClearActiveTask(Result<ClearActiveTaskOutcome, ApplicationError>),
    WorklogPage(Result<WorklogPage, ApplicationError>),
    GlobalWorklogPage(Result<GlobalWorklogPage, ApplicationError>),
    Worklog(Result<Worklog, ApplicationError>),
    ReportTotals(Result<ReportTotals, ApplicationError>),
    RemoteReportTotals(Result<ReportPresentation, ApplicationError>),
}

#[derive(Debug)]
pub(crate) struct CompletedRequest {
    pub(crate) request: ApplicationRequest,
    pub(crate) outcome: ApplicationOutcome,
}

impl CompletedRequest {
    pub(crate) fn metadata_task_ids(
        &self,
        tracking: Option<&TrackingState>,
    ) -> std::collections::BTreeSet<TaskId> {
        match &self.outcome {
            ApplicationOutcome::GlobalWorklogPage(Ok(page)) => page
                .worklogs
                .iter()
                .map(Worklog::task_id)
                .chain(page.task_items.iter().map(|item| item.task.id()))
                .chain(
                    page.tracking
                        .as_ref()
                        .and_then(|read| read.active_task_item.as_ref())
                        .map(|item| item.task.id()),
                )
                .collect(),
            ApplicationOutcome::WorklogPage(Ok(page)) => {
                let ApplicationRequest::WorklogsForTask { task_id, .. } = self.request else {
                    unreachable!("task history is requested for a task")
                };
                Some(task_id)
                    .into_iter()
                    .chain(
                        page.snapshot
                            .as_ref()
                            .and_then(|snapshot| snapshot.active_worklog.as_ref())
                            .map(Worklog::task_id),
                    )
                    .collect()
            }
            ApplicationOutcome::ReportTotals(Ok(totals)) => totals
                .rows
                .iter()
                .map(|row| row.task.id())
                .chain(tracking.and_then(active_task_id))
                .collect(),
            ApplicationOutcome::RemoteReportTotals(Ok(totals)) => {
                totals.rows.iter().map(|row| row.task_id).collect()
            }
            _ => std::collections::BTreeSet::new(),
        }
    }
}

fn active_task_id(tracking: &TrackingState) -> Option<TaskId> {
    match tracking {
        TrackingState::Idle => None,
        TrackingState::Running { worklog } => Some(worklog.task_id()),
    }
}

impl ApplicationRequest {
    pub(crate) fn needs_task_list(&self) -> bool {
        self.is_write()
            || matches!(
                self,
                Self::PreviewInactiveTasks { .. } | Self::RefreshTaskList
            )
    }
}

pub(crate) fn execute_local<S: TrackerApplicationService>(
    application: &mut S,
    request: ApplicationRequest,
) -> CompletedRequest {
    let outcome = match &request {
        ApplicationRequest::RefreshTaskList => {
            ApplicationOutcome::TaskListRefresh(application.refresh_task_list())
        }
        ApplicationRequest::CreateTask { .. }
        | ApplicationRequest::RenameTask { .. }
        | ApplicationRequest::ArchiveTask { .. }
        | ApplicationRequest::PreviewInactiveTasks { .. }
        | ApplicationRequest::ArchiveInactiveTasks { .. }
        | ApplicationRequest::UnarchiveTask { .. } => execute_local_task(application, &request),
        ApplicationRequest::SetActiveTask {
            task_id,
            occurred_at,
            ..
        } => ApplicationOutcome::SetActiveTask(application.set_active_task(*task_id, *occurred_at)),
        ApplicationRequest::ClearActiveTask {
            expected_active,
            occurred_at,
        } => ApplicationOutcome::ClearActiveTask(
            application.clear_active_task(*expected_active, *occurred_at),
        ),
        ApplicationRequest::WorklogsForTask { task_id, after } => {
            ApplicationOutcome::WorklogPage(application.worklogs_for_task(*task_id, after.as_ref()))
        }
        ApplicationRequest::AllWorklogs { after } => {
            ApplicationOutcome::GlobalWorklogPage(application.all_worklogs(after.as_ref()))
        }
        ApplicationRequest::MoveWorklog {
            id,
            expected_source_task_id,
            expected,
            destination_task_id,
        } => ApplicationOutcome::Worklog(application.move_worklog(
            *id,
            *expected_source_task_id,
            *expected,
            *destination_task_id,
        )),
        ApplicationRequest::CorrectWorklog {
            id,
            expected,
            replacement,
            occurred_at,
        } => ApplicationOutcome::Worklog(application.correct_worklog(
            *id,
            *expected,
            *replacement,
            *occurred_at,
        )),
        ApplicationRequest::DeleteCompletedWorklog {
            id,
            expected_task_id,
            expected,
        } => ApplicationOutcome::Worklog(application.delete_completed_worklog(
            *id,
            *expected_task_id,
            *expected,
        )),
        ApplicationRequest::ReportTotals { start, end, now } => {
            ApplicationOutcome::ReportTotals(application.report_totals(*start, *end, *now))
        }
    };
    CompletedRequest { request, outcome }
}

pub(crate) async fn execute_remote(
    application: &mut RemoteApplication,
    request: ApplicationRequest,
) -> CompletedRequest {
    let outcome = match &request {
        ApplicationRequest::RefreshTaskList => ApplicationOutcome::TaskListRefresh(
            application.refresh_task_list().await.map_err(|error| {
                if error.is_unavailable() {
                    ApplicationError::RemoteUnavailable(error.to_string())
                } else {
                    ApplicationError::RemoteProtocol(error.to_string())
                }
            }),
        ),
        ApplicationRequest::CreateTask { .. }
        | ApplicationRequest::RenameTask { .. }
        | ApplicationRequest::ArchiveTask { .. }
        | ApplicationRequest::PreviewInactiveTasks { .. }
        | ApplicationRequest::ArchiveInactiveTasks { .. }
        | ApplicationRequest::UnarchiveTask { .. } => {
            execute_remote_task(application, &request).await
        }
        ApplicationRequest::SetActiveTask { .. } | ApplicationRequest::ClearActiveTask { .. } => {
            execute_remote_tracking(application, &request).await
        }
        ApplicationRequest::WorklogsForTask { .. } | ApplicationRequest::AllWorklogs { .. } => {
            execute_remote_history(application, &request).await
        }
        ApplicationRequest::MoveWorklog { .. }
        | ApplicationRequest::CorrectWorklog { .. }
        | ApplicationRequest::DeleteCompletedWorklog { .. } => {
            execute_remote_worklog(application, &request).await
        }
        ApplicationRequest::ReportTotals { start, end, now } => {
            ApplicationOutcome::RemoteReportTotals(
                execute_remote_report(application, *start, *end, *now).await,
            )
        }
    };
    CompletedRequest { request, outcome }
}

async fn execute_remote_tracking(
    application: &mut RemoteApplication,
    request: &ApplicationRequest,
) -> ApplicationOutcome {
    match request {
        ApplicationRequest::SetActiveTask {
            task_id,
            expected_active,
            occurred_at,
        } => ApplicationOutcome::SetActiveTask(
            application
                .set_active_task_with_expected_active(*task_id, *expected_active, *occurred_at)
                .await,
        ),
        ApplicationRequest::ClearActiveTask {
            expected_active,
            occurred_at,
        } => ApplicationOutcome::ClearActiveTask(
            application
                .clear_active_task(*expected_active, *occurred_at)
                .await,
        ),
        _ => unreachable!("remote tracking dispatch receives its own requests"),
    }
}

async fn execute_remote_history(
    application: &mut RemoteApplication,
    request: &ApplicationRequest,
) -> ApplicationOutcome {
    match request {
        ApplicationRequest::WorklogsForTask { task_id, after } => {
            let result = application
                .worklogs_for_task(*task_id, after.as_ref())
                .await;
            if result.is_ok() {
                let _ = application.read_task(*task_id).await;
            }
            ApplicationOutcome::WorklogPage(result)
        }
        ApplicationRequest::AllWorklogs { after } => {
            let result = application.all_worklogs(after.as_ref()).await;
            if let Ok(page) = &result {
                resolve_history_tasks(application, &page.worklogs).await;
            }
            ApplicationOutcome::GlobalWorklogPage(result)
        }
        _ => unreachable!("remote history dispatch receives its own requests"),
    }
}

async fn execute_remote_worklog(
    application: &mut RemoteApplication,
    request: &ApplicationRequest,
) -> ApplicationOutcome {
    match request {
        ApplicationRequest::MoveWorklog {
            id,
            expected_source_task_id,
            expected,
            destination_task_id,
        } => ApplicationOutcome::Worklog(
            application
                .move_worklog(
                    *id,
                    *expected_source_task_id,
                    *expected,
                    *destination_task_id,
                )
                .await,
        ),
        ApplicationRequest::CorrectWorklog {
            id,
            expected,
            replacement,
            occurred_at,
        } => ApplicationOutcome::Worklog(
            application
                .correct_worklog(*id, *expected, *replacement, *occurred_at)
                .await,
        ),
        ApplicationRequest::DeleteCompletedWorklog {
            id,
            expected_task_id,
            expected,
        } => ApplicationOutcome::Worklog(
            application
                .delete_completed_worklog(*id, *expected_task_id, *expected)
                .await,
        ),
        _ => unreachable!("remote worklog dispatch receives its own requests"),
    }
}

async fn execute_remote_report(
    application: &mut RemoteApplication,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<ReportPresentation, ApplicationError> {
    let dto = application.task_totals(start, end, now).await?;
    let mut rows = Vec::with_capacity(dto.rows.len());
    for row in dto.rows {
        let id: TaskId = row.task_id.parse().map_err(|_| {
            ApplicationError::RemoteProtocol("Invalid report task identifier".to_owned())
        })?;
        if application.task_item(id).is_none() {
            let _ = application.read_task(id).await;
        }
        rows.push(ReportPresentationRow {
            task_id: id,
            task_name: application.task(id).map(|task| task.name().to_string()),
            duration: chrono::TimeDelta::microseconds(row.duration_us),
        });
    }
    Ok(ReportPresentation {
        rows,
        total: chrono::TimeDelta::microseconds(dto.total_us),
    })
}

async fn resolve_history_tasks(application: &mut RemoteApplication, worklogs: &[Worklog]) {
    let ids: std::collections::BTreeSet<_> = worklogs.iter().map(Worklog::task_id).collect();
    for id in ids {
        if application.task_item(id).is_none() {
            let _ = application.read_task(id).await;
        }
    }
}

fn execute_local_task<S: TrackerApplicationService>(
    application: &mut S,
    request: &ApplicationRequest,
) -> ApplicationOutcome {
    match request {
        ApplicationRequest::CreateTask { name, occurred_at } => {
            ApplicationOutcome::Task(application.create_task(name.clone(), *occurred_at))
        }
        ApplicationRequest::RenameTask {
            id,
            name,
            occurred_at,
            ..
        } => ApplicationOutcome::Task(application.rename_task(*id, name.clone(), *occurred_at)),
        ApplicationRequest::ArchiveTask {
            id, occurred_at, ..
        } => ApplicationOutcome::Task(application.archive_task(*id, *occurred_at)),
        ApplicationRequest::PreviewInactiveTasks { as_of } => {
            ApplicationOutcome::InactiveTaskPreview(application.preview_inactive_tasks(*as_of).map(
                |tasks| {
                    let sample_names = tasks
                        .iter()
                        .take(3)
                        .map(|task| task.name().to_string())
                        .collect();
                    InactiveTaskPreview::Local {
                        as_of: *as_of,
                        candidate_ids: tasks.iter().map(Task::id).collect(),
                        sample_names,
                    }
                },
            ))
        }
        ApplicationRequest::ArchiveInactiveTasks { preview } => {
            let InactiveTaskPreview::Local {
                as_of,
                candidate_ids,
                ..
            } = preview
            else {
                unreachable!("local bulk archive uses a local preview")
            };
            ApplicationOutcome::ArchivedInactiveTasks(
                application
                    .archive_inactive_tasks(candidate_ids, *as_of)
                    .map(|tasks| tasks.len()),
            )
        }
        ApplicationRequest::UnarchiveTask { id, occurred_at } => {
            ApplicationOutcome::Task(application.unarchive_task(*id, *occurred_at))
        }
        _ => unreachable!("only task management requests reach this executor"),
    }
}

async fn execute_remote_task(
    application: &mut RemoteApplication,
    request: &ApplicationRequest,
) -> ApplicationOutcome {
    match request {
        ApplicationRequest::CreateTask { name, occurred_at } => {
            ApplicationOutcome::Task(application.create_task(name.clone(), *occurred_at).await)
        }
        ApplicationRequest::RenameTask {
            id,
            expected_name,
            name,
            occurred_at,
        } => ApplicationOutcome::Task(
            application
                .rename_task_with_expected_name(*id, expected_name, name.clone(), *occurred_at)
                .await,
        ),
        ApplicationRequest::ArchiveTask {
            id,
            expected_name,
            occurred_at,
        } => ApplicationOutcome::Task(
            application
                .archive_task_with_expected_name(*id, expected_name, *occurred_at)
                .await,
        ),
        ApplicationRequest::PreviewInactiveTasks { as_of } => {
            let result = async {
                let preview = application.preview_inactive_tasks(*as_of).await?;
                let tasks = application.resolve_preview_tasks(&preview).await?;
                let sample_names = tasks.into_iter().take(5).map(|task| task.name).collect();
                Ok(InactiveTaskPreview::Remote {
                    preview,
                    sample_names,
                })
            }
            .await;
            ApplicationOutcome::InactiveTaskPreview(result)
        }
        ApplicationRequest::ArchiveInactiveTasks { preview } => {
            let InactiveTaskPreview::Remote { preview, .. } = preview else {
                unreachable!("remote bulk archive uses a remote preview")
            };
            ApplicationOutcome::ArchivedInactiveTasks(
                application.archive_inactive_tasks(preview).await,
            )
        }
        ApplicationRequest::UnarchiveTask { id, occurred_at } => {
            ApplicationOutcome::Task(application.unarchive_task(*id, *occurred_at).await)
        }
        _ => unreachable!("only task management requests reach this executor"),
    }
}

#[cfg(test)]
mod intent_tests {
    use chrono::{DateTime, TimeDelta, Utc};
    use tracker_domain::{TaskId, TaskName, WorklogId, WorklogTimes};

    use crate::screens::task_list::InactiveTaskPreview;

    use super::ApplicationRequest as Request;

    #[test]
    fn local_bulk_archive_uses_the_preview_and_updates_the_task_list() {
        use super::{ApplicationOutcome, execute_local};
        use crate::test_support::{TestService, at, task};
        use tracker_application::TaskQueries;

        let old_task = task(1, "Old task");
        let recent_task = task(2, "Recently tracked task");
        let as_of = at(20 * 24 * 60 * 60);
        let mut service = TestService::with_tasks(vec![old_task.clone(), recent_task.clone()]);
        service.latest_work_starts.push((recent_task.id(), as_of));
        let preview = execute_local(&mut service, Request::PreviewInactiveTasks { as_of });
        let ApplicationOutcome::InactiveTaskPreview(Ok(preview)) = preview.outcome else {
            panic!("local preview should succeed");
        };
        assert_eq!(preview.count(), 1);
        assert_eq!(preview.sample_names(), &["Old task"]);

        let completed = execute_local(&mut service, Request::ArchiveInactiveTasks { preview });
        assert!(matches!(
            completed.outcome,
            ApplicationOutcome::ArchivedInactiveTasks(Ok(1))
        ));
        let items = service.tasks(tracker_application::TaskOrdering::default());
        let archived = items
            .iter()
            .find(|item| item.task.id() == old_task.id())
            .unwrap();
        let active = items
            .iter()
            .find(|item| item.task.id() == recent_task.id())
            .unwrap();
        assert!(archived.task.is_archived());
        assert!(!active.task.is_archived());
    }

    #[test]
    fn write_intent_ignores_time_but_keeps_operation_target_and_expected_state() {
        let at = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
        let later = at + TimeDelta::seconds(1);
        let task = TaskId::generate();
        let other_task = TaskId::generate();
        let worklog = WorklogId::generate();
        let other_worklog = WorklogId::generate();
        let name = TaskName::new("First name").unwrap();
        let other_name = TaskName::new("Second name").unwrap();
        let times = WorklogTimes::new(at, Some(at + TimeDelta::seconds(10)));
        let other_times = WorklogTimes::new(later, Some(at + TimeDelta::seconds(11)));

        let same = [
            (
                Request::CreateTask {
                    name: name.clone(),
                    occurred_at: at,
                },
                Request::CreateTask {
                    name: name.clone(),
                    occurred_at: later,
                },
            ),
            (
                Request::RenameTask {
                    id: task,
                    expected_name: TaskName::new("Reviewed name").unwrap(),
                    name: name.clone(),
                    occurred_at: at,
                },
                Request::RenameTask {
                    id: task,
                    expected_name: TaskName::new("Reviewed name").unwrap(),
                    name: name.clone(),
                    occurred_at: later,
                },
            ),
            (
                Request::ArchiveTask {
                    id: task,
                    expected_name: TaskName::new("Reviewed name").unwrap(),
                    occurred_at: at,
                },
                Request::ArchiveTask {
                    id: task,
                    expected_name: TaskName::new("Reviewed name").unwrap(),
                    occurred_at: later,
                },
            ),
            (
                Request::UnarchiveTask {
                    id: task,
                    occurred_at: at,
                },
                Request::UnarchiveTask {
                    id: task,
                    occurred_at: later,
                },
            ),
            (
                Request::SetActiveTask {
                    task_id: task,
                    expected_active: None,
                    occurred_at: at,
                },
                Request::SetActiveTask {
                    task_id: task,
                    expected_active: None,
                    occurred_at: later,
                },
            ),
            (
                Request::ClearActiveTask {
                    expected_active: worklog,
                    occurred_at: at,
                },
                Request::ClearActiveTask {
                    expected_active: worklog,
                    occurred_at: later,
                },
            ),
            (
                Request::MoveWorklog {
                    id: worklog,
                    expected_source_task_id: task,
                    expected: times,
                    destination_task_id: other_task,
                },
                Request::MoveWorklog {
                    id: worklog,
                    expected_source_task_id: task,
                    expected: times,
                    destination_task_id: other_task,
                },
            ),
            (
                Request::CorrectWorklog {
                    id: worklog,
                    expected: times,
                    replacement: other_times,
                    occurred_at: at,
                },
                Request::CorrectWorklog {
                    id: worklog,
                    expected: times,
                    replacement: other_times,
                    occurred_at: later,
                },
            ),
            (
                Request::DeleteCompletedWorklog {
                    id: worklog,
                    expected_task_id: task,
                    expected: times,
                },
                Request::DeleteCompletedWorklog {
                    id: worklog,
                    expected_task_id: task,
                    expected: times,
                },
            ),
        ];
        for (left, right) in same {
            assert!(
                left.same_write_intent(&right),
                "{left:?} should match {right:?}"
            );
        }

        let distinct = [
            Request::CreateTask {
                name: other_name.clone(),
                occurred_at: at,
            },
            Request::RenameTask {
                id: other_task,
                expected_name: TaskName::new("Reviewed name").unwrap(),
                name: name.clone(),
                occurred_at: at,
            },
            Request::RenameTask {
                id: task,
                expected_name: TaskName::new("Reviewed name").unwrap(),
                name: other_name,
                occurred_at: at,
            },
            Request::ArchiveTask {
                id: other_task,
                expected_name: TaskName::new("Reviewed name").unwrap(),
                occurred_at: at,
            },
            Request::UnarchiveTask {
                id: other_task,
                occurred_at: at,
            },
            Request::SetActiveTask {
                task_id: other_task,
                expected_active: None,
                occurred_at: at,
            },
            Request::ClearActiveTask {
                expected_active: other_worklog,
                occurred_at: at,
            },
            Request::MoveWorklog {
                id: other_worklog,
                expected_source_task_id: task,
                expected: times,
                destination_task_id: other_task,
            },
            Request::MoveWorklog {
                id: worklog,
                expected_source_task_id: other_task,
                expected: times,
                destination_task_id: other_task,
            },
            Request::MoveWorklog {
                id: worklog,
                expected_source_task_id: task,
                expected: other_times,
                destination_task_id: other_task,
            },
            Request::MoveWorklog {
                id: worklog,
                expected_source_task_id: task,
                expected: times,
                destination_task_id: task,
            },
            Request::CorrectWorklog {
                id: other_worklog,
                expected: times,
                replacement: other_times,
                occurred_at: at,
            },
            Request::CorrectWorklog {
                id: worklog,
                expected: other_times,
                replacement: other_times,
                occurred_at: at,
            },
            Request::CorrectWorklog {
                id: worklog,
                expected: times,
                replacement: times,
                occurred_at: at,
            },
            Request::DeleteCompletedWorklog {
                id: other_worklog,
                expected_task_id: task,
                expected: times,
            },
            Request::DeleteCompletedWorklog {
                id: worklog,
                expected_task_id: other_task,
                expected: times,
            },
            Request::DeleteCompletedWorklog {
                id: worklog,
                expected_task_id: task,
                expected: other_times,
            },
        ];
        let baselines = [
            Request::CreateTask {
                name: name.clone(),
                occurred_at: at,
            },
            Request::RenameTask {
                id: task,
                expected_name: TaskName::new("Reviewed name").unwrap(),
                name: name.clone(),
                occurred_at: at,
            },
            Request::RenameTask {
                id: task,
                expected_name: TaskName::new("Reviewed name").unwrap(),
                name,
                occurred_at: at,
            },
            Request::ArchiveTask {
                id: task,
                expected_name: TaskName::new("Reviewed name").unwrap(),
                occurred_at: at,
            },
            Request::UnarchiveTask {
                id: task,
                occurred_at: at,
            },
            Request::SetActiveTask {
                task_id: task,
                expected_active: None,
                occurred_at: at,
            },
            Request::ClearActiveTask {
                expected_active: worklog,
                occurred_at: at,
            },
            Request::MoveWorklog {
                id: worklog,
                expected_source_task_id: task,
                expected: times,
                destination_task_id: other_task,
            },
            Request::MoveWorklog {
                id: worklog,
                expected_source_task_id: task,
                expected: times,
                destination_task_id: other_task,
            },
            Request::MoveWorklog {
                id: worklog,
                expected_source_task_id: task,
                expected: times,
                destination_task_id: other_task,
            },
            Request::MoveWorklog {
                id: worklog,
                expected_source_task_id: task,
                expected: times,
                destination_task_id: other_task,
            },
            Request::CorrectWorklog {
                id: worklog,
                expected: times,
                replacement: other_times,
                occurred_at: at,
            },
            Request::CorrectWorklog {
                id: worklog,
                expected: times,
                replacement: other_times,
                occurred_at: at,
            },
            Request::CorrectWorklog {
                id: worklog,
                expected: times,
                replacement: other_times,
                occurred_at: at,
            },
            Request::DeleteCompletedWorklog {
                id: worklog,
                expected_task_id: task,
                expected: times,
            },
            Request::DeleteCompletedWorklog {
                id: worklog,
                expected_task_id: task,
                expected: times,
            },
            Request::DeleteCompletedWorklog {
                id: worklog,
                expected_task_id: task,
                expected: times,
            },
        ];
        for (baseline, different) in baselines.iter().zip(distinct.iter()) {
            assert!(
                !baseline.same_write_intent(different),
                "{baseline:?} should differ from {different:?}"
            );
        }
        assert!(
            !Request::ArchiveTask {
                id: task,
                expected_name: TaskName::new("Reviewed name").unwrap(),
                occurred_at: at
            }
            .same_write_intent(&Request::UnarchiveTask {
                id: task,
                occurred_at: at
            })
        );
        assert!(
            !Request::AllWorklogs { after: None }
                .same_write_intent(&Request::AllWorklogs { after: None })
        );
    }

    #[test]
    fn bulk_archive_write_intent_matches_only_the_same_captured_preview() {
        let as_of = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
        let task = TaskId::generate();
        let preview = InactiveTaskPreview::Local {
            as_of,
            candidate_ids: vec![task],
            sample_names: vec!["old task".to_owned()],
        };
        let same = Request::ArchiveInactiveTasks {
            preview: preview.clone(),
        };
        let repeated = Request::ArchiveInactiveTasks { preview };
        let distinct = Request::ArchiveInactiveTasks {
            preview: InactiveTaskPreview::Local {
                as_of,
                candidate_ids: vec![TaskId::generate()],
                sample_names: vec!["other task".to_owned()],
            },
        };

        assert!(same.same_write_intent(&repeated));
        assert!(!same.same_write_intent(&distinct));
    }
}

#[cfg(test)]
mod reviewed_intent_tests {
    use super::*;

    #[test]
    fn queued_writes_distinguish_the_reviewed_name_and_active_worklog() {
        let at = Utc::now();
        let id = TaskId::generate();
        let first = TaskName::new("Original name").unwrap();
        let second = TaskName::new("Changed name").unwrap();
        let desired = TaskName::new("Draft name").unwrap();
        let rename = |expected_name| ApplicationRequest::RenameTask {
            id,
            expected_name,
            name: desired.clone(),
            occurred_at: at,
        };
        assert!(!rename(first.clone()).same_write_intent(&rename(second.clone())));
        let archive = |expected_name| ApplicationRequest::ArchiveTask {
            id,
            expected_name,
            occurred_at: at,
        };
        assert!(!archive(first).same_write_intent(&archive(second)));
        let tracking = |expected_active| ApplicationRequest::SetActiveTask {
            task_id: id,
            expected_active,
            occurred_at: at,
        };
        assert!(!tracking(None).same_write_intent(&tracking(Some(WorklogId::generate()))));
    }
}

#[cfg(test)]
mod remote_resource_tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::{ApplicationOutcome, ApplicationRequest, execute_remote};
    use crate::app::AppState;
    use crate::test_support::{at, task, worklog_id};
    use tracker_application::TaskListItem;
    use tracker_domain::{ActiveWorklog, TrackingState};
    use tracker_remote::RemoteApplication;

    fn report_server(
        task_id: tracker_domain::TaskId,
        metadata_available: bool,
    ) -> (String, thread::JoinHandle<Vec<String>>) {
        let report = format!(
            r#"{{"start":"{}","end":"{}","now":"{}","rows":[{{"task_id":"{task_id}","duration_us":60000000}}],"total_us":60000000,"revision":"report-1"}}"#,
            at(100).to_rfc3339(),
            at(300).to_rfc3339(),
            at(200).to_rfc3339(),
        );
        let metadata = format!(
            r#"{{"task":{{"id":"{task_id}","name":"Report task","archived":false,"created_at":"{}","updated_at":"{}","latest_work_start":null}},"revision":"task-2"}}"#,
            at(100).to_rfc3339(),
            at(100).to_rfc3339(),
        );
        let mut responses = vec![
            (200, r#"{"status":"ok","protocol_version":3}"#.to_owned()),
            (200, report.clone()),
            if metadata_available {
                (200, metadata)
            } else {
                (
                    404,
                    r#"{"code":"not_found","message":"Task metadata is unavailable"}"#.to_owned(),
                )
            },
        ];
        if metadata_available {
            responses.push((200, report));
        }
        resource_server(responses)
    }

    fn resource_server(responses: Vec<(u16, String)>) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let worker = thread::spawn(move || {
            let mut paths = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(3);
            for (status, body) in responses {
                let (mut stream, _) = loop {
                    match listener.accept() {
                        Ok(connection) => break connection,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            if Instant::now() >= deadline {
                                return paths;
                            }
                            thread::sleep(Duration::from_millis(2));
                        }
                        Err(error) => panic!("could not accept resource request: {error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut request = String::new();
                BufReader::new(&mut stream).read_line(&mut request).unwrap();
                paths.push(request.split_whitespace().nth(1).unwrap().to_owned());
                write!(stream, "HTTP/1.1 {status} Reply\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            paths
        });
        (endpoint, worker)
    }

    async fn read_report(metadata_available: bool) {
        let task_id = task(1, "Report task").id();
        let (endpoint, worker) = report_server(task_id, metadata_available);
        let mut application = RemoteApplication::disconnected(&endpoint).unwrap();
        let completed = execute_remote(
            &mut application,
            ApplicationRequest::ReportTotals {
                start: at(100),
                end: at(300),
                now: at(200),
            },
        )
        .await;
        if metadata_available {
            let repeated = execute_remote(
                &mut application,
                ApplicationRequest::ReportTotals {
                    start: at(100),
                    end: at(300),
                    now: at(200),
                },
            )
            .await;
            let ApplicationOutcome::RemoteReportTotals(Ok(totals)) = repeated.outcome else {
                panic!("a repeated report must reuse confirmed task labels");
            };
            assert_eq!(totals.total, chrono::TimeDelta::seconds(60));
            assert_eq!(totals.rows[0].task_name.as_deref(), Some("Report task"));
            let worklog = tracker_domain::ActiveWorklog::begin(
                tracker_domain::WorklogId::generate(),
                task_id,
                at(200),
            )
            .to_worklog();
            super::resolve_history_tasks(&mut application, &[worklog]).await;
            assert!(application.last_failure().is_none());
        }
        let paths = worker.join().unwrap();
        assert_eq!(paths.len(), if metadata_available { 4 } else { 3 });
        assert_eq!(paths[0], "/v1/health");
        assert!(paths[1].starts_with("/v1/reports/task-totals?"));
        assert_eq!(paths[2], format!("/v1/tasks/{task_id}"));
        let ApplicationOutcome::RemoteReportTotals(Ok(totals)) = completed.outcome else {
            panic!("report totals must survive an unavailable task label");
        };
        assert_eq!(totals.total, chrono::TimeDelta::seconds(60));
        assert_eq!(totals.rows.len(), 1);
        assert_eq!(totals.rows[0].task_id, task_id);
        assert_eq!(totals.rows[0].duration, totals.total);
        assert_eq!(
            totals.rows[0].task_name.as_deref(),
            metadata_available.then_some("Report task")
        );
        assert!(application.task_observation().is_none());
        assert!(application.tracking_observation().is_none());
    }

    #[tokio::test]
    async fn report_totals_survive_missing_labels_without_loading_tasks_or_tracking() {
        read_report(false).await;
    }

    #[tokio::test]
    async fn report_and_history_labels_reuse_independent_task_resources_without_replacing_totals() {
        read_report(true).await;
    }

    #[tokio::test]
    async fn global_history_resolves_duplicate_task_labels_once_and_reuses_them_on_later_pages() {
        let task_id = task(1, "History task").id();
        let first_id = worklog_id(1);
        let second_id = worklog_id(2);
        let third_id = worklog_id(3);
        let first_page = format!(
            r#"{{"worklogs":[{{"id":"{first_id}","task_id":"{task_id}","start":"{}","end":"{}"}},{{"id":"{second_id}","task_id":"{task_id}","start":"{}","end":"{}"}}],"next_cursor":{{"task_id":null,"start":"{}","id":"{second_id}","revision":42}},"revision":"history-1"}}"#,
            at(200).to_rfc3339(),
            at(250).to_rfc3339(),
            at(100).to_rfc3339(),
            at(150).to_rfc3339(),
            at(100).to_rfc3339(),
        );
        let second_page = format!(
            r#"{{"worklogs":[{{"id":"{third_id}","task_id":"{task_id}","start":"{}","end":"{}"}}],"next_cursor":null,"revision":"history-2"}}"#,
            at(50).to_rfc3339(),
            at(75).to_rfc3339(),
        );
        let metadata = format!(
            r#"{{"task":{{"id":"{task_id}","name":"History task","archived":false,"created_at":"{}","updated_at":"{}","latest_work_start":"{}"}},"revision":"task-3"}}"#,
            at(0).to_rfc3339(),
            at(0).to_rfc3339(),
            at(200).to_rfc3339(),
        );
        let (endpoint, worker) = resource_server(vec![
            (200, r#"{"status":"ok","protocol_version":3}"#.to_owned()),
            (200, first_page),
            (200, metadata),
            (200, second_page),
        ]);
        let mut application = RemoteApplication::disconnected(&endpoint).unwrap();
        let cached = task(2, "Previously confirmed task");
        let mut state = AppState::load_task_list(
            vec![TaskListItem {
                task: cached.clone(),
                latest_work_start: None,
            }],
            TrackingState::Idle,
        );

        let completed = execute_remote(
            &mut application,
            ApplicationRequest::AllWorklogs { after: None },
        )
        .await;
        let ApplicationOutcome::GlobalWorklogPage(Ok(page)) = &completed.outcome else {
            panic!("the first global page must load with independently resolved task metadata");
        };
        assert_eq!(
            page.worklogs
                .iter()
                .map(|worklog| worklog.id())
                .collect::<Vec<_>>(),
            vec![first_id, second_id],
        );
        assert!(page.task_items.is_empty());
        assert!(page.tracking.is_none());
        let label = application.task_item(task_id).unwrap();
        assert_eq!(label.task.name().as_str(), "History task");
        assert_eq!(label.latest_work_start, Some(at(200)));
        assert!(application.task_observation().is_none());
        assert!(application.tracking_observation().is_none());
        state.publish_request_resources(&completed, vec![label.clone()], None);
        assert_eq!(
            state.catalog().task(task_id).unwrap().name().as_str(),
            "History task"
        );
        assert_eq!(state.catalog().task(cached.id()), Some(&cached));
        assert!(state.tracking().active_worklog().is_none());

        let completed = execute_remote(
            &mut application,
            ApplicationRequest::AllWorklogs {
                after: Some(page.next_cursor.expect("the first page has older rows")),
            },
        )
        .await;
        let ApplicationOutcome::GlobalWorklogPage(Ok(page)) = completed.outcome else {
            panic!("the later global page must reuse its task label");
        };
        assert_eq!(page.worklogs.len(), 1);
        assert_eq!(page.worklogs[0].id(), third_id);
        assert!(page.next_cursor.is_none());
        assert!(page.task_items.is_empty());
        assert!(page.tracking.is_none());
        assert_eq!(
            application.task(task_id).unwrap().name().as_str(),
            "History task"
        );
        assert!(application.task_observation().is_none());
        assert!(application.tracking_observation().is_none());
        assert!(application.last_failure().is_none());

        let paths = worker.join().unwrap();
        assert_eq!(paths.len(), 4);
        assert_eq!(paths[0], "/v1/health");
        assert_eq!(paths[1], "/v1/worklogs");
        assert_eq!(paths[2], format!("/v1/tasks/{task_id}"));
        assert!(paths[3].starts_with("/v1/worklogs?"));
        assert!(paths[3].contains(&format!("after_id={second_id}")));
        assert!(paths[3].contains("after_revision=42"));
    }

    #[tokio::test]
    async fn task_history_publishes_resolved_labels_without_replacing_cached_tasks_or_tracking() {
        let history_task = task(1, "Old history name");
        let cached = task(2, "Previously confirmed task");
        let running = ActiveWorklog::begin(worklog_id(2), cached.id(), at(100));
        let mut state = AppState::load_task_list(
            vec![
                TaskListItem {
                    task: history_task.clone(),
                    latest_work_start: None,
                },
                TaskListItem {
                    task: cached.clone(),
                    latest_work_start: Some(at(100)),
                },
            ],
            TrackingState::Running {
                worklog: running.clone(),
            },
        );
        let task_id = history_task.id();
        let metadata = format!(
            r#"{{"task":{{"id":"{task_id}","name":"Resolved history name","archived":false,"created_at":"{}","updated_at":"{}","latest_work_start":null}},"revision":"task-2"}}"#,
            at(0).to_rfc3339(),
            at(200).to_rfc3339(),
        );
        let (endpoint, worker) = resource_server(vec![
            (200, r#"{"status":"ok","protocol_version":3}"#.to_owned()),
            (
                200,
                r#"{"worklogs":[],"next_cursor":null,"revision":"history-1"}"#.to_owned(),
            ),
            (200, metadata),
        ]);
        let mut application = RemoteApplication::disconnected(&endpoint).unwrap();
        let completed = execute_remote(
            &mut application,
            ApplicationRequest::WorklogsForTask {
                task_id,
                after: None,
            },
        )
        .await;
        let ApplicationOutcome::WorklogPage(Ok(page)) = &completed.outcome else {
            panic!("task history must load with its independently resolved label");
        };
        assert!(page.worklogs.is_empty());
        assert!(page.snapshot.is_none());
        let resolved = application.task_item(task_id).unwrap().clone();

        state.publish_request_resources(&completed, vec![resolved], None);

        assert_eq!(
            state.catalog().task(task_id).unwrap().name().as_str(),
            "Resolved history name"
        );
        assert_eq!(state.catalog().task(cached.id()), Some(&cached));
        assert_eq!(state.tracking().active_worklog(), Some(&running));
        assert!(application.task_observation().is_none());
        assert!(application.tracking_observation().is_none());
        assert_eq!(
            worker.join().unwrap(),
            vec![
                "/v1/health".to_owned(),
                format!("/v1/worklogs?task_id={task_id}"),
                format!("/v1/tasks/{task_id}"),
            ]
        );
    }
}
