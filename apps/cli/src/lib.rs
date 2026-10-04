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

pub async fn run(mut cli: Cli) -> Result<Value, CliError> {
    let now = cli.at.unwrap_or_else(chrono::Utc::now);
    if cli.db.is_some() && cli.server.is_some() {
        return Err(CliError::input("--db and --server are mutually exclusive"));
    }
    let identity = backend::identity(cli.db.as_deref(), cli.server.as_deref())?;
    validate(&mut cli.command, &identity, now)?;
    let mut backend = backend::Backend::open(cli.db, cli.server).await?;
    match cli.command {
        args::Command::Tasks { command } => tasks::execute(&mut backend, command, now).await,
        args::Command::Tracking { command } => tracking::execute(&mut backend, command, now).await,
        args::Command::Worklogs { command } => worklogs::execute(&mut backend, command, now).await,
        args::Command::Reports(args) => reports::execute(&mut backend, args, now).await,
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

fn validate(
    command: &mut args::Command,
    identity: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), CliError> {
    match command {
        args::Command::Tasks { command } => tasks::validate(command, identity, now),
        args::Command::Worklogs { command } => worklogs::validate(command, identity, now),
        args::Command::Reports(args) => {
            let range = reports::range(args, now)?;
            args.timezone = Some(range.timezone);
            Ok(())
        }
        args::Command::Tracking { .. } => Ok(()),
    }
}

#[cfg(test)]
mod tests;
