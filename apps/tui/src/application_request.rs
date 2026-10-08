//! Application use cases requested by the TUI, independent of the selected store.

use chrono::{DateTime, Utc};
use tracker_application::{
    ApplicationError, ClearActiveTaskOutcome, GlobalWorklogCursor, GlobalWorklogPage, ReportTotals,
    SetActiveTaskOutcome, TaskListItem, TaskOrdering, TrackerApplicationService, WorklogCursor,
    WorklogPage,
};
use tracker_domain::{Task, TaskId, TaskName, TrackingState, Worklog, WorklogId, WorklogTimes};
use tracker_remote::RemoteApplication;

use crate::screens::task_list::InactiveTaskPreview;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ApplicationRequest {
    CreateTask {
        name: TaskName,
        occurred_at: DateTime<Utc>,
    },
    RenameTask {
        id: TaskId,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    },
    ArchiveTask {
        id: TaskId,
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
            Self::RenameTask { id, name, .. } => WriteIntent::RenameTask(id, name),
            Self::ArchiveTask { id, .. } => WriteIntent::ArchiveTask(id),
            Self::UnarchiveTask { id, .. } => WriteIntent::UnarchiveTask(id),
            Self::ArchiveInactiveTasks { preview } => WriteIntent::ArchiveInactiveTasks(preview),
            Self::SetActiveTask { task_id, .. } => WriteIntent::SetActiveTask(task_id),
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
    RenameTask(&'a TaskId, &'a TaskName),
    ArchiveTask(&'a TaskId),
    UnarchiveTask(&'a TaskId),
    ArchiveInactiveTasks(&'a InactiveTaskPreview),
    SetActiveTask(&'a TaskId),
    ClearActiveTask(&'a WorklogId),
    MoveWorklog(&'a WorklogId, &'a TaskId, &'a WorklogTimes, &'a TaskId),
    CorrectWorklog(&'a WorklogId, &'a WorklogTimes, &'a WorklogTimes),
    DeleteCompletedWorklog(&'a WorklogId, &'a TaskId, &'a WorklogTimes),
}

#[derive(Debug)]
pub(crate) enum ApplicationOutcome {
    Task(Result<Task, ApplicationError>),
    InactiveTaskPreview(Result<InactiveTaskPreview, ApplicationError>),
    ArchivedInactiveTasks(Result<usize, ApplicationError>),
    SetActiveTask(Result<SetActiveTaskOutcome, ApplicationError>),
    ClearActiveTask(Result<ClearActiveTaskOutcome, ApplicationError>),
    WorklogPage(Result<WorklogPage, ApplicationError>),
    GlobalWorklogPage(Result<GlobalWorklogPage, ApplicationError>),
    Worklog(Result<Worklog, ApplicationError>),
    ReportTotals(Result<ReportTotals, ApplicationError>),
}

#[derive(Debug)]
pub(crate) struct ApplicationSnapshot {
    pub(crate) items: Vec<TaskListItem>,
    pub(crate) tracking: TrackingState,
}

#[derive(Debug)]
pub(crate) struct CompletedRequest {
    pub(crate) request: ApplicationRequest,
    pub(crate) outcome: ApplicationOutcome,
    pub(crate) snapshot: ApplicationSnapshot,
}

impl ApplicationSnapshot {
    pub(crate) fn from_local<S: TrackerApplicationService>(application: &S) -> Self {
        Self {
            items: application.tasks(TaskOrdering::default()),
            tracking: application.current_tracking().clone(),
        }
    }

    pub(crate) fn from_remote(application: &RemoteApplication) -> Self {
        Self {
            items: application.tasks(TaskOrdering::default()),
            tracking: application.current_tracking().clone(),
        }
    }
}

pub(crate) fn execute_local<S: TrackerApplicationService>(
    application: &mut S,
    request: ApplicationRequest,
) -> CompletedRequest {
    let outcome = match &request {
        ApplicationRequest::CreateTask { .. }
        | ApplicationRequest::RenameTask { .. }
        | ApplicationRequest::ArchiveTask { .. }
        | ApplicationRequest::PreviewInactiveTasks { .. }
        | ApplicationRequest::ArchiveInactiveTasks { .. }
        | ApplicationRequest::UnarchiveTask { .. } => execute_local_task(application, &request),
        ApplicationRequest::SetActiveTask {
            task_id,
            occurred_at,
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
    let snapshot = ApplicationSnapshot::from_local(application);
    CompletedRequest {
        request,
        outcome,
        snapshot,
    }
}

pub(crate) async fn execute_remote(
    application: &mut RemoteApplication,
    request: ApplicationRequest,
) -> CompletedRequest {
    let outcome = match &request {
        ApplicationRequest::CreateTask { .. }
        | ApplicationRequest::RenameTask { .. }
        | ApplicationRequest::ArchiveTask { .. }
        | ApplicationRequest::PreviewInactiveTasks { .. }
        | ApplicationRequest::ArchiveInactiveTasks { .. }
        | ApplicationRequest::UnarchiveTask { .. } => {
            execute_remote_task(application, &request).await
        }
        ApplicationRequest::SetActiveTask {
            task_id,
            occurred_at,
        } => ApplicationOutcome::SetActiveTask(
            application.set_active_task(*task_id, *occurred_at).await,
        ),
        ApplicationRequest::ClearActiveTask {
            expected_active,
            occurred_at,
        } => ApplicationOutcome::ClearActiveTask(
            application
                .clear_active_task(*expected_active, *occurred_at)
                .await,
        ),
        ApplicationRequest::WorklogsForTask { task_id, after } => ApplicationOutcome::WorklogPage(
            application
                .worklogs_for_task(*task_id, after.as_ref())
                .await,
        ),
        ApplicationRequest::AllWorklogs { after } => {
            ApplicationOutcome::GlobalWorklogPage(application.all_worklogs(after.as_ref()).await)
        }
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
        ApplicationRequest::ReportTotals { start, end, now } => {
            ApplicationOutcome::ReportTotals(application.report_totals(*start, *end, *now).await)
        }
    };
    let snapshot = ApplicationSnapshot::from_remote(application);
    CompletedRequest {
        request,
        outcome,
        snapshot,
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
        } => ApplicationOutcome::Task(application.rename_task(*id, name.clone(), *occurred_at)),
        ApplicationRequest::ArchiveTask { id, occurred_at } => {
            ApplicationOutcome::Task(application.archive_task(*id, *occurred_at))
        }
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
            name,
            occurred_at,
        } => ApplicationOutcome::Task(
            application
                .rename_task(*id, name.clone(), *occurred_at)
                .await,
        ),
        ApplicationRequest::ArchiveTask { id, occurred_at } => {
            ApplicationOutcome::Task(application.archive_task(*id, *occurred_at).await)
        }
        ApplicationRequest::PreviewInactiveTasks { as_of } => {
            ApplicationOutcome::InactiveTaskPreview(
                application
                    .preview_inactive_tasks(*as_of)
                    .await
                    .map(InactiveTaskPreview::Remote),
            )
        }
        ApplicationRequest::ArchiveInactiveTasks { preview } => {
            let InactiveTaskPreview::Remote(preview) = preview else {
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
    fn local_bulk_archive_uses_the_preview_and_returns_the_updated_snapshot() {
        use super::{ApplicationOutcome, execute_local};
        use crate::test_support::{TestService, at, task};

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
        let archived = completed
            .snapshot
            .items
            .iter()
            .find(|item| item.task.id() == old_task.id())
            .unwrap();
        let active = completed
            .snapshot
            .items
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
                    name: name.clone(),
                    occurred_at: at,
                },
                Request::RenameTask {
                    id: task,
                    name: name.clone(),
                    occurred_at: later,
                },
            ),
            (
                Request::ArchiveTask {
                    id: task,
                    occurred_at: at,
                },
                Request::ArchiveTask {
                    id: task,
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
                    occurred_at: at,
                },
                Request::SetActiveTask {
                    task_id: task,
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
                name: name.clone(),
                occurred_at: at,
            },
            Request::RenameTask {
                id: task,
                name: other_name,
                occurred_at: at,
            },
            Request::ArchiveTask {
                id: other_task,
                occurred_at: at,
            },
            Request::UnarchiveTask {
                id: other_task,
                occurred_at: at,
            },
            Request::SetActiveTask {
                task_id: other_task,
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
                name: name.clone(),
                occurred_at: at,
            },
            Request::RenameTask {
                id: task,
                name,
                occurred_at: at,
            },
            Request::ArchiveTask {
                id: task,
                occurred_at: at,
            },
            Request::UnarchiveTask {
                id: task,
                occurred_at: at,
            },
            Request::SetActiveTask {
                task_id: task,
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
