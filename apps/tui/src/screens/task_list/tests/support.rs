// App test support shared by this screen's test modules.

use std::cell::Cell;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use tracker_application::{
    ApplicationError, ClearActiveTaskOutcome, SetActiveTaskOutcome, TaskListItem, TaskOperations,
    TaskOrdering, TaskQueries, TrackerApplication, TrackingOperations, WorklogCursor,
    WorklogOperations, WorklogPage, WorklogPageSnapshot, WorklogQueries,
};
use tracker_domain::{
    ActiveWorklog, Task, TaskId, TaskName, TrackingState, Worklog, WorklogId, WorklogTimes,
};
use tracker_storage::SqliteRepository;

use crate::app::*;
use crate::command::Command;

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
    worklog_pages: Vec<Result<WorklogPage, ApplicationError>>,
    worklog_reads: Cell<usize>,
    correction_error: Option<ApplicationError>,
    correction_calls: Vec<(WorklogId, WorklogTimes, WorklogTimes, DateTime<Utc>)>,
    deletion_error: Option<ApplicationError>,
    deletion_calls: Vec<(WorklogId, TaskId, WorklogTimes)>,
    authoritative_worklogs: Vec<Worklog>,
    clear_calls: Vec<(WorklogId, DateTime<Utc>)>,
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
            worklog_pages: Vec::new(),
            worklog_reads: Cell::new(0),
            correction_error: None,
            correction_calls: Vec::new(),
            deletion_error: None,
            deletion_calls: Vec::new(),
            authoritative_worklogs: Vec::new(),
            clear_calls: Vec::new(),
        }
    }

    fn failure() -> ApplicationError {
        ApplicationError::storage_failure("write failed")
    }

    fn deletion_target_index(&self, id: WorklogId) -> Result<usize, ApplicationError> {
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
        expected_active: WorklogId,
        occurred_at: DateTime<Utc>,
    ) -> Result<ClearActiveTaskOutcome, ApplicationError> {
        self.clear_calls.push((expected_active, occurred_at));
        self.tracking = TrackingState::Idle;
        Ok(ClearActiveTaskOutcome::AlreadyIdle)
    }
}

impl WorklogOperations for TestService {
    fn correct_worklog(
        &mut self,
        id: WorklogId,
        expected: WorklogTimes,
        replacement: WorklogTimes,
        occurred_at: DateTime<Utc>,
    ) -> Result<Worklog, ApplicationError> {
        self.correction_calls
            .push((id, expected, replacement, occurred_at));
        if let Some(error) = self.correction_error.clone() {
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
        self.deletion_calls.push((id, task_id, expected));
        let index = self.deletion_target_index(id)?;
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
        // Each read consumes the next queued page, so one service can
        // answer an initial load, several older pages, and failures.
        let read = self.worklog_reads.get();
        self.worklog_reads.set(read + 1);
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

fn stamped_task(tag: u128, name: &str, archived: bool, created_at: i64, updated_at: i64) -> Task {
    Task::rehydrate(
        TaskId::from_uuid(uuid::Uuid::from_u128(tag)),
        TaskName::new(name).unwrap(),
        archived,
        at(created_at),
        at(updated_at),
    )
    .unwrap()
}
