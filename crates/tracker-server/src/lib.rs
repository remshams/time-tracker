//! HTTP server for the versioned remote tracker protocol.

use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::fs::{File, OpenOptions};
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, Path as RoutePath, Query, State};
use axum::http::{Request, StatusCode, header};
use axum::middleware::{Next, from_fn};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use tracker_application::{
    ApplicationError, ApplicationFailureCategory, ClearActiveTaskOutcome, GlobalWorklogCursor,
    InactiveTaskOperations, ReportQueries, SetActiveTaskOutcome, TaskOperations, TaskOrdering,
    TaskQueries, TrackerApplication, TrackingOperations, WorklogCursor, WorklogOperations,
    WorklogQueries,
};
use tracker_domain::{
    InactivityPeriod, Task, TaskId, TaskName, TrackingState, WorklogId, WorklogTimes,
};
use tracker_protocol::{
    ArchiveInactiveCandidatesRequest, ArchiveInactiveTasksRequest, CreateTaskRequest,
    DeleteWorklogRequest, ErrorCode, ErrorDto, GlobalWorklogPageDto, HealthDto,
    InactiveTaskCandidatesDto, InactiveTaskPreviewDto, MutationDto, MutationResultDto, ReportDto,
    SetTrackingRequest, SnapshotDto, TaskChangeRequest, TaskDto, WorklogChangeRequest, WorklogDto,
    WorklogPageDto, WriteGuard, parse_task_id, parse_worklog_id,
};
use tracker_storage::SqliteRepository;
use uuid::Uuid;

const BODY_LIMIT: usize = 16 * 1024;
const IDEMPOTENCY_CACHE_SIZE: usize = 1024;
type Shared = Arc<Mutex<Core>>;
type ApiResult<T> = Result<Json<T>, ApiError>;

struct Core {
    _database_lock: File,
    app: TrackerApplication<SqliteRepository>,
    needs_refresh: bool,
    epoch: String,
    sequence: u64,
    completed: HashMap<String, (String, MutationResultDto)>,
    completion_order: VecDeque<String>,
}

impl Core {
    fn revision(&self) -> String {
        format!("{}:{}", self.epoch, self.sequence)
    }

    fn snapshot(&self) -> SnapshotDto {
        let active_worklog = match self.app.current_tracking() {
            TrackingState::Idle => None,
            TrackingState::Running { worklog } => Some(WorklogDto::from(&worklog.to_worklog())),
        };
        SnapshotDto {
            task_items: self
                .app
                .tasks(TaskOrdering::RecentlyCreated)
                .iter()
                .map(Into::into)
                .collect(),
            active_worklog,
            revision: self.revision(),
        }
    }

    fn execute(
        &mut self,
        guard: &WriteGuard,
        fingerprint: String,
        operation: impl FnOnce(
            &mut TrackerApplication<SqliteRepository>,
        ) -> Result<MutationResultDto, ApiError>,
    ) -> Result<MutationDto, ApiError> {
        if Uuid::parse_str(&guard.request_id).is_err() {
            return Err(ApiError::invalid("Invalid request ID"));
        }
        if let Some((old_fingerprint, result)) = self.completed.get(&guard.request_id) {
            return if old_fingerprint == &fingerprint {
                Ok(MutationDto {
                    result: result.clone(),
                    snapshot: self.snapshot(),
                })
            } else {
                Err(ApiError::conflict(
                    "Request ID was reused for a different command",
                ))
            };
        }
        if guard.expected_revision != self.revision() {
            return Err(ApiError::stale());
        }
        let result = match operation(&mut self.app) {
            Ok(result) => result,
            Err(error) => {
                if error.status.is_server_error() {
                    // A command may commit before its follow-up read fails.
                    // The old revision must never authorize another write.
                    self.sequence = self.sequence.saturating_add(1);
                    self.needs_refresh = self.app.refresh_authoritative_state().is_err();
                }
                return Err(error);
            }
        };
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(ApiError::internal)?;
        let response = MutationDto {
            result: result.clone(),
            snapshot: self.snapshot(),
        };
        self.completed
            .insert(guard.request_id.clone(), (fingerprint, result));
        self.completion_order.push_back(guard.request_id.clone());
        if self.completion_order.len() > IDEMPOTENCY_CACHE_SIZE
            && let Some(evicted) = self.completion_order.pop_front()
        {
            self.completed.remove(&evicted);
        }
        Ok(response)
    }
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    code: ErrorCode,
    message: String,
}

impl ApiError {
    fn invalid(message: &str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: ErrorCode::InvalidRequest,
            message: message.to_owned(),
        }
    }

    fn conflict(message: &str) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: ErrorCode::Conflict,
            message: message.to_owned(),
        }
    }

    fn stale() -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: ErrorCode::StaleRevision,
            message: "Tracker state changed. Refresh and retry.".to_owned(),
        }
    }

    fn internal() -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: ErrorCode::Internal,
            message: "Server could not complete the request".to_owned(),
        }
    }
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (
            self.status,
            Json(ErrorDto {
                code: self.code,
                message: self.message,
            }),
        )
            .into_response()
    }
}

impl From<ApplicationError> for ApiError {
    fn from(error: ApplicationError) -> Self {
        let failure = error.failure();
        if matches!(error, ApplicationError::TrackingStateChanged) {
            return Self::conflict(failure.message());
        }
        if failure.recovery_failed()
            || failure.source() != tracker_application::ApplicationFailureSource::Operation
        {
            return Self::internal();
        }
        let (status, code) = failure_status(failure.category());
        Self {
            status,
            code,
            message: failure.message().to_owned(),
        }
    }
}

