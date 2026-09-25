use std::cell::RefCell;
use std::rc::Rc;

use chrono::{DateTime, Utc};
use tracker_domain::{
    ActiveWorklog, Task, TaskId, TaskName, TrackingError, TrackingState, Worklog,
    WorklogCorrectionError, WorklogId, WorklogTimes,
};

use super::*;

#[derive(Default)]
struct Data {
    tasks: Vec<TaskListItem>,
    worklogs: Vec<Worklog>,
    fail_next_write: Option<RepositoryError>,
    fail_reads: bool,
    replacement_on_failure: Option<Worklog>,
    fail_reads_after_write: bool,
    fail_reads_after_task_write: bool,
    hide_next_active_read: bool,
    list_reads: usize,
    move_writes: usize,
    deletion_writes: usize,
    history_revisions: Vec<(TaskId, i64)>,
}

#[derive(Clone, Default)]
struct MemoryRepository(Rc<RefCell<Data>>);

impl MemoryRepository {
    fn with_tasks(tasks: Vec<Task>) -> Self {
        let items = tasks
            .into_iter()
            .map(|task| TaskListItem {
                task,
                latest_work_start: None,
            })
            .collect::<Vec<_>>();
        Self(Rc::new(RefCell::new(Data {
            tasks: items,
            ..Data::default()
        })))
    }

    fn fail_next_write(&self, error: RepositoryError, replacement: Option<Worklog>) {
        let mut data = self.0.borrow_mut();
        data.fail_next_write = Some(error);
        data.replacement_on_failure = replacement;
    }

    fn fail_recovery_after_next_write(&self) {
        self.0.borrow_mut().fail_reads_after_write = true;
    }

    fn fail_refresh_after_next_task_write(&self) {
        self.0.borrow_mut().fail_reads_after_task_write = true;
    }

    fn hide_next_active_read(&self) {
        self.0.borrow_mut().hide_next_active_read = true;
    }

    fn read_guard(&self) -> Result<(), RepositoryError> {
        if self.0.borrow().fail_reads {
            Err(RepositoryError::Backend {
                message: "read failed".to_owned(),
            })
        } else {
            Ok(())
        }
    }

    fn snapshot(data: &Data) -> TrackerSnapshot {
        let mut task_items = data.tasks.clone();
        for item in &mut task_items {
            item.latest_work_start = data
                .worklogs
                .iter()
                .filter(|worklog| worklog.task_id() == item.task.id())
                .map(Worklog::start)
                .max();
        }
        TrackerSnapshot {
            task_items,
            active_worklog: data
                .worklogs
                .iter()
                .find(|worklog| worklog.is_active())
                .cloned(),
        }
    }

    fn revision(data: &Data, task_id: TaskId) -> i64 {
        data.history_revisions
            .iter()
            .find(|(id, _)| *id == task_id)
            .map_or(0, |(_, revision)| *revision)
    }

    fn bump_revision(data: &mut Data, task_id: TaskId) {
        if let Some((_, revision)) = data
            .history_revisions
            .iter_mut()
            .find(|(id, _)| *id == task_id)
        {
            *revision += 1;
        } else {
            data.history_revisions.push((task_id, 1));
        }
    }

    fn take_write_failure(&self) -> Option<RepositoryError> {
        let mut data = self.0.borrow_mut();
        let error = data.fail_next_write.take()?;
        if let Some(replacement) = data.replacement_on_failure.take() {
            data.worklogs.retain(|worklog| !worklog.is_active());
            data.worklogs.push(replacement);
        }
        if data.fail_reads_after_write {
            data.fail_reads = true;
            data.fail_reads_after_write = false;
        }
        Some(error)
    }

    fn active_worklog(&self) -> Result<Option<Worklog>, RepositoryError> {
        self.read_guard()?;
        let mut data = self.0.borrow_mut();
        if data.hide_next_active_read {
            data.hide_next_active_read = false;
            return Ok(None);
        }
        Ok(data
            .worklogs
            .iter()
            .find(|worklog| worklog.is_active())
            .cloned())
    }
}

