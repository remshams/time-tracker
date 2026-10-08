use std::time::Duration;

use chrono::Utc;
use tracker_application::{
    ApplicationFailureCategory, ClearActiveTaskOutcome, SetActiveTaskOutcome,
};
use tracker_domain::{ActiveWorklog, Task, TaskName, TaskNameError};

use crate::app::AppState;
use crate::application_request::{ApplicationOutcome, ApplicationRequest};
use crate::screens::task_list::{InputPurpose, TaskListCommand, TaskListMode, TaskView};
use crate::support::clock::tracking_timestamp;
use crate::support::errors::application_error_text;

impl AppState {
    pub(crate) fn handle_task_list_command(&mut self, command: TaskListCommand) {
        match command {
            TaskListCommand::MoveUp
            | TaskListCommand::MoveDown
            | TaskListCommand::First
            | TaskListCommand::Last
            | TaskListCommand::PageUp
            | TaskListCommand::PageDown
            | TaskListCommand::GPrefix
            | TaskListCommand::ShowActiveTasks
            | TaskListCommand::ShowArchivedTasks
            | TaskListCommand::ShowReports
            | TaskListCommand::ShowAllWorklogs => self.handle_task_navigation(command),
            TaskListCommand::OpenSearch
            | TaskListCommand::CommitSearch
            | TaskListCommand::CancelSearch
            | TaskListCommand::ClearSearch
            | TaskListCommand::InsertSearch(..)
            | TaskListCommand::BackspaceSearch => self.handle_task_search_command(command),
            TaskListCommand::CopySelectedName
            | TaskListCommand::CycleOrdering
            | TaskListCommand::UnarchiveSelected
            | TaskListCommand::ToggleTracking
            | TaskListCommand::OpenHistory => self.handle_task_action(command),
            TaskListCommand::OpenAdd
            | TaskListCommand::OpenRename
            | TaskListCommand::OpenArchiveConfirm
            | TaskListCommand::OpenInactiveArchivePreview
            | TaskListCommand::Insert(..)
            | TaskListCommand::Backspace
            | TaskListCommand::Confirm
            | TaskListCommand::Cancel => self.handle_task_edit_command(command),
        }
        if command != TaskListCommand::GPrefix {
            self.shell_mut().task_list_mut().set_g_prefix(false);
        }
    }

    fn handle_task_navigation(&mut self, command: TaskListCommand) {
        match command {
            TaskListCommand::MoveUp => self.move_task_up(),
            TaskListCommand::MoveDown => self.move_task_down(),
            TaskListCommand::First
            | TaskListCommand::Last
            | TaskListCommand::PageUp
            | TaskListCommand::PageDown => self.jump_task(command),
            TaskListCommand::GPrefix => self.shell_mut().task_list_mut().set_g_prefix(true),
            TaskListCommand::ShowActiveTasks => self.show_tasks(TaskView::Active),
            TaskListCommand::ShowArchivedTasks => self.show_tasks(TaskView::Archived),
            TaskListCommand::ShowReports => self.open_reports(),
            TaskListCommand::ShowAllWorklogs => self.open_all_worklogs(),
            _ => unreachable!("only commands in this dispatch group reach this handler"),
        }
    }

    fn handle_task_search_command(&mut self, command: TaskListCommand) {
        match command {
            TaskListCommand::OpenSearch => self.open_task_search(),
            TaskListCommand::CommitSearch => self.commit_task_search(),
            TaskListCommand::CancelSearch => self.cancel_task_search(),
            TaskListCommand::ClearSearch => self.clear_task_search(),
            TaskListCommand::InsertSearch(character) => self.edit_task_search(|state| {
                state.insert_search(character, TaskName::MAX_LEN);
            }),
            TaskListCommand::BackspaceSearch => {
                self.edit_task_search(|state| state.backspace_search())
            }
            _ => unreachable!("only commands in this dispatch group reach this handler"),
        }
    }

