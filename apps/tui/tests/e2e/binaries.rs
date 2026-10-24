//! Paths to the `tt-cli` and `tt-server` binaries the scenarios use as
//! black-box fixtures.
//!
//! Cargo sets `CARGO_BIN_EXE_<name>` only for binaries of the package under
//! test, and both binaries belong to other workspace packages. The first
//! scenario that needs one runs `cargo build` for both packages once per
//! test process and takes the executable paths from Cargo's JSON artifact
//! messages. The build writes to the target directory and profile that hold
//! the `tt` binary under test, and it inherits this process's environment,
//! so runs under cargo-llvm-cov or cargo-mutants build the fixtures with
//! their own flags and directories. When the binaries are up to date, the
//! build is a quick no-op.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

struct Binaries {
    cli: PathBuf,
    server: PathBuf,
}

static BINARIES: OnceLock<Binaries> = OnceLock::new();

/// The `tt-cli` executable, built on first use.
pub(crate) fn cli() -> &'static Path {
    &binaries().cli
}

/// The `tt-server` executable, built on first use.
pub(crate) fn server() -> &'static Path {
    &binaries().server
}

fn binaries() -> &'static Binaries {
    BINARIES.get_or_init(build)
}

fn build() -> Binaries {
    // `tt` lives in `<target dir>/<profile dir>/`.
    let profile_dir = Path::new(env!("CARGO_BIN_EXE_tt"))
        .parent()
        .expect("the tt binary must have a profile directory");
    let target_dir = profile_dir
        .parent()
        .expect("the profile directory must have a target directory");
    let profile = match profile_dir.file_name().and_then(|name| name.to_str()) {
        Some("debug") => "dev",
        Some(name) => name,
        None => panic!("the profile directory name must be UTF-8"),
    };
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| env!("CARGO").into());
    let output = Command::new(cargo)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "build",
            "--quiet",
            "--message-format=json",
            "--profile",
            profile,
            "--package",
            "tracker-cli",
            "--bin",
            "tt-cli",
            "--package",
            "tracker-server-app",
            "--bin",
            "tt-server",
        ])
        .arg("--target-dir")
        .arg(target_dir)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .expect("cargo must run to build the E2E fixture binaries");
    assert!(
        output.status.success(),
        "building tt-cli and tt-server failed with {}",
        output.status
    );
    let stdout = String::from_utf8(output.stdout).expect("cargo messages must be UTF-8");
    let executable = |name: &str| {
        stdout
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|message| message["reason"] == "compiler-artifact")
            .filter(|message| message["target"]["name"] == name)
            .find_map(|message| message["executable"].as_str().map(PathBuf::from))
            .unwrap_or_else(|| panic!("cargo did not report the {name} executable"))
    };
    Binaries {
        cli: executable("tt-cli"),
        server: executable("tt-server"),
    }
}