fn failure_status(category: ApplicationFailureCategory) -> (StatusCode, ErrorCode) {
    match category {
        ApplicationFailureCategory::WorklogNotFound | ApplicationFailureCategory::TaskNotFound => {
            (StatusCode::NOT_FOUND, ErrorCode::NotFound)
        }
        ApplicationFailureCategory::WorklogChanged => {
            (StatusCode::CONFLICT, ErrorCode::WorklogChanged)
        }
        ApplicationFailureCategory::WorklogHistoryChanged => {
            (StatusCode::CONFLICT, ErrorCode::WorklogHistoryChanged)
        }
        ApplicationFailureCategory::WorklogOverlap => {
            (StatusCode::CONFLICT, ErrorCode::WorklogOverlap)
        }
        ApplicationFailureCategory::ActiveWorklog => {
            (StatusCode::CONFLICT, ErrorCode::ActiveWorklog)
        }
        ApplicationFailureCategory::ActiveTask => (StatusCode::CONFLICT, ErrorCode::ActiveTask),
        ApplicationFailureCategory::InactiveTaskCandidatesChanged => (
            StatusCode::CONFLICT,
            ErrorCode::InactiveTaskCandidatesChanged,
        ),
        ApplicationFailureCategory::General => (StatusCode::BAD_REQUEST, ErrorCode::InvalidRequest),
    }
}

fn lock(shared: &Shared) -> Result<std::sync::MutexGuard<'_, Core>, ApiError> {
    let mut core = shared.lock().map_err(|_| ApiError::internal())?;
    if core.needs_refresh {
        core.app
            .refresh_authoritative_state()
            .map_err(|_| ApiError::internal())?;
        core.needs_refresh = false;
    }
    Ok(core)
}

fn task_id(raw: &str) -> Result<TaskId, ApiError> {
    parse_task_id(raw).map_err(ApiError::invalid)
}

fn worklog_id(raw: &str) -> Result<WorklogId, ApiError> {
    parse_worklog_id(raw).map_err(ApiError::invalid)
}

fn task_name(raw: &str) -> Result<TaskName, ApiError> {
    TaskName::new(raw).map_err(|error| ApiError::invalid(&error.to_string()))
}

fn fingerprint(route: &str, request: &impl Serialize) -> Result<String, ApiError> {
    serde_json::to_string(request)
        .map(|payload| format!("{route}:{payload}"))
        .map_err(|_| ApiError::internal())
}

async fn health() -> Json<HealthDto> {
    Json(HealthDto {
        status: "ok".to_owned(),
        protocol_version: tracker_protocol::VERSION,
    })
}

async fn snapshot(State(shared): State<Shared>) -> ApiResult<SnapshotDto> {
    Ok(Json(lock(&shared)?.snapshot()))
}

const INACTIVE_PREVIEW_SAMPLE_SIZE: usize = 5;
const INACTIVE_AS_OF_TOLERANCE: TimeDelta = TimeDelta::minutes(15);

#[derive(Deserialize)]
struct InactivePreviewQuery {
    as_of: DateTime<Utc>,
}

fn validate_inactive_as_of(as_of: DateTime<Utc>, now: DateTime<Utc>) -> Result<(), ApiError> {
    let difference = as_of.signed_duration_since(now);
    if difference < -INACTIVE_AS_OF_TOLERANCE || difference > INACTIVE_AS_OF_TOLERANCE {
        Err(ApiError::invalid(
            "Inactive task preview time is outside the allowed range",
        ))
    } else {
        Ok(())
    }
}

async fn preview_inactive_tasks(
    State(shared): State<Shared>,
    Query(query): Query<InactivePreviewQuery>,
) -> ApiResult<InactiveTaskPreviewDto> {
    validate_inactive_as_of(query.as_of, Utc::now())?;
    let mut core = lock(&shared)?;
    let tasks = core.app.preview_inactive_tasks(query.as_of)?;
    Ok(Json(InactiveTaskPreviewDto {
        as_of: query.as_of,
        count: tasks.len(),
        sample_names: tasks
            .iter()
            .take(INACTIVE_PREVIEW_SAMPLE_SIZE)
            .map(|task| task.name().as_str().to_owned())
            .collect(),
        revision: core.revision(),
    }))
}

async fn archive_inactive_tasks(
    State(shared): State<Shared>,
    input: Result<Json<ArchiveInactiveTasksRequest>, JsonRejection>,
) -> ApiResult<MutationDto> {
    let request = payload(input)?;
    let key = fingerprint("POST /v1/tasks/archive-inactive", &request)?;
    let mut core = lock(&shared)?;
    let response = core.execute(&request.guard, key, |app| {
        validate_inactive_as_of(request.as_of, Utc::now())?;
        let candidates = app.preview_inactive_tasks(request.as_of)?;
        let ids: Vec<TaskId> = candidates.iter().map(Task::id).collect();
        let archived = app.archive_inactive_tasks(&ids, request.as_of)?;
        Ok(MutationResultDto::ArchivedInactive {
            count: archived.len(),
        })
    })?;
    Ok(Json(response))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InactiveCandidatesQuery {
    as_of: DateTime<Utc>,
    inactive_days: u32,
}

async fn inactive_candidates(
    State(shared): State<Shared>,
    Query(query): Query<InactiveCandidatesQuery>,
) -> ApiResult<InactiveTaskCandidatesDto> {
    validate_inactive_as_of(query.as_of, Utc::now())?;
    let period = InactivityPeriod::new(query.inactive_days)
        .map_err(|error| ApiError::invalid(&error.to_string()))?;
    let mut core = lock(&shared)?;
    let tasks = core
        .app
        .preview_inactive_tasks_with_period(query.as_of, period)?;
    Ok(Json(InactiveTaskCandidatesDto {
        as_of: query.as_of,
        inactive_days: period.days(),
        tasks: tasks.iter().map(TaskDto::from).collect(),
        revision: core.revision(),
    }))
}

async fn archive_inactive_candidates(
    State(shared): State<Shared>,
    input: Result<Json<ArchiveInactiveCandidatesRequest>, JsonRejection>,
) -> ApiResult<MutationDto> {
    let request = payload(input)?;
    let key = fingerprint("POST /v1/tasks/archive-inactive-candidates", &request)?;
    let mut core = lock(&shared)?;
    let response = core.execute(&request.guard, key, |app| {
        validate_inactive_as_of(request.as_of, Utc::now())?;
        let period = InactivityPeriod::new(request.inactive_days)
            .map_err(|error| ApiError::invalid(&error.to_string()))?;
        let candidates = app.preview_inactive_tasks_with_period(request.as_of, period)?;
        let ids: Vec<TaskId> = candidates.iter().map(Task::id).collect();
        let archived = app.archive_inactive_tasks_with_period(&ids, request.as_of, period)?;
        Ok(MutationResultDto::ArchivedInactive {
            count: archived.len(),
        })
    })?;
    Ok(Json(response))
}

fn payload<T>(value: Result<Json<T>, JsonRejection>) -> Result<T, ApiError> {
    value.map(|Json(value)| value).map_err(|rejection| {
        let mut error = ApiError::invalid("Invalid JSON request");
        if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
            error.status = StatusCode::PAYLOAD_TOO_LARGE;
        }
        error
    })
}

