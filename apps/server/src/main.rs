//! Starts the standalone HTTP server and reports startup failures.

mod cli;

use std::error::Error;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use tracker_storage::{default_database_path, ensure_app_data_dir};

fn main() -> ExitCode {
    report(run_app(std::env::args_os().skip(1)))
}

fn report(result: Result<(), Box<dyn Error>>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("tt-server: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_app(args: impl IntoIterator<Item = OsString>) -> Result<(), Box<dyn Error>> {
    match cli::parse(args)? {
        cli::Mode::Help => {
            print!("{}", cli::HELP);
            Ok(())
        }
        cli::Mode::Serve { bind, database } => {
            let database = database_path(database)?;
            tracker_server::run(bind, database).map_err(|error| error as Box<dyn Error>)
        }
    }
}

fn database_path(database: Option<PathBuf>) -> Result<PathBuf, Box<dyn Error>> {
    match database {
        Some(path) => Ok(path),
        None => {
            ensure_app_data_dir()?;
            Ok(default_database_path()?.with_file_name("tt-server.db"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_success_and_failure() {
        assert_eq!(report(Ok(())), ExitCode::SUCCESS);
        assert_eq!(report(Err("startup failed".into())), ExitCode::FAILURE);
    }

    #[test]
    fn help_succeeds_and_invalid_arguments_fail() {
        assert!(run_app(["--help".into()]).is_ok());
        assert!(run_app(["--bind".into(), "0.0.0.0:80".into()]).is_err());
    }

    #[test]
    fn database_selection_preserves_explicit_paths_and_uses_the_server_default() {
        let directory = tempfile::tempdir().unwrap();
        let explicit = directory.path().join("custom.db");
        assert_eq!(database_path(Some(explicit.clone())).unwrap(), explicit);
        assert_eq!(
            database_path(None).unwrap(),
            default_database_path()
                .unwrap()
                .with_file_name("tt-server.db")
        );
        assert!(default_database_path().unwrap().parent().unwrap().is_dir());
    }

    #[test]
    fn server_startup_reports_an_invalid_database_path() {
        let directory = tempfile::tempdir().unwrap();
        let error = run_app([
            "--bind".into(),
            "127.0.0.1:0".into(),
            "--db".into(),
            directory.path().as_os_str().to_owned(),
        ])
        .unwrap_err();
        assert!(!error.to_string().is_empty());
    }
}
