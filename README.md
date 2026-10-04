# Time Tracker

A time tracker written in Rust. `tt` provides the keyboard-driven TUI, and `tt-cli` provides commands for scripts and external agents. Both run locally or connect to a separate `tt` server on Linux and macOS. `plan.md` is the source of truth for scope and milestones.

## Current state

The TUI works end to end. Local mode stores tasks and worklogs in SQLite. Remote mode sends the same actions to a separately running server, which owns its own SQLite database. Both modes seed a new database with three example tasks and keep one timer running across restarts: quitting never stops the active worklog, and the next start resumes it. Tasks are reusable, so starting a task again records a new worklog instead of resuming an old one. Archived tasks can be browsed in their own view and restored from there; restoring preserves the task's identifier, name, and worklogs.

Tasks carry creation and metadata-update timestamps. The default list order puts the most recently worked tasks first without loading their full worklog histories. Tasks without worklogs follow, newest first. The TUI can also order tasks by their latest metadata update or creation time. Starting or switching tracking updates that task's latest-work value and re-sorts the default view, while selection stays with the task.

Both task views support fuzzy name search. Matching tasks appear in order of their latest activity, using the newer of the task's metadata update and latest worklog start. The worklog move dialog uses the same order for its fuzzy destination matches.

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
cargo run -p tracker-cli -- --help
cargo install --path apps/cli   # install the binary as `tt-cli`
```

### Command line interface

`tt-cli` provides task management, tracking, worklog history and editing, and reports without opening a terminal interface. It uses the same local database and application rules as the TUI, or the same remote server through `--server URL`. See the [CLI reference](docs/cli.md) for commands, JSON output, and guarded writes.

### Remote mode

Start a server on an explicit loopback or Tailscale address, then point one or more clients at it:

```sh
tt serve --bind 127.0.0.1:8765
tt --server http://127.0.0.1:8765
```

Use `--db PATH` after `serve` to choose the server database. Without it, the server uses `tt-server.db` in the application-data directory. The local TUI continues to use `tt.db`; starting either mode does not copy or merge data. The client uses the server for every task, timer, history, and report action and never opens the server database directly. It refreshes shared state about once a second and reconnects after interruptions. If a write loses its response, check the refreshed server state before repeating it; the server keeps the timer running while clients are disconnected.

The first remote release assumes a trusted Tailscale network or localhost. HTTP traffic has no application-level authentication or TLS, so bind only to an address available to trusted peers. The server rejects wildcard, public, and ordinary LAN bind addresses and checks non-loopback addresses against the device's current `tailscale ip` output. It also rejects unexpected HTTP Host names. To export or remove the data, stop the server and copy or delete its SQLite database file and associated SQLite sidecar files.

## Data and database location

The database lives in the platform application-data directory:

- Linux: `$XDG_DATA_HOME/Time Tracker/tt.db`, or `~/.local/share/Time Tracker/tt.db` when `XDG_DATA_HOME` is unset
- macOS: `~/Library/Application Support/Time Tracker/tt.db`

On first start the directory is created with mode 0700 and the database file with mode 0600. On every start the final directory and the database file are checked: a symbolic link or an object owned by another user is refused, and permissions of owner-owned objects are repaired to 0700 and 0600. The database itself is opened with `SQLITE_OPEN_NOFOLLOW`.

A brand-new empty database is seeded once with three tasks: Write release notes, Fix the coffee machine, and Plan Friday's demo. The emptiness check and the inserts run in one immediate transaction, so a failure leaves no partial seed and concurrent starts cannot seed twice. A database that already has tasks is left untouched. Migrations are concurrency-safe and a database written by a newer version of Time Tracker is refused with a clear error.

## Keybindings

Navigation is keyboard-first. The footer always lists the keys available in the current mode.

The list panel shows Active and Archived as tabs on its top border, with the current view highlighted. The bottom right border shows the current sort order. Active opens by default. Each view remembers its selection and shows its own empty-state text. The timer header keeps showing the running task and elapsed time in the Archived view.

Shared:

- `Tab`: move to the next task tab. `Shift+Tab`: move to the previous one. Navigation wraps between Active and Archived. The compact footer writes `⇧tab` for Shift+Tab.
- `j` / `k` or Down / Up: move the selection, with safe bounds at both ends.
- `s`: cycle through Recently worked, Recently updated, and Recently created ordering. One session-only choice applies to both task views, and selection follows the same task when its row changes.
- `/`: search names in the current task view. Matches update as you type and use latest-activity order even when another list sort is selected. Up and Down navigate results while typing; Enter keeps the filter and restores normal task actions. Escape cancels editing, or clears a committed filter. Switching views clears the filter. At 60 columns the footer uses `␣` for Space.
- `q` or Escape: quit. An active timer keeps running and is recovered on the next start.
- Only unmodified keys act in the task list and confirmation modes; modifier chords other than Ctrl+C are ignored.

Active view only:

- Space: start the selected task, stop the active task, or switch from the active task to the selected one in a single transaction. Stop and switch instants come from the monotonic elapsed clock, so the stored duration always matches what was displayed, even across system clock adjustments. The write also compares the active start, so a correction from another `tt` process forces a state reload and retry instead of saving a different duration. If another process wins any tracking conflict first, the screen reloads tasks, the active timer, and the elapsed clock from the database before the concise error is shown.
- `a`: add a task. `e`: rename the selected task (the input starts pre-filled). `d`: archive the selected task after confirmation. `D`: preview and archive every active task with no worklog in the last 14 days, after confirmation. New tasks get 14 days before they qualify. A running worklog protects its task. The bulk action includes tasks hidden by search. The active task cannot be archived. After a successful add, rename, or archive, the list is updated from the stored result.

Archiving stays in the Active view and remembers the archived task's identifier, so the Archived view selects that task the next time it is opened.

Archived view only:

- `u`: restore the selected task to the Active view without confirmation. The restore stays in the Archived view and removes the row there, and the Active view remembers the restored task for selection. The task keeps its identifier, name, and worklogs. A successful restore uses the authoritative stored task. After a successful write or an ordinary write failure, the application refreshes active tracking; a refresh failure is reported instead of claiming authoritative state.

Worklog history:

- Enter opens the selected task's history from either task view.
- The panel title shows the path back to the source view, for example `Active › Build testing › Worklogs`. Long task names are shortened to keep `Worklogs` visible.
- `j` / `k` or Down / Up moves between loaded worklogs without wrapping.
- `e` corrects the selected worklog. Completed worklogs expose start and end; active worklogs expose only start.
- `m` opens the move dialog and keeps the user in the source history while they search for a destination. Search is fuzzy, ranks matches by latest activity, and excludes the source and archived tasks. Tab and Shift+Tab switch between search and results; Up and Down navigate destination results even while search has focus, while `j` and `k` navigate only in the result list. Enter moves the worklog, and Escape cancels.
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

Worklog move:

- Moving is separate from correction. Correction preserves the task; move changes only the owning task.
- A move preserves the worklog ID, timestamps, and active or completed state. It rejects archived destinations, same-task overlap, active-worklog conflicts, stale source data, and cancellation without changing storage. A successful move returns both task aggregates, increments both tasks' history revisions, leaves the UI in the source history, and reloads that history without changing either task's `updated_at`.

For the rare valid year outside `0000` through `9999`, chrono uses a signed expanded year such as `-0001` or `+10000`; the month, day, hour, and minute layout stays the same.

Task text input (`a` and `e`):

- Printable characters and Backspace edit the text; Space is ordinary input. Input stops growing at 256 characters, the task-name limit.
- Enter confirms, Escape cancels. An invalid name (empty, control characters, or too long) shows an error and keeps the input open.
- The modal scrolls horizontally: with a long text the tail of the buffer and the cursor stay visible, cutting only on whole characters.

Archive confirmation:

- `y` or Enter confirms, `n` or Escape cancels.
- `D` on the Active tab previews the number of inactive tasks and sample names. A task qualifies only if it was created and its metadata last changed more than 14 days ago and has no worklog overlapping that UTC window or running timer. Confirmation rechecks the same candidate set and archives it in one transaction. If another client changes the set or server revision, open a fresh preview before trying again. The server accepts the preview time only while it is within 15 minutes of its clock.

Ctrl+C quits from every mode.

## Architecture

The workspace has eight packages:

- `apps/cli`: the `tt-cli` binary. Command parsing and handlers are grouped by tasks, tracking, worklogs, and reports. It uses the existing application services and remote client; it does not access raw SQL or set up a terminal.

- `crates/tracker-domain`: tasks, encapsulated worklogs, tracking state, identifiers, and timestamp-correction invariants. `Task` owns its identity, and `Worklog` owns reusable same-task half-open overlap semantics. The crate has no application, terminal, database, or network code.
- `crates/tracker-application`: backend-neutral repository ports and synchronous use cases. Its source is split into `error`, `model`, `repository`, and task, tracking, and worklog service modules. Presentation code uses semantic application failure categories rather than matching repository failures. Commands return the exact domain value when there is no alternative outcome.
- `crates/tracker-storage`: SQLite persistence. Its adapter is split into `mapping`, `tasks`, `tracking`, and `worklogs`. `SqliteRepository` still owns one `Connection`; transaction-aware helpers continue to take an explicit `&Connection`. Application repository ports contain only the queries and writes used by application workflows. Direct SQLite diagnostics compile for storage tests and through the opt-in `test-support` feature used by TUI tests.
- `crates/tracker-protocol`: versioned JSON requests and responses shared by the HTTP adapters.
- `crates/tracker-server`: REST routes that run the application service against the server's SQLite database. It serializes requests against one application instance and checks revisions on writes.
- `crates/tracker-remote`: HTTP adapter implementing the application-service ports for the TUI. It keeps the last confirmed snapshot, sends client-created UTC timestamps, and refreshes after conflicts and reconnections.
- `apps/tui`: the `tt` binary. `app.rs` is the presentation controller. Its private `TaskCatalog`, `TrackingSession`, and `ShellState` owners live under `app/`. `TaskCatalog` owns task data, ordering, and lookup. `ScreenState` owns task-list state, and history moves that exact state in and out when users return. `ShellState` owns the startup timezone, status, lifecycle, and screen. Only the controller calls the application service. The folders under `screens` contain screen state, commands, keymaps, rendering, and tests. Each screen owns its semantic command enum. `command.rs` contains only the event-loop wrapper, which tags a task-list payload, tags a worklog-history payload, or requests global quit. `App::handle` dispatches a tagged payload only when its screen is active. Its single mismatch guard leaves dormant state untouched and makes no application-service call. `components` contains shared stateless rendering functions, while `support` contains clock, timestamp, and error helpers. `ui.rs` renders an immutable `AppView`; it never receives the controller or application service. `terminal.rs` owns setup and cleanup, and `main.rs` creates SQLite storage and the application service before starting the TUI.

The architecture cleanup preserves observable behavior. The domain, application, and SQLite split remains intact. In the TUI, `TaskCatalog` keeps task data across screen transitions, history retains the exact task-list state to restore, tracking state stays paired with its monotonic clock, and shell state owns the screen, status, lifecycle, and startup timezone.

The TUI uses one Tokio event loop for terminal events, a fixed redraw tick, and application request completions. It executes local requests synchronously and awaits remote HTTP requests without blocking terminal input. Screen keymaps return typed local commands or a mode-sensitive quit request. The shared key dispatcher filters non-press events, handles Ctrl+C globally, wraps local payloads with the active screen tag, and normalizes quit requests to the global command. At startup, the TUI resolves one IANA timezone-rules snapshot and uses it for the rest of the process. `tracker-application` validates tracking and correction candidates before writing. It uses atomic repository operations for switches, compare-and-set correction, exact compare-and-delete, and bulk archiving. Bulk archive previews read the full candidate set; confirmation rechecks it inside one write transaction. A deletion accepts only the same ID, task, start, and end the UI read. SQLite returns the affected task's latest-work aggregate from that delete transaction, using `MAX(start_us)`, so the application updates task ordering without a post-commit race. The schema guard at version 5 rejects every direct SQL delete of an active worklog. Deletion leaves history cursor revisions unchanged because it does not move any remaining row. Worklog history uses 50-record cursor pages ordered by start descending and `WorklogId` ascending. Cursors carry their task identifier and its history revision, so they cannot cross tasks and another client's start correction resets pagination instead of omitting or duplicating a moved row. Inserts and end-only changes keep cursors valid. Each history page, the requested and active tasks' latest-work aggregates, and global active state come from one bounded read snapshot, so recently-worked ordering, a running row, and the timer header agree without scanning every task's history. Clearing tracking includes the worklog the TUI expects to stop, so stale state cannot stop another process's timer. The TUI displays worklog timestamps in local `YYYY-MM-DD HH:MM` form. Changed correction fields align to local wall-clock second 00 before conversion to UTC. Historical offsets can therefore produce a UTC instant with nonzero seconds. Unchanged fields retain their stored UTC seconds and microseconds. Durations and live timers remain `HH:MM:SS`. The TUI does not sequence persistence or handle SQLite errors.

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