async fn create_task(
    State(shared): State<Shared>,
    input: Result<Json<CreateTaskRequest>, JsonRejection>,
) -> ApiResult<MutationDto> {
    let request = payload(input)?;
    let id = task_id(&request.task_id)?;
    let name = task_name(&request.name)?;
    let key = fingerprint("POST /v1/tasks", &request)?;
    let mut core = lock(&shared)?;
    let response = core.execute(&request.guard, key, |app| {
        app.create_task_with_id(id, name, request.occurred_at)
            .map(|task| MutationResultDto::Task(TaskDto::from(&task)))
            .map_err(Into::into)
    })?;
    Ok(Json(response))
}

async fn change_task(
    State(shared): State<Shared>,
    RoutePath(raw_id): RoutePath<String>,
    input: Result<Json<TaskChangeRequest>, JsonRejection>,
) -> ApiResult<MutationDto> {
    let request = payload(input)?;
    let id = task_id(&raw_id)?;
    let key = fingerprint(&format!("PATCH /v1/tasks/{raw_id}"), &request)?;
    let guard = request.guard().clone();
    let mut core = lock(&shared)?;
    let response = core.execute(&guard, key, |app| {
        let task = match request {
            TaskChangeRequest::Rename {
                name, occurred_at, ..
            } => app.rename_task(id, task_name(&name)?, occurred_at)?,
            TaskChangeRequest::Archive { occurred_at, .. } => app.archive_task(id, occurred_at)?,
            TaskChangeRequest::Restore { occurred_at, .. } => {
                app.unarchive_task(id, occurred_at)?
            }
        };
        Ok(MutationResultDto::Task(TaskDto::from(&task)))
    })?;
    Ok(Json(response))
}

async fn set_tracking(
    State(shared): State<Shared>,
    input: Result<Json<SetTrackingRequest>, JsonRejection>,
) -> ApiResult<MutationDto> {
    let request = payload(input)?;
    let desired = request.task_id.as_deref().map(task_id).transpose()?;
    let new_id = request.worklog_id.as_deref().map(worklog_id).transpose()?;
    let expected = request
        .expected_active
        .as_deref()
        .map(worklog_id)
        .transpose()?;
    if desired.is_some() != new_id.is_some() {
        return Err(ApiError::invalid(
            "A worklog ID is required when setting an active task",
        ));
    }
    let key = fingerprint("PUT /v1/tracking", &request)?;
    let mut core = lock(&shared)?;
    let response = core.execute(&request.guard, key, |app| {
        let current = match app.current_tracking() {
            TrackingState::Idle => None,
            TrackingState::Running { worklog } => Some(worklog.id()),
        };
        if current != expected {
            return Err(ApiError::conflict(
                "Active worklog changed. Refresh and retry.",
            ));
        }
        match (desired, new_id, current) {
            (Some(task_id), Some(new_id), _) => {
                let outcome = match current {
                    None => {
                        app.start_tracking_if_idle_with_id(task_id, new_id, request.occurred_at)
                    }
                    Some(_) => app.set_active_task_with_id(task_id, new_id, request.occurred_at),
                }?;
                match outcome {
                    SetActiveTaskOutcome::Started { worklog } => {
                        Ok(MutationResultDto::Worklog(WorklogDto::from(&worklog)))
                    }
                    SetActiveTaskOutcome::Switched { stopped, started } => {
                        Ok(MutationResultDto::TrackingSwitched {
                            stopped: WorklogDto::from(&stopped),
                            started: WorklogDto::from(&started),
                        })
                    }
                    SetActiveTaskOutcome::AlreadyActive { worklog } => {
                        Ok(MutationResultDto::TrackingAlreadyActive(WorklogDto::from(
                            &worklog.to_worklog(),
                        )))
                    }
                }
            }
            (None, None, Some(active)) => match app
                .clear_active_task(active, request.occurred_at)?
            {
                ClearActiveTaskOutcome::Stopped { worklog } => {
                    Ok(MutationResultDto::Worklog(WorklogDto::from(&worklog)))
                }
                ClearActiveTaskOutcome::AlreadyIdle => Ok(MutationResultDto::TrackingAlreadyIdle),
            },
            (None, None, None) => Ok(MutationResultDto::TrackingAlreadyIdle),
            _ => Err(ApiError::invalid("Invalid tracking request")),
        }
    })?;
    Ok(Json(response))
}

#[derive(Deserialize)]
struct PageQuery {
    after_start: Option<DateTime<Utc>>,
    after_id: Option<String>,
    after_revision: Option<i64>,
}

