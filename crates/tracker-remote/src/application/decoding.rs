use super::{
    ActiveWorklog, ApplicationError, ClearActiveTaskOutcome, GlobalWorklogCursor, HashSet,
    HistoryRead, InactiveTaskPreviewDto, InactivityPeriod, MutationResultDto, RemoteError,
    ReportDto, SetActiveTaskOutcome, Task, TaskDto, TaskId, TaskListItem, TaskListStateDto,
    TaskName, TasksDto, Tracker, TrackingState, Worklog, WorklogDto, WorklogId, WorklogPageDto,
    canonical, protocol_failure,
};
use tracker_protocol::{WorklogCursorError, parse_worklog_cursor};

/// Returns the parsed candidate IDs in preview order.
pub(super) fn validate_inactive_preview(
    preview: &InactiveTaskPreviewDto,
) -> Result<Vec<TaskId>, ApplicationError> {
    if InactivityPeriod::new(preview.inactive_days).is_err()
        || preview.as_of != canonical(preview.as_of)
        || preview.count != preview.candidate_task_ids.len()
    {
        return Err(protocol_failure("invalid inactive task preview"));
    }
    let mut seen = HashSet::new();
    let mut ids = Vec::with_capacity(preview.candidate_task_ids.len());
    for raw_id in &preview.candidate_task_ids {
        let id = raw_id
            .parse::<TaskId>()
            .map_err(|_| protocol_failure("invalid inactive task preview"))?;
        if !seen.insert(id) {
            return Err(protocol_failure("invalid inactive task preview"));
        }
        ids.push(id);
    }
    Ok(ids)
}

pub(super) fn validate_report(dto: &ReportDto) -> Result<(), ApplicationError> {
    let mut ids = HashSet::new();
    let mut sum = 0_i64;
    if dto.end <= dto.start {
        return Err(protocol_failure("invalid report context"));
    }
    for row in &dto.rows {
        let id = row
            .task_id
            .parse::<TaskId>()
            .map_err(|_| protocol_failure("invalid report row"))?;
        if row.duration_us <= 0 || !ids.insert(id) {
            return Err(protocol_failure("invalid report row"));
        }
        sum = sum
            .checked_add(row.duration_us)
            .ok_or(ApplicationError::ReportDurationOverflow)?;
    }
    if sum != dto.total_us {
        return Err(protocol_failure("report total does not match rows"));
    }
    Ok(())
}

fn validate_active_task(
    items: &[TaskListItem],
    active: Option<&Worklog>,
) -> Result<(), RemoteError> {
    if let Some(active) = active
        && !items
            .iter()
            .any(|item| item.task.id() == active.task_id() && !item.task.is_archived())
    {
        return Err(RemoteError::Protocol(
            "active worklog has no active task".into(),
        ));
    }
    Ok(())
}

pub(super) fn decode_tasks(dto: TasksDto) -> Result<Vec<TaskListItem>, RemoteError> {
    let mut ids = HashSet::new();
    dto.tasks
        .into_iter()
        .map(|dto| {
            let item = decode_task_item(dto)?;
            if !ids.insert(item.task.id()) {
                return Err(RemoteError::Protocol("duplicate task".into()));
            }
            Ok(item)
        })
        .collect()
}

pub(super) fn decode_task_item(dto: TaskDto) -> Result<TaskListItem, RemoteError> {
    let latest_work_start = dto.latest_work_start;
    Ok(TaskListItem {
        task: decode_task(dto)?,
        latest_work_start,
    })
}

pub(super) fn decode_task(dto: TaskDto) -> Result<Task, RemoteError> {
    let id = dto
        .id
        .parse()
        .map_err(|_| RemoteError::Protocol("invalid task id".into()))?;
    let name =
        TaskName::new(&dto.name).map_err(|_| RemoteError::Protocol("invalid task name".into()))?;
    Task::rehydrate(id, name, dto.archived, dto.created_at, dto.updated_at)
        .map_err(|error| RemoteError::Protocol(error.to_string()))
}

fn decode_worklog(dto: WorklogDto) -> Result<Worklog, RemoteError> {
    let id = dto
        .id
        .parse()
        .map_err(|_| RemoteError::Protocol("invalid worklog id".into()))?;
    let task_id = dto
        .task_id
        .parse()
        .map_err(|_| RemoteError::Protocol("invalid worklog task id".into()))?;
    Worklog::new(id, task_id, canonical(dto.start), dto.end.map(canonical))
        .map_err(|error| RemoteError::Protocol(error.to_string()))
}

pub(super) fn decode_active(
    dto: Option<WorklogDto>,
) -> Result<(Option<Worklog>, TrackingState), RemoteError> {
    let worklog = dto.map(decode_worklog).transpose()?;
    let tracking = match worklog.clone() {
        Some(worklog) => Tracker::resume(worklog)
            .map_err(|error| RemoteError::Protocol(error.to_string()))?
            .state()
            .clone(),
        None => TrackingState::Idle,
    };
    Ok((worklog, tracking))
}

