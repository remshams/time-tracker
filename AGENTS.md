# AGENTS.md

Time Tracker is a Rust time-tracking application. It starts with a TUI and may later add synced storage, web clients, and external integrations.

## General

- Always apply the `unslop` skill when writing or editing any text (responses, code comments, docs, commit messages, etc.)

## Validation

- After completing a task or feature, run `cargo test`, `cargo mutants`, `cargo llvm-cov --lcov --output-path lcov.info`, and `cargo crap --lcov lcov.info` before handoff.
- Use cargo-mutants 27.1.0. Install it with `cargo install --locked cargo-mutants --version 27.1.0` if it is unavailable.
- Use cargo-llvm-cov 0.9.0 and cargo-crap 0.4.3. Install them with `rustup component add llvm-tools-preview`, `cargo +stable install --locked --version 0.9.0 cargo-llvm-cov`, and `cargo +stable install --locked --version 0.4.3 cargo-crap` if they are unavailable.
- Treat missed and timed-out mutants, and CRAP scores above the configured threshold, as failures. Report any that cannot be resolved.
