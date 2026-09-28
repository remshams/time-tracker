//! End-to-end scenarios: the real `tt` binary in a pseudo-terminal,
//! asserted through its rendered screen.
//!
//! Scenarios speak the language of the interface (headers, task rows,
//! dialogs, status messages) through the component objects in `e2e::page`,
//! never through coordinates, cell styles, or raw output. Support code
//! lives beside them by concern: process control in `e2e::driver`, test
//! fixtures in `e2e::context`, database probes in `e2e::database`.
//!
//! Tests run on Linux and macOS. Every child gets a temporary `HOME`, so
//! `tt` always operates on a temporary database and never touches the
//! user's real data on either platform.

#![cfg(any(target_os = "linux", target_os = "macos"))]

// A test root resolves its submodules beside itself, not in a directory
// named after the file, so each direct submodule states its path.
#[path = "e2e/context.rs"]
mod context;
#[path = "e2e/controlled_proxy.rs"]
mod controlled_proxy;
#[path = "e2e/database.rs"]
mod database;
#[path = "e2e/driver.rs"]
mod driver;
#[path = "e2e/page.rs"]
mod page;
#[path = "e2e/remote.rs"]
mod remote;
#[path = "e2e/scenarios.rs"]
mod scenarios;