    fn handle_task_action(&mut self, command: TaskListCommand) {
        match command {
            TaskListCommand::CopySelectedName => self.copy_selected_task_name(),
            TaskListCommand::CycleOrdering => self.cycle_ordering(),
            TaskListCommand::UnarchiveSelected => self.unarchive_selected(),
            TaskListCommand::ToggleTracking => self.toggle_tracking(),
            TaskListCommand::OpenHistory => self.open_history(),
            _ => unreachable!("only commands in this dispatch group reach this handler"),
        }
    }

    fn handle_task_edit_command(&mut self, command: TaskListCommand) {
        match command {
            TaskListCommand::OpenAdd => self.open_add(),
            TaskListCommand::OpenRename => self.open_rename(),
            TaskListCommand::OpenArchiveConfirm => self.open_archive_confirm(),
            TaskListCommand::OpenInactiveArchivePreview => self.open_inactive_archive_preview(),
            TaskListCommand::Insert(character) => self
                .shell_mut()
                .task_list_mut()
                .insert_name(character, TaskName::MAX_LEN),
            TaskListCommand::Backspace => self.shell_mut().task_list_mut().backspace_name(),
            TaskListCommand::Confirm => self.confirm_task_list(),
            TaskListCommand::Cancel => self.cancel_task_list_mode(),
            _ => unreachable!("command is handled by an earlier dispatch group"),
        }
    }

    fn copy_selected_task_name(&mut self) {
        if let Some(name) = self.selected_task().map(|task| task.name().to_string()) {
            self.copy_text(&name);
        }
    }

    fn cancel_task_list_mode(&mut self) {
        if matches!(self.shell().task_list().mode(), TaskListMode::Search) {
            self.cancel_task_search();
        } else if let TaskListMode::PreviewingInactiveTasks { as_of } =
            self.shell().task_list().mode()
        {
            let as_of = *as_of;
            self.discard_queued_inactive_preview(as_of);
            self.shell_mut().task_list_mut().close_mode();
        } else if matches!(
            self.shell().task_list().mode(),
            TaskListMode::ArchivingInactiveTasks { .. }
        ) {
            // A confirmed write cannot be dismissed while it is pending.
        } else {
            self.shell_mut().task_list_mut().close_mode();
        }
    }

    fn jump_task(&mut self, command: TaskListCommand) {
        if !self.task_list_is_normal() {
            return;
        }
        let list = self.shell().task_list();
        let visible = self
            .catalog()
            .visible_tasks(list.view(), list.search_query());
        if visible.is_empty() {
            return;
        }
        let index = list
            .selection()
            .and_then(|id| visible.iter().position(|task| task.id() == id))
            .unwrap_or(0);
        let target = match command {
            TaskListCommand::First => 0,
            TaskListCommand::Last => visible.len() - 1,
            TaskListCommand::PageUp => index.saturating_sub(10),
            TaskListCommand::PageDown => (index + 10).min(visible.len() - 1),
            _ => return,
        };
        let selected = visible[target].id();
        self.shell_mut()
            .task_list_mut()
            .set_selection(Some(selected));
    }

    fn task_list_is_normal(&self) -> bool {
        self.shell().screen() == crate::screens::Screen::TaskList
            && matches!(self.shell().task_list().mode(), TaskListMode::Normal)
    }

    fn task_list_is_navigable(&self) -> bool {
        self.shell().screen() == crate::screens::Screen::TaskList
            && matches!(
                self.shell().task_list().mode(),
                TaskListMode::Normal | TaskListMode::Search
            )
    }

    fn open_task_search(&mut self) {
        if !self.task_list_is_normal() {
            return;
        }
        self.shell_mut().task_list_mut().open_search();
        self.reconcile_search_selection();
    }

    fn edit_task_search(&mut self, edit: impl FnOnce(&mut crate::screens::TaskListState)) {
        if !matches!(self.shell().task_list().mode(), TaskListMode::Search) {
            return;
        }
        edit(self.shell_mut().task_list_mut());
        self.reconcile_search_selection();
    }

    fn reconcile_search_selection(&mut self) {
        let list = self.shell().task_list();
        let visible = self
            .catalog()
            .visible_tasks(list.view(), list.search_query());
        let selected = list
            .selection()
            .filter(|id| visible.iter().any(|task| task.id() == *id))
            .or_else(|| visible.first().map(|task| task.id()));
        self.shell_mut().task_list_mut().set_selection(selected);
    }

