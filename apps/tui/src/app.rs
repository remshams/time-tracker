//! Global TUI presentation controller.

mod shell_state;
mod task_catalog;
mod tracking_session;
mod view;

use std::{
    collections::VecDeque,
    ops::{Deref, DerefMut},
};

use chrono_tz::Tz;
use crossterm::event::KeyEvent;
use tracker_application::{TaskListItem, TaskOrdering, TrackerApplicationService};
use tracker_domain::TrackingState;

#[cfg(test)]
use crate::application_request::execute_local;
use crate::application_request::{ApplicationRequest, CompletedRequest};
use crate::command::Command;
use crate::screens::Screen;
use crate::screens::task_list::TaskListState;
use crate::support::timestamps::startup_timezone;

use self::shell_state::ShellState;
use self::task_catalog::TaskCatalog;
#[cfg(test)]
pub(crate) use self::tracking_session::TestClock;
use self::tracking_session::TrackingSession;
pub(crate) use self::view::AppView;

/// The most recent message shown in the status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Empty,
    Info(String),
    Error(String),
}

/// Coordinates service calls and the presentation state owners.
pub(crate) struct App<S: TrackerApplicationService> {
    application: S,
    state: AppState,
}

/// Presentation state that can be updated without borrowing the application service.
pub(crate) struct AppState {
    cached_items: Vec<TaskListItem>,
    catalog: TaskCatalog,
    tracking: TrackingSession,
    shell: ShellState,
    pending_effects: VecDeque<AppEffect>,
    active_request: Option<ApplicationRequest>,
}

/// A request paired with the state update to apply after it finishes.
pub(crate) struct AppEffect {
    pub(crate) request: ApplicationRequest,
    completion: EffectCompletion,
}

type EffectCompletion = Box<dyn FnOnce(&mut AppState, CompletedRequest)>;
const MAX_PENDING_EFFECTS: usize = 32;

impl<S: TrackerApplicationService> App<S> {
    /// Builds presentation state from an already loaded application service.
    pub fn load(application: S) -> Self {
        let state = AppState::load_from_snapshot(
            application.tasks(TaskOrdering::default()),
            application.current_tracking().clone(),
        );
        Self { application, state }
    }

    #[cfg(test)]
    fn load_with_timezone(application: S, timezone: Tz, timezone_status: Option<&str>) -> Self {
        let items = application.tasks(TaskOrdering::default());
        let state = AppState::assemble(
            items,
            TrackingSession::new(application.current_tracking().clone()),
            timezone,
            timezone_status,
        );
        Self { application, state }
    }

    pub(crate) fn into_parts(self) -> (S, AppState) {
        (self.application, self.state)
    }

    #[cfg(test)]
    pub(crate) fn load_in_timezone(application: S, timezone: Tz) -> Self {
        Self::load_with_timezone(application, timezone, None)
    }

    #[cfg(test)]
    pub(crate) fn load_with_test_clock(
        application: S,
        timezone: Tz,
        wall_clock: chrono::DateTime<chrono::Utc>,
    ) -> (Self, TestClock) {
        let items = application.tasks(TaskOrdering::default());
        let (tracking, clock) =
            TrackingSession::with_test_clock(application.current_tracking().clone(), wall_clock);
        (
            Self {
                application,
                state: AppState::assemble(items, tracking, timezone, None),
            },
            clock,
        )
    }

    #[cfg(test)]
    pub(crate) fn handle(&mut self, command: Command) {
        self.state.handle_command(command);
        self.drain_effects();
    }

    #[cfg(test)]
    fn drain_effects(&mut self) {
        for _ in 0..32 {
            let Some(effect) = self.state.take_effect() else {
                return;
            };
            let completed = execute_local(&mut self.application, effect.request.clone());
            self.state.complete_effect(effect, completed);
        }
        panic!("application effects did not settle after 32 completions");
    }

    #[cfg(test)]
    pub(crate) fn refresh_reports(&mut self) {
        self.state.refresh_reports();
        self.drain_effects();
    }

    #[cfg(test)]
    pub(crate) fn refresh_reports_now(&mut self) {
        self.state.refresh_reports_now();
        self.drain_effects();
    }

    #[cfg(test)]
    pub(crate) fn refresh_reports_if_needed(&mut self, now: chrono::DateTime<chrono::Utc>) {
        self.state.refresh_reports_if_needed(now);
        self.drain_effects();
    }

