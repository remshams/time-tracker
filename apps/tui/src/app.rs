//! TUI presentation state and semantic command handling.

use std::time::{Duration, Instant};

use chrono::{DateTime, TimeDelta, Utc};
use tracker_application::{
    ApplicationError, ClearActiveTaskOutcome, SetActiveTaskOutcome, TaskOrdering, TaskOutcome,
    TrackerApplicationService,
};
use tracker_domain::{Task, TaskId, TaskName, TaskNameError, TrackingError, TrackingState};

use crate::command::Command;

/// What the TUI is currently asking of the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Input {
        purpose: InputPurpose,
        buffer: String,
    },
    ConfirmArchive {
        task_id: TaskId,
        name: String,
    },
}

/// What a confirmed text input does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputPurpose {
    Add,
    Rename { task_id: TaskId },
}

/// The most recent message shown in the status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Info(String),
    Error(String),
}

/// Which task list the interface currently shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskView {
    Active,
    Archived,
}

/// The event loop's two lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    Running,
    Quitting,
}

/// Derives displayed elapsed time from a monotonic clock.
#[derive(Debug, Clone)]
pub(crate) struct ElapsedClock {
    anchor: Instant,
    base: Duration,
}

impl ElapsedClock {
    pub(crate) fn since(start: DateTime<Utc>) -> Self {
        Self::anchored(Self::base_since(start, Utc::now()))
    }

    pub(crate) fn anchored(base: Duration) -> Self {
        Self {
            anchor: Instant::now(),
            base,
        }
    }

    fn base_since(start: DateTime<Utc>, now: DateTime<Utc>) -> Duration {
        (now - start).to_std().unwrap_or(Duration::ZERO)
    }

    pub(crate) fn at(&self, since_anchor: Duration) -> Duration {
        self.base + since_anchor
    }

    pub(crate) fn elapsed(&self) -> Duration {
        self.at(self.anchor.elapsed())
    }
}

