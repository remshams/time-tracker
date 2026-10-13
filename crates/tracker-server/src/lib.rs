//! HTTP server for the versioned remote tracker protocol.

mod bootstrap;
mod core;
mod error;
mod reports;
mod resources;
mod security;
mod tasks;
mod tracking;
mod worklogs;

pub use bootstrap::{router_for_database, run};
pub use security::allowed_bind_address;