    #[cfg(test)]
    pub(crate) fn tick_reports_at(&mut self, now: chrono::DateTime<chrono::Utc>) {
        self.state.tick_reports_at(now);
        self.drain_effects();
    }

    #[cfg(test)]
    pub fn is_running(&self) -> bool {
        self.state.is_running()
    }

    #[cfg(test)]
    pub(crate) fn app_view(&self) -> AppView<'_> {
        self.state.app_view()
    }

    #[cfg(test)]
    pub(crate) fn command_for(&self, key: KeyEvent) -> Option<Command> {
        self.state.command_for(key)
    }

    #[cfg(test)]
    pub(crate) fn application_mut(&mut self) -> &mut S {
        &mut self.application
    }
}

impl<S: TrackerApplicationService> Deref for App<S> {
    type Target = AppState;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

impl<S: TrackerApplicationService> DerefMut for App<S> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.state
    }
}

impl AppState {
    pub(crate) fn handle_command(&mut self, command: Command) {
        match (self.shell.screen(), command) {
            (_, Command::Quit) => {
                self.shell.quit();
                self.discard_queued_reads();
            }
            (Screen::TaskList, Command::TaskList(command)) => {
                self.handle_task_list_command(command);
            }
            (Screen::WorklogHistory, Command::WorklogHistory(command)) => {
                self.handle_worklog_history_command(command);
            }
            (Screen::Reports, Command::Reports(command)) => self.handle_report_command(command),
            (Screen::AllWorklogs, Command::AllWorklogs(command)) => {
                self.handle_all_worklogs_command(command)
            }
            _ => {}
        }
    }

    pub(crate) fn load_from_snapshot(items: Vec<TaskListItem>, tracking: TrackingState) -> Self {
        let (timezone, timezone_status) = startup_timezone();
        Self::assemble(
            items,
            TrackingSession::new(tracking),
            timezone,
            timezone_status,
        )
    }

    fn assemble(
        items: Vec<TaskListItem>,
        tracking: TrackingSession,
        timezone: Tz,
        timezone_status: Option<&str>,
    ) -> Self {
        let mut sorted_items = items.clone();
        TaskOrdering::default().sort_items(&mut sorted_items);
        let catalog = TaskCatalog::new(sorted_items);
        let task_list = TaskListState::new(
            catalog
                .tasks(crate::screens::TaskView::Active)
                .first()
                .map(|task| task.id()),
        );
        let status = match timezone_status {
            Some(message) => Status::Error(message.to_owned()),
            None if tracking.elapsed().is_some() => {
                Status::Info("Recovered the previous active timer".to_owned())
            }
            None => Status::Empty,
        };
        Self {
            cached_items: items,
            catalog,
            tracking,
            shell: ShellState::new(status, task_list, timezone),
            pending_effects: VecDeque::new(),
            active_request: None,
        }
    }

    pub(crate) fn enqueue(
        &mut self,
        request: ApplicationRequest,
        completion: impl FnOnce(&mut AppState, CompletedRequest) + 'static,
    ) -> bool {
        if !self.shell.is_running() && !request.is_write() {
            return false;
        }
        if request.is_write()
            && (self
                .active_request
                .as_ref()
                .is_some_and(|active| active.same_write_intent(&request))
                || self
                    .pending_effects
                    .iter()
                    .any(|effect| effect.request.same_write_intent(&request)))
        {
            return false;
        }
        if matches!(request, ApplicationRequest::ReportTotals { .. }) {
            self.pending_effects.retain(|effect| {
                !matches!(effect.request, ApplicationRequest::ReportTotals { .. })
            });
        }
        if self.pending_effects.len() >= MAX_PENDING_EFFECTS {
            self.shell.error("Too many pending requests");
            return false;
        }
        self.pending_effects.push_back(AppEffect {
            request,
            completion: Box::new(completion),
        });
        true
    }

    pub(crate) fn take_effect(&mut self) -> Option<AppEffect> {
        if self.active_request.is_some() {
            return None;
        }
        let effect = self.pending_effects.pop_front()?;
        self.active_request = Some(effect.request.clone());
        Some(effect)
    }

