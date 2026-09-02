# AGENTS.md

Time Tracker is a Rust time-tracking application. It starts with a TUI and may later add synced storage, web clients, and external integrations.

## General

- Always apply the `unslop` skill when writing or editing any text (responses, code comments, docs, commit messages, etc.)

## Validation

- After completing a task or feature, run `python3 -m unittest discover -s scripts/tests -p 'test_*.py'`, `cargo test`, `cargo mutants`, `cargo llvm-cov --lcov --output-path lcov.info`, and `cargo crap --workspace --lcov lcov.info` before handoff.
- Use cargo-mutants 27.1.0. Install it with `cargo install --locked cargo-mutants --version 27.1.0` if it is unavailable.
- Use cargo-llvm-cov 0.9.0 and cargo-crap 0.4.3. Install them with `rustup component add llvm-tools-preview`, `cargo +stable install --locked --version 0.9.0 cargo-llvm-cov`, and `cargo +stable install --locked --version 0.4.3 cargo-crap` if they are unavailable.
- Treat missed and timed-out mutants, and CRAP scores above the configured threshold, as failures. Report any that cannot be resolved.
- `.cargo/mutants.toml` excludes one named glue mutant: `RealEffects::show_cursor` in `apps/tui/src/terminal.rs`. Ratatui's `Terminal::drop` re-shows the cursor after our restore, so removing that crossterm call is not observable by any test; the same bytes reach the terminal either way.
- `cargo-crap` cannot match the macro-generated functions in `crates/tracker-domain/src/ids.rs`. Direct tests for those functions remain required; do not refactor the ID types to satisfy the report.
