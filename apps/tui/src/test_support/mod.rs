// Shared fixtures and service spies for TUI unit tests.

pub(crate) mod keymap;

use std::cell::RefCell;
use std::rc::Rc;
pub(crate) use std::time::{Duration, Instant};

pub(crate) use chrono::{
    DateTime, FixedOffset, MappedLocalTime, NaiveDate, NaiveDateTime, TimeDelta, TimeZone, Utc,
};
pub(crate) use tracker_application::{
    ApplicationError, ApplicationFailureCategory, ClearActiveTaskOutcome, ReportQueries,
    ReportTotals, RepositoryError, SetActiveTaskOutcome, TaskListItem, TaskOperations,
    TaskOrdering, TaskQueries, TrackerApplication, TrackerApplicationService, TrackingOperations,
    WorklogCursor, WorklogOperations, WorklogPage, WorklogPageSnapshot, WorklogQueries,
};
pub(crate) use tracker_domain::{
    ActiveWorklog, Task, TaskId, TaskName, TrackingState, Worklog, WorklogCorrectionError,
    WorklogId, WorklogTimes,
};
pub(crate) use tracker_storage::SqliteRepository;

pub(crate) use crate::app::{App, Status, TestClock};
pub(crate) use crate::command::Command;
pub(crate) use crate::screens::task_list::{InputPurpose, TaskListCommand, TaskListMode, TaskView};
pub(crate) use crate::screens::worklog_history::{
    CorrectionField, HistoryAvailability, WorklogHistoryCommand,
};
pub(crate) use crate::screens::{Screen, ScreenState, WorklogHistoryMode};
pub(crate) use crate::support::clock::{ElapsedClock, tracking_timestamp};
pub(crate) use crate::support::errors::ACTIVE_WORKLOG_DELETE_MESSAGE;
pub(crate) use crate::support::timestamps::*;

pub(crate) fn at(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 0).unwrap()
}

