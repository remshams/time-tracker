# Time Tracker

A keyboard-first time tracker written in Rust. `tt` is a terminal application for Linux and macOS; optional remote storage and more clients are planned. `plan.md` is the source of truth for scope and milestones.

## Current state

The TUI works end to end. It stores tasks and worklogs in SQLite, seeds a new database with three example tasks, and keeps one timer running across restarts: quitting never stops the active worklog, and the next start resumes it. Tasks are reusable, so starting a task again records a new worklog instead of resuming an old one. Archived tasks can be browsed in their own view and restored from there; restoring preserves the task's identifier, name, and worklogs.

Tasks carry creation and metadata-update timestamps. The default list order puts the most recently worked tasks first without loading their full worklog histories. Tasks without worklogs follow, newest first. The TUI can also order tasks by their latest metadata update or creation time. Starting or switching tracking updates that task's latest-work value and re-sorts the default view, while selection stays with the task.

Enter opens the selected task's worklog history from either task view. History loads newest first in bounded batches of 50 and includes the active worklog. Timestamps display in an IANA timezone resolved once when the process starts, as `YYYY-MM-DD HH:MM`, without seconds, fractions, or a visible offset. The active row shares the timer header's monotonic duration. A selected history row can be corrected without changing its identity, task, or active state. Completed rows can also be permanently deleted. Press `d` to open a confirmation, then press `y` or Enter. Pressing `d` again confirms too. Escape or `n` cancels. Running worklogs are rejected with the exact message `Running worklogs cannot be deleted`. Deletion works for archived-task history. After deletion, history selects the row now at the deleted index. If no row remains there, it selects the preceding final row. Only empty history has no selection.

The database enforces the tracking rules itself, so a second `tt` process sees the same bounds: an archived task cannot receive new worklogs, a task with an active worklog cannot be archived, at most one worklog is active, and worklogs for the same task cannot overlap. Touching and zero-duration intervals are valid, and worklogs for different tasks may overlap. Deletion compares the exact worklog ID, task ID, start, and end before removing a completed row. The delete transaction also returns the task's latest worklog start, so task ordering does not rely on a second read. Seeding, switching, correction, and deletion compare-and-set writes run in transactions, so simultaneous processes neither double-seed a new database nor silently overwrite stale state.

While a timer runs, the visible elapsed time comes from a monotonic clock anchored to the worklog's UTC start, so system clock adjustments do not make the timer jump. Stopping or switching derives its UTC instant from the same clock, so the persisted duration always matches the displayed one. Correcting an active start re-anchors that monotonic clock.

Task names are trimmed, non-empty, at most 256 characters long, and free of control characters. The same rules guard names read back from the database. Renaming, archiving, or restoring a task advances its metadata-update timestamp without moving it backward. Tracking activity does not change that timestamp because recent work is derived from worklogs separately.

## Prerequisites

- Linux or macOS
- A terminal at least 60 columns wide. Narrower terminals show a resize message instead of the interactive interface.
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

Two task views share the same list screen. The Active view is the default at startup. Both views support selection movement and quitting, remember their own selection, and show their own text when they have no tasks ("No active tasks." versus "No archived tasks."). The timer header keeps showing the running task's name and elapsed time in the Archived view too.

Shared:

- `h`: switch to the Active view. `l`: switch to the Archived view. Switching to the view already shown does nothing.
- `j` / `k` or Down / Up: move the selection, with safe bounds at both ends.
- `s`: cycle through Recently worked, Recently updated, and Recently created ordering. One session-only choice applies to both task views, and selection follows the same task when its row changes.
- `q` or Escape: quit. An active timer keeps running and is recovered on the next start.
- Only unmodified keys act in the task list and confirmation modes; modifier chords other than Ctrl+C are ignored.

Active view only:

