//! Noninteractive commands over the same application services as the TUI.

mod args;
mod backend;
mod error;
mod process;
mod reports;
mod tasks;
mod tracking;
mod worklogs;

pub use args::Cli;
pub use error::CliError;
pub use process::process;

use serde_json::Value;

pub async fn run(cli: Cli) -> Result<Value, CliError> {
    let now = cli.at.unwrap_or_else(chrono::Utc::now);
    let identity = backend::identity(cli.db.as_deref(), cli.server.as_deref())?;
    let request = validate(cli.command, &identity, now)?;
    let mut backend = backend::Backend::open(identity, cli.db, cli.server).await?;
    match request {
        Request::Tasks(command) => tasks::execute(&mut backend, command, now).await,
        Request::Tracking(command) => tracking::execute(&mut backend, command, now).await,
        Request::Worklogs(request) => worklogs::execute(&mut backend, request, now).await,
        Request::Reports(args) => reports::execute(&mut backend, args, now).await,
    }
}

pub(crate) fn json<T: serde::Serialize>(value: T) -> Result<Value, CliError> {
    serde_json::to_value(value).map_err(CliError::storage)
}

pub(crate) fn read_json<T: serde::de::DeserializeOwned>(input: &str) -> Result<T, CliError> {
    use std::io::Read;
    const MAX_BYTES: usize = 1024 * 1024;
    let text = if let Some(path) = input.strip_prefix('@') {
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK);
        }
        let file = options.open(path).map_err(CliError::input)?;
        if !file.metadata().map_err(CliError::input)?.is_file() {
            return Err(CliError::input("JSON input must be a regular file"));
        }
        let mut text = String::new();
        file.take(MAX_BYTES as u64 + 1)
            .read_to_string(&mut text)
            .map_err(CliError::input)?;
        text
    } else {
        if input.len() > MAX_BYTES {
            return Err(CliError::input("JSON input exceeds 1 MiB"));
        }
        input.to_owned()
    };
    if text.len() > MAX_BYTES {
        return Err(CliError::input("JSON input exceeds 1 MiB"));
    }
    serde_json::from_str(&text).map_err(CliError::input)
}

/// A command that passed validation and is ready to run against the backend.
enum Request {
    Tasks(args::Tasks),
    Tracking(args::Tracking),
    Worklogs(worklogs::Request),
    Reports(args::ReportArgs),
}

/// Rejects invalid input before a backend is opened, so it never creates a database.
fn validate(
    command: args::Command,
    identity: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Request, CliError> {
    Ok(match command {
        args::Command::Tasks { mut command } => {
            tasks::validate(&mut command, identity, now)?;
            Request::Tasks(command)
        }
        args::Command::Worklogs { command } => {
            Request::Worklogs(worklogs::validate(command, identity, now)?)
        }
        args::Command::Reports(mut args) => {
            let range = reports::range(&args, now)?;
            args.timezone = Some(range.timezone);
            Request::Reports(args)
        }
        args::Command::Tracking { command } => Request::Tracking(command),
    })
}

#[cfg(test)]
mod tests;