    fn commit_task_search(&mut self) {
        if matches!(self.shell().task_list().mode(), TaskListMode::Search) {
            self.shell_mut().task_list_mut().commit_search();
        }
    }

    fn cancel_task_search(&mut self) {
        if matches!(self.shell().task_list().mode(), TaskListMode::Search) {
            self.shell_mut().task_list_mut().cancel_search();
        }
    }

    fn clear_task_search(&mut self) {
        if self.task_list_is_normal() {
            self.shell_mut().task_list_mut().clear_search();
        }
    }

    fn selected_task(&self) -> Option<&Task> {
        let list = self.shell().task_list();
        let selected = list.selection()?;
        self.catalog()
            .visible_tasks(list.view(), list.search_query())
            .into_iter()
            .find(|task| task.id() == selected)
    }

    fn move_task_up(&mut self) {
        if !self.task_list_is_navigable() {
            return;
        }
        let list = self.shell().task_list();
        let tasks = self
            .catalog()
            .visible_tasks(list.view(), list.search_query());
        let selected = list
            .selection()
            .and_then(|id| tasks.iter().position(|task| task.id() == id));
        let index = match selected {
            None => tasks.len().checked_sub(1),
            Some(0) => Some(0),
            Some(index) => Some(index - 1),
        };
        let id = index.and_then(|index| tasks.get(index).map(|task| task.id()));
        self.shell_mut().task_list_mut().set_selection(id);
    }

    fn move_task_down(&mut self) {
        if !self.task_list_is_navigable() {
            return;
        }
        let list = self.shell().task_list();
        let tasks = self
            .catalog()
            .visible_tasks(list.view(), list.search_query());
        let id = if tasks.is_empty() {
            None
        } else {
            let index = list
                .selection()
                .and_then(|id| tasks.iter().position(|task| task.id() == id))
                .map_or(0, |index| index.saturating_add(1).min(tasks.len() - 1));
            Some(tasks[index].id())
        };
        self.shell_mut().task_list_mut().set_selection(id);
    }

    fn show_tasks(&mut self, target: TaskView) {
        if !self.task_list_is_normal() {
            return;
        }
        let first = self.catalog().tasks(target).first().map(Task::id);
        self.shell_mut().task_list_mut().show(target, first);
    }

    fn cycle_ordering(&mut self) {
        if !self.task_list_is_normal() {
            return;
        }
        self.catalog_mut().cycle_ordering();
        self.reload_tasks();
        let label = self.catalog().ordering_label();
        self.shell_mut().info(format!("Sorted by {label}"));
    }

    fn accepts_active_actions(&self) -> bool {
        self.task_list_is_normal() && self.shell().task_list().view() == TaskView::Active
    }

    fn open_add(&mut self) {
        if self.accepts_active_actions() {
            self.shell_mut()
                .task_list_mut()
                .open_input(InputPurpose::Add, String::new());
        }
    }

    fn open_rename(&mut self) {
        if !self.accepts_active_actions() {
            return;
        }
        let Some((task_id, name)) = self
            .selected_task()
            .map(|task| (task.id(), task.name().to_string()))
        else {
            return;
        };
        self.shell_mut()
            .task_list_mut()
            .open_input(InputPurpose::Rename { task_id }, name);
    }

    fn open_archive_confirm(&mut self) {
        if !self.accepts_active_actions() {
            return;
        }
        let Some((task_id, name)) = self
            .selected_task()
            .map(|task| (task.id(), task.name().to_string()))
        else {
            return;
        };
        self.shell_mut()
            .task_list_mut()
            .open_archive_confirmation(task_id, name);
    }

    fn confirm_task_list(&mut self) {
        match self.shell().task_list().mode() {
            TaskListMode::Input { .. } => self.confirm_input(),
            TaskListMode::ConfirmArchive { .. } => self.confirm_archive(),
            TaskListMode::ConfirmInactiveArchive { .. } => self.confirm_inactive_archive(),
            TaskListMode::Normal
            | TaskListMode::Search
            | TaskListMode::PreviewingInactiveTasks { .. }
            | TaskListMode::ArchivingInactiveTasks { .. } => {}
        }
    }