fn page_cursor(task_id: TaskId, query: PageQuery) -> Result<Option<WorklogCursor>, ApiError> {
    match (query.after_start, query.after_id, query.after_revision) {
        (None, None, None) => Ok(None),
        (Some(start), Some(id), Some(revision)) if revision >= 0 => Ok(Some(WorklogCursor {
            task_id,
            start,
            id: worklog_id(&id)?,
            revision,
        })),
        _ => Err(ApiError::invalid("Incomplete worklog cursor")),
    }
}

async fn task_worklogs(
    State(shared): State<Shared>,
    RoutePath(raw_id): RoutePath<String>,
    Query(query): Query<PageQuery>,
) -> ApiResult<WorklogPageDto> {
    let id = task_id(&raw_id)?;
    let cursor = page_cursor(id, query)?;
    let mut core = lock(&shared)?;
    let page = core.app.worklogs_for_task(id, cursor.as_ref())?;
    Ok(Json(WorklogPageDto::from_page(&page, core.revision())))
}

async fn all_worklogs(
    State(shared): State<Shared>,
    Query(query): Query<PageQuery>,
) -> ApiResult<GlobalWorklogPageDto> {
    let cursor = match (query.after_start, query.after_id, query.after_revision) {
        (None, None, None) => None,
        (Some(start), Some(id), Some(revision)) if revision >= 0 => Some(GlobalWorklogCursor {
            start,
            id: worklog_id(&id)?,
            revision,
        }),
        _ => return Err(ApiError::invalid("Incomplete worklog cursor")),
    };
    let mut core = lock(&shared)?;
    let page = core.app.all_worklogs(cursor.as_ref())?;
    Ok(Json(GlobalWorklogPageDto::from_page(
        &page,
        core.revision(),
    )))
}

async fn change_worklog(
    State(shared): State<Shared>,
    RoutePath(raw_id): RoutePath<String>,
    input: Result<Json<WorklogChangeRequest>, JsonRejection>,
) -> ApiResult<MutationDto> {
    let request = payload(input)?;
    let id = worklog_id(&raw_id)?;
    let key = fingerprint(&format!("PATCH /v1/worklogs/{raw_id}"), &request)?;
    let guard = request.guard().clone();
    let mut core = lock(&shared)?;
    let response = core.execute(&guard, key, |app| {
        let worklog = match request {
            WorklogChangeRequest::Move {
                expected_task_id,
                expected_start,
                expected_end,
                destination_task_id,
                ..
            } => app.move_worklog(
                id,
                task_id(&expected_task_id)?,
                WorklogTimes::new(expected_start, expected_end),
                task_id(&destination_task_id)?,
            )?,
            WorklogChangeRequest::Correct {
                expected_start,
                expected_end,
                replacement_start,
                replacement_end,
                occurred_at,
                ..
            } => app.correct_worklog(
                id,
                WorklogTimes::new(expected_start, expected_end),
                WorklogTimes::new(replacement_start, replacement_end),
                occurred_at,
            )?,
        };
        Ok(MutationResultDto::Worklog(WorklogDto::from(&worklog)))
    })?;
    Ok(Json(response))
}

async fn delete_worklog(
    State(shared): State<Shared>,
    RoutePath(raw_id): RoutePath<String>,
    input: Result<Json<DeleteWorklogRequest>, JsonRejection>,
) -> ApiResult<MutationDto> {
    let request = payload(input)?;
    let id = worklog_id(&raw_id)?;
    let key = fingerprint(&format!("DELETE /v1/worklogs/{raw_id}"), &request)?;
    let mut core = lock(&shared)?;
    let response = core.execute(&request.guard, key, |app| {
        app.delete_completed_worklog(
            id,
            task_id(&request.expected_task_id)?,
            WorklogTimes::new(request.expected_start, Some(request.expected_end)),
        )
        .map(|worklog| MutationResultDto::Worklog(WorklogDto::from(&worklog)))
        .map_err(Into::into)
    })?;
    Ok(Json(response))
}

#[derive(Deserialize)]
struct ReportQuery {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    now: DateTime<Utc>,
}

async fn report(
    State(shared): State<Shared>,
    Query(query): Query<ReportQuery>,
) -> ApiResult<ReportDto> {
    let mut core = lock(&shared)?;
    let totals = core.app.report_totals(query.start, query.end, query.now)?;
    Ok(Json(ReportDto::from_totals(&totals, core.snapshot())))
}

/// Creates a router backed by one SQLite database. Each handler serializes access
/// to the application service, including reads that refresh its snapshot.
pub fn router_for_database(path: &Path) -> Result<Router, Box<dyn Error + Send + Sync>> {
    let database_lock = lock_database(path)?;
    let repository = SqliteRepository::open(path)?;
    let app = TrackerApplication::load(repository)?;
    let core = Core {
        _database_lock: database_lock,
        app,
        needs_refresh: false,
        epoch: Uuid::now_v7().to_string(),
        sequence: 0,
        completed: HashMap::new(),
        completion_order: VecDeque::new(),
    };
    let shared = Arc::new(Mutex::new(core));
    Ok(Router::new()
        .route("/v1/health", get(health))
        .route("/v1/snapshot", get(snapshot))
        .route("/v1/tasks", post(create_task))
        .route("/v1/tasks/inactive-candidates", get(inactive_candidates))
        .route(
            "/v1/tasks/archive-inactive-candidates",
            post(archive_inactive_candidates),
        )
        .route("/v1/tasks/inactive-preview", get(preview_inactive_tasks))
        .route("/v1/tasks/archive-inactive", post(archive_inactive_tasks))
        .route("/v1/tasks/{id}", axum::routing::patch(change_task))
        .route("/v1/tracking", axum::routing::put(set_tracking))
        .route("/v1/tasks/{id}/worklogs", get(task_worklogs))
        .route("/v1/worklogs", get(all_worklogs))
        .route(
            "/v1/worklogs/{id}",
            axum::routing::patch(change_worklog).delete(delete_worklog),
        )
        .route("/v1/reports", get(report))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .with_state(shared))
}

