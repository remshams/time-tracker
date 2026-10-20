//! Arguments for the standalone HTTP server.

use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Serve {
        bind: SocketAddr,
        database: Option<PathBuf>,
    },
    Help,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}; run `tt-server --help` for usage")]
pub(crate) struct CliError(&'static str);

pub(crate) const HELP: &str = "Usage:\n  tt-server --bind ADDRESS:PORT [--db PATH]\n  tt-server --help\n\nBind to an explicit loopback or Tailscale address.\nThe default database is tt-server.db in the application data directory.\nCompatibility form for existing systemd units: tt-server serve --bind ADDRESS:PORT [--db PATH]\n";

pub(crate) fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Mode, CliError> {
    let mut args = args.into_iter().peekable();
    if args.peek().is_some_and(|arg| arg == "serve") {
        args.next();
    }
    if args
        .peek()
        .is_some_and(|arg| arg == "--help" || arg == "-h")
    {
        args.next();
        return if args.next().is_none() {
            Ok(Mode::Help)
        } else {
            Err(CliError("unexpected argument after help"))
        };
    }
    parse_serve(args)
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
        bind: bind.ok_or(CliError("server needs --bind ADDRESS:PORT"))?,
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
    fn bind_requires_a_private_explicit_address() {
        assert!(parse(args(&[])).is_err());
        assert!(parse(args(&["--bind", "0.0.0.0:8765"])).is_err());
        assert!(parse(args(&["--bind", "192.168.1.10:8765"])).is_err());
        assert!(parse(args(&["--bind", "127.0.0.1:8765"])).is_ok());
        assert!(parse(args(&["--bind", "100.100.100.100:8765"])).is_ok());
        assert!(parse(args(&["--bind", "[fd7a:115c:a1e0::1]:8765"])).is_ok());
    }

    #[test]
    fn database_path_is_optional_and_repeated_flags_fail() {
        assert_eq!(
            parse(args(&["--db", "/tmp/server.db", "--bind", "[::1]:8765"])).unwrap(),
            Mode::Serve {
                bind: "[::1]:8765".parse().unwrap(),
                database: Some("/tmp/server.db".into()),
            }
        );
        assert!(parse(args(&["--bind", "127.0.0.1:1", "--bind", "127.0.0.1:2"])).is_err());
    }

    #[test]
    fn rejects_incomplete_and_unsafe_options() {
        for values in [
            vec!["--bind"],
            vec!["--bind", "not-an-address"],
            vec!["--bind", "100.63.0.1:8765"],
            vec!["--bind", "100.128.0.1:8765"],
            vec!["--bind", "[fd7a:115c:a1e1::1]:8765"],
            vec!["--db"],
            vec!["--db", ""],
            vec!["--db", "a.db", "--db", "b.db", "--bind", "127.0.0.1:1"],
            vec!["--bind", "127.0.0.1:1", "extra"],
        ] {
            assert!(parse(args(&values)).is_err(), "accepted {values:?}");
        }
    }

    #[test]
    fn help_accepts_no_further_arguments() {
        assert_eq!(parse(args(&["--help"])), Ok(Mode::Help));
        assert_eq!(parse(args(&["-h"])), Ok(Mode::Help));
        assert_eq!(
            parse(args(&["--help", "extra"])).unwrap_err().to_string(),
            "unexpected argument after help; run `tt-server --help` for usage"
        );
    }

    #[test]
    fn compatibility_form_matches_the_canonical_form() {
        for values in [
            vec![],
            vec!["--help"],
            vec!["-h"],
            vec!["--help", "extra"],
            vec!["--bind", "127.0.0.1:8765"],
            vec!["--db", "server.db", "--bind", "[::1]:8765"],
            vec!["--db", ""],
            vec!["--bind", "0.0.0.0:8765"],
            vec!["unknown"],
        ] {
            let canonical = parse(args(&values));
            let mut compatibility = vec!["serve"];
            compatibility.extend(values);
            assert_eq!(canonical, parse(args(&compatibility)));
        }
        assert!(parse(args(&["serve", "serve", "--bind", "127.0.0.1:8765"])).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_bind_is_rejected_but_database_paths_are_preserved() {
        use std::os::unix::ffi::OsStringExt;
        let invalid = OsString::from_vec(vec![0xff]);
        assert!(parse(vec![OsString::from("--bind"), invalid.clone()]).is_err());
        assert_eq!(
            parse(vec![
                OsString::from("--bind"),
                OsString::from("127.0.0.1:1"),
                OsString::from("--db"),
                invalid.clone()
            ])
            .unwrap(),
            Mode::Serve {
                bind: "127.0.0.1:1".parse().unwrap(),
                database: Some(invalid.into()),
            }
        );
    }
}
