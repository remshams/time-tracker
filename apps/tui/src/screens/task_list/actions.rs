use std::time::Duration;

use chrono::Utc;
use tracker_application::{
    ApplicationFailureCategory, ClearActiveTaskOutcome, SetActiveTaskOutcome,
    TrackerApplicationService,
};
use tracker_domain::{ActiveWorklog, Task, TaskName, TaskNameError};

use crate::app::App;
use crate::screens::task_list::{InputPurpose, TaskListCommand, TaskListMode, TaskView};
use crate::support::clock::tracking_timestamp;
use crate::support::errors::application_error_text;

impl<S: TrackerApplicationService> App<S> {
    pub(crate) fn handle_task_list_command(&mut self, command: TaskListCommand) {
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
            TaskListCommand::CopySelectedName => {
                if let Some(name) = self.selected_task().map(|task| task.name().to_string()) {
                    self.copy_text(&name);
                }
            }
            TaskListCommand::CycleOrdering => self.cycle_ordering(),
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
            TaskListCommand::UnarchiveSelected => self.unarchive_selected(),
            TaskListCommand::ToggleTracking => self.toggle_tracking(),
            TaskListCommand::OpenAdd => self.open_add(),
            TaskListCommand::OpenRename => self.open_rename(),
            TaskListCommand::OpenArchiveConfirm => self.open_archive_confirm(),
            TaskListCommand::Insert(character) => self
                .shell_mut()
                .task_list_mut()
                .insert_name(character, TaskName::MAX_LEN),
            TaskListCommand::Backspace => self.shell_mut().task_list_mut().backspace_name(),
            TaskListCommand::Confirm => self.confirm_task_list(),
            TaskListCommand::Cancel => {
                if matches!(self.shell().task_list().mode(), TaskListMode::Search) {
                    self.cancel_task_search();
                } else {
                    self.shell_mut().task_list_mut().close_mode();
                }
            }
            TaskListCommand::OpenHistory => self.open_history(),
        }
        if command != TaskListCommand::GPrefix {
            self.shell_mut().task_list_mut().set_g_prefix(false);
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
            TaskListMode::Normal | TaskListMode::Search => {}
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

#[cfg(test)]
mod navigation_tests {
    use crate::command::Command;
    use crate::screens::task_list::{TaskListCommand, TaskView};
    use crate::test_support::{TestService, app_in_timezone, task};

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
}