    pub(crate) fn complete_effect(&mut self, effect: AppEffect, completed: CompletedRequest) {
        debug_assert!(self.active_request.is_some());
        debug_assert_eq!(effect.request, completed.request);
        self.active_request = None;
        (effect.completion)(self, completed);
    }

    #[cfg(test)]
    pub(crate) fn has_pending_effect(&self) -> bool {
        !self.pending_effects.is_empty()
    }

    pub(crate) fn has_queued_write(&self) -> bool {
        self.pending_effects
            .iter()
            .any(|effect| effect.request.is_write())
    }

    pub(crate) fn active_request_is_write(&self) -> bool {
        self.active_request
            .as_ref()
            .is_some_and(ApplicationRequest::is_write)
    }

    pub(crate) fn discard_queued_reads(&mut self) {
        self.pending_effects
            .retain(|effect| effect.request.is_write());
    }

    #[cfg(test)]
    pub(crate) fn request_in_flight(&self) -> bool {
        self.active_request.is_some()
    }

    pub fn is_running(&self) -> bool {
        self.shell.is_running()
    }

    pub(crate) fn expire_copy_confirmation(&mut self) {
        self.shell.expire_copy_confirmation();
    }

    pub(crate) fn app_view(&self) -> AppView<'_> {
        AppView::new(&self.catalog, &self.tracking, &self.shell)
    }

    pub(crate) fn command_for(&self, key: KeyEvent) -> Option<Command> {
        crate::screens::map_key(self.shell.input_state(), key)
    }

    pub(crate) fn catalog(&self) -> &TaskCatalog {
        &self.catalog
    }

    pub(crate) fn catalog_mut(&mut self) -> &mut TaskCatalog {
        &mut self.catalog
    }

    pub(crate) fn tracking(&self) -> &TrackingSession {
        &self.tracking
    }

    pub(crate) fn tracking_mut(&mut self) -> &mut TrackingSession {
        &mut self.tracking
    }

    pub(crate) fn shell(&self) -> &ShellState {
        &self.shell
    }

    pub(crate) fn shell_mut(&mut self) -> &mut ShellState {
        &mut self.shell
    }

    pub(crate) fn replace_items(&mut self, items: Vec<TaskListItem>) {
        self.cached_items = items;
        self.reload_tasks();
    }

    pub(crate) fn reload_tasks(&mut self) {
        let view = self.shell.task_list().view();
        let selected = self.shell.task_list().selection();
        let mut items = self.cached_items.clone();
        self.catalog.ordering().sort_items(&mut items);
        let resolved = self.catalog.reload(items, view, selected);
        let query = self.shell.task_list().search_query();
        let visible = self.catalog.visible_tasks(view, query);
        let resolved = resolved
            .filter(|id| visible.iter().any(|task| task.id() == *id))
            .or_else(|| visible.first().map(|task| task.id()));
        self.shell.task_list_mut().set_selection(resolved);
    }

    pub(crate) fn sync_from_snapshot(
        &mut self,
        items: Vec<TaskListItem>,
        tracking: TrackingState,
        fresh_active: bool,
    ) {
        self.replace_items(items);
        self.tracking.sync(tracking, fresh_active);
    }
}

#[cfg(test)]
mod effect_tests {
    use std::{cell::RefCell, rc::Rc};

    use chrono::{DateTime, TimeDelta, Utc};
    use tracker_application::{
        ApplicationError, ApplicationFailureCategory, ClearActiveTaskOutcome, SetActiveTaskOutcome,
        TaskOperations, TaskOrdering, TaskQueries, TrackerApplication, TrackingOperations,
        WorklogOperations, WorklogQueries,
    };
    use tracker_domain::{TaskName, TrackingState, WorklogTimes};
    use tracker_storage::SqliteRepository;

    use crate::application_request::{
        ApplicationOutcome, ApplicationRequest, ApplicationSnapshot, CompletedRequest,
        execute_local,
    };
    use crate::command::Command;

    use super::{AppState, MAX_PENDING_EFFECTS};

