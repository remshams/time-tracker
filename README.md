# Time Tracker

A keyboard-first time tracker written in Rust. `tt` is a terminal application for Linux and macOS; a sync server and more clients are planned. `plan.md` is the source of truth for scope and milestones.

## Current state

The TUI works end to end. It stores tasks and time entries in SQLite, seeds a new database with three example tasks, and keeps one timer running across restarts: quitting never stops the active entry, and the next start resumes it. Tasks are reusable, so starting a task again records a new time entry instead of resuming an old one. Archived tasks stay in the database but are hidden from the list.

The database enforces the tracking rules itself, so a second `tt` process sees the same bounds: an archived task cannot receive entries, a task with an active entry cannot be archived, at most one entry is active, and duplicate entry identifiers are reported as such. Seeding and switching happen inside single transactions, so simultaneous starts of two `tt` processes neither double-seed a new database nor lose a switch.

While a timer runs, the visible elapsed time comes from a monotonic clock anchored to the entry's UTC start, so system clock adjustments do not make the timer jump. Stopping or switching derives its UTC instant from the same clock, so the persisted duration always matches the displayed one.

Task names are trimmed, non-empty, at most 256 characters long, and free of control characters. The same rules guard names read back from the database.

## Prerequisites

- Linux or macOS
- Rust 1.88 or newer, installed with [rustup](https://rustup.rs). The workspace uses edition 2024, and Ratatui requires Rust 1.88.
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

- `j` / `k` or Down / Up: move the selection, with safe bounds at both ends. `h` and `l` are reserved for future navigation and do nothing. Only unmodified keys act in the task list and confirmation modes; modifier chords other than Ctrl+C are ignored.
- Space: start the selected task, stop the active task, or switch from the active task to the selected one in a single transaction. Stop and switch instants come from the monotonic elapsed clock, so the stored duration always matches what was displayed, even across system clock adjustments. If another `tt` process won a conflict first, the screen reloads tasks, the active timer, and the elapsed clock from the database before the concise error is shown.
- `a`: add a task. `e`: rename the selected task (the input starts pre-filled). `d`: archive the selected task after confirmation. The active task cannot be archived. After a successful add, rename, or archive, the list is updated in place from the stored result.
- `q` or Escape: quit. An active timer keeps running and is recovered on the next start.

Text input (`a` and `e`):

- Printable characters and Backspace edit the text; Space is ordinary input. Input stops growing at 256 characters, the task-name limit.
- Enter confirms, Escape cancels. An invalid name (empty, control characters, or too long) shows an error and keeps the input open.
- The modal scrolls horizontally: with a long text the tail of the buffer and the cursor stay visible, cutting only on whole characters.

Archive confirmation:

- `y` or Enter confirms, `n` or Escape cancels.

Ctrl+C quits from every mode.

## Architecture

The workspace has three packages:

- `crates/tracker-core`: the domain model. Tasks, time entries, the tracker and its commands, and the `TrackerRepository` trait. No terminal, database, or network code.
- `crates/tracker-storage`: SQLite persistence. Implements `TrackerRepository`, owns the schema migrations and the platform paths.
- `apps/tui`: the `tt` binary. `app.rs` holds the state and applies semantic commands, `keymap.rs` maps raw keys to commands, `ui.rs` renders, `styles.rs` defines the terminal styles, `terminal.rs` owns setup and cleanup, and `main.rs` wires it together.

The event loop is synchronous. Commands apply to a cloned candidate first, so a storage error can never desynchronize memory and SQLite. Successful add, rename, and archive writes update the visible list from the stored result; a tracking write that loses a cross-process conflict reloads the interface state from the database instead.

Terminal setup and teardown are staged: raw mode, the alternate screen, and cursor visibility are tracked in one restoration state shared by the guard and the panic hook, so exactly the completed stages are restored exactly once, and a raw-mode failure writes no escape sequence at all.

## Terminal theming

The TUI uses named ANSI colors, terminal defaults, and reverse video rather than fixed RGB values. The terminal emulator supplies the palette, so the same binary works with local themes and over SSH. It has no Omarchy runtime dependency.

Theme-related source files are `apps/tui/src/styles.rs` and `scripts/audit-omarchy-themes.py`. The audit role mappings mirror `styles.rs`; change both files together. Run the audit against an Omarchy themes checkout:

```sh
python3 scripts/audit-omarchy-themes.py /path/to/omarchy/themes
python3 scripts/audit-omarchy-themes.py --strict /path/to/omarchy/themes
```

The audit discovers theme directory names and requires the current inventory of exactly 17 dark and 5 light themes. The default audit reports contrast misses without failing. `--strict` fails on misses. A few themes may remain known exceptions until their upstream palettes change.

## Validation

Run these before every handoff:

```sh
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo mutants --workspace
cargo llvm-cov --workspace --all-features --lcov --output-path lcov.info
cargo crap --workspace --lcov lcov.info
```

Missed and timed-out mutants fail the check, as do CRAP scores above 30. The pinned tool versions and full rules live in `AGENTS.md`.

## Hooks

The pre-commit hook checks formatting. After cloning, point Git at the repository-owned hooks:

```sh
git config core.hooksPath .githooks
```
