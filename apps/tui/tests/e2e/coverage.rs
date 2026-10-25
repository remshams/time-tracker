//! The coverage environment every child process of the suite inherits.

use std::ffi::OsString;

/// The variable cargo-llvm-cov sets to tell instrumented binaries where to
/// write their profile.
const PROFILE_FILE: &str = "LLVM_PROFILE_FILE";

/// The coverage profile setting for a child that starts from a cleared
/// environment, or `None` outside a coverage run.
///
/// When the suite runs under cargo-llvm-cov, passing the profile file
/// through makes the child's execution count toward the measured coverage.
/// Both `std::process::Command::envs` and `termlens::TerminalBuilder::envs`
/// accept the result directly.
pub(crate) fn profile_env() -> Option<(&'static str, OsString)> {
    std::env::var_os(PROFILE_FILE).map(|profile| (PROFILE_FILE, profile))
}