    #[test]
    fn reads_queue_during_a_write_and_follow_ups_can_be_queued_after_completion() {
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
        let request = ApplicationRequest::CreateTask {
            name: TaskName::new("New task").unwrap(),
            occurred_at: Utc::now(),
        };
        assert!(state.enqueue(request.clone(), |state, _| {
            assert!(!state.request_in_flight());
            assert!(state.enqueue(ApplicationRequest::AllWorklogs { after: None }, |_, _| {}));
        }));
        assert!(!state.enqueue(request.clone(), |_, _| {}));
        assert!(state.has_pending_effect());

        let effect = state.take_effect().expect("queued request");
        assert!(state.request_in_flight());
        assert!(!state.has_pending_effect());
        assert!(!state.enqueue(request.clone(), |_, _| {}));
        assert!(state.enqueue(ApplicationRequest::AllWorklogs { after: None }, |_, _| {}));
        assert!(state.take_effect().is_none());

        state.complete_effect(
            effect,
            CompletedRequest {
                request,
                outcome: ApplicationOutcome::Task(Err(ApplicationError::InvalidReportRange)),
                snapshot: ApplicationSnapshot {
                    items: Vec::new(),
                    tracking: TrackingState::Idle,
                },
            },
        );
        assert!(state.has_pending_effect());
        assert!(!state.request_in_flight());
    }

    #[test]
    fn pending_report_reads_keep_only_the_latest_request() {
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
        let active = ApplicationRequest::AllWorklogs { after: None };
        assert!(state.enqueue(active, |_, _| {}));
        let _active_effect = state.take_effect().expect("active read");
        let now = Utc::now();
        for offset in 0..3 {
            assert!(state.enqueue(
                ApplicationRequest::ReportTotals {
                    start: now,
                    end: now + chrono::Duration::hours(1),
                    now: now + chrono::Duration::seconds(offset),
                },
                |_, _| {},
            ));
        }
        assert_eq!(state.pending_effects.len(), 1);
        let ApplicationRequest::ReportTotals { now: latest, .. } =
            &state.pending_effects.front().unwrap().request
        else {
            panic!("expected report request");
        };
        assert_eq!(*latest, now + chrono::Duration::seconds(2));
    }

    #[test]
    fn a_write_waits_for_an_active_read_and_a_second_write_is_rejected() {
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
        let read = ApplicationRequest::AllWorklogs { after: None };
        assert!(state.enqueue(read.clone(), |_, _| {}));
        let active = state.take_effect().expect("active read");
        let write = ApplicationRequest::CreateTask {
            name: TaskName::new("Queued task").unwrap(),
            occurred_at: Utc::now(),
        };
        assert!(state.enqueue(write.clone(), |_, _| {}));
        assert!(!state.enqueue(write.clone(), |_, _| {}));
        assert!(state.take_effect().is_none());
        state.complete_effect(
            active,
            CompletedRequest {
                request: read,
                outcome: ApplicationOutcome::GlobalWorklogPage(Err(
                    ApplicationError::InvalidReportRange,
                )),
                snapshot: ApplicationSnapshot {
                    items: Vec::new(),
                    tracking: TrackingState::Idle,
                },
            },
        );
        assert_eq!(state.take_effect().expect("queued write").request, write);
    }

    #[test]
    fn distinct_writes_execute_in_confirmation_order() {
        let mut application =
            TrackerApplication::load(SqliteRepository::open_in_memory().unwrap()).unwrap();
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
        let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
        let first = ApplicationRequest::CreateTask {
            name: TaskName::new("First task").unwrap(),
            occurred_at: at,
        };
        let second = ApplicationRequest::CreateTask {
            name: TaskName::new("Second task").unwrap(),
            occurred_at: at + TimeDelta::seconds(1),
        };
        let observed = Rc::new(RefCell::new(Vec::new()));
        for request in [first.clone(), second] {
            let observed = Rc::clone(&observed);
            assert!(state.enqueue(request, move |_, completed| {
                let ApplicationOutcome::Task(Ok(task)) = completed.outcome else {
                    panic!("task creation should succeed");
                };
                observed.borrow_mut().push(task.name().as_str().to_owned());
            }));
        }
        assert!(!state.enqueue(
            ApplicationRequest::CreateTask {
                name: TaskName::new("First task").unwrap(),
                occurred_at: at + TimeDelta::seconds(2),
            },
            |_, _| {},
        ));
        while let Some(effect) = state.take_effect() {
            let completed = execute_local(&mut application, effect.request.clone());
            state.complete_effect(effect, completed);
        }
        assert_eq!(observed.borrow().as_slice(), ["First task", "Second task"]);
        assert_eq!(application.tasks(TaskOrdering::RecentlyCreated).len(), 2);
    }

