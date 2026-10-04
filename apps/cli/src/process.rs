use std::ffi::OsString;
use std::io::Write;

use clap::Parser;

use crate::{Cli, CliError, run};

pub fn process(
    arguments: impl IntoIterator<Item = impl Into<OsString> + Clone>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    let cli = match Cli::try_parse_from(arguments) {
        Ok(cli) => cli,
        Err(error) if !error.use_stderr() => {
            return match write!(stdout, "{error}").and_then(|_| stdout.flush()) {
                Ok(()) => 0,
                Err(error) => report(Err(CliError::storage(error)), stdout, stderr),
            };
        }
        Err(error) => return report(Err(CliError::input(error)), stdout, stderr),
    };
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => return report(Err(CliError::storage(error)), stdout, stderr),
    };
    report(runtime.block_on(run(cli)), stdout, stderr)
}

pub(crate) fn report(
    result: Result<serde_json::Value, CliError>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    match result {
        Ok(data) => match writeln!(stdout, "{}", serde_json::json!({"ok": true, "data": data}))
            .and_then(|_| stdout.flush())
        {
            Ok(()) => 0,
            Err(error) => failure(CliError::storage(error), stderr),
        },
        Err(error) => failure(error, stderr),
    }
}

fn failure(error: CliError, stderr: &mut impl Write) -> u8 {
    match writeln!(
        stderr,
        "{}",
        serde_json::json!({"ok": false, "error": error})
    )
    .and_then(|_| stderr.flush())
    {
        Ok(()) => error.exit_code,
        Err(_) => 4,
    }
}
