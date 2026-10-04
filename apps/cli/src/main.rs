use std::process::ExitCode;

fn main() -> ExitCode {
    ExitCode::from(tracker_cli::process(
        std::env::args_os(),
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn executable_returns_input_status_for_test_harness_arguments() {
        assert_eq!(super::main(), std::process::ExitCode::from(2));
    }
}