    #[test]
    fn queued_worklog_write_keeps_its_expected_times_for_backend_validation() {
        let mut application =
            TrackerApplication::load(SqliteRepository::open_in_memory().unwrap()).unwrap();
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
        let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
        let task = application
            .create_task(TaskName::new("Tracked task").unwrap(), at)
            .unwrap();
        let started = match application
            .set_active_task(task.id(), at + TimeDelta::seconds(1))
            .unwrap()
        {
            SetActiveTaskOutcome::Started { worklog } => worklog,
            other => panic!("expected started worklog, got {other:?}"),
        };
        let completed = match application
            .clear_active_task(started.id(), at + TimeDelta::seconds(10))
            .unwrap()
        {
            ClearActiveTaskOutcome::Stopped { worklog } => worklog,
            other => panic!("expected stopped worklog, got {other:?}"),
        };
        let original = completed.times();
        let competing = WorklogTimes::new(
            original.start() + TimeDelta::seconds(1),
            original.end().map(|end| end + TimeDelta::seconds(1)),
        );
        let queued = WorklogTimes::new(
            original.start() + TimeDelta::seconds(2),
            original.end().map(|end| end + TimeDelta::seconds(2)),
        );
        assert!(state.enqueue(
            ApplicationRequest::CorrectWorklog {
                id: completed.id(),
                expected: original,
                replacement: queued,
                occurred_at: at + TimeDelta::seconds(12),
            },
            |_, _| {},
        ));
        application
            .correct_worklog(
                completed.id(),
                original,
                competing,
                at + TimeDelta::seconds(11),
            )
            .unwrap();

        let effect = state.take_effect().unwrap();
        let result = execute_local(&mut application, effect.request.clone());
        let ApplicationOutcome::Worklog(Err(error)) = &result.outcome else {
            panic!("stale correction should fail");
        };
        assert_eq!(
            error.failure().category(),
            ApplicationFailureCategory::WorklogChanged,
        );
        state.complete_effect(effect, result);
        let page = application.worklogs_for_task(task.id(), None).unwrap();
        assert_eq!(page.worklogs[0].times(), competing);
    }

    #[test]
    fn distinct_pending_writes_stop_at_the_queue_limit_with_a_visible_error() {
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
        let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
        for index in 0..MAX_PENDING_EFFECTS {
            assert!(state.enqueue(
                ApplicationRequest::CreateTask {
                    name: TaskName::new(&format!("Task {index}")).unwrap(),
                    occurred_at: at,
                },
                |_, _| {},
            ));
        }
        assert!(!state.enqueue(
            ApplicationRequest::CreateTask {
                name: TaskName::new("Overflow task").unwrap(),
                occurred_at: at,
            },
            |_, _| {},
        ));
        assert_eq!(state.pending_effects.len(), MAX_PENDING_EFFECTS);
        assert_eq!(
            state.shell().status(),
            &super::Status::Error("Too many pending requests".into())
        );
    }

    #[test]
    fn quitting_discards_queued_reads_and_keeps_an_accepted_write() {
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
        let read = ApplicationRequest::AllWorklogs { after: None };
        assert!(state.enqueue(read.clone(), |_, _| {}));
        let active = state.take_effect().expect("active read");
        assert!(state.enqueue(read.clone(), |_, _| {}));
        let write = ApplicationRequest::CreateTask {
            name: TaskName::new("Saved task").unwrap(),
            occurred_at: Utc::now(),
        };
        assert!(state.enqueue(write.clone(), |_, _| {}));

        state.handle_command(Command::Quit);
        assert!(!state.is_running());
        assert!(!state.active_request_is_write());
        assert!(state.has_queued_write());
        assert_eq!(state.pending_effects.len(), 1);
        assert!(!state.enqueue(read.clone(), |_, _| {}));

        state.complete_effect(
            active,
            CompletedRequest {
                request: read,
                outcome: ApplicationOutcome::GlobalWorklogPage(Err(
                    ApplicationError::InvalidReportRange,
                )),
                snapshot: ApplicationSnapshot {
                    items: Vec::new(),
                    tracking: TrackingState::Idle,
                },
            },
        );
        assert_eq!(state.take_effect().expect("accepted write").request, write);
        assert!(state.active_request_is_write());
    }
}
