//! Router construction, database ownership and server startup.

use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::fs::{File, OpenOptions};
use std::net::SocketAddr;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use axum::extract::DefaultBodyLimit;
use axum::middleware::from_fn;
use axum::routing::{get, post};
use axum::{Json, Router};
use tracker_application::TrackerApplication;
use tracker_protocol::HealthDto;
use tracker_storage::SqliteRepository;

use crate::core::Core;
use crate::reports::report;
use crate::resources::{daily_overview, task_list_state};
use crate::security::{reject_unexpected_host, tailscale_address_matches, validate_bind_address};
use crate::tasks::{
    archive_inactive_tasks, change_task, create_task, preview_inactive_tasks, task, tasks,
};
use crate::tracking::{set_tracking, tracking};
use crate::worklogs::{all_worklogs, change_worklog, delete_worklog, worklog};

const BODY_LIMIT: usize = 16 * 1024;

async fn health() -> Json<HealthDto> {
    Json(HealthDto {
        status: "ok".to_owned(),
        protocol_version: tracker_protocol::VERSION,
    })
}
/// Creates a router backed by one SQLite database. Each handler serializes access
/// to the application service, including reads that refresh its resource caches.
pub fn router_for_database(path: &Path) -> Result<Router, Box<dyn Error + Send + Sync>> {
    let database_lock = lock_database(path)?;
    let repository = SqliteRepository::open(path)?;
    let app = TrackerApplication::load(repository)?;
    let core = Core {
        _database_lock: database_lock,
        app,
        needs_refresh: false,
        completed: HashMap::new(),
        completion_order: VecDeque::new(),
    };
    let shared = Arc::new(Mutex::new(core));
    Ok(Router::new()
        .route("/v1/health", get(health))
        .route("/v1/tasks", get(tasks).post(create_task))
        .route("/v1/tasks/inactive-preview", get(preview_inactive_tasks))
        .route("/v1/tasks/archive-inactive", post(archive_inactive_tasks))
        .route("/v1/tasks/{id}", get(task).patch(change_task))
        .route("/v1/tracking", get(tracking).put(set_tracking))
        .route("/v1/worklogs", get(all_worklogs))
        .route(
            "/v1/worklogs/{id}",
            get(worklog).patch(change_worklog).delete(delete_worklog),
        )
        .route("/v1/reports/task-totals", get(report))
        .route("/v1/state/task-list", get(task_list_state))
        .route("/v1/state/daily-overview", get(daily_overview))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .with_state(shared))
}

pub(super) fn lock_database(path: &Path) -> Result<File, Box<dyn Error + Send + Sync>> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