- Space: start the selected task, stop the active task, or switch from the active task to the selected one in a single transaction. Stop and switch instants come from the monotonic elapsed clock, so the stored duration always matches what was displayed, even across system clock adjustments. The write also compares the active start, so a correction from another `tt` process forces a state reload and retry instead of saving a different duration. If another process wins any tracking conflict first, the screen reloads tasks, the active timer, and the elapsed clock from the database before the concise error is shown.
- `a`: add a task. `e`: rename the selected task (the input starts pre-filled). `d`: archive the selected task after confirmation. The active task cannot be archived. After a successful add, rename, or archive, the list is updated in place from the stored result.

Archiving stays in the Active view and remembers the archived task's identifier, so the Archived view selects that task the next time it is opened.

Archived view only:

- `u`: restore the selected task to the Active view without confirmation. The restore stays in the Archived view and removes the row there, and the Active view remembers the restored task for selection. The task keeps its identifier, name, and worklogs. A successful restore uses the authoritative stored task. After a successful write or an ordinary write failure, the application refreshes active tracking; a refresh failure is reported instead of claiming authoritative state.

Worklog history:

- Enter opens the selected task's history from either task view.
- `j` / `k` or Down / Up moves between loaded worklogs without wrapping.
- `e` corrects the selected worklog. Completed worklogs expose start and end; active worklogs expose only start.
- `d` opens a permanent-deletion confirmation for a completed row. `y` or Enter confirms. A second `d` confirms the deletion, so `y` or Enter is not needed. `n` or Escape cancels. The dialog shows the interval and warns that the action cannot be undone. Running rows are never deletable, and archived-task history supports the same action. If deleting loaded rows exposes an older page, `o` loads it and selects its first row.
- `o` loads the next batch of older worklogs when one exists.
- `r` discards the loaded snapshot and reloads its newest batch.
- Escape returns to the same task and task view. `q` and Ctrl+C quit without stopping active tracking.

Worklog correction:

- The fields contain local `YYYY-MM-DD HH:MM` timestamps. Input is local time. Changed fields align to local wall-clock second 00 before conversion to UTC, while unchanged fields keep their stored seconds and fractions. Times skipped by a daylight-saving change are rejected. When a local time occurs twice, the editor keeps the original timestamp's offset where possible.
- `Tab` or `Shift+Tab` switches between start and end when both exist.
- `j` and `k` move the focused displayed minute forward and backward by five minutes. `J` and `K` do the same by one hour. A nudge makes the field minute-precise, so it discards that field's hidden seconds and fractions before applying the adjustment. If a historical second-level offset change makes the result impossible to represent exactly as a local minute, the adjustment is rejected and the draft stays open.
- Typing edits at the cursor. Left and Right move the cursor; Backspace and Delete remove characters.
- Enter saves. Escape cancels without writing. Ctrl+C quits without writing or stopping active tracking.
- Invalid, overlapping, and stale corrections keep the draft open. The app uses one timezone-rules snapshot for history, correction, parsing, ambiguity handling, and nudges. It first accepts a valid `TZ` IANA name, including common colon and zoneinfo-path forms, then asks Linux or macOS for the OS IANA timezone. If both fail, it uses UTC and shows an error status. A timestamp whose local conversion falls outside chrono's editable range cannot open correction. A successful correction reloads the newest history page because changing a start may reorder the rows. If that reload fails after the save commits, history is marked unavailable until `r` succeeds rather than showing rows from an obsolete ordering snapshot.

For the rare valid year outside `0000` through `9999`, chrono uses a signed expanded year such as `-0001` or `+10000`; the month, day, hour, and minute layout stays the same.

Task text input (`a` and `e`):

- Printable characters and Backspace edit the text; Space is ordinary input. Input stops growing at 256 characters, the task-name limit.
- Enter confirms, Escape cancels. An invalid name (empty, control characters, or too long) shows an error and keeps the input open.
- The modal scrolls horizontally: with a long text the tail of the buffer and the cursor stay visible, cutting only on whole characters.

Archive confirmation:

- `y` or Enter confirms, `n` or Escape cancels.

Ctrl+C quits from every mode.

## Architecture

The workspace has four packages:

