# Time Tracker

A keyboard-first time tracker written in Rust. It starts as a terminal application, `tt`, on Linux and macOS, and may later gain a web client and integrations. `plan.md` is the source of truth for product scope and milestones.

## Current scope

The repository is bootstrapped: a Cargo workspace with the `tracker-core` library and the `tracker-tui` application. Both compile, but no feature work has landed. The task list UI and key handling arrive in milestone 1; persistence and sync come later.

## Prerequisites

- Linux or macOS
- Rust 1.85 or newer, installed with [rustup](https://rustup.rs). The workspace uses edition 2024.
- For coverage: `rustup component add llvm-tools-preview`

## Build and run

```sh
cargo run -p tracker-tui        # run in development
cargo install --path apps/tui   # install the binary as `tt`
```

## Keybindings

The TUI follows one pattern across screens: `h`, `j`, `k`, and `l` navigate, mnemonic keys such as `a` invoke actions, and the footer shows the keys available on the current screen. The first screen moves the task selection with `j` and `k` (up and down arrows work as aliases) and quits with `q` or Escape. These keys are wired up in milestone 1; the current binary is a stub.

## Validation

Run these before every handoff:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo mutants --workspace
cargo llvm-cov --workspace --all-features --lcov --output-path lcov.info
cargo crap --lcov lcov.info
```

Missed and timed-out mutants fail the check, as do CRAP scores above 30. The pinned tool versions and full rules live in `AGENTS.md`.

## Hooks

The pre-commit hook checks formatting. After cloning, point Git at the repository-owned hooks:

```sh
git config core.hooksPath .githooks
```