fn lock_database(path: &Path) -> Result<File, Box<dyn Error + Send + Sync>> {
    let path = std::path::absolute(path)?;
    let parent = path
        .parent()
        .ok_or("database path has no parent directory")?;
    let name = path.file_name().ok_or("database path has no file name")?;
    let mut lock_name = name.to_os_string();
    lock_name.push(".lock");
    let lock_path = std::fs::canonicalize(parent)?.join(lock_name);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(lock_path)?;
    let metadata = file.metadata()?;
    // SAFETY: geteuid has no arguments and cannot fail.
    if !valid_lock_owner(metadata.is_file(), metadata.uid(), unsafe {
        libc::geteuid()
    }) {
        return Err("database lock file must be a regular file owned by the current user".into());
    }
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    // These disjoint flock flags add up to an exclusive, nonblocking lock.
    let flags = libc::LOCK_EX + libc::LOCK_NB;
    // SAFETY: the file remains open while flock uses its valid descriptor.
    if unsafe { libc::flock(file.as_raw_fd(), flags) } != 0 {
        return Err("database is already served by another process".into());
    }
    Ok(file)
}

fn valid_lock_owner(is_file: bool, owner_uid: u32, process_uid: u32) -> bool {
    is_file && owner_uid == process_uid
}

/// Serves the remote tracker until the process receives a shutdown signal.
pub fn run(bind: SocketAddr, database_path: PathBuf) -> Result<(), Box<dyn Error + Send + Sync>> {
    validate_bind_address(bind, |address| {
        tailscale_address_matches(address, |family| {
            Command::new("tailscale").args(["ip", family]).output()
        })
    })
    .map_err(|message| -> Box<dyn Error + Send + Sync> { message.into() })?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let router = router_for_database(&database_path)?.layer(from_fn(move |request, next| {
            reject_unexpected_host(request, next, bind)
        }));
        let listener = tokio::net::TcpListener::bind(bind).await?;
        axum::serve(listener, router).await?;
        Ok(())
    })
}

fn validate_bind_address(
    bind: SocketAddr,
    assigned_to_tailscale: impl FnOnce(IpAddr) -> bool,
) -> Result<(), &'static str> {
    if !allowed_bind_address(bind.ip()) {
        return Err("bind must name one localhost or Tailscale address");
    }
    if !bind.ip().is_loopback() && !assigned_to_tailscale(bind.ip()) {
        return Err("bind address is not assigned to this device by Tailscale");
    }
    Ok(())
}

fn tailscale_address_matches(
    address: IpAddr,
    lookup: impl FnOnce(&str) -> io::Result<Output>,
) -> bool {
    let family = if address.is_ipv4() { "-4" } else { "-6" };
    let Ok(output) = lookup(family) else {
        return false;
    };
    output.status.success()
        && std::str::from_utf8(&output.stdout)
            .ok()
            .is_some_and(|text| {
                text.lines()
                    .any(|line| line.trim().parse::<IpAddr>() == Ok(address))
            })
}

/// Accepted interface addresses for the unauthenticated first release.
pub fn allowed_bind_address(address: IpAddr) -> bool {
    if address.is_loopback() {
        return true;
    }
    match address {
        IpAddr::V4(ip) => {
            let octets = ip.octets();
            octets[0] == 100 && (64..=127).contains(&octets[1])
        }
        IpAddr::V6(ip) => ip.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
    }
}

async fn reject_unexpected_host(
    request: Request<axum::body::Body>,
    next: Next,
    bind: SocketAddr,
) -> Response {
    let allowed = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|host| host_allowed(host, bind));
    if allowed {
        next.run(request).await
    } else {
        StatusCode::FORBIDDEN.into_response()
    }
}

fn host_allowed(host: &str, bind: SocketAddr) -> bool {
    let address = bind.ip().to_string();
    let address = if bind.is_ipv6() {
        format!("[{address}]")
    } else {
        address
    };
    host == address
        || host == format!("{address}:{}", bind.port())
        || bind.ip().is_loopback()
            && (host.eq_ignore_ascii_case("localhost")
                || host.eq_ignore_ascii_case(&format!("localhost:{}", bind.port())))
}

#[cfg(test)]
mod tracking_tests {
    use super::*;

