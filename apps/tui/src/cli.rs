//! Process modes for the terminal client and HTTP server.

use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Local,
    Remote {
        server: String,
    },
    Serve {
        bind: SocketAddr,
        database: Option<PathBuf>,
    },
    Help,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}; run `tt --help` for usage")]
pub(crate) struct CliError(&'static str);

pub(crate) const HELP: &str = "Usage:\n  tt\n  tt --server http://HOST:PORT\n  tt serve --bind ADDRESS:PORT [--db PATH]\n\nThe server accepts an explicit loopback or Tailscale address.\n";

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
        Some("serve") => parse_serve(args),
        _ => Err(CliError("unknown argument")),
    }
}

fn parse_serve(mut args: impl Iterator<Item = OsString>) -> Result<Mode, CliError> {
    let mut bind = None;
    let mut database = None;
    while let Some(flag) = args.next() {
        match flag.to_str() {
            Some("--bind") if bind.is_none() => {
                let value = args
                    .next()
                    .and_then(|value| value.into_string().ok())
                    .ok_or(CliError("--bind needs an IP address and port"))?;
                let address: SocketAddr = value
                    .parse()
                    .map_err(|_| CliError("--bind needs an IP address and port"))?;
                if !tracker_server::allowed_bind_address(address.ip()) {
                    return Err(CliError("--bind must use a loopback or Tailscale address"));
                }
                bind = Some(address);
            }
            Some("--db") if database.is_none() => {
                let path = args.next().ok_or(CliError("--db needs a path"))?;
                if path.is_empty() {
                    return Err(CliError("--db needs a path"));
                }
                database = Some(PathBuf::from(path));
            }
            _ => return Err(CliError("unknown or repeated server option")),
        }
    }
    Ok(Mode::Serve {
        bind: bind.ok_or(CliError("serve needs --bind ADDRESS:PORT"))?,
        database,
    })
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
    fn serve_requires_a_private_explicit_bind_address() {
        assert!(parse(args(&["serve"])).is_err());
        assert!(parse(args(&["serve", "--bind", "0.0.0.0:8765"])).is_err());
        assert!(parse(args(&["serve", "--bind", "192.168.1.10:8765"])).is_err());
        assert!(parse(args(&["serve", "--bind", "127.0.0.1:8765"])).is_ok());
        assert!(parse(args(&["serve", "--bind", "100.100.100.100:8765"])).is_ok());
        assert!(parse(args(&["serve", "--bind", "[fd7a:115c:a1e0::1]:8765"])).is_ok());
    }

    #[test]
    fn server_database_path_is_optional_and_repeated_flags_fail() {
        assert_eq!(
            parse(args(&[
                "serve",
                "--db",
                "/tmp/server.db",
                "--bind",
                "[::1]:8765"
            ]))
            .unwrap(),
            Mode::Serve {
                bind: "[::1]:8765".parse().unwrap(),
                database: Some("/tmp/server.db".into()),
            }
        );
        assert!(
            parse(args(&[
                "serve",
                "--bind",
                "127.0.0.1:1",
                "--bind",
                "127.0.0.1:2"
            ]))
            .is_err()
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
    fn serve_rejects_incomplete_and_unsafe_options() {
        for values in [
            vec!["serve", "--bind"],
            vec!["serve", "--bind", "not-an-address"],
            vec!["serve", "--bind", "100.63.0.1:8765"],
            vec!["serve", "--bind", "100.128.0.1:8765"],
            vec!["serve", "--bind", "[fd7a:115c:a1e1::1]:8765"],
            vec!["serve", "--db"],
            vec!["serve", "--db", ""],
            vec![
                "serve",
                "--db",
                "a.db",
                "--db",
                "b.db",
                "--bind",
                "127.0.0.1:1",
            ],
            vec!["serve", "--bind", "127.0.0.1:1", "extra"],
        ] {
            assert!(parse(args(&values)).is_err(), "accepted {values:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_remote_url_and_bind_address_are_rejected() {
        use std::os::unix::ffi::OsStringExt;

        let invalid = OsString::from_vec(vec![0xff]);
        assert!(parse(vec![OsString::from("--server"), invalid.clone()]).is_err());
        assert!(
            parse(vec![
                OsString::from("serve"),
                OsString::from("--bind"),
                invalid
            ])
            .is_err()
        );
    }
}
