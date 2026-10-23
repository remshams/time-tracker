//! Shared application state and command receipts under one operation lock.

use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::sync::{Arc, Mutex};

use tracker_application::TrackerApplication;
use tracker_domain::TrackingState;
use tracker_protocol::{MutationDto, MutationResultDto, WorklogDto, WriteGuard};
use tracker_storage::SqliteRepository;
use uuid::Uuid;

use crate::error::ApiError;

const IDEMPOTENCY_CACHE_SIZE: usize = 1024;
pub(super) type Shared = Arc<Mutex<Core>>;

pub(super) struct Core {
    _database_lock: File,
    pub(super) app: TrackerApplication<SqliteRepository>,
    needs_refresh: bool,
    completed: HashMap<String, (String, MutationDto)>,
    completion_order: VecDeque<String>,
}

impl Core {
    /// Keeps the database lock alongside the application and starts with no cached receipts.
    pub(super) fn new(database_lock: File, app: TrackerApplication<SqliteRepository>) -> Self {
        Self {
            _database_lock: database_lock,
            app,
            needs_refresh: false,
            completed: HashMap::new(),
            completion_order: VecDeque::new(),
        }
    }

    pub(super) fn active_worklog(&self) -> Option<WorklogDto> {
        match self.app.current_tracking() {
            TrackingState::Idle => None,
            TrackingState::Running { worklog } => Some(WorklogDto::from(&worklog.to_worklog())),
        }
    }

    pub(super) fn execute(
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
                    replayed: true,
                    ..result.clone()
                })
            } else {
                Err(ApiError::conflict(
                    "Request ID was reused for a different command",
                ))
            };
        }
        let result = match operation(&mut self.app) {
            Ok(result) => result,
            Err(error) => {
                if error.status.is_server_error() {
                    // A command may commit before its follow-up read fails.
                    // Reload the caches so later reads report the stored state.
                    self.needs_refresh = self.app.refresh_task_list().is_err();
                }
                return Err(error);
            }
        };
        let response = MutationDto {
            request_id: guard.request_id.clone(),
            replayed: false,
            result,
        };
        self.completed
            .insert(guard.request_id.clone(), (fingerprint, response.clone()));
        self.completion_order.push_back(guard.request_id.clone());
        if self.completion_order.len() > IDEMPOTENCY_CACHE_SIZE
            && let Some(evicted) = self.completion_order.pop_front()
        {
            self.completed.remove(&evicted);
        }
        Ok(response)
    }
}
pub(super) fn lock(shared: &Shared) -> Result<std::sync::MutexGuard<'_, Core>, ApiError> {
    let mut core = shared.lock().map_err(|_| ApiError::internal())?;
    if core.needs_refresh {
        core.app
            .refresh_task_list()
            .map_err(|_| ApiError::internal())?;
        core.needs_refresh = false;
    }
    Ok(core)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bootstrap::lock_database;

    #[test]
    fn idempotency_cache_evicts_only_after_its_capacity_is_exceeded() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("idempotency.db");
        let database_lock = lock_database(&path).unwrap();
        let repository = SqliteRepository::open(&path).unwrap();
        let app = TrackerApplication::load(repository).unwrap();
        let mut core = Core::new(database_lock, app);
        for index in 0..IDEMPOTENCY_CACHE_SIZE - 1 {
            let request_id = format!("cached-{index}");
            core.completed.insert(
                request_id.clone(),
                (
                    "fingerprint".to_owned(),
                    tracker_protocol::MutationDto {
                        request_id: request_id.clone(),
                        replayed: false,
                        result: MutationResultDto::TrackingAlreadyIdle,
                    },
                ),
            );
            core.completion_order.push_back(request_id);
        }
        let guard = WriteGuard {
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
}