    fn core(path: &Path) -> Shared {
        let database_lock = lock_database(path).unwrap();
        let repository = SqliteRepository::open(path).unwrap();
        let mut app = TrackerApplication::load(repository).unwrap();
        app.create_task(TaskName::new("Write release notes").unwrap(), at(1))
            .unwrap();
        app.create_task(TaskName::new("Review changes").unwrap(), at(2))
            .unwrap();
        Arc::new(Mutex::new(Core {
            _database_lock: database_lock,
            app,
            needs_refresh: false,
            epoch: "tracking-test".to_owned(),
            sequence: 0,
            completed: HashMap::new(),
            completion_order: VecDeque::new(),
        }))
    }

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).unwrap()
    }

    fn request(
        shared: &Shared,
        task_id: TaskId,
        worklog_id: WorklogId,
        expected: Option<WorklogId>,
        seconds: i64,
    ) -> SetTrackingRequest {
        SetTrackingRequest {
            task_id: Some(task_id.to_string()),
            worklog_id: Some(worklog_id.to_string()),
            expected_active: expected.map(|id| id.to_string()),
            occurred_at: at(seconds),
            guard: WriteGuard {
                expected_revision: shared.lock().unwrap().revision(),
                request_id: Uuid::now_v7().to_string(),
            },
        }
    }

    #[tokio::test]
    async fn idle_request_preserves_a_timer_started_directly_in_the_database() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tracker.db");
        let shared = core(&path);
        let tasks = shared.lock().unwrap().app.tasks(TaskOrdering::default());
        let mut other = TrackerApplication::load(SqliteRepository::open(&path).unwrap()).unwrap();
        other.set_active_task(tasks[1].task.id(), at(100)).unwrap();
        let foreign = other.current_tracking().clone();
        let new_id = WorklogId::generate();
        let request = request(&shared, tasks[0].task.id(), new_id, None, 200);
        let rejected = set_tracking(State(shared.clone()), Ok(Json(request)))
            .await
            .unwrap_err();
        assert_eq!(rejected.status, StatusCode::CONFLICT);
        assert_eq!(shared.lock().unwrap().app.current_tracking(), &foreign);
        let repository = SqliteRepository::open(&path).unwrap();
        assert!(repository.find_worklog(new_id).unwrap().is_none());
        assert_eq!(
            repository.active_worklog().unwrap().unwrap().task_id(),
            tasks[1].task.id()
        );
    }

    #[tokio::test]
    async fn conditional_idle_start_keeps_the_id_and_explicit_switch_remains_supported() {
        let directory = tempfile::tempdir().unwrap();
        let shared = core(&directory.path().join("tracker.db"));
        let tasks = shared.lock().unwrap().app.tasks(TaskOrdering::default());
        let first_id = WorklogId::generate();
        let first_request = request(&shared, tasks[0].task.id(), first_id, None, 100);
        let first = set_tracking(State(shared.clone()), Ok(Json(first_request.clone())))
            .await
            .unwrap()
            .0;
        assert_eq!(
            first.snapshot.active_worklog.as_ref().unwrap().id,
            first_id.to_string()
        );
        assert_eq!(
            first.snapshot.active_worklog.as_ref().unwrap().start,
            at(100)
        );
        let repeated = set_tracking(State(shared.clone()), Ok(Json(first_request)))
            .await
            .unwrap()
            .0;
        assert_eq!(
            repeated.snapshot.active_worklog,
            first.snapshot.active_worklog
        );
        let next_id = WorklogId::generate();
        let next_request = request(&shared, tasks[1].task.id(), next_id, Some(first_id), 200);
        let switched = set_tracking(State(shared.clone()), Ok(Json(next_request)))
            .await
            .unwrap()
            .0;
        assert_eq!(
            switched.snapshot.active_worklog.as_ref().unwrap().id,
            next_id.to_string()
        );
        assert!(
            matches!(switched.result, MutationResultDto::TrackingSwitched { stopped, started }
            if stopped.id == first_id.to_string() && stopped.end == Some(at(200))
                && started.start == at(200))
        );
    }
}