- `crates/tracker-domain`: tasks, encapsulated worklogs, tracking state, identifiers, and timestamp-correction invariants. `Task` owns its identity, and `Worklog` owns reusable same-task half-open overlap semantics. The crate has no application, terminal, database, or network code.
- `crates/tracker-application`: backend-neutral repository ports and synchronous use cases. Its source is split into `error`, `model`, `repository`, and task, tracking, and worklog service modules. Presentation code uses semantic application failure categories rather than matching repository failures. Commands return the exact domain value when there is no alternative outcome.
- `crates/tracker-storage`: SQLite persistence. Its adapter is split into `mapping`, `tasks`, `tracking`, and `worklogs`. `SqliteRepository` still owns one `Connection`; transaction-aware helpers continue to take an explicit `&Connection`. Application repository ports contain only the queries and writes used by application workflows. Direct SQLite diagnostics compile for storage tests and through the opt-in `test-support` feature used by TUI tests.
- `apps/tui`: the `tt` binary. `app.rs` is the presentation controller. Its private `TaskCatalog`, `TrackingSession`, and `ShellState` owners live under `app/`. `TaskCatalog` owns task data, ordering, and lookup. `ScreenState` owns task-list state, and history moves that exact state in and out when users return. `ShellState` owns the startup timezone, status, lifecycle, and screen. Only the controller calls the application service. The folders under `screens` contain screen state, commands, keymaps, rendering, and tests. Each screen owns its semantic command enum. `command.rs` contains only the event-loop wrapper, which tags a task-list payload, tags a worklog-history payload, or requests global quit. `App::handle` dispatches a tagged payload only when its screen is active. Its single mismatch guard leaves dormant state untouched and makes no application-service call. `components` contains shared stateless rendering functions, while `support` contains clock, timestamp, and error helpers. `ui.rs` renders an immutable `AppView`; it never receives the controller or application service. `terminal.rs` owns setup and cleanup, and `main.rs` creates SQLite storage and the application service before starting the TUI.

The architecture cleanup preserves observable behavior. The domain, application, and SQLite split remains intact. In the TUI, `TaskCatalog` keeps task data across screen transitions, history retains the exact task-list state to restore, tracking state stays paired with its monotonic clock, and shell state owns the screen, status, lifecycle, and startup timezone.

The event loop stays synchronous. Screen keymaps return typed local commands or a mode-sensitive quit request. The shared key dispatcher filters non-press events, handles Ctrl+C globally, wraps local payloads with the active screen tag, and normalizes quit requests to the global command. At startup, the TUI resolves one IANA timezone-rules snapshot and uses it for the rest of the process. `tracker-application` validates tracking and correction candidates before writing. It uses atomic repository operations for switches, compare-and-set correction, and exact compare-and-delete. A deletion accepts only the same ID, task, start, and end the UI read. SQLite returns the affected task's latest-work aggregate from that delete transaction, using `MAX(start_us)`, so the application updates task ordering without a post-commit race. The schema guard at version 5 rejects every direct SQL delete of an active worklog. Deletion leaves history cursor revisions unchanged because it does not move any remaining row. Worklog history uses 50-record cursor pages ordered by start descending and `WorklogId` ascending. Cursors carry their task identifier and its history revision, so they cannot cross tasks and another client's start correction resets pagination instead of omitting or duplicating a moved row. Inserts and end-only changes keep cursors valid. Each history page, the requested and active tasks' latest-work aggregates, and global active state come from one bounded read snapshot, so recently-worked ordering, a running row, and the timer header agree without scanning every task's history. Clearing tracking includes the worklog the TUI expects to stop, so stale state cannot stop another process's timer. The TUI displays worklog timestamps in local `YYYY-MM-DD HH:MM` form. Changed correction fields align to local wall-clock second 00 before conversion to UTC. Historical offsets can therefore produce a UTC instant with nonzero seconds. Unchanged fields retain their stored UTC seconds and microseconds. Durations and live timers remain `HH:MM:SS`. The TUI does not sequence persistence or handle SQLite errors.

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