/// Converts displayed monotonic elapsed time to a client-created UTC instant.
fn tracking_timestamp(start: DateTime<Utc>, elapsed: Duration) -> DateTime<Utc> {
    let delta = TimeDelta::from_std(elapsed).unwrap_or(TimeDelta::MAX);
    start
        .checked_add_signed(delta)
        .unwrap_or(DateTime::<Utc>::MAX_UTC)
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

fn application_error_text(error: &ApplicationError) -> String {
    match error {
        ApplicationError::Domain(error) => error.to_string(),
        ApplicationError::Repository(error) | ApplicationError::TrackingWrite(error) => {
            format!("Storage error: {error}")
        }
        ApplicationError::TrackingRecovery(error) | ApplicationError::TaskRecovery(error) => {
            format!("Storage error: {error}")
        }
        ApplicationError::TrackingStateChanged => {
            "Tracking state changed in another client. Refreshed state.".to_owned()
        }
    }
}

/// Task-list presentation state.
///
/// The two views each remember their selected task by id across view
/// switches and refreshes, so archiving and unarchiving can restore the
/// selection to the row the user was on.
pub struct App<S: TrackerApplicationService> {
    application: S,
    tasks: Vec<Task>,
    archived_tasks: Vec<Task>,
    ordering: TaskOrdering,
    view: TaskView,
    active_selection: Option<TaskId>,
    archived_selection: Option<TaskId>,
    tracking: TrackingState,
    mode: Mode,
    status: Status,
    clock: Option<ElapsedClock>,
    lifecycle: Lifecycle,
}

impl<S: TrackerApplicationService> App<S> {
    /// Builds presentation state from an already loaded application service.
    pub fn load(application: S) -> Self {
        let ordering = TaskOrdering::default();
        let (tasks, archived_tasks) = task_lists(&application, ordering);
        let tracking = application.current_tracking().clone();
        let clock = match &tracking {
            TrackingState::Idle => None,
            TrackingState::Running { worklog } => Some(ElapsedClock::since(worklog.start)),
        };
        let status = if clock.is_some() {
            Status::Info("Recovered the previous active timer".to_owned())
        } else {
            Status::Info("Ready".to_owned())
        };
        let active_selection = tasks.first().map(|task| task.id);
        Self {
            application,
            tasks,
            archived_tasks,
            ordering,
            view: TaskView::Active,
            active_selection,
            archived_selection: None,
            tracking,
            mode: Mode::Normal,
            status,
            clock,
            lifecycle: Lifecycle::Running,
        }
    }

    pub fn handle(&mut self, command: Command) {
        match command {
            Command::MoveUp => self.move_up(),
            Command::MoveDown => self.move_down(),
            Command::ShowActiveTasks => self.show_tasks(TaskView::Active),
            Command::ShowArchivedTasks => self.show_tasks(TaskView::Archived),
            Command::CycleOrdering => self.cycle_ordering(),
            Command::UnarchiveSelected => self.unarchive_selected(),
            Command::ToggleTracking => self.toggle_tracking(),
            Command::OpenAdd => self.open_add(),
            Command::OpenRename => self.open_rename(),
            Command::OpenArchiveConfirm => self.open_archive_confirm(),
            Command::Confirm => self.confirm(),
            Command::Cancel => self.mode = Mode::Normal,
            Command::Insert(character) => {
                if let Mode::Input { buffer, .. } = &mut self.mode
                    && buffer.chars().count() < TaskName::MAX_LEN
                {
                    buffer.push(character);
                }
            }
            Command::Backspace => {
                if let Mode::Input { buffer, .. } = &mut self.mode {
                    buffer.pop();
                }
            }
            Command::Quit => self.lifecycle = Lifecycle::Quitting,
        }
    }

    pub fn is_running(&self) -> bool {
        self.lifecycle == Lifecycle::Running
    }

    /// The tasks of the view that is currently shown.
    pub fn tasks(&self) -> &[Task] {
        self.tasks_in(self.view)
    }

    pub fn view(&self) -> TaskView {
        self.view
    }

    /// The session-only ordering shared by the active and archived views.
    #[cfg(test)]
    pub fn ordering(&self) -> TaskOrdering {
        self.ordering
    }

    /// The concise label for the current ordering.
    pub fn ordering_label(&self) -> &'static str {
        ordering_label(self.ordering)
    }

    /// The selected row of the currently shown view.
    ///
    /// The selection is remembered by task id, so it follows the task across
    /// refreshes; a task that is no longer in this view selects nothing.
    pub fn selected(&self) -> Option<usize> {
        let id = self.selection_id()?;
        self.tasks_in(self.view)
            .iter()
            .position(|task| task.id == id)
    }

    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn active_task_id(&self) -> Option<TaskId> {
        match &self.tracking {
            TrackingState::Idle => None,
            TrackingState::Running { worklog } => Some(worklog.task_id),
        }
    }

    pub fn active_task_name(&self) -> Option<&str> {
        let task_id = self.active_task_id()?;
        // Query the application service, not the visible list: the timer
        // header must keep naming the active task in the archived view too.
        Some(self.application.task(task_id)?.name().as_str())
    }

    pub fn elapsed(&self) -> Option<Duration> {
        self.clock.as_ref().map(ElapsedClock::elapsed)
    }

    fn active_worklog(&self) -> Option<&tracker_domain::ActiveWorklog> {
        match &self.tracking {
            TrackingState::Idle => None,
            TrackingState::Running { worklog } => Some(worklog),
        }
    }

    fn tasks_in(&self, view: TaskView) -> &[Task] {
        match view {
            TaskView::Active => &self.tasks,
            TaskView::Archived => &self.archived_tasks,
        }
    }

    fn selection_id(&self) -> Option<TaskId> {
        match self.view {
            TaskView::Active => self.active_selection,
            TaskView::Archived => self.archived_selection,
        }
    }

    fn set_selection_id(&mut self, id: Option<TaskId>) {
        match self.view {
            TaskView::Active => self.active_selection = id,
            TaskView::Archived => self.archived_selection = id,
        }
    }

    fn selected_task(&self) -> Option<&Task> {
        self.tasks_in(self.view).get(self.selected()?)
    }

    fn move_up(&mut self) {
        let tasks = self.tasks_in(self.view);
        let index = match self.selected() {
            None => tasks.len().checked_sub(1),
            Some(0) => Some(0),
            Some(index) => Some(index - 1),
        };
        let id = index.and_then(|index| tasks.get(index)).map(|task| task.id);
        self.set_selection_id(id);
    }

    fn move_down(&mut self) {
        let tasks = self.tasks_in(self.view);
        if tasks.is_empty() {
            self.set_selection_id(None);
            return;
        }
        let last = tasks.len() - 1;
        let index = match self.selected() {
            None => 0,
            Some(index) => index.saturating_add(1).min(last),
        };
        self.set_selection_id(Some(tasks[index].id));
    }

    /// Switches to the requested task list.
    ///
    /// Only normal mode switches views, and switching to the shown view does
    /// nothing. A view visited with no remembered selection starts on its
    /// first row.
    fn show_tasks(&mut self, target: TaskView) {
        if self.mode != Mode::Normal || self.view == target {
            return;
        }
        self.view = target;
        if self.selection_id().is_none()
            && let Some(first) = self.tasks_in(target).first()
        {
            self.set_selection_id(Some(first.id));
        }
    }

    /// Cycles the one ordering shared by both task views.
    fn cycle_ordering(&mut self) {
        if self.mode != Mode::Normal {
            return;
        }
        self.ordering = next_ordering(self.ordering);
        self.sync_tasks_from_application();
        self.status = Status::Info(format!("Sorted by {}", self.ordering_label()));
    }

    /// Translates Space into desired tracking state with an explicit client
    /// timestamp. The application service owns persistence and recovery.
    ///
    /// Tracking is an active-view, normal-mode action; the guard keeps a
    /// stray command from acting in the archived view or a modal.
    fn toggle_tracking(&mut self) {
        if self.mode != Mode::Normal || self.view != TaskView::Active {
            return;
        }
        let Some(task) = self.selected_task().cloned() else {
            return;
        };
        let active = self.active_worklog().cloned();
        let was_active = active.as_ref().map(|worklog| worklog.task_id);
        let occurred_at = active.as_ref().map_or_else(Utc::now, |worklog| {
            let start = worklog.start;
            let elapsed = self
                .clock
                .as_ref()
                .map_or(Duration::ZERO, ElapsedClock::elapsed);
            tracking_timestamp(start, elapsed)
        });

        let result = if was_active == Some(task.id) {
            self.application
                .clear_active_task(active.expect("active task has a worklog").id, occurred_at)
                .map(|outcome| match outcome {
                    ClearActiveTaskOutcome::Stopped { .. }
                    | ClearActiveTaskOutcome::AlreadyIdle => ("stopped", false),
                })
        } else {
            self.application
                .set_active_task(task.id, occurred_at)
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

    /// Copies backend-neutral query state after an application operation.
    fn sync_from_application(&mut self, fresh_active: bool) {
        self.sync_tasks_from_application();
        self.tracking = self.application.current_tracking().clone();
        self.clock = match &self.tracking {
            TrackingState::Idle => None,
            TrackingState::Running { .. } if fresh_active => {
                Some(ElapsedClock::anchored(Duration::ZERO))
            }
            TrackingState::Running { worklog } => Some(ElapsedClock::since(worklog.start)),
        };
    }

    fn sync_tasks_from_application(&mut self) {
        let previous_index = self.selected();
        let preferred = self.selection_id();
        (self.tasks, self.archived_tasks) = task_lists(&self.application, self.ordering);
        // Prefer the remembered task, then clamp the previous row to the
        // nearest row that remains, so archiving and unarchiving keep the
        // selection on a sensible neighbor.
        let visible = self.tasks_in(self.view);
        let resolved = preferred
            .and_then(|id| visible.iter().position(|task| task.id == id))
            .or_else(|| {
                previous_index
                    .filter(|_| !visible.is_empty())
                    .map(|index| index.min(visible.len() - 1))
            });
        let id = resolved
            .and_then(|index| visible.get(index))
            .map(|task| task.id);
        self.set_selection_id(id);
    }

    fn open_add(&mut self) {
        if !self.accepts_active_actions() {
            return;
        }
        self.mode = Mode::Input {
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
        self.mode = Mode::Input {
            purpose: InputPurpose::Rename { task_id: task.id },
            buffer: task.name().to_string(),
        };
    }

    fn open_archive_confirm(&mut self) {
        if !self.accepts_active_actions() {
            return;
        }
        let Some(task) = self.selected_task() else {
            return;
        };
        self.mode = Mode::ConfirmArchive {
            task_id: task.id,
            name: task.name().to_string(),
        };
    }

    /// Whether add, rename, archive, and tracking may act right now.
    ///
    /// These actions belong to the active view's normal mode only. The key
    /// map already refuses to emit them elsewhere; this guard is the second
    /// line of defense in command handling.
    fn accepts_active_actions(&self) -> bool {
        self.mode == Mode::Normal && self.view == TaskView::Active
    }

    /// Restores the selected archived task to the active list.
    ///
    /// Only the archived view's normal mode unarchives. Success stays in the
    /// archived view, selects the restored task in the active view, and
    /// clamps the archived selection. Both outcomes resynchronize from the
    /// application, so a worklog another client started shows up in the
    /// timer header and a failed write leaves no stale tracking state
    /// behind the status line.
    fn unarchive_selected(&mut self) {
        if self.mode != Mode::Normal || self.view != TaskView::Archived {
            return;
        }
        let Some(task) = self.selected_task().cloned() else {
            return;
        };
        match self.application.unarchive_task(task.id, Utc::now()) {
            Ok(TaskOutcome::Unarchived(restored)) => {
                self.active_selection = Some(restored.id);
                self.sync_from_application(false);
                self.status = Status::Info(format!("Restored \"{}\"", restored.name()));
            }
            Ok(TaskOutcome::Created(_) | TaskOutcome::Renamed(_) | TaskOutcome::Archived(_)) => {
                unreachable!("unarchive returned another task outcome")
            }
            Err(error) => {
                self.sync_from_application(false);
                self.status = Status::Error(application_error_text(&error));
            }
        }
    }

    fn confirm(&mut self) {
        match &self.mode {
            Mode::Input { .. } => self.confirm_input(),
            Mode::ConfirmArchive { .. } => self.confirm_archive(),
            Mode::Normal => {}
        }
    }

    fn confirm_input(&mut self) {
        let Mode::Input { purpose, buffer } = self.mode.clone() else {
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
        match result {
            Ok(TaskOutcome::Created(task)) => {
                self.sync_tasks_from_application();
                self.set_selection_id(Some(task.id));
                self.mode = Mode::Normal;
                self.status = Status::Info(format!("Added \"{}\"", task.name()));
            }
            Ok(TaskOutcome::Renamed(task)) => {
                self.sync_tasks_from_application();
                self.mode = Mode::Normal;
                self.status = Status::Info(format!("Renamed to \"{}\"", task.name()));
            }
            Ok(TaskOutcome::Archived(_)) => unreachable!("input cannot archive a task"),
            Ok(TaskOutcome::Unarchived(_)) => unreachable!("input cannot unarchive a task"),
            Err(error) => self.status = Status::Error(application_error_text(&error)),
        }
    }

    fn confirm_archive(&mut self) {
        let Mode::ConfirmArchive { task_id, .. } = self.mode.clone() else {
            return;
        };
        match self.application.archive_task(task_id, Utc::now()) {
            Ok(TaskOutcome::Archived(task)) => {
                self.sync_from_application(false);
                // The archived view will select the newly archived task when
                // opened.
                self.archived_selection = Some(task.id);
                self.mode = Mode::Normal;
                self.status = Status::Info(format!("Archived \"{}\"", task.name()));
            }
            Err(ApplicationError::Domain(TrackingError::TaskIsActive { .. })) => {
                self.sync_from_application(false);
                self.mode = Mode::Normal;
                self.status = Status::Error("The active task cannot be archived".to_owned());
            }
            Err(error) => {
                self.sync_from_application(false);
                self.status = Status::Error(application_error_text(&error));
            }
            Ok(TaskOutcome::Created(_) | TaskOutcome::Renamed(_) | TaskOutcome::Unarchived(_)) => {
                unreachable!("archive returned another task outcome")
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn freeze_elapsed_for_tests(&mut self, base: Duration) {
        self.clock = Some(ElapsedClock::anchored(base));
    }
}

/// Builds both views from one ordered application read.
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

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use tracker_application::{
        RepositoryError, TaskListItem, TaskOperations, TaskOutcome, TaskQueries,
        TrackerApplication, TrackingOperations, WorklogQueries,
    };
    use tracker_domain::{ActiveWorklog, Task, TaskId, TaskName, Worklog, WorklogId};
    use tracker_storage::SqliteRepository;

    use super::*;

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).unwrap()
    }

    fn app_with(names: &[&str]) -> App<TrackerApplication<SqliteRepository>> {
        let repository = SqliteRepository::open_in_memory().unwrap();
        for name in names {
            repository
                .create_task(Task::create(
                    TaskId::generate(),
                    TaskName::new(name).unwrap(),
                    at(100),
                ))
                .unwrap();
        }
        App::load(TrackerApplication::load(repository).unwrap())
    }

    fn text(status: &Status) -> &str {
        match status {
            Status::Info(text) | Status::Error(text) => text,
        }
    }

    struct TestService {
        tasks: Vec<Task>,
        tracking: TrackingState,
        fail_create: bool,
        fail_rename: bool,
        fail_archive: bool,
        fail_unarchive: bool,
        archive_activates: Option<DateTime<Utc>>,
        unarchive_activates: Option<DateTime<Utc>>,
        set_returns_already_active: bool,
        set_timestamp: Option<DateTime<Utc>>,
        latest_work_starts: Vec<(TaskId, DateTime<Utc>)>,
    }

    impl TestService {
        fn with_tasks(tasks: Vec<Task>) -> Self {
            Self {
                tasks,
                tracking: TrackingState::Idle,
                fail_create: false,
                fail_rename: false,
                fail_archive: false,
                fail_unarchive: false,
                archive_activates: None,
                unarchive_activates: None,
                set_returns_already_active: false,
                set_timestamp: None,
                latest_work_starts: Vec::new(),
            }
        }

        fn failure() -> ApplicationError {
            ApplicationError::Repository(RepositoryError::Backend {
                message: "write failed".to_owned(),
            })
        }
    }

    impl TaskQueries for TestService {
        fn tasks(&self, ordering: TaskOrdering) -> Vec<TaskListItem> {
            let mut items = self
                .tasks
                .iter()
                .cloned()
                .map(|task| TaskListItem {
                    latest_work_start: self
                        .latest_work_starts
                        .iter()
                        .find(|(id, _)| *id == task.id)
                        .map(|(_, start)| *start),
                    task,
                })
                .collect::<Vec<_>>();
            ordering.sort_items(&mut items);
            items
        }

        fn task(&self, id: TaskId) -> Option<&Task> {
            self.tasks.iter().find(|task| task.id == id)
        }
    }

    impl TaskOperations for TestService {
        fn create_task(
            &mut self,
            name: TaskName,
            occurred_at: DateTime<Utc>,
        ) -> Result<TaskOutcome, ApplicationError> {
            if self.fail_create {
                return Err(Self::failure());
            }
            let task = Task::create(
                TaskId::from_uuid(uuid::Uuid::from_u128(2)),
                name,
                occurred_at,
            );
            self.tasks.push(task.clone());
            Ok(TaskOutcome::Created(task))
        }

        fn rename_task(
            &mut self,
            id: TaskId,
            name: TaskName,
            occurred_at: DateTime<Utc>,
        ) -> Result<TaskOutcome, ApplicationError> {
            if self.fail_rename {
                return Err(Self::failure());
            }
            let task = self
                .tasks
                .iter_mut()
                .find(|task| task.id == id)
                .expect("test task exists");
            task.rename(name, occurred_at);
            Ok(TaskOutcome::Renamed(task.clone()))
        }

        fn archive_task(
            &mut self,
            id: TaskId,
            occurred_at: DateTime<Utc>,
        ) -> Result<TaskOutcome, ApplicationError> {
            if self.fail_archive {
                return Err(Self::failure());
            }
            let task = self
                .tasks
                .iter_mut()
                .find(|task| task.id == id)
                .expect("test task exists");
            task.archive(occurred_at);
            let archived = task.clone();
            if let Some(start) = self.archive_activates {
                let other = self
                    .tasks
                    .iter()
                    .find(|task| task.id != id && !task.is_archived())
                    .expect("the test service needs another active task");
                self.tracking = TrackingState::Running {
                    worklog: ActiveWorklog::begin(WorklogId::generate(), other.id, start),
                };
            }
            Ok(TaskOutcome::Archived(archived))
        }

        fn unarchive_task(
            &mut self,
            id: TaskId,
            occurred_at: DateTime<Utc>,
        ) -> Result<TaskOutcome, ApplicationError> {
            if self.fail_unarchive {
                return Err(Self::failure());
            }
            let task = self
                .tasks
                .iter_mut()
                .find(|task| task.id == id)
                .expect("test task exists");
            task.restore(occurred_at);
            // Stands in for a second client that restored the task and
            // started tracking it before this unarchive ran.
            if let Some(start) = self.unarchive_activates {
                self.tracking = TrackingState::Running {
                    worklog: ActiveWorklog::begin(
                        WorklogId::from_uuid(uuid::Uuid::from_u128(20)),
                        task.id,
                        start,
                    ),
                };
            }
            Ok(TaskOutcome::Unarchived(task.clone()))
        }
    }

    impl TrackingOperations for TestService {
        fn current_tracking(&self) -> &TrackingState {
            &self.tracking
        }

        fn set_active_task(
            &mut self,
            task_id: TaskId,
            occurred_at: DateTime<Utc>,
        ) -> Result<SetActiveTaskOutcome, ApplicationError> {
            let started_at = self.set_timestamp.unwrap_or(occurred_at);
            if let Some((_, latest)) = self
                .latest_work_starts
                .iter_mut()
                .find(|(id, _)| *id == task_id)
            {
                *latest = (*latest).max(started_at);
            } else {
                self.latest_work_starts.push((task_id, started_at));
            }
            let worklog = Worklog::begin(
                WorklogId::from_uuid(uuid::Uuid::from_u128(99)),
                task_id,
                started_at,
            );
            let active = ActiveWorklog::begin(worklog.id, task_id, started_at);
            self.tracking = TrackingState::Running {
                worklog: active.clone(),
            };
            if self.set_returns_already_active {
                Ok(SetActiveTaskOutcome::AlreadyActive { worklog: active })
            } else {
                Ok(SetActiveTaskOutcome::Started { worklog })
            }
        }

        fn clear_active_task(
            &mut self,
            _expected_active: WorklogId,
            _occurred_at: DateTime<Utc>,
        ) -> Result<ClearActiveTaskOutcome, ApplicationError> {
            self.tracking = TrackingState::Idle;
            Ok(ClearActiveTaskOutcome::AlreadyIdle)
        }
    }

    impl WorklogQueries for TestService {
        fn worklogs_for_task(&self, _task_id: TaskId) -> Result<Vec<Worklog>, ApplicationError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn application_and_tui_recover_tracking_after_a_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tracker.db");
        let task = Task::create(TaskId::generate(), TaskName::new("alpha").unwrap(), at(100));
        {
            let repository = SqliteRepository::open(&path).unwrap();
            repository.create_task(task.clone()).unwrap();
            repository
                .insert_worklog(&Worklog::begin(
                    WorklogId::generate(),
                    task.id,
                    DateTime::from_timestamp(100, 0).unwrap(),
                ))
                .unwrap();
        }
        let app =
            App::load(TrackerApplication::load(SqliteRepository::open(&path).unwrap()).unwrap());
        assert_eq!(app.active_task_id(), Some(task.id));
        assert_eq!(
            app.status(),
            &Status::Info("Recovered the previous active timer".to_owned())
        );
    }

    #[test]
    fn fresh_and_empty_apps_have_safe_selection() {
        let app = app_with(&["one", "two"]);
        assert_eq!(app.selected(), Some(0));
        assert_eq!(app.status(), &Status::Info("Ready".to_owned()));
        let mut empty = app_with(&[]);
        empty.handle(Command::MoveDown);
        empty.handle(Command::MoveUp);
        assert_eq!(empty.selected(), None);
    }

    #[test]
    fn selection_movement_stays_inside_the_task_list() {
        let mut app = app_with(&["one", "two", "three"]);
        app.handle(Command::MoveUp);
        assert_eq!(app.selected(), Some(0));
        for _ in 0..5 {
            app.handle(Command::MoveDown);
        }
        assert_eq!(app.selected(), Some(2));
        app.handle(Command::MoveUp);
        assert_eq!(app.selected(), Some(1));
        app.active_selection = None;
        app.handle(Command::MoveUp);
        assert_eq!(app.selected(), Some(2));
    }

    #[test]
    fn adding_a_task_updates_the_list_and_selection() {
        let mut app = app_with(&["one"]);
        app.handle(Command::OpenAdd);
        for character in "new task".chars() {
            app.handle(Command::Insert(character));
        }
        app.handle(Command::Confirm);
        assert_eq!(app.mode(), &Mode::Normal);
        assert_eq!(app.selected(), Some(0));
        assert_eq!(
            app.tasks()[app.selected().unwrap()].name().as_str(),
            "new task"
        );
        assert_eq!(app.status(), &Status::Info("Added \"new task\"".to_owned()));
    }

    #[test]
    fn adding_in_the_middle_selects_the_new_task() {
        let task = |tag, name| {
            Task::create(
                TaskId::from_uuid(uuid::Uuid::from_u128(tag)),
                TaskName::new(name).unwrap(),
                at(100),
            )
        };
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "one"),
            task(3, "three"),
        ]));
        app.handle(Command::MoveDown);
        app.handle(Command::OpenAdd);
        app.handle(Command::Insert('t'));
        app.handle(Command::Confirm);
        assert_eq!(
            app.tasks().iter().map(|task| task.id).collect::<Vec<_>>(),
            vec![
                TaskId::from_uuid(uuid::Uuid::from_u128(2)),
                TaskId::from_uuid(uuid::Uuid::from_u128(1)),
                TaskId::from_uuid(uuid::Uuid::from_u128(3)),
            ]
        );
        assert_eq!(app.selected(), Some(0));
    }

    #[test]
    fn refreshing_tasks_preserves_the_selected_task_after_reordering() {
        let task = |tag, name| {
            Task::create(
                TaskId::from_uuid(uuid::Uuid::from_u128(tag)),
                TaskName::new(name).unwrap(),
                at(100),
            )
        };
        let selected = task(3, "three");
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "one"),
            selected.clone(),
        ]));
        app.handle(Command::MoveDown);
        app.application.tasks.insert(1, task(2, "two"));
        app.sync_tasks_from_application();
        assert_eq!(app.selected(), Some(2));
        assert_eq!(app.tasks()[2].id, selected.id);
    }

    #[test]
    fn failed_writes_keep_the_active_modal_and_input_buffer() {
        let task = Task::create(
            TaskId::from_uuid(uuid::Uuid::from_u128(1)),
            TaskName::new("one").unwrap(),
            at(100),
        );
        let mut service = TestService::with_tasks(vec![task]);
        service.fail_create = true;
        service.fail_rename = true;
        service.fail_archive = true;
        let mut app = App::load(service);

        app.handle(Command::OpenAdd);
        for character in "blocked".chars() {
            app.handle(Command::Insert(character));
        }
        app.handle(Command::Confirm);
        assert!(matches!(
            app.mode(),
            Mode::Input {
                purpose: InputPurpose::Add,
                buffer,
            } if buffer == "blocked"
        ));
        assert_eq!(
            app.status(),
            &Status::Error("Storage error: write failed".to_owned())
        );
        app.handle(Command::Cancel);

        app.handle(Command::OpenRename);
        app.handle(Command::Confirm);
        assert!(matches!(
            app.mode(),
            Mode::Input {
                purpose: InputPurpose::Rename { .. },
                buffer,
            } if buffer == "one"
        ));
        assert_eq!(
            app.status(),
            &Status::Error("Storage error: write failed".to_owned())
        );
        app.handle(Command::Cancel);

        app.handle(Command::OpenArchiveConfirm);
        app.handle(Command::Confirm);
        assert!(matches!(app.mode(), Mode::ConfirmArchive { .. }));
        assert_eq!(
            app.status(),
            &Status::Error("Storage error: write failed".to_owned())
        );
    }

    #[test]
    fn invalid_input_keeps_the_dialog_and_reports_the_same_text() {
        let mut app = app_with(&["one"]);
        app.handle(Command::OpenAdd);
        app.handle(Command::Confirm);
        assert!(matches!(app.mode(), Mode::Input { .. }));
        assert_eq!(
            app.status(),
            &Status::Error("The task name must not be empty".to_owned())
        );
    }

    #[test]
    fn renaming_a_task_keeps_its_row_selected() {
        let mut app = app_with(&["old"]);
        app.handle(Command::OpenRename);
        for _ in 0..3 {
            app.handle(Command::Backspace);
        }
        for character in "new".chars() {
            app.handle(Command::Insert(character));
        }
        app.handle(Command::Confirm);
        assert_eq!(app.tasks()[0].name().as_str(), "new");
        assert_eq!(app.selected(), Some(0));
        assert_eq!(app.status(), &Status::Info("Renamed to \"new\"".to_owned()));
    }

    #[test]
    fn archiving_hides_the_task_and_clamps_selection() {
        let mut app = app_with(&["alpha", "beta"]);
        app.handle(Command::MoveDown);
        app.handle(Command::OpenArchiveConfirm);
        app.handle(Command::Confirm);
        assert_eq!(app.tasks().len(), 1);
        assert_eq!(app.tasks()[0].name().as_str(), "alpha");
        assert_eq!(app.selected(), Some(0));
        assert_eq!(app.status(), &Status::Info("Archived \"beta\"".to_owned()));
    }

    #[test]
    fn a_successful_archive_copies_tracking_refreshed_by_the_application() {
        let alpha = task(1, "alpha");
        let beta = task(2, "beta");
        let mut app = App::load(TestService::with_tasks(vec![alpha.clone(), beta.clone()]));
        app.application.archive_activates = Some(at(200));

        app.handle(Command::OpenArchiveConfirm);
        app.handle(Command::Confirm);

        assert_eq!(app.active_task_id(), Some(beta.id));
        assert_eq!(app.active_task_name(), Some("beta"));
        assert_eq!(app.status(), &Status::Info("Archived \"alpha\"".to_owned()));
    }

    #[test]
    fn archiving_the_active_task_keeps_the_timer() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::ToggleTracking);
        let active = app.active_task_id();
        app.handle(Command::OpenArchiveConfirm);
        app.handle(Command::Confirm);
        assert_eq!(app.active_task_id(), active);
        assert_eq!(app.tasks().len(), 1);
        assert_eq!(
            app.status(),
            &Status::Error("The active task cannot be archived".to_owned())
        );
    }

    #[test]
    fn space_starts_stops_and_restarts_with_separate_worklogs() {
        let mut app = app_with(&["alpha"]);
        let task_id = app.tasks()[0].id;
        app.handle(Command::ToggleTracking);
        assert_eq!(app.active_task_id(), Some(task_id));
        assert_eq!(text(app.status()), "Started \"alpha\"");
        app.handle(Command::ToggleTracking);
        assert_eq!(app.active_task_id(), None);
        assert_eq!(text(app.status()), "Stopped \"alpha\"");
        app.handle(Command::ToggleTracking);
        let worklogs = app.application.worklogs_for_task(task_id).unwrap();
        assert_eq!(worklogs.len(), 2);
        assert_ne!(worklogs[0].id, worklogs[1].id);
    }

    #[test]
    fn space_switches_to_another_task() {
        let mut app = app_with(&["alpha", "beta"]);
        app.handle(Command::ToggleTracking);
        app.handle(Command::MoveDown);
        let beta = app.tasks()[1].id;
        app.handle(Command::ToggleTracking);
        assert_eq!(app.active_task_id(), Some(beta));
        assert_eq!(
            app.status(),
            &Status::Info("Switched to \"beta\"".to_owned())
        );
    }

    #[test]
    fn tracking_outcomes_choose_the_right_status_and_clock_anchor() {
        let task = |tag, name| {
            Task::create(
                TaskId::from_uuid(uuid::Uuid::from_u128(tag)),
                TaskName::new(name).unwrap(),
                at(100),
            )
        };
        let old = DateTime::from_timestamp(100, 0).unwrap();

        let mut started_service = TestService::with_tasks(vec![task(1, "alpha")]);
        started_service.set_timestamp = Some(old);
        let mut started = App::load(started_service);
        started.handle(Command::ToggleTracking);
        assert_eq!(
            started.status(),
            &Status::Info("Started \"alpha\"".to_owned())
        );
        assert!(started.elapsed().unwrap() < Duration::from_secs(1));

        let mut existing_service = TestService::with_tasks(vec![task(1, "alpha")]);
        existing_service.set_returns_already_active = true;
        existing_service.set_timestamp = Some(old);
        let mut existing = App::load(existing_service);
        existing.handle(Command::ToggleTracking);
        assert_eq!(
            existing.status(),
            &Status::Info("Started \"alpha\"".to_owned())
        );
        assert!(existing.elapsed().unwrap() > Duration::from_secs(60));

        let alpha = task(1, "alpha");
        let beta = task(2, "beta");
        let mut switched_service = TestService::with_tasks(vec![alpha.clone(), beta.clone()]);
        switched_service.tracking = TrackingState::Running {
            worklog: ActiveWorklog::begin(
                WorklogId::from_uuid(uuid::Uuid::from_u128(10)),
                alpha.id,
                old,
            ),
        };
        switched_service.set_returns_already_active = true;
        switched_service.set_timestamp = Some(old);
        let mut switched = App::load(switched_service);
        switched.handle(Command::MoveDown);
        switched.handle(Command::ToggleTracking);
        assert_eq!(
            switched.status(),
            &Status::Info("Switched to \"beta\"".to_owned())
        );
    }

    #[test]
    fn switching_uses_one_monotonic_timestamp_for_both_worklogs() {
        let mut app = app_with(&["alpha", "beta"]);
        let alpha = app.tasks()[0].id;
        let beta = app.tasks()[1].id;
        app.handle(Command::ToggleTracking);
        app.freeze_elapsed_for_tests(Duration::from_secs(125));
        app.handle(Command::MoveDown);
        app.handle(Command::ToggleTracking);
        let alpha_worklog = app
            .application
            .worklogs_for_task(alpha)
            .unwrap()
            .pop()
            .unwrap();
        let beta_worklog = app
            .application
            .worklogs_for_task(beta)
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(alpha_worklog.end, Some(beta_worklog.start));
    }

    #[test]
    fn a_backward_wall_clock_jump_persists_a_nonnegative_duration() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tracker.db");
        let task = Task::create(TaskId::generate(), TaskName::new("alpha").unwrap(), at(100));
        let start = Utc::now() + TimeDelta::hours(1);
        let repository = SqliteRepository::open(&path).unwrap();
        repository.create_task(task.clone()).unwrap();
        repository
            .insert_worklog(&Worklog::begin(WorklogId::generate(), task.id, start))
            .unwrap();
        let mut app = App::load(TrackerApplication::load(repository).unwrap());

        app.handle(Command::ToggleTracking);

        let stored = app
            .application
            .worklogs_for_task(task.id)
            .unwrap()
            .remove(0);
        let duration = (stored.end.unwrap() - stored.start).to_std().unwrap();
        assert!(duration < Duration::from_secs(60), "got {duration:?}");
        assert!(stored.end.unwrap() >= start);
    }

    #[test]
    fn a_forward_wall_clock_jump_persists_the_displayed_duration() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tracker.db");
        let task = Task::create(TaskId::generate(), TaskName::new("alpha").unwrap(), at(100));
        let start = Utc::now() - TimeDelta::hours(2);
        let repository = SqliteRepository::open(&path).unwrap();
        repository.create_task(task.clone()).unwrap();
        repository
            .insert_worklog(&Worklog::begin(WorklogId::generate(), task.id, start))
            .unwrap();
        let mut app = App::load(TrackerApplication::load(repository).unwrap());
        app.freeze_elapsed_for_tests(Duration::from_secs(5));
        let shown = app.elapsed().unwrap();

        app.handle(Command::ToggleTracking);

        let stored = app
            .application
            .worklogs_for_task(task.id)
            .unwrap()
            .remove(0);
        let duration = (stored.end.unwrap() - stored.start).to_std().unwrap();
        assert!(
            duration >= shown,
            "persisted {duration:?} < shown {shown:?}"
        );
        assert!(
            duration - shown < Duration::from_secs(2),
            "persisted {duration:?} must match shown {shown:?}"
        );
        assert!(
            duration < Duration::from_secs(60),
            "the two-hour wall-clock gap leaked into the worklog: {duration:?}"
        );
    }

    #[test]
    fn quitting_in_a_dialog_does_not_stop_tracking() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::ToggleTracking);
        app.handle(Command::OpenAdd);
        app.handle(Command::Quit);
        assert!(!app.is_running());
        assert!(app.active_task_id().is_some());
    }

    #[test]
    fn quitting_from_every_mode_leaves_tracking_active() {
        for command in [
            None,
            Some(Command::OpenAdd),
            Some(Command::OpenRename),
            Some(Command::OpenArchiveConfirm),
        ] {
            let task = Task::create(
                TaskId::from_uuid(uuid::Uuid::from_u128(1)),
                TaskName::new("one").unwrap(),
                at(100),
            );
            let mut service = TestService::with_tasks(vec![task.clone()]);
            service.tracking = TrackingState::Running {
                worklog: ActiveWorklog::begin(
                    WorklogId::from_uuid(uuid::Uuid::from_u128(10)),
                    task.id,
                    DateTime::from_timestamp(100, 0).unwrap(),
                ),
            };
            let mut app = App::load(service);
            if let Some(command) = command {
                app.handle(command);
            }
            app.handle(Command::Quit);
            assert!(!app.is_running());
            assert_eq!(app.active_task_id(), Some(task.id));
        }
    }

    #[test]
    fn input_is_bounded_to_the_domain_limit() {
        let mut app = app_with(&[]);
        app.handle(Command::OpenAdd);
        for character in "x".repeat(TaskName::MAX_LEN + 10).chars() {
            app.handle(Command::Insert(character));
        }
        app.handle(Command::Confirm);
        assert_eq!(
            app.tasks()[0].name().as_str().chars().count(),
            TaskName::MAX_LEN
        );
    }

    #[test]
    fn elapsed_clock_and_client_timestamp_share_one_duration() {
        let clock = ElapsedClock::anchored(Duration::from_secs(100));
        assert_eq!(clock.at(Duration::from_secs(5)), Duration::from_secs(105));
        let start = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            tracking_timestamp(start, Duration::from_secs(5)),
            start + TimeDelta::seconds(5)
        );
    }

    #[test]
    fn future_clock_anchors_at_zero_and_timestamp_overflow_saturates() {
        let now = Utc::now();
        assert_eq!(
            ElapsedClock::base_since(now + TimeDelta::seconds(1), now),
            Duration::ZERO
        );
        assert_eq!(
            ElapsedClock::base_since(now - TimeDelta::seconds(5), now),
            Duration::from_secs(5)
        );
        assert_eq!(
            tracking_timestamp(DateTime::<Utc>::MAX_UTC, Duration::MAX),
            DateTime::<Utc>::MAX_UTC
        );
    }

    #[test]
    fn cancel_commands_preserve_expected_state() {
        let mut app = app_with(&["one"]);
        app.handle(Command::OpenAdd);
        app.handle(Command::Insert('x'));
        app.handle(Command::Cancel);
        assert_eq!(app.mode(), &Mode::Normal);
        assert_eq!(app.tasks().len(), 1);
    }

    fn task(tag: u128, name: &str) -> Task {
        Task::create(
            TaskId::from_uuid(uuid::Uuid::from_u128(tag)),
            TaskName::new(name).unwrap(),
            at(100),
        )
    }

    fn archived_task(tag: u128, name: &str) -> Task {
        let mut task = task(tag, name);
        assert!(task.archive(at(100)));
        task
    }

    fn stamped_task(
        tag: u128,
        name: &str,
        archived: bool,
        created_at: i64,
        updated_at: i64,
    ) -> Task {
        Task::rehydrate(
            TaskId::from_uuid(uuid::Uuid::from_u128(tag)),
            TaskName::new(name).unwrap(),
            archived,
            at(created_at),
            at(updated_at),
        )
        .unwrap()
    }

    #[test]
    fn ordering_defaults_cycles_and_is_shared_by_both_views() {
        let tasks = vec![
            stamped_task(1, "older active", false, 100, 900),
            stamped_task(2, "newer active", false, 800, 800),
            stamped_task(3, "older archived", true, 200, 700),
            stamped_task(4, "newer archived", true, 600, 600),
        ];
        let mut service = TestService::with_tasks(tasks);
        service.latest_work_starts = vec![(TaskId::from_uuid(uuid::Uuid::from_u128(3)), at(1_000))];
        let mut app = App::load(service);

        assert_eq!(app.ordering(), TaskOrdering::RecentlyWorked);
        assert_eq!(app.tasks()[0].name().as_str(), "newer active");
        app.handle(Command::ShowArchivedTasks);
        assert_eq!(app.tasks()[0].name().as_str(), "older archived");
        app.handle(Command::ShowActiveTasks);
        app.handle(Command::CycleOrdering);
        assert_eq!(app.ordering(), TaskOrdering::RecentlyUpdated);
        assert_eq!(app.tasks()[0].name().as_str(), "older active");

        app.handle(Command::ShowArchivedTasks);
        assert_eq!(app.ordering(), TaskOrdering::RecentlyUpdated);
        assert_eq!(app.tasks()[0].name().as_str(), "older archived");
        app.handle(Command::CycleOrdering);
        assert_eq!(app.ordering(), TaskOrdering::RecentlyCreated);
        assert_eq!(app.tasks()[0].name().as_str(), "newer archived");
        app.handle(Command::CycleOrdering);
        assert_eq!(app.ordering(), TaskOrdering::RecentlyWorked);
    }

    #[test]
    fn sorting_preserves_each_views_selected_task_id() {
        let active = stamped_task(1, "active selection", false, 100, 900);
        let archived = stamped_task(3, "archived selection", true, 100, 900);
        let mut app = App::load(TestService::with_tasks(vec![
            active.clone(),
            stamped_task(2, "new active", false, 800, 800),
            archived.clone(),
            stamped_task(4, "new archived", true, 800, 800),
        ]));

        app.handle(Command::MoveDown);
        assert_eq!(app.tasks()[app.selected().unwrap()].id, active.id);
        app.handle(Command::ShowArchivedTasks);
        app.handle(Command::MoveDown);
        assert_eq!(app.tasks()[app.selected().unwrap()].id, archived.id);

        app.handle(Command::CycleOrdering);
        assert_eq!(app.tasks()[app.selected().unwrap()].id, archived.id);
        app.handle(Command::ShowActiveTasks);
        assert_eq!(app.tasks()[app.selected().unwrap()].id, active.id);
    }

    #[test]
    fn input_and_confirmation_modes_block_sorting() {
        let mut app = App::load(TestService::with_tasks(vec![task(1, "alpha")]));
        app.handle(Command::OpenAdd);
        app.handle(Command::CycleOrdering);
        assert_eq!(app.ordering(), TaskOrdering::RecentlyWorked);
        assert!(matches!(app.mode(), Mode::Input { .. }));

        app.handle(Command::Cancel);
        app.handle(Command::OpenArchiveConfirm);
        app.handle(Command::CycleOrdering);
        assert_eq!(app.ordering(), TaskOrdering::RecentlyWorked);
        assert!(matches!(app.mode(), Mode::ConfirmArchive { .. }));
    }

    #[test]
    fn start_switch_and_stop_keep_selection_on_the_operated_task() {
        let mut app = app_with(&["alpha", "beta"]);
        let alpha = app
            .tasks()
            .iter()
            .find(|task| task.name().as_str() == "alpha")
            .unwrap()
            .id;
        let beta = app
            .tasks()
            .iter()
            .find(|task| task.name().as_str() == "beta")
            .unwrap()
            .id;

        app.handle(Command::MoveDown);
        app.handle(Command::ToggleTracking);
        assert_eq!(app.tasks()[0].id, beta);
        assert_eq!(app.tasks()[app.selected().unwrap()].id, beta);

        app.handle(Command::MoveDown);
        app.handle(Command::ToggleTracking);
        assert_eq!(app.tasks()[0].id, alpha);
        assert_eq!(app.tasks()[app.selected().unwrap()].id, alpha);

        app.handle(Command::ToggleTracking);
        assert_eq!(app.tasks()[0].id, alpha, "stopping does not reorder");
        assert_eq!(app.tasks()[app.selected().unwrap()].id, alpha);
    }

    #[test]
    fn rename_reorders_recently_updated_and_keeps_the_task_selected() {
        let alpha = task(1, "alpha");
        let beta = task(2, "beta");
        let mut app = App::load(TestService::with_tasks(vec![alpha, beta.clone()]));
        app.handle(Command::CycleOrdering);
        app.handle(Command::MoveDown);
        app.handle(Command::OpenRename);
        app.handle(Command::Backspace);
        app.handle(Command::Insert('x'));
        app.handle(Command::Confirm);

        assert_eq!(app.ordering(), TaskOrdering::RecentlyUpdated);
        assert_eq!(app.tasks()[0].id, beta.id);
        assert_eq!(app.tasks()[app.selected().unwrap()].id, beta.id);
    }

    #[test]
    fn the_app_starts_in_the_active_view_and_switches_directionally() {
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "alpha"),
            archived_task(3, "gone"),
        ]));
        assert_eq!(app.view(), TaskView::Active);
        assert_eq!(app.tasks().len(), 1, "the archived task is not visible");

        app.handle(Command::ShowArchivedTasks);
        assert_eq!(app.view(), TaskView::Archived);
        assert_eq!(app.tasks().len(), 1);

        // l on the archived view is idempotent.
        app.handle(Command::ShowArchivedTasks);
        assert_eq!(app.view(), TaskView::Archived);

        app.handle(Command::ShowActiveTasks);
        assert_eq!(app.view(), TaskView::Active);

        // h on the active view is idempotent.
        app.handle(Command::ShowActiveTasks);
        assert_eq!(app.view(), TaskView::Active);
    }

    #[test]
    fn a_same_view_switch_does_not_select_for_a_view_without_a_memory() {
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "alpha"),
            task(2, "beta"),
            archived_task(3, "gone"),
        ]));
        // No remembered selection: the view's own command is a no-op and
        // must not invent one from the first row.
        app.active_selection = None;
        app.handle(Command::ShowActiveTasks);
        assert_eq!(app.selected(), None);

        app.handle(Command::ShowArchivedTasks);
        assert_eq!(app.selected(), Some(0), "a fresh view starts on row one");
        app.archived_selection = None;
        app.handle(Command::ShowArchivedTasks);
        assert_eq!(app.selected(), None);
    }

    #[test]
    fn each_view_remembers_its_selection_across_switches() {
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "alpha"),
            task(2, "beta"),
            archived_task(3, "gone"),
        ]));
        app.handle(Command::MoveDown);
        app.handle(Command::ShowArchivedTasks);
        assert_eq!(
            app.selected(),
            Some(0),
            "a fresh view starts on its first row"
        );

        app.handle(Command::ShowActiveTasks);
        assert_eq!(app.selected(), Some(1), "the active selection came back");
        assert_eq!(
            app.tasks()[1].id,
            TaskId::from_uuid(uuid::Uuid::from_u128(2))
        );

        app.handle(Command::ShowArchivedTasks);
        assert_eq!(app.selected(), Some(0), "the archived selection came back");
    }

    #[test]
    fn archived_movement_stays_inside_the_archived_list() {
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "alpha"),
            archived_task(3, "gone"),
            archived_task(4, "also gone"),
        ]));
        app.handle(Command::ShowArchivedTasks);
        assert_eq!(app.selected(), Some(0));
        app.handle(Command::MoveDown);
        assert_eq!(app.selected(), Some(1));
        app.handle(Command::MoveDown);
        assert_eq!(app.selected(), Some(1));
        app.handle(Command::MoveUp);
        assert_eq!(app.selected(), Some(0));
    }

    #[test]
    fn an_empty_view_has_no_selection() {
        let mut app = App::load(TestService::with_tasks(vec![task(1, "alpha")]));
        app.handle(Command::ShowArchivedTasks);
        assert_eq!(app.selected(), None);
        app.handle(Command::MoveDown);
        app.handle(Command::MoveUp);
        assert_eq!(app.selected(), None);
    }

    #[test]
    fn the_archived_view_refuses_active_actions_in_command_handling() {
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "alpha"),
            archived_task(3, "gone"),
        ]));
        app.handle(Command::ShowArchivedTasks);

        app.handle(Command::ToggleTracking);
        app.handle(Command::OpenAdd);
        app.handle(Command::OpenRename);
        app.handle(Command::OpenArchiveConfirm);

        assert_eq!(app.mode(), &Mode::Normal);
        assert_eq!(app.view(), TaskView::Archived);
        assert_eq!(app.active_task_id(), None, "no tracking was started");

        // Movement still works after the refused actions.
        app.handle(Command::ShowActiveTasks);
        app.handle(Command::ShowArchivedTasks);
        app.handle(Command::MoveDown);
        app.handle(Command::MoveUp);
        assert_eq!(app.selected(), Some(0));
    }

    #[test]
    fn the_active_view_refuses_unarchiving() {
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "alpha"),
            archived_task(3, "gone"),
        ]));
        app.handle(Command::UnarchiveSelected);

        assert_eq!(app.view(), TaskView::Active);
        assert_eq!(app.status(), &Status::Info("Ready".to_owned()));
        assert_eq!(app.tasks().len(), 1, "nothing was unarchived");
    }

    #[test]
    fn a_modal_in_the_archived_view_refuses_unarchiving() {
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "alpha"),
            archived_task(3, "gone"),
        ]));
        app.handle(Command::ShowArchivedTasks);
        // No command path reaches a modal on the archived view; the guard
        // must still refuse one for whenever the key map changes.
        app.mode = Mode::Input {
            purpose: InputPurpose::Add,
            buffer: String::new(),
        };
        app.handle(Command::UnarchiveSelected);

        assert!(matches!(app.mode(), Mode::Input { .. }));
        assert_eq!(app.view(), TaskView::Archived);
        assert_eq!(app.tasks().len(), 1, "nothing was unarchived");
        assert_eq!(app.status(), &Status::Info("Ready".to_owned()));
    }

    #[test]
    fn modals_block_view_switching_and_unarchiving() {
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "alpha"),
            archived_task(3, "gone"),
        ]));
        app.handle(Command::OpenAdd);
        app.handle(Command::Insert('x'));

        // Each switch is asserted immediately, so a guard that only blocks
        // one of the two commands cannot cancel the other out.
        app.handle(Command::ShowArchivedTasks);
        assert_eq!(app.view(), TaskView::Active, "the modal blocks the switch");

        app.handle(Command::ShowActiveTasks);
        assert_eq!(app.view(), TaskView::Active);

        app.handle(Command::UnarchiveSelected);
        assert!(matches!(app.mode(), Mode::Input { .. }));
        assert_eq!(app.view(), TaskView::Active);
        assert!(matches!(app.mode(), Mode::Input { buffer, .. } if buffer == "x"));

        app.handle(Command::Cancel);
        app.handle(Command::OpenArchiveConfirm);
        app.handle(Command::UnarchiveSelected);
        assert!(matches!(app.mode(), Mode::ConfirmArchive { .. }));
        assert_eq!(app.view(), TaskView::Active);
    }

    #[test]
    fn archiving_remembers_the_task_in_the_archived_view_and_clamps() {
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "alpha"),
            task(2, "beta"),
            task(3, "gamma"),
        ]));
        app.handle(Command::MoveDown);
        app.handle(Command::OpenArchiveConfirm);
        app.handle(Command::Confirm);

        assert_eq!(app.view(), TaskView::Active);
        assert_eq!(app.selected(), Some(1), "the selection clamped to gamma");

        app.handle(Command::ShowArchivedTasks);
        assert_eq!(app.selected(), Some(0));
        assert_eq!(
            app.tasks()[0].id,
            TaskId::from_uuid(uuid::Uuid::from_u128(2))
        );
    }

    #[test]
    fn archiving_the_last_row_clamps_to_the_previous_row() {
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "alpha"),
            task(2, "beta"),
        ]));
        app.handle(Command::MoveDown);
        app.handle(Command::OpenArchiveConfirm);
        app.handle(Command::Confirm);
        assert_eq!(app.tasks().len(), 1);
        assert_eq!(app.selected(), Some(0));
        assert_eq!(
            app.tasks()[0].id,
            TaskId::from_uuid(uuid::Uuid::from_u128(1))
        );
    }

    #[test]
    fn unarchiving_stays_in_the_archived_view_and_reports_restored() {
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "alpha"),
            task(2, "beta"),
            archived_task(3, "gone"),
        ]));
        app.handle(Command::ShowArchivedTasks);
        app.handle(Command::UnarchiveSelected);

        assert_eq!(app.view(), TaskView::Archived);
        assert_eq!(app.status(), &Status::Info("Restored \"gone\"".to_owned()));
        assert_eq!(app.tasks().len(), 0, "the restored task left the list");
        assert_eq!(app.selected(), None, "the archived list is empty now");

        app.handle(Command::ShowActiveTasks);
        assert_eq!(app.tasks().len(), 3);
        assert_eq!(app.selected(), Some(2), "the restored task is selected");
        assert_eq!(
            app.tasks()[2].id,
            TaskId::from_uuid(uuid::Uuid::from_u128(3))
        );
    }

    #[test]
    fn unarchiving_clamps_the_archived_selection_to_the_nearest_row() {
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "alpha"),
            archived_task(3, "gone"),
            archived_task(4, "also gone"),
        ]));
        app.handle(Command::ShowArchivedTasks);
        app.handle(Command::MoveDown);
        app.handle(Command::UnarchiveSelected);

        assert_eq!(app.view(), TaskView::Archived);
        assert_eq!(app.tasks().len(), 1);
        assert_eq!(app.selected(), Some(0));
        assert_eq!(
            app.tasks()[0].id,
            TaskId::from_uuid(uuid::Uuid::from_u128(3))
        );
    }

    #[test]
    fn a_failed_unarchive_keeps_the_view_selection_and_reports_the_error() {
        let mut service = TestService::with_tasks(vec![task(1, "alpha"), archived_task(3, "gone")]);
        service.fail_unarchive = true;
        let mut app = App::load(service);
        // Another client's activity reaches the service after the app loaded;
        // the failed write must still resynchronize the TUI's tracking state.
        app.application.tracking = TrackingState::Running {
            worklog: ActiveWorklog::begin(
                WorklogId::from_uuid(uuid::Uuid::from_u128(20)),
                TaskId::from_uuid(uuid::Uuid::from_u128(3)),
                DateTime::from_timestamp(100, 0).unwrap(),
            ),
        };
        app.handle(Command::ShowArchivedTasks);
        app.handle(Command::UnarchiveSelected);

        assert_eq!(app.view(), TaskView::Archived);
        assert_eq!(app.selected(), Some(0));
        assert_eq!(
            app.tasks()[0].id,
            TaskId::from_uuid(uuid::Uuid::from_u128(3))
        );
        assert_eq!(
            app.active_task_id(),
            Some(TaskId::from_uuid(uuid::Uuid::from_u128(3))),
            "the failed unarchive still refreshes tracking state"
        );
        assert_eq!(
            app.status(),
            &Status::Error("Storage error: write failed".to_owned())
        );
    }

    #[test]
    fn unarchiving_refreshes_a_timer_another_client_started() {
        let mut service = TestService::with_tasks(vec![task(1, "alpha"), archived_task(3, "gone")]);
        service.unarchive_activates = Some(DateTime::from_timestamp(100, 0).unwrap());
        let mut app = App::load(service);
        app.handle(Command::ShowArchivedTasks);
        app.handle(Command::UnarchiveSelected);

        assert_eq!(app.view(), TaskView::Archived);
        assert_eq!(
            app.active_task_id(),
            Some(TaskId::from_uuid(uuid::Uuid::from_u128(3))),
            "the concurrent worklog reached the TUI's tracking state"
        );
        assert_eq!(app.active_task_name(), Some("gone"));
        assert!(
            app.elapsed().unwrap() > Duration::from_secs(60),
            "the clock anchored to the concurrent worklog's start"
        );
        assert_eq!(app.status(), &Status::Info("Restored \"gone\"".to_owned()));

        assert_eq!(app.tasks().len(), 0, "the restored task left the list");
        app.handle(Command::ShowActiveTasks);
        assert_eq!(app.selected(), Some(1), "the restored task is selected");
        assert_eq!(
            app.tasks()[1].id,
            TaskId::from_uuid(uuid::Uuid::from_u128(3))
        );
    }

    #[test]
    fn the_timer_header_resolves_the_active_task_outside_the_visible_list() {
        let mut app = App::load(TestService::with_tasks(vec![archived_task(3, "gone")]));
        let alpha = task(1, "alpha");
        app.application.tasks.push(alpha.clone());
        app.tracking = TrackingState::Running {
            worklog: ActiveWorklog::begin(
                WorklogId::from_uuid(uuid::Uuid::from_u128(10)),
                alpha.id,
                DateTime::from_timestamp(100, 0).unwrap(),
            ),
        };
        app.handle(Command::ShowArchivedTasks);

        assert_eq!(app.view(), TaskView::Archived);
        assert_eq!(app.tasks().len(), 1, "only archived tasks are listed");
        assert_eq!(app.active_task_name(), Some("alpha"));
    }
}