    fn open_inactive_archive_preview(&mut self) {
        if !self.accepts_active_actions() {
            return;
        }
        let as_of = Utc::now();
        self.shell_mut()
            .task_list_mut()
            .open_inactive_archive_preview(as_of);
        let dialog_generation = self.shell().task_list().dialog_generation();
        let accepted = self.enqueue(
            ApplicationRequest::PreviewInactiveTasks { as_of },
            move |app, completed| {
                let same_dialog = app.shell().task_list().view() == TaskView::Active
                    && app.shell().task_list().dialog_generation() == dialog_generation
                    && app.shell().task_list().mode()
                        == &TaskListMode::PreviewingInactiveTasks { as_of };
                let ApplicationOutcome::InactiveTaskPreview(result) = completed.outcome else {
                    unreachable!("inactive task preview returns a preview")
                };
                app.sync_from_snapshot(
                    completed.snapshot.items,
                    completed.snapshot.tracking,
                    false,
                );
                if !same_dialog {
                    return;
                }
                match result {
                    Ok(preview) if preview.count() == 0 => {
                        app.shell_mut().task_list_mut().close_mode();
                        app.shell_mut().info("No inactive tasks to archive");
                    }
                    Ok(preview) => app
                        .shell_mut()
                        .task_list_mut()
                        .confirm_inactive_archive(preview),
                    Err(error) => {
                        app.shell_mut().task_list_mut().close_mode();
                        app.shell_mut().error(application_error_text(&error));
                    }
                }
            },
        );
        if !accepted {
            self.shell_mut().task_list_mut().close_mode();
        }
    }

    fn confirm_inactive_archive(&mut self) {
        let mode_at_request = self.shell().task_list().mode().clone();
        let TaskListMode::ConfirmInactiveArchive { preview } = mode_at_request.clone() else {
            return;
        };
        if preview.count() == 0 {
            self.shell_mut().task_list_mut().close_mode();
            self.shell_mut().info("No inactive tasks to archive");
            return;
        }
        self.shell_mut()
            .task_list_mut()
            .begin_inactive_archive(preview.clone());
        let mode_at_request = self.shell().task_list().mode().clone();
        let completion_mode = mode_at_request.clone();
        let dialog_generation = self.shell().task_list().dialog_generation();
        let accepted = self.enqueue(
            ApplicationRequest::ArchiveInactiveTasks {
                preview: preview.clone(),
            },
            move |app, completed| {
                let same_dialog = app.shell().task_list().dialog_generation() == dialog_generation
                    && app.shell().task_list().mode() == &completion_mode;
                let ApplicationOutcome::ArchivedInactiveTasks(result) = completed.outcome else {
                    unreachable!("inactive task archive returns a count")
                };
                app.sync_from_snapshot(
                    completed.snapshot.items,
                    completed.snapshot.tracking,
                    false,
                );
                match result {
                    Ok(count) => {
                        if same_dialog {
                            app.shell_mut().task_list_mut().close_mode();
                        }
                        let noun = if count == 1 { "task" } else { "tasks" };
                        app.shell_mut()
                            .info(format!("Archived {count} inactive {noun}"));
                    }
                    Err(error)
                        if error.failure().category()
                            == ApplicationFailureCategory::InactiveTaskCandidatesChanged =>
                    {
                        if same_dialog {
                            app.shell_mut().task_list_mut().close_mode();
                        }
                        app.shell_mut()
                            .error("Inactive task list changed. Preview again");
                    }
                    Err(error) => {
                        if same_dialog {
                            app.shell_mut().task_list_mut().close_mode();
                        }
                        app.shell_mut().error(format!(
                            "{}; press D to preview again",
                            application_error_text(&error)
                        ));
                    }
                }
            },
        );
        if !accepted {
            self.shell_mut().task_list_mut().close_mode();
            self.shell_mut().error("Archive request was not queued");
        }
    }