pub(crate) fn app_with(names: &[&str]) -> App<TrackerApplication<SqliteRepository>> {
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

pub(crate) fn app_in_timezone<S: TrackerApplicationService>(
    application: S,
    timezone: chrono_tz::Tz,
) -> App<S> {
    App::load_in_timezone(application, timezone)
}

pub(crate) fn app_with_test_clock<S: TrackerApplicationService>(
    application: S,
    timezone: chrono_tz::Tz,
    wall_clock: DateTime<Utc>,
) -> (App<S>, TestClock) {
    App::load_with_test_clock(application, timezone, wall_clock)
}

pub(crate) fn replace_start<S: TrackerApplicationService>(
    app: &mut App<S>,
    text: impl Into<String>,
) {
    replace_timestamp(app, text.into());
}

pub(crate) fn replace_end<S: TrackerApplicationService>(app: &mut App<S>, text: impl Into<String>) {
    if app
        .app_view()
        .correction()
        .expect("correction must be open")
        .focused()
        == CorrectionField::Start
    {
        app.handle(Command::WorklogHistory(
            WorklogHistoryCommand::SwitchCorrectionField,
        ));
    }
    replace_timestamp(app, text.into());
}

fn replace_timestamp<S: TrackerApplicationService>(app: &mut App<S>, text: String) {
    let draft = app
        .app_view()
        .correction()
        .expect("correction must be open");
    let count = match draft.focused() {
        CorrectionField::Start => draft.start().text().chars().count(),
        CorrectionField::End => draft
            .end()
            .expect("the focused end input must exist")
            .text()
            .chars()
            .count(),
    };
    for _ in 0..count {
        app.handle(Command::WorklogHistory(WorklogHistoryCommand::Backspace));
    }
    for character in text.chars() {
        app.handle(Command::WorklogHistory(WorklogHistoryCommand::Insert(
            character,
        )));
    }
}

pub(crate) fn text(status: &Status) -> &str {
    match status {
        Status::Empty => "",
        Status::Info(text) | Status::Error(text) => text,
    }
}

#[derive(Default)]
struct SpyState {
    worklog_reads: usize,
    report_reads: Vec<(DateTime<Utc>, DateTime<Utc>, DateTime<Utc>)>,
    report_result: Option<Result<ReportTotals, ApplicationError>>,
    report_tasks: Option<Vec<Task>>,
    correction_calls: Vec<(WorklogId, WorklogTimes, WorklogTimes, DateTime<Utc>)>,
    move_calls: Vec<(WorklogId, TaskId, WorklogTimes, TaskId)>,
    deletion_calls: Vec<(WorklogId, TaskId, WorklogTimes)>,
    clear_calls: Vec<(WorklogId, DateTime<Utc>)>,
    correction_error: Option<ApplicationError>,
    move_error: Option<ApplicationError>,
    external_tracking: Option<TrackingState>,
    latest_work_overrides: Vec<(TaskId, DateTime<Utc>)>,
    latest_work_starts: Vec<(TaskId, Option<DateTime<Utc>>)>,
}

#[derive(Clone)]
pub(crate) struct TestServiceSpy {
    state: Rc<RefCell<SpyState>>,
}

impl TestServiceSpy {
    pub(crate) fn report_reads(&self) -> Vec<(DateTime<Utc>, DateTime<Utc>, DateTime<Utc>)> {
        self.state.borrow().report_reads.clone()
    }

    pub(crate) fn set_report_result(&self, result: Result<ReportTotals, ApplicationError>) {
        self.state.borrow_mut().report_result = Some(result);
    }

    pub(crate) fn set_report_tasks(&self, tasks: Vec<Task>) {
        self.state.borrow_mut().report_tasks = Some(tasks);
    }

    pub(crate) fn worklog_reads(&self) -> usize {
        self.state.borrow().worklog_reads
    }

    pub(crate) fn correction_calls(
        &self,
    ) -> Vec<(WorklogId, WorklogTimes, WorklogTimes, DateTime<Utc>)> {
        self.state.borrow().correction_calls.clone()
    }

    pub(crate) fn move_calls(&self) -> Vec<(WorklogId, TaskId, WorklogTimes, TaskId)> {
        self.state.borrow().move_calls.clone()
    }

    pub(crate) fn deletion_calls(&self) -> Vec<(WorklogId, TaskId, WorklogTimes)> {
        self.state.borrow().deletion_calls.clone()
    }

    pub(crate) fn clear_calls(&self) -> Vec<(WorklogId, DateTime<Utc>)> {
        self.state.borrow().clear_calls.clone()
    }

    pub(crate) fn set_correction_error(&self, error: ApplicationError) {
        self.state.borrow_mut().correction_error = Some(error);
    }

    pub(crate) fn set_move_error(&self, error: ApplicationError) {
        self.state.borrow_mut().move_error = Some(error);
    }

    pub(crate) fn set_external_tracking(&self, tracking: TrackingState) {
        self.state.borrow_mut().external_tracking = Some(tracking);
    }

    pub(crate) fn set_latest_work_start(&self, task_id: TaskId, start: DateTime<Utc>) {
        let mut state = self.state.borrow_mut();
        upsert_latest(&mut state.latest_work_overrides, task_id, start);
        record_latest(&mut state.latest_work_starts, task_id, Some(start));
    }

    pub(crate) fn latest_work_start(&self, task_id: TaskId) -> Option<DateTime<Utc>> {
        self.state
            .borrow()
            .latest_work_starts
            .iter()
            .find(|(id, _)| *id == task_id)
            .and_then(|(_, start)| *start)
    }
}

fn record_latest(
    values: &mut Vec<(TaskId, Option<DateTime<Utc>>)>,
    task_id: TaskId,
    start: Option<DateTime<Utc>>,
) {
    if let Some((_, existing)) = values.iter_mut().find(|(id, _)| *id == task_id) {
        *existing = start;
    } else {
        values.push((task_id, start));
    }
}

fn upsert_latest(values: &mut Vec<(TaskId, DateTime<Utc>)>, task_id: TaskId, start: DateTime<Utc>) {
    if let Some((_, existing)) = values.iter_mut().find(|(id, _)| *id == task_id) {
        *existing = start;
    } else {
        values.push((task_id, start));
    }
}

pub(crate) struct TestService {
    spy: TestServiceSpy,
    pub(crate) tasks: Vec<Task>,
    pub(crate) tracking: TrackingState,
    pub(crate) fail_create: bool,
    pub(crate) fail_rename: bool,
    pub(crate) fail_archive: bool,
    pub(crate) fail_unarchive: bool,
    pub(crate) archive_activates: Option<DateTime<Utc>>,
    pub(crate) unarchive_activates: Option<DateTime<Utc>>,
    pub(crate) set_returns_already_active: bool,
    pub(crate) set_returns_switched: bool,
    pub(crate) set_timestamp: Option<DateTime<Utc>>,
    pub(crate) latest_work_starts: Vec<(TaskId, DateTime<Utc>)>,
    pub(crate) tasks_after_next_worklog_read: Option<Vec<Task>>,
    pub(crate) worklog_pages: Vec<Result<WorklogPage, ApplicationError>>,
    pub(crate) correction_error: Option<ApplicationError>,
    pub(crate) move_error: Option<ApplicationError>,
    pub(crate) deletion_error: Option<ApplicationError>,
    pub(crate) authoritative_worklogs: Vec<Worklog>,
}

impl TestService {
    pub(crate) fn with_tasks(tasks: Vec<Task>) -> Self {
        Self {
            spy: TestServiceSpy {
                state: Rc::new(RefCell::new(SpyState::default())),
            },
            tasks,
            tracking: TrackingState::Idle,
            fail_create: false,
            fail_rename: false,
            fail_archive: false,
            fail_unarchive: false,
            archive_activates: None,
            unarchive_activates: None,
            set_returns_already_active: false,
            set_returns_switched: false,
            set_timestamp: None,
            latest_work_starts: Vec::new(),
            tasks_after_next_worklog_read: None,
            worklog_pages: Vec::new(),
            correction_error: None,
            move_error: None,
            deletion_error: None,
            authoritative_worklogs: Vec::new(),
        }
    }

    pub(crate) fn failure() -> ApplicationError {
        ApplicationError::storage_failure("write failed")
    }

    pub(crate) fn spy(&self) -> TestServiceSpy {
        {
            let mut state = self.spy.state.borrow_mut();
            for (task_id, start) in &self.latest_work_starts {
                record_latest(&mut state.latest_work_starts, *task_id, Some(*start));
            }
        }
        self.spy.clone()
    }

    fn apply_external_state(&mut self) {
        let mut state = self.spy.state.borrow_mut();
        if let Some(tracking) = state.external_tracking.take() {
            self.tracking = tracking;
        }
        for (task_id, start) in state.latest_work_overrides.drain(..) {
            if let Some((_, existing)) = self
                .latest_work_starts
                .iter_mut()
                .find(|(id, _)| *id == task_id)
            {
                *existing = start;
            } else {
                self.latest_work_starts.push((task_id, start));
            }
        }
    }

    fn authoritative_worklog_index(&self, id: WorklogId) -> Result<usize, ApplicationError> {
        self.authoritative_worklogs
            .iter()
            .position(|worklog| worklog.id() == id)
            .ok_or_else(|| ApplicationError::worklog_not_found(id))
    }

    fn validate_deletion_target(
        stored: &Worklog,
        id: WorklogId,
        task_id: TaskId,
        expected: WorklogTimes,
    ) -> Result<(), ApplicationError> {
        if stored.is_active() {
            return Err(ApplicationError::active_worklog(id));
        }
        if stored.task_id() != task_id || stored.times() != expected {
            return Err(ApplicationError::worklog_changed(id));
        }
        Ok(())
    }

    fn update_latest_work_start(&mut self, task_id: TaskId) {
        let latest = self
            .authoritative_worklogs
            .iter()
            .filter(|candidate| candidate.task_id() == task_id)
            .map(Worklog::start)
            .max();
        let position = self
            .latest_work_starts
            .iter()
            .position(|(candidate, _)| *candidate == task_id);
        match (position, latest) {
            (Some(position), Some(latest)) => self.latest_work_starts[position].1 = latest,
            (Some(position), None) => {
                self.latest_work_starts.remove(position);
            }
            (None, Some(latest)) => self.latest_work_starts.push((task_id, latest)),
            (None, None) => {}
        }
        record_latest(
            &mut self.spy.state.borrow_mut().latest_work_starts,
            task_id,
            latest,
        );
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
                    .find(|(id, _)| *id == task.id())
                    .map(|(_, start)| *start),
                task,
            })
            .collect::<Vec<_>>();
        ordering.sort_items(&mut items);
        items
    }

    fn task(&self, id: TaskId) -> Option<&Task> {
        self.tasks.iter().find(|task| task.id() == id)
    }
}

