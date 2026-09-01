# Time Tracker

A keyboard-first time tracker written in Rust. `tt` is a terminal application for Linux and macOS; a sync server and more clients are planned. `plan.md` is the source of truth for scope and milestones.

## Current state

The TUI works end to end. It stores tasks and time entries in SQLite, seeds a new database with three example tasks, and keeps one timer running across restarts: quitting never stops the active entry, and the next start resumes it. Tasks are reusable, so starting a task again records a new time entry instead of resuming an old one. Archived tasks stay in the database but are hidden from the list.

The database enforces the tracking rules itself, so a second `tt` process sees the same bounds: an archived task cannot receive entries, a task with an active entry cannot be archived, at most one entry is active, and duplicate entry identifiers are reported as such. Seeding and switching happen inside single transactions, so simultaneous starts of two `tt` processes neither double-seed a new database nor lose a switch.

While a timer runs, the visible elapsed time comes from a monotonic clock anchored to the entry's UTC start, so system clock adjustments do not make the timer jump.

Task names are trimmed, non-empty, at most 256 characters long, and free of control characters. The same rules guard names read back from the database.

## Prerequisites

- Linux or macOS
- Rust 1.85 or newer, installed with [rustup](https://rustup.rs). The workspace uses edition 2024.
- For coverage: `rustup component add llvm-tools-preview`

## Build and run

```sh
cargo run -p tracker-tui        # run in development
cargo install --path apps/tui   # install the binary as `tt`
```

## Data and database location

The database lives in the platform application-data directory:

- Linux: `$XDG_DATA_HOME/Time Tracker/tt.db`, or `~/.local/share/Time Tracker/tt.db` when `XDG_DATA_HOME` is unset
- macOS: `~/Library/Application Support/Time Tracker/tt.db`

On first start the directory is created with mode 0700 and the database file with mode 0600. On every start the final directory and the database file are checked: a symbolic link or an object owned by another user is refused, and permissions of owner-owned objects are repaired to 0700 and 0600. The database itself is opened with `SQLITE_OPEN_NOFOLLOW`.

A brand-new empty database is seeded once with three tasks: Write release notes, Fix the coffee machine, and Plan Friday's demo. The emptiness check and the inserts run in one immediate transaction, so a failure leaves no partial seed and concurrent starts cannot seed twice. A database that already has tasks is left untouched. Migrations are concurrency-safe and a database written by a newer version of Time Tracker is refused with a clear error.

## Keybindings

Navigation is keyboard-first. The footer always lists the keys available in the current mode.

Task list:

- `j` / `k` or Down / Up: move the selection, with safe bounds at both ends. `h` and `l` are reserved for future navigation and do nothing.
- Space: start the selected task, stop the active task, or switch from the active task to the selected one in a single transaction.
- `a`: add a task. `e`: rename the selected task (the input starts pre-filled). `d`: archive the selected task after confirmation. The active task cannot be archived.
- `q` or Escape: quit. An active timer keeps running and is recovered on the next start.

Text input (`a` and `e`):

- Printable characters and Backspace edit the text; Space is ordinary input.
- Enter confirms, Escape cancels. An empty name shows an error and keeps the input open.

Archive confirmation:

- `y` or Enter confirms, `n` or Escape cancels.

Ctrl+C quits from every mode.

## Architecture

The workspace has three packages:

- `crates/tracker-core`: the domain model. Tasks, time entries, the tracker and its commands, and the `TrackerRepository` trait. No terminal, database, or network code.
- `crates/tracker-storage`: SQLite persistence. Implements `TrackerRepository`, owns the schema migrations and the platform paths.
- `apps/tui`: the `tt` binary. `app.rs` holds the state and applies semantic commands, `keymap.rs` maps raw keys to commands, `ui.rs` renders, `terminal.rs` owns setup and cleanup, and `main.rs` wires it together.

The event loop is synchronous. Commands apply to a cloned candidate first, so a storage error can never desynchronize memory and SQLite.

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
