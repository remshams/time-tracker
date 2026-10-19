//! Process modes for the terminal client.

use std::ffi::OsString;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Local,
    Remote { server: String },
    Help,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}; run `tt --help` for usage")]
pub(crate) struct CliError(&'static str);

pub(crate) const HELP: &str = "Usage:\n  tt\n  tt --server http://HOST:PORT\n  tt --help\n\nStart an HTTP server with tt-server --bind ADDRESS:PORT [--db PATH].\n";

pub(crate) fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Mode, CliError> {
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        return Ok(Mode::Local);
    };
    match first.to_str() {
        Some("--help" | "-h") if args.next().is_none() => Ok(Mode::Help),
        Some("--server") => {
            let server = args
                .next()
                .and_then(|value| value.into_string().ok())
                .filter(|value| !value.is_empty())
                .ok_or(CliError("--server needs a URL"))?;
            if args.next().is_some() {
                return Err(CliError("unexpected argument after the server URL"));
            }
            Ok(Mode::Remote { server })
        }
        Some("serve") => Err(CliError(
            "serve moved to `tt-server --bind ADDRESS:PORT [--db PATH]`",
        )),
        _ => Err(CliError("unknown argument")),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{Mode, parse};

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn no_options_select_local_mode() {
        assert_eq!(parse(args(&[])).unwrap(), Mode::Local);
    }

    #[test]
    fn a_server_url_selects_remote_mode() {
        assert_eq!(
            parse(args(&["--server", "http://127.0.0.1:8765"])).unwrap(),
            Mode::Remote {
                server: "http://127.0.0.1:8765".to_owned()
            }
        );
    }

    #[test]
    fn help_and_remote_mode_reject_extra_or_missing_arguments() {
        assert_eq!(parse(args(&["--help"])).unwrap(), Mode::Help);
        assert_eq!(parse(args(&["-h"])).unwrap(), Mode::Help);
        for values in [
            vec!["--help", "extra"],
            vec!["--server"],
            vec!["--server", ""],
            vec!["--server", "http://127.0.0.1:8765", "extra"],
            vec!["unknown"],
        ] {
            assert!(parse(args(&values)).is_err(), "accepted {values:?}");
        }
    }

    #[test]
    fn serve_points_to_the_standalone_server() {
        let error = parse(args(&["serve", "--bind", "127.0.0.1:8765"])).unwrap_err();
        assert_eq!(
            error.to_string(),
            "serve moved to `tt-server --bind ADDRESS:PORT [--db PATH]`; run `tt --help` for usage"
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_remote_urls_are_rejected() {
        use std::os::unix::ffi::OsStringExt;
        assert!(
            parse(vec![
                OsString::from("--server"),
                OsString::from_vec(vec![0xff])
            ])
            .is_err()
        );
    }
}