    fn confirm_input(&mut self) {
        let mode_at_request = self.shell().task_list().mode().clone();
        let dialog_generation = self.shell().task_list().dialog_generation();
        let TaskListMode::Input { purpose, buffer } = mode_at_request.clone() else {
            return;
        };
        let name = match TaskName::new(&buffer) {
            Ok(name) => name,
            Err(error) => {
                self.shell_mut().error(task_name_error_text(error));
                return;
            }
        };
        let request = match purpose {
            InputPurpose::Add => ApplicationRequest::CreateTask {
                name,
                occurred_at: Utc::now(),
            },
            InputPurpose::Rename { task_id } => ApplicationRequest::RenameTask {
                id: task_id,
                name,
                occurred_at: Utc::now(),
            },
        };
        self.enqueue(request, move |app, completed| {
            let same_dialog = app.shell().screen() == crate::screens::Screen::TaskList
                && app.shell().task_list().dialog_generation() == dialog_generation
                && app.shell().task_list().mode() == &mode_at_request;
            let ApplicationOutcome::Task(result) = completed.outcome else {
                unreachable!("task mutation returns a task")
            };
            match (purpose, result) {
                (InputPurpose::Add, Ok(task)) => {
                    app.sync_from_snapshot(
                        completed.snapshot.items,
                        completed.snapshot.tracking,
                        false,
                    );
                    if same_dialog {
                        app.shell_mut()
                            .task_list_mut()
                            .set_selection(Some(task.id()));
                        app.shell_mut().task_list_mut().close_mode();
                    }
                    app.shell_mut().info(format!("Added \"{}\"", task.name()));
                }
                (InputPurpose::Rename { .. }, Ok(task)) => {
                    app.sync_from_snapshot(
                        completed.snapshot.items,
                        completed.snapshot.tracking,
                        false,
                    );
                    if same_dialog {
                        app.shell_mut().task_list_mut().close_mode();
                    }
                    app.shell_mut()
                        .info(format!("Renamed to \"{}\"", task.name()));
                }
                (_, Err(error)) => app.shell_mut().error(application_error_text(&error)),
            }
        });
    }

    fn confirm_archive(&mut self) {
        let mode_at_request = self.shell().task_list().mode().clone();
        let dialog_generation = self.shell().task_list().dialog_generation();
        let TaskListMode::ConfirmArchive { task_id, .. } = mode_at_request.clone() else {
            return;
        };
        self.enqueue(
            ApplicationRequest::ArchiveTask {
                id: task_id,
                occurred_at: Utc::now(),
            },
            move |app, completed| {
                let same_dialog = app.shell().screen() == crate::screens::Screen::TaskList
                    && app.shell().task_list().dialog_generation() == dialog_generation
                    && app.shell().task_list().mode() == &mode_at_request;
                let ApplicationOutcome::Task(result) = completed.outcome else {
                    unreachable!("archive returns a task")
                };
                app.sync_from_snapshot(
                    completed.snapshot.items,
                    completed.snapshot.tracking,
                    false,
                );
                match result {
                    Ok(task) => {
                        app.shell_mut()
                            .task_list_mut()
                            .remember(TaskView::Archived, Some(task.id()));
                        if same_dialog {
                            app.shell_mut().task_list_mut().close_mode();
                        }
                        app.shell_mut()
                            .info(format!("Archived \"{}\"", task.name()));
                    }
                    Err(error)
                        if error.failure().category() == ApplicationFailureCategory::ActiveTask =>
                    {
                        if same_dialog {
                            app.shell_mut().task_list_mut().close_mode();
                        }
                        app.shell_mut().error("The active task cannot be archived");
                    }
                    Err(error) => {
                        app.shell_mut().error(application_error_text(&error));
                    }
                }
            },
        );
    }