#[cfg(test)]
mod security_tests {
    use super::{
        Core, IDEMPOTENCY_CACHE_SIZE, allowed_bind_address, host_allowed, lock_database,
        reject_unexpected_host, router_for_database, run, tailscale_address_matches,
        valid_lock_owner, validate_bind_address, validate_inactive_as_of,
    };
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::middleware::from_fn;
    use chrono::{TimeDelta, Utc};
    use std::net::SocketAddr;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};
    use std::{collections::HashMap, collections::VecDeque};
    use tower::ServiceExt;
    use tracker_application::{RepositoryError, TrackerApplication};
    use tracker_protocol::{MutationResultDto, WriteGuard};
    use tracker_storage::SqliteRepository;
    use uuid::Uuid;

    #[test]
    fn inactive_preview_accepts_exact_time_limits() {
        let now = Utc::now();
        let tolerance = TimeDelta::minutes(15);
        let tick = TimeDelta::microseconds(1);
        assert!(validate_inactive_as_of(now - tolerance, now).is_ok());
        assert!(validate_inactive_as_of(now + tolerance, now).is_ok());
        assert!(validate_inactive_as_of(now - tolerance - tick, now).is_err());
        assert!(validate_inactive_as_of(now + tolerance + tick, now).is_err());
    }

    #[test]
    fn accepts_only_explicit_loopback_or_tailscale_addresses() {
        for address in ["127.0.0.1", "::1", "100.100.100.100", "fd7a:115c:a1e0::1"] {
            assert!(allowed_bind_address(address.parse().unwrap()));
        }
        for address in [
            "0.0.0.0",
            "::",
            "192.168.1.2",
            "8.8.8.8",
            "ff02::1",
            "100.63.255.255",
            "100.128.0.1",
            "100.1.0.1",
            "192.100.0.1",
        ] {
            assert!(!allowed_bind_address(address.parse().unwrap()));
        }
        for address in ["100.64.0.1", "100.127.255.254"] {
            assert!(allowed_bind_address(address.parse().unwrap()));
        }
    }

    #[test]
    fn rejects_dns_rebinding_hostnames() {
        let bind = "127.0.0.1:8765".parse().unwrap();
        assert!(host_allowed("127.0.0.1:8765", bind));
        assert!(host_allowed("localhost:8765", bind));
        assert!(!host_allowed("example.test:8765", bind));
        assert!(!host_allowed("127.0.0.1:9999", bind));
        let bind = "[fd7a:115c:a1e0::1]:8765".parse().unwrap();
        assert!(host_allowed("[fd7a:115c:a1e0::1]:8765", bind));
        assert!(!host_allowed("localhost:8765", bind));
    }

    #[test]
    fn second_server_cannot_open_or_migrate_a_locked_database() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("locked.db");
        let _lock = lock_database(&database).unwrap();
        assert!(router_for_database(&database).is_err());
        assert!(!database.exists());
    }

    #[test]
    fn lock_file_must_be_regular_and_owned_by_current_user() {
        assert!(valid_lock_owner(true, 42, 42));
        assert!(!valid_lock_owner(false, 42, 42));
        assert!(!valid_lock_owner(true, 43, 42));
    }

    #[tokio::test]
    async fn host_middleware_rejects_missing_and_untrusted_hosts() {
        let directory = tempfile::tempdir().unwrap();
        let bind: SocketAddr = "127.0.0.1:8765".parse().unwrap();
        let router = router_for_database(&directory.path().join("host.db"))
            .unwrap()
            .layer(from_fn(move |request, next| {
                reject_unexpected_host(request, next, bind)
            }));

        let allowed = Request::builder()
            .uri("/v1/health")
            .header("host", "localhost:8765")
            .body(Body::empty())
            .unwrap();
        let response = router.clone().oneshot(allowed).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let untrusted = Request::builder()
            .uri("/v1/health")
            .header("host", "tracker.attacker.test:8765")
            .body(Body::empty())
            .unwrap();
        let response = router.clone().oneshot(untrusted).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let missing = Request::builder()
            .uri("/v1/health")
            .body(Body::empty())
            .unwrap();
        let response = router.oneshot(missing).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn run_reports_a_listener_bind_failure() {
        let directory = tempfile::tempdir().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let error = run(
            listener.local_addr().unwrap(),
            directory.path().join("tracker.db"),
        )
        .expect_err("an occupied port must fail startup");
        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::AddrInUse
        );
    }

    #[test]
    fn run_fails_closed_for_a_tailscale_range_address_not_assigned_to_this_host() {
        let directory = tempfile::tempdir().unwrap();
        let bind: SocketAddr = "100.100.100.100:0".parse().unwrap();
        let error = run(bind, directory.path().join("unused.db"))
            .expect_err("an unassigned Tailscale address must not be bound");
        assert!(error.to_string().contains("not assigned to this device"));
        assert!(!directory.path().join("unused.db").exists());
    }

    #[test]
    fn run_rejects_non_loopback_non_tailscale_addresses_before_opening_storage() {
        let directory = tempfile::tempdir().unwrap();
        let bind: SocketAddr = "192.168.1.20:0".parse().unwrap();
        let error = run(bind, directory.path().join("unused.db"))
            .expect_err("an ordinary LAN address must not be bound");
        assert!(error.to_string().contains("localhost or Tailscale"));
        assert!(!directory.path().join("unused.db").exists());
    }

    #[test]
    fn bind_validation_skips_tailscale_lookup_for_loopback_and_requires_it_elsewhere() {
        let loopback: SocketAddr = "127.0.0.1:8765".parse().unwrap();
        let mut lookup_called = false;
        assert!(
            validate_bind_address(loopback, |_| {
                lookup_called = true;
                false
            })
            .is_ok()
        );
        assert!(!lookup_called, "loopback does not need a Tailscale lookup");

        let tailscale_address: SocketAddr = "100.100.100.100:8765".parse().unwrap();
        assert!(
            validate_bind_address(tailscale_address, |address| {
                address == tailscale_address.ip()
            })
            .is_ok()
        );
        assert!(
            validate_bind_address(tailscale_address, |_| false)
                .unwrap_err()
                .contains("not assigned")
        );

        let ordinary_lan: SocketAddr = "192.168.1.20:8765".parse().unwrap();
        assert!(
            validate_bind_address(ordinary_lan, |_| {
                panic!("invalid bind addresses must fail before checking Tailscale")
            })
            .unwrap_err()
            .contains("localhost or Tailscale")
        );
    }

    #[test]
    fn tailscale_lookup_checks_family_output_and_success_status() {
        assert!(tailscale_address_matches(
            "100.101.102.103".parse().unwrap(),
            |family| {
                assert_eq!(family, "-4");
                Ok(lookup_output(0, b"100.101.102.103\n"))
            }
        ));
        assert!(tailscale_address_matches(
            "fd7a:115c:a1e0::42".parse().unwrap(),
            |family| {
                assert_eq!(family, "-6");
                Ok(lookup_output(0, b"fd7a:115c:a1e0::42\n"))
            }
        ));
        assert!(!tailscale_address_matches(
            "100.101.102.103".parse().unwrap(),
            |_| Ok(lookup_output(0, b"100.101.102.104\n"))
        ));
        assert!(!tailscale_address_matches(
            "100.101.102.103".parse().unwrap(),
            |_| Ok(lookup_output(1, b"100.101.102.103\n"))
        ));
        assert!(!tailscale_address_matches(
            "100.101.102.103".parse().unwrap(),
            |_| Ok(lookup_output(0, b"\xff"))
        ));
        assert!(!tailscale_address_matches(
            "100.101.102.103".parse().unwrap(),
            |_| Err(std::io::Error::from(std::io::ErrorKind::NotFound))
        ));
    }

    fn lookup_output(status: i32, stdout: &[u8]) -> Output {
        Output {
            status: ExitStatus::from_raw(status << 8),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        }
    }

    #[test]
    fn idempotency_cache_evicts_only_after_its_capacity_is_exceeded() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("idempotency.db");
        let database_lock = lock_database(&path).unwrap();
        let repository = SqliteRepository::open(&path).unwrap();
        let app = TrackerApplication::load(repository).unwrap();
        let mut core = Core {
            _database_lock: database_lock,
            app,
            needs_refresh: false,
            epoch: "test-epoch".to_owned(),
            sequence: 0,
            completed: HashMap::new(),
            completion_order: VecDeque::new(),
        };
        for index in 0..IDEMPOTENCY_CACHE_SIZE - 1 {
            let request_id = format!("cached-{index}");
            core.completed.insert(
                request_id.clone(),
                (
                    "fingerprint".to_owned(),
                    MutationResultDto::TrackingAlreadyIdle,
                ),
            );
            core.completion_order.push_back(request_id);
        }
        let guard = WriteGuard {
            expected_revision: core.revision(),
            request_id: Uuid::now_v7().to_string(),
        };
        let boundary_request_id = guard.request_id.clone();

        core.execute(&guard, "new fingerprint".to_owned(), |_| {
            Ok(MutationResultDto::TrackingAlreadyIdle)
        })
        .unwrap();

        assert_eq!(core.completion_order.len(), IDEMPOTENCY_CACHE_SIZE);
        assert!(core.completed.contains_key("cached-0"));
        assert!(core.completed.contains_key(&boundary_request_id));
    }

    #[test]
    fn operation_errors_preserve_the_http_status_and_protocol_code() {
        use tracker_application::{ApplicationError, ApplicationFailureCategory as Category};
        use tracker_protocol::ErrorCode;

        let cases = [
            (
                Category::General,
                StatusCode::BAD_REQUEST,
                ErrorCode::InvalidRequest,
            ),
            (
                Category::TaskNotFound,
                StatusCode::NOT_FOUND,
                ErrorCode::NotFound,
            ),
            (
                Category::WorklogNotFound,
                StatusCode::NOT_FOUND,
                ErrorCode::NotFound,
            ),
            (
                Category::WorklogChanged,
                StatusCode::CONFLICT,
                ErrorCode::WorklogChanged,
            ),
            (
                Category::WorklogHistoryChanged,
                StatusCode::CONFLICT,
                ErrorCode::WorklogHistoryChanged,
            ),
            (
                Category::WorklogOverlap,
                StatusCode::CONFLICT,
                ErrorCode::WorklogOverlap,
            ),
            (
                Category::ActiveWorklog,
                StatusCode::CONFLICT,
                ErrorCode::ActiveWorklog,
            ),
            (
                Category::ActiveTask,
                StatusCode::CONFLICT,
                ErrorCode::ActiveTask,
            ),
            (
                Category::InactiveTaskCandidatesChanged,
                StatusCode::CONFLICT,
                ErrorCode::InactiveTaskCandidatesChanged,
            ),
        ];
        for (category, status, code) in cases {
            let error = ApplicationError::semantic_failure(category, "Operation rejected");
            let response = super::ApiError::from(error);
            assert_eq!(response.status, status, "{category:?}");
            assert_eq!(response.code, code, "{category:?}");
            assert_eq!(response.message, "Operation rejected");
        }
    }

    #[test]
    fn backend_and_corrupt_data_errors_map_to_sanitized_internal_errors() {
        let task_id = tracker_domain::TaskId::generate();
        let cases = [
            (
                tracker_application::ApplicationError::Repository(RepositoryError::CorruptData {
                    field: "task name",
                }),
                StatusCode::INTERNAL_SERVER_ERROR,
                tracker_protocol::ErrorCode::Internal,
            ),
            (
                tracker_application::ApplicationError::Repository(RepositoryError::Backend {
                    message: "database password leaked".to_owned(),
                }),
                StatusCode::INTERNAL_SERVER_ERROR,
                tracker_protocol::ErrorCode::Internal,
            ),
            (
                tracker_application::ApplicationError::TrackingWrite(RepositoryError::Backend {
                    message: "tracking storage secret".to_owned(),
                }),
                StatusCode::INTERNAL_SERVER_ERROR,
                tracker_protocol::ErrorCode::Internal,
            ),
            (
                tracker_application::ApplicationError::WorklogCorrectionWrite {
                    write: RepositoryError::Backend {
                        message: "correction secret".into(),
                    },
                },
                StatusCode::INTERNAL_SERVER_ERROR,
                tracker_protocol::ErrorCode::Internal,
            ),
            (
                tracker_application::ApplicationError::WorklogMoveWrite {
                    write: RepositoryError::CorruptData { field: "password" },
                },
                StatusCode::INTERNAL_SERVER_ERROR,
                tracker_protocol::ErrorCode::Internal,
            ),
            (
                tracker_application::ApplicationError::WorklogDeletionWrite {
                    write: RepositoryError::Backend {
                        message: "deletion secret".into(),
                    },
                },
                StatusCode::INTERNAL_SERVER_ERROR,
                tracker_protocol::ErrorCode::Internal,
            ),
            (
                tracker_application::ApplicationError::WorklogDeletionRecovery {
                    write: RepositoryError::WorklogChanged {
                        id: tracker_domain::WorklogId::generate(),
                    },
                    recovery: RepositoryError::Backend {
                        message: "recovery secret".into(),
                    },
                },
                StatusCode::INTERNAL_SERVER_ERROR,
                tracker_protocol::ErrorCode::Internal,
            ),
            (
                tracker_application::ApplicationError::TaskRecovery(
                    RepositoryError::TaskNotFound { id: task_id },
                ),
                StatusCode::INTERNAL_SERVER_ERROR,
                tracker_protocol::ErrorCode::Internal,
            ),
            (
                tracker_application::ApplicationError::Repository(RepositoryError::TaskNotFound {
                    id: task_id,
                }),
                StatusCode::NOT_FOUND,
                tracker_protocol::ErrorCode::NotFound,
            ),
            (
                tracker_application::ApplicationError::semantic_failure(
                    tracker_application::ApplicationFailureCategory::TaskNotFound,
                    "Task not found",
                ),
                StatusCode::NOT_FOUND,
                tracker_protocol::ErrorCode::NotFound,
            ),
            (
                tracker_application::ApplicationError::WorklogMoveWrite {
                    write: RepositoryError::TaskNotFound { id: task_id },
                },
                StatusCode::NOT_FOUND,
                tracker_protocol::ErrorCode::NotFound,
            ),
        ];
        for (error, expected_status, expected_code) in cases {
            let mapped = super::ApiError::from(error);
            assert_eq!(mapped.status, expected_status);
            assert_eq!(mapped.code, expected_code);
            if expected_status.is_server_error() {
                assert_eq!(mapped.message, "Server could not complete the request");
            }
            assert!(!mapped.message.contains("password"));
            assert!(!mapped.message.contains("secret"));
        }
    }
}
