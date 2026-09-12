use std::time::Duration;

use chrono::Utc;
use tracker_application::{
    ApplicationFailureCategory, ClearActiveTaskOutcome, SetActiveTaskOutcome,
    TrackerApplicationService,
};
use tracker_domain::{ActiveWorklog, Task, TaskName, TaskNameError};

use crate::app::App;
use crate::command::Command;
use crate::screens::task_list::{InputPurpose, TaskListMode, TaskView};
use crate::support::clock::tracking_timestamp;
use crate::support::errors::application_error_text;

impl<S: TrackerApplicationService> App<S> {
    pub(crate) fn handle_task_list_command(&mut self, command: Command) {
        match command {
            Command::MoveUp => self.move_task_up(),
            Command::MoveDown => self.move_task_down(),
            Command::ShowActiveTasks => self.show_tasks(TaskView::Active),
            Command::ShowArchivedTasks => self.show_tasks(TaskView::Archived),
            Command::CycleOrdering => self.cycle_ordering(),
            Command::UnarchiveSelected => self.unarchive_selected(),
            Command::ToggleTracking => self.toggle_tracking(),
            Command::OpenAdd => self.open_add(),
            Command::OpenRename => self.open_rename(),
            Command::OpenArchiveConfirm => self.open_archive_confirm(),
            Command::Insert(character) => self
                .shell_mut()
                .task_list_mut()
                .insert_name(character, TaskName::MAX_LEN),
            Command::Backspace => self.shell_mut().task_list_mut().backspace_name(),
            Command::Confirm => self.confirm_task_list(),
            Command::Cancel => self.shell_mut().task_list_mut().close_mode(),
            _ => {}
        }
    }

    fn task_list_is_normal(&self) -> bool {
        self.shell().screen() == crate::screens::Screen::TaskList
            && matches!(self.shell().task_list().mode(), TaskListMode::Normal)
    }

    fn selected_task(&self) -> Option<&Task> {
        let list = self.shell().task_list();
        let selected = list.selection()?;
        self.catalog()
            .tasks(list.view())
            .iter()
            .find(|task| task.id() == selected)
    }

    fn selected_index(&self) -> Option<usize> {
        let list = self.shell().task_list();
        let selected = list.selection()?;
        self.catalog()
            .tasks(list.view())
            .iter()
            .position(|task| task.id() == selected)
    }

    fn move_task_up(&mut self) {
        if !self.task_list_is_normal() {
            return;
        }
        let index = match self.selected_index() {
            None => self
                .catalog()
                .tasks(self.shell().task_list().view())
                .len()
                .checked_sub(1),
            Some(0) => Some(0),
            Some(index) => Some(index - 1),
        };
        let id = index.and_then(|index| {
            self.catalog()
                .tasks(self.shell().task_list().view())
                .get(index)
                .map(Task::id)
        });
        self.shell_mut().task_list_mut().set_selection(id);
    }

    fn move_task_down(&mut self) {
        if !self.task_list_is_normal() {
            return;
        }
        let view = self.shell().task_list().view();
        let tasks = self.catalog().tasks(view);
        let id = if tasks.is_empty() {
            None
        } else {
            let index = self
                .selected_index()
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
            TaskListMode::Normal => {}
        }
    }

    fn confirm_input(&mut self) {
        let TaskListMode::Input { purpose, buffer } = self.shell().task_list().mode().clone()
        else {
            return;
        };
        let name = match TaskName::new(&buffer) {
            Ok(name) => name,
            Err(error) => {
                self.shell_mut().error(task_name_error_text(error));
                return;
            }
        };
        let result = match purpose {
            InputPurpose::Add => self.application_mut().create_task(name, Utc::now()),
            InputPurpose::Rename { task_id } => {
                self.application_mut()
                    .rename_task(task_id, name, Utc::now())
            }
        };
        match (purpose, result) {
            (InputPurpose::Add, Ok(task)) => {
                self.reload_tasks();
                self.shell_mut()
                    .task_list_mut()
                    .set_selection(Some(task.id()));
                self.shell_mut().task_list_mut().close_mode();
                self.shell_mut().info(format!("Added \"{}\"", task.name()));
            }
            (InputPurpose::Rename { .. }, Ok(task)) => {
                self.reload_tasks();
                self.shell_mut().task_list_mut().close_mode();
                self.shell_mut()
                    .info(format!("Renamed to \"{}\"", task.name()));
            }
            (_, Err(error)) => self.shell_mut().error(application_error_text(&error)),
        }
    }

    fn confirm_archive(&mut self) {
        let TaskListMode::ConfirmArchive { task_id, .. } = self.shell().task_list().mode().clone()
        else {
            return;
        };
        match self.application_mut().archive_task(task_id, Utc::now()) {
            Ok(task) => {
                self.sync_from_application(false);
                self.shell_mut()
                    .task_list_mut()
                    .remember(TaskView::Archived, Some(task.id()));
                self.shell_mut().task_list_mut().close_mode();
                self.shell_mut()
                    .info(format!("Archived \"{}\"", task.name()));
            }
            Err(error) if error.failure().category() == ApplicationFailureCategory::ActiveTask => {
                self.sync_from_application(false);
                self.shell_mut().task_list_mut().close_mode();
                self.shell_mut().error("The active task cannot be archived");
            }
            Err(error) => {
                self.sync_from_application(false);
                self.shell_mut().error(application_error_text(&error));
            }
        }
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
        match self.application_mut().unarchive_task(task.id(), Utc::now()) {
            Ok(restored) => {
                self.shell_mut()
                    .task_list_mut()
                    .remember(TaskView::Active, Some(restored.id()));
                self.sync_from_application(false);
                self.shell_mut()
                    .info(format!("Restored \"{}\"", restored.name()));
            }
            Err(error) => {
                self.sync_from_application(false);
                self.shell_mut().error(application_error_text(&error));
            }
        }
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
        let result = if was_active == Some(task.id()) {
            self.application_mut()
                .clear_active_task(active.expect("active task has a worklog").id(), occurred_at)
                .map(|outcome| match outcome {
                    ClearActiveTaskOutcome::Stopped { .. }
                    | ClearActiveTaskOutcome::AlreadyIdle => ("stopped", false),
                })
        } else {
            self.application_mut()
                .set_active_task(task.id(), occurred_at)
                .map(|outcome| match outcome {
                    SetActiveTaskOutcome::Started { .. } => ("started", true),
                    SetActiveTaskOutcome::Switched { .. } => ("switched", true),
                    SetActiveTaskOutcome::AlreadyActive { .. } if was_active.is_some() => {
                        ("switched", false)
                    }
                    SetActiveTaskOutcome::AlreadyActive { .. } => ("started", false),
                })
        };
        match result {
            Ok((action, fresh_active)) => {
                self.sync_from_application(fresh_active);
                let message = match action {
                    "started" => format!("Started \"{}\"", task.name()),
                    "switched" => format!("Switched to \"{}\"", task.name()),
                    _ => format!("Stopped \"{}\"", task.name()),
                };
                self.shell_mut().info(message);
            }
            Err(error) => {
                self.sync_from_application(false);
                self.shell_mut().error(application_error_text(&error));
            }
        }
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