    fn unarchive_selected(&mut self) {
        if self.shell().screen() != crate::screens::Screen::TaskList
            || !self.shell().task_list().accepts_unarchiving()
        {
            return;
        }
        let Some(task) = self.selected_task().cloned() else {
            return;
        };
        self.enqueue(
            ApplicationRequest::UnarchiveTask {
                id: task.id(),
                occurred_at: Utc::now(),
            },
            |app, completed| {
                let ApplicationOutcome::Task(result) = completed.outcome else {
                    unreachable!("unarchive returns a task")
                };
                app.sync_from_snapshot(
                    completed.snapshot.items,
                    completed.snapshot.tracking,
                    false,
                );
                match result {
                    Ok(restored) => {
                        app.shell_mut()
                            .task_list_mut()
                            .remember(TaskView::Active, Some(restored.id()));
                        app.shell_mut()
                            .info(format!("Restored \"{}\"", restored.name()));
                    }
                    Err(error) => {
                        app.shell_mut().error(application_error_text(&error));
                    }
                }
            },
        );
    }

    fn toggle_tracking(&mut self) {
        if !self.accepts_active_actions() {
            return;
        }
        let Some(task) = self.selected_task().cloned() else {
            return;
        };
        let active = self.tracking().active_worklog().cloned();
        let was_active = active.as_ref().map(ActiveWorklog::task_id);
        let occurred_at = active.as_ref().map_or_else(Utc::now, |worklog| {
            tracking_timestamp(
                worklog.start(),
                self.tracking().elapsed().unwrap_or(Duration::ZERO),
            )
        });
        let request = if was_active == Some(task.id()) {
            ApplicationRequest::ClearActiveTask {
                expected_active: active.expect("active task has a worklog").id(),
                occurred_at,
            }
        } else {
            ApplicationRequest::SetActiveTask {
                task_id: task.id(),
                occurred_at,
            }
        };
        self.enqueue(request, move |app, completed| {
            let result = match completed.outcome {
                ApplicationOutcome::ClearActiveTask(result) => {
                    result.map(|outcome| match outcome {
                        ClearActiveTaskOutcome::Stopped { .. }
                        | ClearActiveTaskOutcome::AlreadyIdle => ("stopped", false),
                    })
                }
                ApplicationOutcome::SetActiveTask(result) => result.map(|outcome| match outcome {
                    SetActiveTaskOutcome::Started { .. } => ("started", true),
                    SetActiveTaskOutcome::Switched { .. } => ("switched", true),
                    SetActiveTaskOutcome::AlreadyActive { .. } if was_active.is_some() => {
                        ("switched", false)
                    }
                    SetActiveTaskOutcome::AlreadyActive { .. } => ("started", false),
                }),
                _ => unreachable!("tracking request returns a tracking outcome"),
            };
            match result {
                Ok((action, fresh_active)) => {
                    app.sync_from_snapshot(
                        completed.snapshot.items,
                        completed.snapshot.tracking,
                        fresh_active,
                    );
                    let message = match action {
                        "started" => format!("Started \"{}\"", task.name()),
                        "switched" => format!("Switched to \"{}\"", task.name()),
                        _ => format!("Stopped \"{}\"", task.name()),
                    };
                    app.shell_mut().info(message);
                }
                Err(error) => {
                    app.sync_from_snapshot(
                        completed.snapshot.items,
                        completed.snapshot.tracking,
                        false,
                    );
                    app.shell_mut().error(application_error_text(&error));
                }
            }
        });
    }
}

fn task_name_error_text(error: TaskNameError) -> String {
    match error {
        TaskNameError::Empty => "The task name must not be empty".to_owned(),
        TaskNameError::Control => "The task name must not contain control characters".to_owned(),
        TaskNameError::TooLong => format!(
            "The task name must be at most {} characters",
            TaskName::MAX_LEN
        ),
    }
}

#[cfg(test)]
mod navigation_tests {
    use crate::app::Status;
    use crate::app::{AppEffect, AppState};
    use crate::application_request::{
        ApplicationOutcome, ApplicationRequest, ApplicationSnapshot, CompletedRequest,
    };
    use crate::command::Command;
    use crate::screens::task_list::{InactiveTaskPreview, TaskListCommand, TaskListMode, TaskView};
    use crate::test_support::{TestService, app_in_timezone, task};
    use tracker_application::TaskListItem;
    use tracker_domain::{Task, TrackingState};

