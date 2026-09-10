use std::time::Duration;

use chrono::Utc;
use tracker_application::{
    ApplicationFailureCategory, ClearActiveTaskOutcome, SetActiveTaskOutcome, TaskOrdering,
    TrackerApplicationService,
};
use tracker_domain::{Task, TaskId, TaskName, TaskNameError, TrackingState, WorklogId};

use crate::app::{App, Status};
use crate::command::Command;
use crate::support::clock::{ElapsedClock, tracking_timestamp};
use crate::support::errors::application_error_text;

use super::{InputPurpose, TaskListMode, TaskListState, TaskView};

pub(crate) fn load_state<S: TrackerApplicationService>(application: &S) -> TaskListState {
    let ordering = TaskOrdering::default();
    let (tasks, archived_tasks) = task_lists(application, ordering);
    TaskListState::new(tasks, archived_tasks, ordering)
}

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
            Command::Insert(character) => self.insert_task_name(character),
            Command::Backspace => self.backspace_task_name(),
            Command::Confirm => self.confirm_task_list(),
            Command::Cancel => self.cancel_task_list(),
            _ => {}
        }
    }

    /// The tasks of the view currently shown.
    pub fn tasks(&self) -> &[Task] {
        self.task_list().tasks_in(self.view())
    }

    pub fn view(&self) -> TaskView {
        self.task_list().view()
    }

    #[cfg(test)]
    pub fn ordering(&self) -> TaskOrdering {
        self.task_list().ordering
    }

    pub fn ordering_label(&self) -> &'static str {
        ordering_label(self.task_list().ordering)
    }

    pub fn selected(&self) -> Option<usize> {
        let id = self.selection_id()?;
        self.tasks().iter().position(|task| task.id() == id)
    }

    pub fn active_task_id(&self) -> Option<TaskId> {
        match &self.tracking {
            TrackingState::Idle => None,
            TrackingState::Running { worklog } => Some(worklog.task_id()),
        }
    }

    pub fn active_task_name(&self) -> Option<&str> {
        self.task_name_for(self.active_task_id()?)
    }

    pub fn elapsed(&self) -> Option<Duration> {
        self.clock.as_ref().map(ElapsedClock::elapsed)
    }

    pub(crate) fn selected_task(&self) -> Option<&Task> {
        self.tasks().get(self.selected()?)
    }

    pub(crate) fn task_list_is_normal(&self) -> bool {
        matches!(&self.screen, crate::screens::ScreenState::TaskList(state) if matches!(state.mode(), TaskListMode::Normal))
    }

    fn selection_id(&self) -> Option<TaskId> {
        self.task_list().selection()
    }

    fn set_selection_id(&mut self, id: Option<TaskId>) {
        self.task_list_mut().set_selection(id);
    }

    fn move_task_up(&mut self) {
        if !self.task_list_is_normal() {
            return;
        }
        let index = match self.selected() {
            None => self.tasks().len().checked_sub(1),
            Some(0) => Some(0),
            Some(index) => Some(index - 1),
        };
        let id = index
            .and_then(|index| self.tasks().get(index))
            .map(Task::id);
        self.set_selection_id(id);
    }

    fn move_task_down(&mut self) {
        if !self.task_list_is_normal() {
            return;
        }
        if self.tasks().is_empty() {
            self.set_selection_id(None);
            return;
        }
        let last = self.tasks().len() - 1;
        let index = match self.selected() {
            None => 0,
            Some(index) => index.saturating_add(1).min(last),
        };
        self.set_selection_id(Some(self.tasks()[index].id()));
    }

    fn show_tasks(&mut self, target: TaskView) {
        if !self.task_list_is_normal() || self.view() == target {
            return;
        }
        let first = self.task_list().tasks_in(target).first().map(Task::id);
        let state = self.task_list_mut();
        state.view = target;
        if state.selection().is_none() {
            state.set_selection(first);
        }
    }

    fn cycle_ordering(&mut self) {
        if !self.task_list_is_normal() {
            return;
        }
        self.task_list_mut().ordering = next_ordering(self.task_list().ordering);
        self.sync_tasks_from_application();
        self.status = Status::Info(format!("Sorted by {}", self.ordering_label()));
    }

    pub(crate) fn sync_from_application(&mut self, fresh_active: bool) {
        self.sync_tasks_from_application();
        self.sync_tracking_from_application(fresh_active);
    }

    pub(crate) fn sync_tasks_from_application(&mut self) {
        let previous_index = self.selected();
        let preferred = self.selection_id();
        let ordering = self.task_list().ordering;
        let (tasks, archived_tasks) = task_lists(&self.application, ordering);
        {
            let state = self.task_list_mut();
            state.tasks = tasks;
            state.archived_tasks = archived_tasks;
        }
        let visible = self.tasks();
        let resolved = preferred
            .and_then(|id| visible.iter().position(|task| task.id() == id))
            .or_else(|| {
                previous_index
                    .filter(|_| !visible.is_empty())
                    .map(|index| index.min(visible.len() - 1))
            });
        let id = resolved.and_then(|index| visible.get(index)).map(Task::id);
        self.set_selection_id(id);
    }

    fn open_add(&mut self) {
        if !self.accepts_active_actions() {
            return;
        }
        self.task_list_mut().mode = TaskListMode::Input {
            purpose: InputPurpose::Add,
            buffer: String::new(),
        };
    }

    fn open_rename(&mut self) {
        if !self.accepts_active_actions() {
            return;
        }
        let Some(task) = self.selected_task() else {
            return;
        };
        let task_id = task.id();
        let buffer = task.name().to_string();
        self.task_list_mut().mode = TaskListMode::Input {
            purpose: InputPurpose::Rename { task_id },
            buffer,
        };
    }

    fn open_archive_confirm(&mut self) {
        if !self.accepts_active_actions() {
            return;
        }
        let Some(task) = self.selected_task() else {
            return;
        };
        let task_id = task.id();
        let name = task.name().to_string();
        self.task_list_mut().mode = TaskListMode::ConfirmArchive { task_id, name };
    }

    fn accepts_active_actions(&self) -> bool {
        self.task_list_is_normal() && self.view() == TaskView::Active
    }

    fn insert_task_name(&mut self, character: char) {
        if let TaskListMode::Input { buffer, .. } = &mut self.task_list_mut().mode
            && buffer.chars().count() < TaskName::MAX_LEN
        {
            buffer.push(character);
        }
    }

    fn backspace_task_name(&mut self) {
        if let TaskListMode::Input { buffer, .. } = &mut self.task_list_mut().mode {
            buffer.pop();
        }
    }

    fn cancel_task_list(&mut self) {
        self.task_list_mut().mode = TaskListMode::Normal;
    }

    fn confirm_task_list(&mut self) {
        match self.task_list().mode() {
            TaskListMode::Input { .. } => self.confirm_input(),
            TaskListMode::ConfirmArchive { .. } => self.confirm_archive(),
            _ => {}
        }
    }

    fn confirm_input(&mut self) {
        let TaskListMode::Input { purpose, buffer } = self.task_list().mode().clone() else {
            return;
        };
        let name = match TaskName::new(&buffer) {
            Ok(name) => name,
            Err(error) => {
                self.status = Status::Error(task_name_error_text(error));
                return;
            }
        };
        let occurred_at = Utc::now();
        let result = match purpose {
            InputPurpose::Add => self.application.create_task(name, occurred_at),
            InputPurpose::Rename { task_id } => {
                self.application.rename_task(task_id, name, occurred_at)
            }
        };
        match (purpose, result) {
            (InputPurpose::Add, Ok(task)) => {
                self.sync_tasks_from_application();
                self.set_selection_id(Some(task.id()));
                self.task_list_mut().mode = TaskListMode::Normal;
                self.status = Status::Info(format!("Added \"{}\"", task.name()));
            }
            (InputPurpose::Rename { .. }, Ok(task)) => {
                self.sync_tasks_from_application();
                self.task_list_mut().mode = TaskListMode::Normal;
                self.status = Status::Info(format!("Renamed to \"{}\"", task.name()));
            }
            (_, Err(error)) => self.status = Status::Error(application_error_text(&error)),
        }
    }

    fn confirm_archive(&mut self) {
        let TaskListMode::ConfirmArchive { task_id, .. } = self.task_list().mode().clone() else {
            return;
        };
        match self.application.archive_task(task_id, Utc::now()) {
            Ok(task) => {
                self.sync_from_application(false);
                let state = self.task_list_mut();
                state.archived_selection = Some(task.id());
                state.mode = TaskListMode::Normal;
                self.status = Status::Info(format!("Archived \"{}\"", task.name()));
            }
            Err(error) if error.failure().category() == ApplicationFailureCategory::ActiveTask => {
                self.sync_from_application(false);
                self.task_list_mut().mode = TaskListMode::Normal;
                self.status = Status::Error("The active task cannot be archived".to_owned());
            }
            Err(error) => {
                self.sync_from_application(false);
                self.status = Status::Error(application_error_text(&error));
            }
        }
    }

    fn unarchive_selected(&mut self) {
        if !self.task_list_is_normal() || self.view() != TaskView::Archived {
            return;
        }
        let Some(task) = self.selected_task().cloned() else {
            return;
        };
        match self.application.unarchive_task(task.id(), Utc::now()) {
            Ok(restored) => {
                self.task_list_mut().active_selection = Some(restored.id());
                self.sync_from_application(false);
                self.status = Status::Info(format!("Restored \"{}\"", restored.name()));
            }
            Err(error) => {
                self.sync_from_application(false);
                self.status = Status::Error(application_error_text(&error));
            }
        }
    }

    pub(crate) fn active_worklog(&self) -> Option<&tracker_domain::ActiveWorklog> {
        match &self.tracking {
            TrackingState::Idle => None,
            TrackingState::Running { worklog } => Some(worklog),
        }
    }

    pub(crate) fn active_worklog_id(&self) -> Option<WorklogId> {
        self.active_worklog().map(|worklog| worklog.id())
    }

    fn toggle_tracking(&mut self) {
        if !self.task_list_is_normal() || self.view() != TaskView::Active {
            return;
        }
        let Some(task) = self.selected_task().cloned() else {
            return;
        };
        let active = self.active_worklog().cloned();
        let was_active = active.as_ref().map(|worklog| worklog.task_id());
        let occurred_at = active.as_ref().map_or_else(Utc::now, |worklog| {
            let elapsed = self
                .clock
                .as_ref()
                .map_or(Duration::ZERO, ElapsedClock::elapsed);
            tracking_timestamp(worklog.start(), elapsed)
        });

        let result = if was_active == Some(task.id()) {
            self.application
                .clear_active_task(active.expect("active task has a worklog").id(), occurred_at)
                .map(|outcome| match outcome {
                    ClearActiveTaskOutcome::Stopped { .. }
                    | ClearActiveTaskOutcome::AlreadyIdle => ("stopped", false),
                })
        } else {
            self.application
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
                self.status = match action {
                    "started" => Status::Info(format!("Started \"{}\"", task.name())),
                    "switched" => Status::Info(format!("Switched to \"{}\"", task.name())),
                    _ => Status::Info(format!("Stopped \"{}\"", task.name())),
                };
            }
            Err(error) => {
                self.sync_from_application(false);
                self.status = Status::Error(application_error_text(&error));
            }
        }
    }

    pub(crate) fn sync_tracking_from_application(&mut self, fresh_active: bool) {
        let tracking = self.application.current_tracking().clone();
        let unchanged = tracking == self.tracking;
        if fresh_active {
            self.clock = match &tracking {
                TrackingState::Idle => None,
                TrackingState::Running { .. } => Some(ElapsedClock::anchored(Duration::ZERO)),
            };
        } else if !unchanged {
            self.clock = match &tracking {
                TrackingState::Idle => None,
                TrackingState::Running { worklog } => Some(ElapsedClock::since(worklog.start())),
            };
        }
        self.tracking = tracking;
    }

    #[cfg(test)]
    pub(crate) fn set_timezone_for_tests(&mut self, timezone: chrono_tz::Tz) {
        self.timezone = timezone;
        self.frozen_offset = None;
    }

    #[cfg(test)]
    pub(crate) fn freeze_elapsed_for_tests(&mut self, base: Duration) {
        let anchor = std::time::Instant::now()
            .checked_add(Duration::from_secs(86_400))
            .expect("a test clock can advance one day");
        self.clock = Some(ElapsedClock::at_anchor(base, anchor));
    }

    #[cfg(test)]
    pub(crate) fn freeze_offset_for_tests(&mut self, offset: chrono::FixedOffset) {
        self.frozen_offset = Some(offset);
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

fn next_ordering(ordering: TaskOrdering) -> TaskOrdering {
    match ordering {
        TaskOrdering::RecentlyWorked => TaskOrdering::RecentlyUpdated,
        TaskOrdering::RecentlyUpdated => TaskOrdering::RecentlyCreated,
        TaskOrdering::RecentlyCreated => TaskOrdering::RecentlyWorked,
    }
}

fn ordering_label(ordering: TaskOrdering) -> &'static str {
    match ordering {
        TaskOrdering::RecentlyWorked => "recently worked",
        TaskOrdering::RecentlyUpdated => "recently updated",
        TaskOrdering::RecentlyCreated => "recently created",
    }
}

fn task_lists<S: TrackerApplicationService>(
    application: &S,
    ordering: TaskOrdering,
) -> (Vec<Task>, Vec<Task>) {
    application
        .tasks(ordering)
        .into_iter()
        .map(|item| item.task)
        .partition(|task| !task.is_archived())
}