impl TaskOperations for TestService {
    fn create_task(
        &mut self,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        if self.fail_create {
            return Err(Self::failure());
        }
        let task = Task::create(
            TaskId::from_uuid(uuid::Uuid::from_u128(2)),
            name,
            occurred_at,
        );
        self.tasks.push(task.clone());
        Ok(task)
    }

    fn rename_task(
        &mut self,
        id: TaskId,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        if self.fail_rename {
            return Err(Self::failure());
        }
        let task = self
            .tasks
            .iter_mut()
            .find(|task| task.id() == id)
            .expect("test task exists");
        task.rename(name, occurred_at);
        Ok(task.clone())
    }

    fn archive_task(
        &mut self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        self.apply_external_state();
        if self.fail_archive {
            return Err(Self::failure());
        }
        let task = self
            .tasks
            .iter_mut()
            .find(|task| task.id() == id)
            .expect("test task exists");
        task.archive(occurred_at);
        let archived = task.clone();
        if let Some(start) = self.archive_activates {
            let other = self
                .tasks
                .iter()
                .find(|task| task.id() != id && !task.is_archived())
                .expect("the test service needs another active task");
            self.tracking = TrackingState::Running {
                worklog: ActiveWorklog::begin(WorklogId::generate(), other.id(), start),
            };
        }
        Ok(archived)
    }

