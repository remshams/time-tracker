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
        match (self, other) {
            (Self::CreateTask { name: left, .. }, Self::CreateTask { name: right, .. }) => {
                left == right
            }
            (
                Self::RenameTask {
                    id: left_id,
                    name: left_name,
                    ..
                },
                Self::RenameTask {
                    id: right_id,
                    name: right_name,
                    ..
                },
            ) => left_id == right_id && left_name == right_name,
            (Self::ArchiveTask { id: left, .. }, Self::ArchiveTask { id: right, .. })
            | (Self::UnarchiveTask { id: left, .. }, Self::UnarchiveTask { id: right, .. }) => {
                left == right
            }
            (
                Self::ArchiveInactiveTasks { preview: left },
                Self::ArchiveInactiveTasks { preview: right },
            ) => left == right,
            (
                Self::SetActiveTask { task_id: left, .. },
                Self::SetActiveTask { task_id: right, .. },
            ) => left == right,
            (
                Self::ClearActiveTask {
                    expected_active: left,
                    ..
                },
                Self::ClearActiveTask {
                    expected_active: right,
                    ..
                },
            ) => left == right,
            (
                Self::MoveWorklog {
                    id: left_id,
                    expected_source_task_id: left_source,
                    expected: left_expected,
                    destination_task_id: left_destination,
                },
                Self::MoveWorklog {
                    id: right_id,
                    expected_source_task_id: right_source,
                    expected: right_expected,
                    destination_task_id: right_destination,
                },
            ) => {
                left_id == right_id
                    && left_source == right_source
                    && left_expected == right_expected
                    && left_destination == right_destination
            }
            (
                Self::CorrectWorklog {
                    id: left_id,
                    expected: left_expected,
                    replacement: left_replacement,
                    ..
                },
                Self::CorrectWorklog {
                    id: right_id,
                    expected: right_expected,
                    replacement: right_replacement,
                    ..
                },
            ) => {
                left_id == right_id
                    && left_expected == right_expected
                    && left_replacement == right_replacement
            }
            (
                Self::DeleteCompletedWorklog {
                    id: left_id,
                    expected_task_id: left_task,
                    expected: left_expected,
                },
                Self::DeleteCompletedWorklog {
                    id: right_id,
                    expected_task_id: right_task,
                    expected: right_expected,
                },
            ) => left_id == right_id && left_task == right_task && left_expected == right_expected,
            _ => false,
        }
    }
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

#[cfg(test)]
mod intent_tests {
    use chrono::{DateTime, TimeDelta, Utc};
    use tracker_domain::{TaskId, TaskName, WorklogId, WorklogTimes};

    use crate::screens::task_list::InactiveTaskPreview;

    use super::ApplicationRequest as Request;

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