pub(super) fn decode_history(
    dto: &WorklogPageDto,
    task_id: Option<TaskId>,
) -> Result<HistoryRead, RemoteError> {
    let worklogs = dto
        .worklogs
        .iter()
        .cloned()
        .map(decode_worklog)
        .collect::<Result<Vec<_>, _>>()?;
    if task_id.is_some_and(|id| worklogs.iter().any(|worklog| worklog.task_id() != id)) {
        return Err(RemoteError::Protocol(
            "history contains another task".into(),
        ));
    }
    let next_cursor = dto
        .next_cursor
        .as_ref()
        .map(
            |cursor| match parse_worklog_cursor(&cursor.id, cursor.revision) {
                Ok((id, revision)) => Ok(GlobalWorklogCursor { id, revision }),
                Err(WorklogCursorError::InvalidId) => {
                    Err(RemoteError::Protocol("invalid cursor worklog id".into()))
                }
                Err(WorklogCursorError::NegativeRevision) => Err(RemoteError::Protocol(
                    "invalid history cursor anchor".into(),
                )),
            },
        )
        .transpose()?;
    if next_cursor.is_some_and(|cursor| worklogs.last().is_none_or(|last| last.id() != cursor.id)) {
        return Err(RemoteError::Protocol(
            "invalid history cursor anchor".into(),
        ));
    }
    Ok(HistoryRead {
        worklogs,
        next_cursor,
    })
}

fn app_decode_task(dto: TaskDto) -> Result<Task, ApplicationError> {
    decode_task(dto).map_err(|error| protocol_failure(&error.to_string()))
}

fn app_decode_worklog(dto: WorklogDto) -> Result<Worklog, ApplicationError> {
    decode_worklog(dto).map_err(|error| protocol_failure(&error.to_string()))
}

pub(super) fn task_result(
    result: MutationResultDto,
    expected_id: TaskId,
) -> Result<Task, ApplicationError> {
    match result {
        MutationResultDto::Task(task) => {
            let task = app_decode_task(task)?;
            if task.id() != expected_id {
                return Err(protocol_failure("task result has a different id"));
            }
            Ok(task)
        }
        _ => Err(protocol_failure("wrong task result")),
    }
}

pub(super) fn worklog_result(
    result: MutationResultDto,
    expected_id: WorklogId,
) -> Result<Worklog, ApplicationError> {
    match result {
        MutationResultDto::Worklog(worklog) => {
            let worklog = app_decode_worklog(worklog)?;
            if worklog.id() != expected_id {
                return Err(protocol_failure("worklog result has a different id"));
            }
            Ok(worklog)
        }
        _ => Err(protocol_failure("wrong worklog result")),
    }
}

pub(super) fn set_tracking_result(
    result: MutationResultDto,
    task_id: TaskId,
    new_id: WorklogId,
    old_active: Option<WorklogId>,
) -> Result<SetActiveTaskOutcome, ApplicationError> {
    match result {
        MutationResultDto::Worklog(dto) => {
            let worklog = app_decode_worklog(dto)?;
            started_tracking_result(worklog, task_id, new_id, old_active)
        }
        MutationResultDto::TrackingSwitched { stopped, started } => {
            let stopped = app_decode_worklog(stopped)?;
            let started = app_decode_worklog(started)?;
            switched_tracking_result(stopped, started, task_id, new_id, old_active)
        }
        MutationResultDto::TrackingAlreadyActive(dto) => {
            let worklog = app_decode_worklog(dto)?;
            if !worklog.is_active() || worklog.task_id() != task_id {
                return Err(protocol_failure("invalid already-active worklog"));
            }
            Ok(SetActiveTaskOutcome::AlreadyActive {
                worklog: ActiveWorklog::begin(worklog.id(), worklog.task_id(), worklog.start()),
            })
        }
        _ => Err(protocol_failure("wrong tracking result")),
    }
}

fn started_tracking_result(
    worklog: Worklog,
    task_id: TaskId,
    new_id: WorklogId,
    old_active: Option<WorklogId>,
) -> Result<SetActiveTaskOutcome, ApplicationError> {
    if old_active.is_some()
        || worklog.id() != new_id
        || worklog.task_id() != task_id
        || !worklog.is_active()
    {
        return Err(protocol_failure("invalid started worklog"));
    }
    Ok(SetActiveTaskOutcome::Started { worklog })
}

fn switched_tracking_result(
    stopped: Worklog,
    started: Worklog,
    task_id: TaskId,
    new_id: WorklogId,
    old_active: Option<WorklogId>,
) -> Result<SetActiveTaskOutcome, ApplicationError> {
    if old_active != Some(stopped.id())
        || stopped.is_active()
        || started.id() != new_id
        || started.task_id() != task_id
        || !started.is_active()
    {
        return Err(protocol_failure("invalid switched worklogs"));
    }
    Ok(SetActiveTaskOutcome::Switched { stopped, started })
}

pub(super) fn clear_tracking_result(
    result: MutationResultDto,
    expected_active: WorklogId,
) -> Result<ClearActiveTaskOutcome, ApplicationError> {
    match result {
        MutationResultDto::Worklog(dto) => {
            let worklog = app_decode_worklog(dto)?;
            if worklog.id() != expected_active || worklog.is_active() {
                return Err(protocol_failure("invalid stopped worklog"));
            }
            Ok(ClearActiveTaskOutcome::Stopped { worklog })
        }
        MutationResultDto::TrackingAlreadyIdle => Ok(ClearActiveTaskOutcome::AlreadyIdle),
        _ => Err(protocol_failure("wrong tracking result")),
    }
}

pub(super) fn decode_task_list_state(
    dto: TaskListStateDto,
) -> Result<(Vec<TaskListItem>, TrackingState), RemoteError> {
    let items = decode_tasks(TasksDto { tasks: dto.tasks })?;
    let (active, state) = decode_active(dto.active_worklog)?;
    validate_active_task(&items, active.as_ref())?;
    Ok((items, state))
}