    fn unarchive_task(
        &mut self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, ApplicationError> {
        self.apply_external_state();
        if self.fail_unarchive {
            return Err(Self::failure());
        }
        let task = self
            .tasks
            .iter_mut()
            .find(|task| task.id() == id)
            .expect("test task exists");
        task.restore(occurred_at);
        // Stands in for a second client that restored the task and
        // started tracking it before this unarchive ran.
        if let Some(start) = self.unarchive_activates {
            self.tracking = TrackingState::Running {
                worklog: ActiveWorklog::begin(
                    WorklogId::from_uuid(uuid::Uuid::from_u128(20)),
                    task.id(),
                    start,
                ),
            };
        }
        Ok(task.clone())
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
        self.apply_external_state();
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
        let active = ActiveWorklog::begin(worklog.id(), task_id, started_at);
        let previous = self.tracking.clone();
        self.tracking = TrackingState::Running {
            worklog: active.clone(),
        };
        if self.set_returns_already_active {
            Ok(SetActiveTaskOutcome::AlreadyActive { worklog: active })
        } else if self.set_returns_switched {
            let TrackingState::Running { worklog: previous } = previous else {
                panic!("a switched outcome requires an active worklog");
            };
            let stopped = Worklog::new(
                previous.id(),
                previous.task_id(),
                previous.start(),
                Some(started_at),
            )
            .expect("the replacement task starts after the active worklog");
            Ok(SetActiveTaskOutcome::Switched {
                stopped,
                started: worklog,
            })
        } else {
            Ok(SetActiveTaskOutcome::Started { worklog })
        }
    }

    fn clear_active_task(
        &mut self,
        expected_active: WorklogId,
        occurred_at: DateTime<Utc>,
    ) -> Result<ClearActiveTaskOutcome, ApplicationError> {
        self.apply_external_state();
        self.spy
            .state
            .borrow_mut()
            .clear_calls
            .push((expected_active, occurred_at));
        self.tracking = TrackingState::Idle;
        Ok(ClearActiveTaskOutcome::AlreadyIdle)
    }
}

impl WorklogOperations for TestService {
    fn move_worklog(
        &mut self,
        id: WorklogId,
        source_task_id: TaskId,
        expected: WorklogTimes,
        destination_task_id: TaskId,
    ) -> Result<Worklog, ApplicationError> {
        self.apply_external_state();
        self.spy.state.borrow_mut().move_calls.push((
            id,
            source_task_id,
            expected,
            destination_task_id,
        ));
        let move_error = self
            .spy
            .state
            .borrow()
            .move_error
            .clone()
            .or_else(|| self.move_error.clone());
        if let Some(error) = move_error {
            return Err(error);
        }
        let index = self.authoritative_worklog_index(id)?;
        let stored = &self.authoritative_worklogs[index];
        if stored.task_id() != source_task_id || stored.times() != expected {
            return Err(ApplicationError::worklog_changed(id));
        }
        let destination = self
            .tasks
            .iter()
            .find(|task| task.id() == destination_task_id)
            .ok_or(ApplicationError::Repository(
                RepositoryError::TaskNotFound {
                    id: destination_task_id,
                },
            ))?;
        if destination.is_archived() {
            return Err(ApplicationError::Repository(
                RepositoryError::TaskArchived {
                    id: destination_task_id,
                },
            ));
        }
        let moved = stored.moved_to(destination_task_id)?;
        self.authoritative_worklogs[index] = moved.clone();
        if moved.is_active() {
            self.tracking = TrackingState::Running {
                worklog: ActiveWorklog::begin(moved.id(), moved.task_id(), moved.start()),
            };
        }
        self.update_latest_work_start(source_task_id);
        self.update_latest_work_start(destination_task_id);
        Ok(moved)
    }