impl TaskRepository for MemoryRepository {
    fn create_task(&self, task: Task) -> Result<(), RepositoryError> {
        if let Some(error) = self.take_write_failure() {
            return Err(error);
        }
        let mut data = self.0.borrow_mut();
        if data.tasks.iter().any(|item| item.task.id() == task.id()) {
            return Err(RepositoryError::TaskAlreadyExists { id: task.id() });
        }
        data.tasks.push(TaskListItem {
            task,
            latest_work_start: None,
        });
        data.tasks.sort_by_key(|a| a.task.id());
        Ok(())
    }

    fn tracker_snapshot(&self) -> Result<TrackerSnapshot, RepositoryError> {
        self.read_guard()?;
        Ok(Self::snapshot(&self.0.borrow()))
    }

    fn rename_task(
        &self,
        id: TaskId,
        name: TaskName,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, RepositoryError> {
        if let Some(error) = self.take_write_failure() {
            return Err(error);
        }
        let mut data = self.0.borrow_mut();
        let index = data
            .tasks
            .iter()
            .position(|item| item.task.id() == id)
            .ok_or(RepositoryError::TaskNotFound { id })?;
        // The domain method carries the semantics the storage statement
        // implements: an equal name changes nothing, and a real rename
        // never moves `updated_at` backward.
        data.tasks[index].task.rename(name, occurred_at);
        Ok(data.tasks[index].task.clone())
    }

    fn archive_task(
        &self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, RepositoryError> {
        if let Some(error) = self.take_write_failure() {
            return Err(error);
        }
        let mut data = self.0.borrow_mut();
        let index = data
            .tasks
            .iter()
            .position(|item| item.task.id() == id)
            .ok_or(RepositoryError::TaskNotFound { id })?;
        // The archive trigger's rule: a running task cannot archive. An
        // already archived task cannot run, so the idempotent case
        // passes.
        if !data.tasks[index].task.is_archived()
            && data
                .worklogs
                .iter()
                .any(|worklog| worklog.task_id() == id && worklog.is_active())
        {
            return Err(RepositoryError::TaskIsActive { id });
        }
        data.tasks[index].task.archive(occurred_at);
        let task = data.tasks[index].task.clone();
        if data.fail_reads_after_task_write {
            data.fail_reads = true;
            data.fail_reads_after_task_write = false;
        }
        Ok(task)
    }

    fn unarchive_task(
        &self,
        id: TaskId,
        occurred_at: DateTime<Utc>,
    ) -> Result<Task, RepositoryError> {
        if let Some(error) = self.take_write_failure() {
            return Err(error);
        }
        let mut data = self.0.borrow_mut();
        let index = data
            .tasks
            .iter()
            .position(|item| item.task.id() == id)
            .ok_or(RepositoryError::TaskNotFound { id })?;
        data.tasks[index].task.restore(occurred_at);
        let task = data.tasks[index].task.clone();
        if data.fail_reads_after_task_write {
            data.fail_reads = true;
            data.fail_reads_after_task_write = false;
        }
        Ok(task)
    }
}

impl ReportRepository for MemoryRepository {
    fn report_read(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<ReportRead, RepositoryError> {
        self.read_guard()?;
        let data = self.0.borrow();
        let mut rows = Vec::new();
        for item in &data.tasks {
            let duration = data
                .worklogs
                .iter()
                .filter(|worklog| worklog.task_id() == item.task.id())
                .map(|worklog| {
                    let clipped_start = worklog.start().max(start);
                    let clipped_end = worklog.end().unwrap_or(now).min(end);
                    (clipped_end - clipped_start).max(chrono::TimeDelta::zero())
                })
                .sum::<chrono::TimeDelta>();
            if duration > chrono::TimeDelta::zero() {
                rows.push(ReportRow {
                    task: item.task.clone(),
                    duration,
                });
            }
        }
        Ok(ReportRead {
            rows,
            snapshot: Self::snapshot(&data),
        })
    }
}

fn has_same_task_overlap(
    worklogs: &[Worklog],
    candidate: &Worklog,
    excluded: Option<WorklogId>,
) -> bool {
    worklogs
        .iter()
        .any(|stored| Some(stored.id()) != excluded && stored.overlaps(candidate))
}

impl WorklogRepository for MemoryRepository {
    fn find_worklog(&self, id: WorklogId) -> Result<Option<Worklog>, RepositoryError> {
        self.read_guard()?;
        Ok(self
            .0
            .borrow()
            .worklogs
            .iter()
            .find(|worklog| worklog.id() == id)
            .cloned())
    }

    fn compare_and_move_worklog(
        &self,
        id: WorklogId,
        expected_source_task_id: TaskId,
        expected: WorklogTimes,
        destination_task_id: TaskId,
    ) -> Result<WorklogMove, RepositoryError> {
        self.0.borrow_mut().move_writes += 1;
        if let Some(error) = self.take_write_failure() {
            return Err(error);
        }
        let mut data = self.0.borrow_mut();
        let index = data
            .worklogs
            .iter()
            .position(|worklog| worklog.id() == id)
            .ok_or(RepositoryError::WorklogNotFound { id })?;
        let stored = data.worklogs[index].clone();
        if stored.task_id() != expected_source_task_id || stored.times() != expected {
            return Err(RepositoryError::WorklogChanged { id });
        }
        if destination_task_id == expected_source_task_id {
            return Err(RepositoryError::Constraint {
                message: "a worklog must move to a different task".to_owned(),
            });
        }
        let destination = data
            .tasks
            .iter()
            .find(|item| item.task.id() == destination_task_id)
            .ok_or(RepositoryError::TaskNotFound {
                id: destination_task_id,
            })?;
        if destination.task.is_archived() {
            return Err(RepositoryError::TaskArchived {
                id: destination_task_id,
            });
        }
        let moved =
            stored
                .moved_to(destination_task_id)
                .map_err(|error| RepositoryError::Constraint {
                    message: error.to_string(),
                })?;
        if has_same_task_overlap(&data.worklogs, &moved, Some(id)) {
            return Err(RepositoryError::SameTaskWorklogOverlap { id });
        }
        data.worklogs[index] = moved.clone();
        Self::bump_revision(&mut data, expected_source_task_id);
        Self::bump_revision(&mut data, destination_task_id);
        let source_task_latest_work_start = data
            .worklogs
            .iter()
            .filter(|worklog| worklog.task_id() == expected_source_task_id)
            .map(Worklog::start)
            .max();
        let destination_task_latest_work_start = data
            .worklogs
            .iter()
            .filter(|worklog| worklog.task_id() == destination_task_id)
            .map(Worklog::start)
            .max();
        let active_worklog = data
            .worklogs
            .iter()
            .find(|worklog| worklog.is_active())
            .cloned();
        let active_task_latest_work_start = match active_worklog.as_ref() {
            Some(active) if active.task_id() == expected_source_task_id => {
                source_task_latest_work_start
            }
            Some(active) if active.task_id() == destination_task_id => {
                destination_task_latest_work_start
            }
            Some(active) => data
                .worklogs
                .iter()
                .filter(|worklog| worklog.task_id() == active.task_id())
                .map(Worklog::start)
                .max(),
            None => None,
        };
        Ok(WorklogMove {
            worklog: moved,
            source_task_latest_work_start,
            destination_task_latest_work_start,
            active_worklog,
            active_task_latest_work_start,
        })
    }

    fn compare_and_set_worklog_times(
        &self,
        id: WorklogId,
        expected: WorklogTimes,
        replacement: WorklogTimes,
    ) -> Result<WorklogCorrection, RepositoryError> {
        if let Some(error) = self.take_write_failure() {
            return Err(error);
        }
        let mut data = self.0.borrow_mut();
        let index = data
            .worklogs
            .iter()
            .position(|worklog| worklog.id() == id)
            .ok_or(RepositoryError::WorklogNotFound { id })?;
        let stored = &data.worklogs[index];
        let stored_start = stored.start();
        if stored.times() != expected {
            return Err(RepositoryError::WorklogChanged { id });
        }
        if stored.is_active() != replacement.is_active() {
            return Err(RepositoryError::Constraint {
                message: "a correction must preserve active state".to_owned(),
            });
        }
        let corrected = Worklog::new(
            stored.id(),
            stored.task_id(),
            replacement.start(),
            replacement.end(),
        )
        .map_err(|error| RepositoryError::Constraint {
            message: error.to_string(),
        })?;
        if has_same_task_overlap(&data.worklogs, &corrected, Some(id)) {
            return Err(RepositoryError::SameTaskWorklogOverlap { id });
        }
        if corrected.start() != stored_start {
            Self::bump_revision(&mut data, corrected.task_id());
        }
        data.worklogs[index] = corrected.clone();
        let task_latest_work_start = data
            .worklogs
            .iter()
            .filter(|worklog| worklog.task_id() == corrected.task_id())
            .map(Worklog::start)
            .max();
        let active_worklog = data
            .worklogs
            .iter()
            .find(|worklog| worklog.is_active())
            .cloned();
        let active_task_latest_work_start = match active_worklog.as_ref() {
            Some(active) if active.task_id() == corrected.task_id() => task_latest_work_start,
            Some(active) => data
                .worklogs
                .iter()
                .filter(|worklog| worklog.task_id() == active.task_id())
                .map(Worklog::start)
                .max(),
            None => None,
        };
        Ok(WorklogCorrection {
            worklog: corrected,
            task_latest_work_start,
            active_worklog,
            active_task_latest_work_start,
        })
    }

    fn compare_and_delete_completed_worklog(
        &self,
        id: WorklogId,
        expected_task_id: TaskId,
        expected: WorklogTimes,
    ) -> Result<WorklogDeletion, RepositoryError> {
        self.0.borrow_mut().deletion_writes += 1;
        if let Some(error) = self.take_write_failure() {
            return Err(error);
        }
        let mut data = self.0.borrow_mut();
        let index = data
            .worklogs
            .iter()
            .position(|worklog| worklog.id() == id)
            .ok_or(RepositoryError::WorklogNotFound { id })?;
        let stored = &data.worklogs[index];
        if stored.is_active() {
            return Err(RepositoryError::WorklogIsActive { id });
        }
        if stored.task_id() != expected_task_id || stored.times() != expected {
            return Err(RepositoryError::WorklogChanged { id });
        }
        let worklog = data.worklogs.remove(index);
        let task_latest_work_start = data
            .worklogs
            .iter()
            .filter(|candidate| candidate.task_id() == worklog.task_id())
            .map(Worklog::start)
            .max();
        Ok(WorklogDeletion {
            worklog,
            task_latest_work_start,
        })
    }

    fn worklog_page(
        &self,
        task_id: TaskId,
        after: Option<&WorklogCursor>,
    ) -> Result<WorklogPage, RepositoryError> {
        self.read_guard()?;
        let data = self.0.borrow();
        let revision = Self::revision(&data, task_id);
        if after.is_some_and(|cursor| cursor.task_id != task_id || cursor.revision != revision) {
            return Err(RepositoryError::WorklogHistoryChanged { task_id });
        }
        let active_worklog = data
            .worklogs
            .iter()
            .find(|worklog| worklog.is_active())
            .cloned();
        let requested_task_latest_work_start = data
            .worklogs
            .iter()
            .filter(|worklog| worklog.task_id() == task_id)
            .map(Worklog::start)
            .max();
        let active_task_latest_work_start = active_worklog.as_ref().and_then(|active| {
            if active.task_id() == task_id {
                requested_task_latest_work_start
            } else {
                data.worklogs
                    .iter()
                    .filter(|worklog| worklog.task_id() == active.task_id())
                    .map(Worklog::start)
                    .max()
            }
        });
        let mut worklogs = data
            .worklogs
            .iter()
            .filter(|worklog| worklog.task_id() == task_id)
            .cloned()
            .collect::<Vec<_>>();
        // History order: start descending, then identifier ascending.
        worklogs.sort_by_key(|worklog| (std::cmp::Reverse(worklog.start()), worklog.id()));
        let first = match after {
            None => 0,
            Some(cursor) => worklogs
                .iter()
                .position(|worklog| {
                    (std::cmp::Reverse(worklog.start()), worklog.id())
                        > (std::cmp::Reverse(cursor.start), cursor.id)
                })
                .unwrap_or(worklogs.len()),
        };
        // One worklog past the page size reveals whether a next page
        // follows; the extra worklog is dropped again before returning.
        let overshot = worklogs
            .into_iter()
            .skip(first)
            .take(WORKLOG_PAGE_SIZE + 1)
            .collect::<Vec<_>>();
        let has_next = overshot.len() > WORKLOG_PAGE_SIZE;
        let worklogs = overshot
            .into_iter()
            .take(WORKLOG_PAGE_SIZE)
            .collect::<Vec<_>>();
        let next_cursor = has_next.then(|| {
            let last = worklogs.last().expect("a full page has a last worklog");
            WorklogCursor {
                task_id,
                start: last.start(),
                id: last.id(),
                revision,
            }
        });
        Ok(WorklogPage {
            worklogs,
            snapshot: WorklogPageSnapshot {
                requested_task_latest_work_start,
                active_worklog,
                active_task_latest_work_start,
            },
            next_cursor,
        })
    }
}

impl TrackingRepository for MemoryRepository {
    fn insert_worklog(&self, worklog: &Worklog) -> Result<(), RepositoryError> {
        if let Some(error) = self.take_write_failure() {
            return Err(error);
        }
        let mut data = self.0.borrow_mut();
        let task = data
            .tasks
            .iter()
            .find(|item| item.task.id() == worklog.task_id())
            .ok_or(RepositoryError::TaskNotFound {
                id: worklog.task_id(),
            })?;
        if task.task.is_archived() {
            return Err(RepositoryError::TaskArchived {
                id: worklog.task_id(),
            });
        }
        if data
            .worklogs
            .iter()
            .any(|stored| stored.id() == worklog.id())
        {
            return Err(RepositoryError::WorklogAlreadyExists { id: worklog.id() });
        }
        if worklog.is_active() && data.worklogs.iter().any(Worklog::is_active) {
            return Err(RepositoryError::ActiveWorklogExists);
        }
        if has_same_task_overlap(&data.worklogs, worklog, None) {
            return Err(RepositoryError::SameTaskWorklogOverlap { id: worklog.id() });
        }
        data.worklogs.push(worklog.clone());
        Ok(())
    }

    fn stop_worklog(
        &self,
        id: WorklogId,
        expected_start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Worklog, RepositoryError> {
        if let Some(error) = self.take_write_failure() {
            return Err(error);
        }
        let mut data = self.0.borrow_mut();
        let index = data
            .worklogs
            .iter()
            .position(|worklog| worklog.id() == id)
            .ok_or(RepositoryError::WorklogNotFound { id })?;
        let stored = &data.worklogs[index];
        if !stored.is_active() {
            return Err(RepositoryError::WorklogAlreadyStopped { id });
        }
        if stored.start() != expected_start {
            return Err(RepositoryError::WorklogChanged { id });
        }
        let stopped = Worklog::new(stored.id(), stored.task_id(), stored.start(), Some(end))
            .map_err(|error| RepositoryError::Constraint {
                message: error.to_string(),
            })?;
        if has_same_task_overlap(&data.worklogs, &stopped, Some(id)) {
            return Err(RepositoryError::SameTaskWorklogOverlap { id });
        }
        data.worklogs[index] = stopped.clone();
        Ok(stopped)
    }

    fn switch_worklog(
        &self,
        id: WorklogId,
        expected_start: DateTime<Utc>,
        stop_at: DateTime<Utc>,
        next: &Worklog,
    ) -> Result<(), RepositoryError> {
        if let Some(error) = self.take_write_failure() {
            return Err(error);
        }
        let mut data = self.0.borrow_mut();
        let Some(index) = data.worklogs.iter().position(|worklog| worklog.id() == id) else {
            return Err(RepositoryError::WorklogNotFound { id });
        };
        if !data.worklogs[index].is_active() {
            return Err(RepositoryError::WorklogAlreadyStopped { id });
        }
        if data.worklogs[index].start() != expected_start {
            return Err(RepositoryError::WorklogChanged { id });
        }
        if !next.is_active() {
            return Err(RepositoryError::Constraint {
                message: "a switch replacement must be active".to_owned(),
            });
        }
        let task = data
            .tasks
            .iter()
            .find(|item| item.task.id() == next.task_id())
            .ok_or(RepositoryError::TaskNotFound { id: next.task_id() })?;
        if task.task.is_archived() {
            return Err(RepositoryError::TaskArchived { id: next.task_id() });
        }
        if data
            .worklogs
            .iter()
            .any(|worklog| worklog.id() == next.id())
        {
            return Err(RepositoryError::WorklogAlreadyExists { id: next.id() });
        }

        let active = &data.worklogs[index];
        let stopped = Worklog::new(active.id(), active.task_id(), active.start(), Some(stop_at))
            .map_err(|error| RepositoryError::Constraint {
                message: error.to_string(),
            })?;
        let mut staged = data.worklogs.clone();
        staged[index] = stopped;
        if has_same_task_overlap(&staged, next, None) {
            return Err(RepositoryError::SameTaskWorklogOverlap { id: next.id() });
        }
        staged.push(next.clone());
        data.worklogs = staged;
        Ok(())
    }
}

fn at(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 0).unwrap()
}

fn at_nanos(seconds: i64, nanos: u32) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, nanos).unwrap()
}

fn task(tag: u128, name: &str) -> Task {
    Task::create(
        TaskId::from_uuid(uuid::Uuid::from_u128(tag)),
        TaskName::new(name).unwrap(),
        at(100),
    )
}

fn stamped_task(tag: u128, name: &str, created: i64, updated: i64) -> Task {
    Task::rehydrate(
        TaskId::from_uuid(uuid::Uuid::from_u128(tag)),
        TaskName::new(name).unwrap(),
        false,
        at(created),
        at(updated),
    )
    .unwrap()
}

fn worklog(tag: u128, task_id: TaskId, start: i64) -> Worklog {
    Worklog::begin(
        WorklogId::from_uuid(uuid::Uuid::from_u128(tag)),
        task_id,
        at(start),
    )
}

fn completed_worklog(tag: u128, task_id: TaskId, start: i64, end: i64) -> Worklog {
    Worklog::new(
        WorklogId::from_uuid(uuid::Uuid::from_u128(tag)),
        task_id,
        at(start),
        Some(at(end)),
    )
    .unwrap()
}

fn ordered_names(
    application: &TrackerApplication<MemoryRepository>,
    ordering: TaskOrdering,
) -> Vec<String> {
    application
        .tasks(ordering)
        .into_iter()
        .map(|item| item.task.name().to_string())
        .collect()
}

mod error;
mod model;
mod reports;
mod tasks;
mod tracking;
mod worklogs;