    fn pending_archive() -> (AppState, AppEffect, Task) {
        let selected = task(1, "selected task");
        let mut state = AppState::load_from_snapshot(
            vec![TaskListItem {
                task: selected.clone(),
                latest_work_start: None,
            }],
            TrackingState::Idle,
        );
        state.handle_task_list_command(TaskListCommand::OpenArchiveConfirm);
        state.handle_task_list_command(TaskListCommand::Confirm);
        let effect = state.take_effect().expect("archive request");
        (state, effect, selected)
    }

    #[test]
    fn a_late_inactive_preview_does_not_reopen_after_cancel() {
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
        state.handle_task_list_command(TaskListCommand::OpenInactiveArchivePreview);
        let effect = state.take_effect().expect("preview request");
        let TaskListMode::PreviewingInactiveTasks { as_of } = state.shell().task_list().mode()
        else {
            panic!("preview request should show its loading mode");
        };
        let preview = InactiveTaskPreview::Local {
            as_of: *as_of,
            candidate_ids: vec![task(1, "old task").id()],
            sample_names: vec!["old task".to_owned()],
        };

        state.handle_task_list_command(TaskListCommand::Cancel);
        let request = effect.request.clone();
        state.complete_effect(
            effect,
            CompletedRequest {
                request,
                outcome: ApplicationOutcome::InactiveTaskPreview(Ok(preview)),
                snapshot: ApplicationSnapshot {
                    items: Vec::new(),
                    tracking: TrackingState::Idle,
                },
            },
        );

        assert_eq!(state.shell().task_list().mode(), &TaskListMode::Normal);
    }

    #[test]
    fn changed_candidates_close_the_in_flight_dialog_and_require_a_new_preview() {
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
        state
            .shell_mut()
            .task_list_mut()
            .confirm_inactive_archive(InactiveTaskPreview::Local {
                as_of: chrono::Utc::now(),
                candidate_ids: vec![task(1, "old task").id()],
                sample_names: vec!["old task".to_owned()],
            });
        state.handle_task_list_command(TaskListCommand::Confirm);
        let effect = state.take_effect().expect("bulk archive request");
        let request = effect.request.clone();
        state.complete_effect(
            effect,
            CompletedRequest {
                request,
                outcome: ApplicationOutcome::ArchivedInactiveTasks(Err(
                    tracker_application::RepositoryError::InactiveTaskCandidatesChanged.into(),
                )),
                snapshot: ApplicationSnapshot {
                    items: Vec::new(),
                    tracking: TrackingState::Idle,
                },
            },
        );

        assert_eq!(state.shell().task_list().mode(), &TaskListMode::Normal);
        assert_eq!(
            state.shell().status(),
            &Status::Error("Inactive task list changed. Preview again".to_owned())
        );
    }

    #[test]
    fn a_bulk_archive_completion_keeps_a_newer_task_list_mode_open() {
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
        state
            .shell_mut()
            .task_list_mut()
            .confirm_inactive_archive(InactiveTaskPreview::Local {
                as_of: chrono::Utc::now(),
                candidate_ids: vec![task(1, "old task").id()],
                sample_names: vec!["old task".to_owned()],
            });
        state.handle_task_list_command(TaskListCommand::Confirm);
        let effect = state.take_effect().expect("bulk archive request");
        let request = effect.request.clone();
        state.shell_mut().task_list_mut().open_search();

        state.complete_effect(
            effect,
            CompletedRequest {
                request,
                outcome: ApplicationOutcome::ArchivedInactiveTasks(Ok(1)),
                snapshot: ApplicationSnapshot {
                    items: Vec::new(),
                    tracking: TrackingState::Idle,
                },
            },
        );

        assert_eq!(state.shell().task_list().mode(), &TaskListMode::Search);
        assert_eq!(
            state.shell().status(),
            &Status::Info("Archived 1 inactive task".to_owned())
        );
    }

    #[test]
    fn a_rejected_preview_request_closes_the_loading_modal() {
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
        for _ in 0..32 {
            assert!(state.enqueue(ApplicationRequest::AllWorklogs { after: None }, |_, _| {}));
        }
        assert!(!state.enqueue(ApplicationRequest::AllWorklogs { after: None }, |_, _| {}));

        state.handle_task_list_command(TaskListCommand::OpenInactiveArchivePreview);

        assert_eq!(state.shell().task_list().mode(), &TaskListMode::Normal);
        assert_eq!(
            state.shell().status(),
            &Status::Error("Too many pending requests".to_owned())
        );
    }

