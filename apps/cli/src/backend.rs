use std::path::{Path, PathBuf};

use tracker_application::TrackerApplication;
use tracker_client::TrackerBackend;
use tracker_remote::RemoteApplication;
use tracker_storage::{SqliteRepository, default_database_path, ensure_app_data_dir};

use crate::CliError;

pub(crate) struct Backend {
    pub identity: String,
    pub kind: BackendKind,
}

pub(crate) type BackendKind = TrackerBackend;

pub(crate) fn identity(database: Option<&Path>, server: Option<&str>) -> Result<String, CliError> {
    if let Some(server) = server {
        RemoteApplication::disconnected(server).map_err(CliError::remote)?;
        let url = reqwest::Url::parse(server).map_err(CliError::input)?;
        return Ok(format!("remote:{url}"));
    }
    let path = database
        .map(Path::to_path_buf)
        .map(Ok)
        .unwrap_or_else(default_database_path)
        .map_err(CliError::storage)?;
    let path = canonical_database_path(&path).map_err(CliError::storage)?;
    let path = path
        .to_str()
        .ok_or_else(|| CliError::input("database path must be valid UTF-8"))?;
    Ok(format!("local:{path}"))
}

fn canonical_database_path(path: &Path) -> std::io::Result<PathBuf> {
    let mut ancestor = std::path::absolute(path)?;
    let mut suffix = Vec::new();
    loop {
        if let Ok(mut resolved) = std::fs::canonicalize(&ancestor) {
            for component in suffix.into_iter().rev() {
                resolved.push(component);
            }
            return Ok(resolved);
        }
        let name = ancestor
            .file_name()
            .ok_or_else(|| std::io::Error::other("database ancestor cannot be resolved"))?
            .to_os_string();
        suffix.push(name);
        if !ancestor.pop() {
            return Err(std::io::Error::other(
                "database ancestor cannot be resolved",
            ));
        }
    }
}

impl Backend {
    pub async fn report_totals(
        &mut self,
        start: chrono::DateTime<chrono::Utc>,
        end: chrono::DateTime<chrono::Utc>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<tracker_application::ReportTotals, CliError> {
        self.kind
            .report_totals(start, end, now)
            .await
            .map_err(CliError::application)
    }

    /// Opens the selected backend. `identity` must come from [`identity`] for the same arguments.
    pub async fn open(
        identity: String,
        database: Option<PathBuf>,
        server: Option<String>,
    ) -> Result<Self, CliError> {
        let kind = if let Some(server) = server {
            BackendKind::Remote(Box::new(
                RemoteApplication::connect(&server)
                    .await
                    .map_err(CliError::remote)?,
            ))
        } else {
            let path = match database {
                Some(path) => path,
                None => {
                    ensure_app_data_dir().map_err(CliError::storage)?;
                    default_database_path().map_err(CliError::storage)?
                }
            };
            let repository = SqliteRepository::open(path).map_err(CliError::storage)?;
            BackendKind::Local(TrackerApplication::load(repository).map_err(CliError::application)?)
        };
        Ok(Self { identity, kind })
    }
}

impl std::ops::Deref for Backend {
    type Target = TrackerBackend;

    fn deref(&self) -> &Self::Target {
        &self.kind
    }
}

impl std::ops::DerefMut for Backend {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.kind
    }
}
