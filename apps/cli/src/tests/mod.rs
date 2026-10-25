use std::path::Path;

use clap::Parser;
use serde_json::Value;

use crate::{Cli, run};

mod archive;
mod backend;
mod errors;
mod parser;
mod process;
mod remote;
mod reports;
mod tasks;
mod tracking;
mod worklogs;

const T0: &str = "2026-01-01T00:00:00Z";
const T1: &str = "2026-01-01T01:00:00Z";
const T2: &str = "2026-01-01T02:00:00Z";
const T3: &str = "2026-01-01T03:00:00Z";

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().unwrap()
}

fn command(path: &Path, at: &str, arguments: &[&str]) -> Result<Value, crate::CliError> {
    let path = path.to_str().unwrap();
    let cli = Cli::try_parse_from(
        ["tt-cli", "--db", path, "--at", at]
            .into_iter()
            .chain(arguments.iter().copied()),
    )
    .unwrap();
    runtime().block_on(run(cli))
}