    fn finish_archive(state: &mut AppState, effect: AppEffect, mut selected: Task) {
        selected.archive(chrono::Utc::now());
        let request = effect.request.clone();
        state.complete_effect(
            effect,
            CompletedRequest {
                request,
                outcome: ApplicationOutcome::Task(Ok(selected.clone())),
                snapshot: ApplicationSnapshot {
                    items: vec![TaskListItem {
                        task: selected,
                        latest_work_start: None,
                    }],
                    tracking: TrackingState::Idle,
                },
            },
        );
    }

    #[test]
    fn archived_result_keeps_a_hidden_confirmation_open() {
        let (mut state, effect, selected) = pending_archive();
        state.shell_mut().open_reports(chrono::Utc::now());
        finish_archive(&mut state, effect, selected);
        assert!(matches!(
            state.shell().task_list().mode(),
            TaskListMode::ConfirmArchive { .. }
        ));
    }

    #[test]
    fn archived_result_keeps_a_newer_confirmation_open() {
        let (mut state, effect, selected) = pending_archive();
        state.handle_task_list_command(TaskListCommand::Cancel);
        state.handle_task_list_command(TaskListCommand::OpenArchiveConfirm);
        finish_archive(&mut state, effect, selected);
        assert!(matches!(
            state.shell().task_list().mode(),
            TaskListMode::ConfirmArchive { .. }
        ));
    }

    #[test]
    fn archived_result_keeps_a_search_started_after_confirmation() {
        let (mut state, effect, selected) = pending_archive();
        state.handle_task_list_command(TaskListCommand::Cancel);
        state.handle_task_list_command(TaskListCommand::OpenSearch);
        finish_archive(&mut state, effect, selected);
        assert_eq!(state.shell().task_list().mode(), &TaskListMode::Search);
    }

    #[test]
    fn page_motion_uses_the_selected_task_as_its_start() {
        let tasks = (1..=25).map(|id| task(id, &format!("task {id}"))).collect();
        let service = TestService::with_tasks(tasks);
        let mut app = app_in_timezone(service, chrono_tz::UTC);
        let ids: Vec<_> = app
            .catalog()
            .visible_tasks(TaskView::Active, None)
            .iter()
            .map(|task| task.id())
            .collect();
        app.shell_mut().task_list_mut().set_selection(Some(ids[12]));
        app.handle(Command::TaskList(TaskListCommand::PageUp));
        assert_eq!(app.shell().task_list().selection(), Some(ids[2]));
        app.handle(Command::TaskList(TaskListCommand::PageDown));
        assert_eq!(app.shell().task_list().selection(), Some(ids[12]));
    }

    #[test]
    fn completed_add_keeps_a_newer_dialog_open() {
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
        state.handle_task_list_command(TaskListCommand::OpenAdd);
        state.handle_task_list_command(TaskListCommand::Insert('a'));
        state.handle_task_list_command(TaskListCommand::Confirm);
        let effect = state.take_effect().expect("add request");
        state.handle_task_list_command(TaskListCommand::Cancel);
        state.handle_task_list_command(TaskListCommand::OpenAdd);
        state.handle_task_list_command(TaskListCommand::Insert('a'));

        let added = task(1, "a");
        let request = effect.request.clone();
        state.complete_effect(
            effect,
            CompletedRequest {
                request,
                outcome: ApplicationOutcome::Task(Ok(added.clone())),
                snapshot: ApplicationSnapshot {
                    items: vec![TaskListItem {
                        task: added,
                        latest_work_start: None,
                    }],
                    tracking: TrackingState::Idle,
                },
            },
        );
        assert!(matches!(
            state.shell().task_list().mode(),
            TaskListMode::Input { .. }
        ));
        assert_eq!(state.catalog().tasks(TaskView::Active).len(), 1);
    }
}