    fn correct_worklog(
        &mut self,
        id: WorklogId,
        expected: WorklogTimes,
        replacement: WorklogTimes,
        occurred_at: DateTime<Utc>,
    ) -> Result<Worklog, ApplicationError> {
        self.apply_external_state();
        self.spy
            .state
            .borrow_mut()
            .correction_calls
            .push((id, expected, replacement, occurred_at));
        let correction_error = self
            .spy
            .state
            .borrow()
            .correction_error
            .clone()
            .or_else(|| self.correction_error.clone());
        if let Some(error) = correction_error {
            return Err(error);
        }
        let original = self
            .worklog_pages
            .iter()
            .filter_map(|page| page.as_ref().ok())
            .flat_map(|page| &page.worklogs)
            .find(|worklog| worklog.id() == id)
            .cloned()
            .expect("the corrected worklog exists in a queued page");
        let corrected = original.corrected(replacement, occurred_at)?;
        if corrected.is_active() {
            self.tracking = TrackingState::Running {
                worklog: ActiveWorklog::begin(
                    corrected.id(),
                    corrected.task_id(),
                    corrected.start(),
                ),
            };
        }
        Ok(corrected)
    }

    fn delete_completed_worklog(
        &mut self,
        id: WorklogId,
        task_id: TaskId,
        expected: WorklogTimes,
    ) -> Result<Worklog, ApplicationError> {
        self.apply_external_state();
        self.spy
            .state
            .borrow_mut()
            .deletion_calls
            .push((id, task_id, expected));
        let index = self.authoritative_worklog_index(id)?;
        Self::validate_deletion_target(&self.authoritative_worklogs[index], id, task_id, expected)?;
        if let Some(error) = self.deletion_error.clone() {
            return Err(error);
        }
        let worklog = self.authoritative_worklogs.remove(index);
        self.update_latest_work_start(task_id);
        Ok(worklog)
    }
}

impl WorklogQueries for TestService {
    fn worklogs_for_task(
        &mut self,
        _task_id: TaskId,
        _after: Option<&WorklogCursor>,
    ) -> Result<WorklogPage, ApplicationError> {
        self.apply_external_state();
        if let Some(tasks) = self.tasks_after_next_worklog_read.take() {
            self.tasks = tasks;
        }
        // Each read consumes the next queued page, so one service can
        // answer an initial load, several older pages, and failures.
        let read = self.spy.state.borrow().worklog_reads;
        self.spy.state.borrow_mut().worklog_reads = read + 1;
        let result = self.worklog_pages.get(read).cloned().unwrap_or_else(|| {
            Ok(WorklogPage {
                worklogs: Vec::new(),
                snapshot: WorklogPageSnapshot {
                    requested_task_latest_work_start: None,
                    active_worklog: None,
                    active_task_latest_work_start: None,
                },
                next_cursor: None,
            })
        });
        if let Ok(page) = &result {
            self.tracking = match &page.snapshot.active_worklog {
                Some(worklog) => TrackingState::Running {
                    worklog: ActiveWorklog::begin(worklog.id(), worklog.task_id(), worklog.start()),
                },
                None => TrackingState::Idle,
            };
        }
        result
    }
}

impl ReportQueries for TestService {
    fn report_totals(
        &mut self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<ReportTotals, ApplicationError> {
        let mut spy = self.spy.state.borrow_mut();
        spy.report_reads.push((start, end, now));
        let result = spy.report_result.clone().unwrap_or_else(|| {
            Ok(ReportTotals {
                rows: Vec::new(),
                total: TimeDelta::zero(),
            })
        });
        if result.is_ok()
            && let Some(tasks) = spy.report_tasks.take()
        {
            self.tasks = tasks;
        }
        result
    }
}

pub(crate) fn task(tag: u128, name: &str) -> Task {
    Task::create(
        TaskId::from_uuid(uuid::Uuid::from_u128(tag)),
        TaskName::new(name).unwrap(),
        at(100),
    )
}

pub(crate) fn archived_task(tag: u128, name: &str) -> Task {
    let mut task = task(tag, name);
    assert!(task.archive(at(100)));
    task
}

pub(crate) fn worklog_id(tag: u128) -> WorklogId {
    WorklogId::from_uuid(uuid::Uuid::from_u128(tag))
}

/// A stopped worklog for the task, started at `start` and running one
/// minute.
pub(crate) fn history_worklog(tag: u128, task_id: TaskId, start: i64) -> Worklog {
    Worklog::new(worklog_id(tag), task_id, at(start), Some(at(start + 60))).unwrap()
}

pub(crate) fn cursor(start: i64, tag: u128) -> WorklogCursor {
    WorklogCursor {
        task_id: TaskId::from_uuid(uuid::Uuid::from_u128(1)),
        start: at(start),
        id: worklog_id(tag),
        revision: 0,
    }
}

pub(crate) fn page(worklogs: Vec<Worklog>, next_cursor: Option<WorklogCursor>) -> WorklogPage {
    let requested_task_latest_work_start = worklogs.iter().map(Worklog::start).max();
    let active_worklog = worklogs.iter().find(|worklog| worklog.is_active()).cloned();
    WorklogPage {
        snapshot: WorklogPageSnapshot {
            requested_task_latest_work_start,
            active_task_latest_work_start: active_worklog.as_ref().map(Worklog::start),
            active_worklog,
        },
        worklogs,
        next_cursor,
    }
}

pub(crate) fn page_with_active(
    worklogs: Vec<Worklog>,
    active_worklog: Option<Worklog>,
    next_cursor: Option<WorklogCursor>,
) -> WorklogPage {
    let requested_task_latest_work_start = worklogs.iter().map(Worklog::start).max();
    WorklogPage {
        snapshot: WorklogPageSnapshot {
            requested_task_latest_work_start,
            active_task_latest_work_start: active_worklog.as_ref().map(Worklog::start),
            active_worklog,
        },
        worklogs,
        next_cursor,
    }
}

pub(crate) fn correction_history_app_with_spy_in(
    initial: Worklog,
    reload: Vec<Worklog>,
    timezone: chrono_tz::Tz,
) -> (App<TestService>, TestServiceSpy) {
    let task = task(1, "alpha");
    let mut service = TestService::with_tasks(vec![task]);
    service.worklog_pages = vec![
        Ok(page(vec![initial], Some(cursor(50, 50)))),
        Ok(page(reload, None)),
    ];
    let spy = service.spy();
    let mut app = app_in_timezone(service, timezone);
    app.handle(Command::TaskList(TaskListCommand::OpenHistory));
    (app, spy)
}

pub(crate) fn correction_app(initial: Worklog, reload: Vec<Worklog>) -> App<TestService> {
    correction_app_with_spy(initial, reload).0
}

pub(crate) fn correction_app_with_spy(
    initial: Worklog,
    reload: Vec<Worklog>,
) -> (App<TestService>, TestServiceSpy) {
    let (mut app, spy) =
        correction_history_app_with_spy_in(initial, reload, chrono_tz::Africa::Johannesburg);
    app.handle(Command::WorklogHistory(
        WorklogHistoryCommand::OpenCorrection,
    ));
    (app, spy)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CorrectionTestZone;

impl CorrectionTestZone {
    fn offset(seconds: i64) -> FixedOffset {
        let offset = if seconds < 3_600 {
            3_600
        } else if seconds < 18_000 {
            7_200
        } else if seconds < 100_000 {
            3_600
        } else {
            10_800
        };
        FixedOffset::east_opt(offset).unwrap()
    }
}

fn early_local_offset(seconds: i64) -> MappedLocalTime<FixedOffset> {
    if seconds < 7_200 {
        MappedLocalTime::Single(FixedOffset::east_opt(3_600).unwrap())
    } else if seconds < 10_800 {
        MappedLocalTime::None
    } else {
        MappedLocalTime::Single(FixedOffset::east_opt(7_200).unwrap())
    }
}

fn late_local_offset(seconds: i64) -> MappedLocalTime<FixedOffset> {
    if seconds < 25_200 {
        MappedLocalTime::Ambiguous(
            FixedOffset::east_opt(7_200).unwrap(),
            FixedOffset::east_opt(3_600).unwrap(),
        )
    } else if seconds < 103_600 {
        MappedLocalTime::Single(FixedOffset::east_opt(3_600).unwrap())
    } else if seconds < 110_800 {
        MappedLocalTime::None
    } else {
        MappedLocalTime::Single(FixedOffset::east_opt(10_800).unwrap())
    }
}

impl TimeZone for CorrectionTestZone {
    type Offset = FixedOffset;

    fn from_offset(_offset: &Self::Offset) -> Self {
        Self
    }

    fn offset_from_local_date(&self, local: &NaiveDate) -> MappedLocalTime<Self::Offset> {
        self.offset_from_local_datetime(&local.and_hms_opt(0, 0, 0).unwrap())
    }

    fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> MappedLocalTime<Self::Offset> {
        let seconds = local.and_utc().timestamp();
        if seconds < 21_600 {
            early_local_offset(seconds)
        } else {
            late_local_offset(seconds)
        }
    }

    fn offset_from_utc_date(&self, utc: &NaiveDate) -> Self::Offset {
        Self::offset(utc.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp())
    }

    fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> Self::Offset {
        Self::offset(utc.and_utc().timestamp())
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SubminuteTransitionZone;

impl TimeZone for SubminuteTransitionZone {
    type Offset = FixedOffset;

    fn from_offset(_offset: &Self::Offset) -> Self {
        Self
    }

    fn offset_from_local_date(&self, local: &NaiveDate) -> MappedLocalTime<Self::Offset> {
        self.offset_from_local_datetime(&local.and_hms_opt(0, 0, 0).unwrap())
    }

    fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> MappedLocalTime<Self::Offset> {
        let seconds = local.and_utc().timestamp();
        if seconds < 330 {
            MappedLocalTime::Single(FixedOffset::east_opt(30).unwrap())
        } else if seconds < 360 {
            MappedLocalTime::None
        } else {
            MappedLocalTime::Single(FixedOffset::east_opt(60).unwrap())
        }
    }

    fn offset_from_utc_date(&self, utc: &NaiveDate) -> Self::Offset {
        self.offset_from_utc_datetime(&utc.and_hms_opt(0, 0, 0).unwrap())
    }

    fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> Self::Offset {
        if utc.and_utc().timestamp() < 300 {
            FixedOffset::east_opt(30).unwrap()
        } else {
            FixedOffset::east_opt(60).unwrap()
        }
    }
}

pub(crate) fn stamped_task(
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
