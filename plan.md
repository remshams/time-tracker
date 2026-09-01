# Time tracker plan

## Status

This repository contains planning and agent guidance. Milestones 1 and 2 are implemented: the `tt` TUI renders the task list, tracks time locally in SQLite, and recovers the active timer across restarts. Server sync and additional clients are not started.

Milestones 1 and 2 are approved, together with the bootstrap work they require. Implementation uses the approved subagents below.

## Settled decisions

| Decision | Outcome |
|---|---|
| Product and binary names | Product `Time Tracker`, binary `tt` |
| Rust packages | `tracker-core` library and `tracker-tui` application |
| Initial platforms | Linux and macOS |
| Navigation | Keyboard-first. Vim-style keys are primary, with arrow-key aliases where they make sense. |
| MVP data | Any small hard-coded task list is acceptable. |
| Repository host | Forgejo at `ssh://git@forgejo/remshams/time-tracker`, using push-to-create. Use repository hooks and local validation; do not add hosted CI yet. |
| Quality setup | Use the Jira adapter's `AGENTS.md` validation rules and pinned tool versions, adapted only to describe Time Tracker. Configure the rest of this project's tooling independently. |
| Tenancy | One tracker and one timeline. There is no account or user-management system. |

## Product direction

Build a Rust time tracker that starts as a terminal application on Linux and macOS. It can later support a web client, Omarchy integrations, and other plugins.

The owner should eventually be able to start tracking on one computer, switch computers, and continue without losing elapsed time. The first sync version may assume that only one client writes at a time. We should still model revisions and idempotent commands so an accidental retry does not duplicate or corrupt an entry.

## Recommended technology

### TUI MVP

- Stable Rust with edition 2024.
- A Cargo workspace with two packages:
  - `crates/tracker-core`, a library for task and tracking types.
  - `apps/tui`, the terminal binary.
- `ratatui` for widgets and test rendering.
- `crossterm` for terminal input, alternate-screen handling, and raw mode.
- A synchronous event loop for the hard-coded MVP. Do not add Tokio, storage, or networking before they are needed.
- `ratatui::backend::TestBackend` for deterministic rendering tests. Prefer direct buffer assertions at first. Snapshot tests can be added if the layout becomes costly to assert by hand.
- A central command map for keybindings. Widgets should handle commands such as `MoveDown` or `AddTask`, not raw key events. This keeps Vim keys, arrow aliases, help text, and tests consistent as the TUI grows.

The small core crate is worthwhile even for a hard-coded list. It gives future clients a shared domain model without tying them to terminal code. A larger clean-architecture split would be premature.

### Storage and sync direction after the MVP

This is a recommendation, not part of the first milestone.

- Put local persistence behind a repository trait owned by the core/application layer.
- Use SQLite for local state and a durable outbox of unsynced commands.
- Use a server-authoritative HTTP API. Add Server-Sent Events or WebSockets only when pushed updates are useful. Periodic HTTP sync is enough for the first cross-machine version.
- Give records stable UUIDv7 identifiers. Give each aggregate a monotonically increasing revision.
- Make start, stop, and edit commands idempotent by assigning each command a stable identifier.
- Store timestamps in UTC. While a timer runs in one process, derive its display from a monotonic clock so wall-clock adjustments do not make the visible timer jump.
- Enforce one active timer for the single tracker on the server. Under the initial single-writer rule, reject stale revisions instead of trying to merge concurrent active timers.
- Use PostgreSQL on the server only when the server is introduced. SQLite remains the local client database.

A central server is simpler than peer-to-peer sync and gives future web clients one API. We should not share a SQLite file through cloud storage because file-level sync can corrupt the database and cannot enforce the active-timer rule.

## MVP scope

The first executable milestone does only this:

- Open a full-screen terminal UI.
- Render a fixed list of tasks from Rust data.
- Show which row is selected.
- Move the selection with `j` and `k`, with the up and down arrow keys as aliases.
- Exit with `q` or Escape.
- Establish the keybinding pattern used by later screens: `h`, `j`, `k`, and `l` navigate; mnemonic keys such as `a` invoke actions; the footer shows available keys for the current screen.
- Restore the terminal after normal exit and after errors.
- Test the rendered list and selection movement without requiring an interactive terminal.

The MVP does not track time, persist data, call a server, authenticate users, or run background sync.

A suggested initial list is:

1. Write release notes
2. Fix the coffee machine
3. Plan Friday's demo

The exact text is easy to replace once the intended demo is clear.

## Architecture

```text
time-tracker/
├── Cargo.toml
├── AGENTS.md
├── plan.md
├── .cargo/
│   └── mutants.toml
├── .githooks/
│   └── pre-commit
├── .cargo-crap.toml
├── crates/
│   └── tracker-core/
│       └── src/lib.rs
└── apps/
    └── tui/
        ├── src/app.rs
        ├── src/main.rs
        ├── src/terminal.rs
        └── src/ui.rs
```

Responsibilities:

- `tracker-core` defines `Task` now and tracking commands, entries, and invariants later. It must not depend on Ratatui, Crossterm, a database, or HTTP.
- `app.rs` owns TUI state and converts input events into state transitions.
- `ui.rs` renders state and contains no terminal lifecycle code.
- `terminal.rs` enters and restores raw mode and the alternate screen.
- `main.rs` wires these parts together and reports failures.

## Threat and invariant review

### MVP invariants

- Terminal setup has one owner. Cleanup runs at most once and attempts to restore raw mode, cursor visibility, and the previous screen even when the event loop returns an error.
- Rendering never mutates application state.
- The selected index is either absent for an empty list or points to an existing task.
- Input handling cannot underflow or overflow the selected index.
- The event loop has two lifecycle states, `Running` and `Quitting`. Only a quit input moves it to `Quitting`; no input moves it back.
- The MVP reads no secrets, opens no network connection, and writes no user data.

A panic hook may restore the terminal before delegating to the previous hook. Cleanup should not rely on the hook alone because normal errors and process signals have different paths. Signal handling beyond Crossterm's normal behavior can wait unless testing shows the terminal is left damaged.

### Future tracking invariants

- The single tracker has at most one active entry across all connected clients.
- A stopped entry has an end time that is not earlier than its start time.
- A command retry has the same effect as the first successful command.
- Revisions increase after each accepted write. A client cannot silently overwrite a newer revision.
- Sync acknowledges an outbox command only after the server has durably accepted it.
- Canceling or losing a sync request leaves the command pending for retry.
- The client never invents elapsed time from a negative wall-clock difference.
- A switch between computers is represented as a durable stop or handoff followed by a start or resume. Closing a TUI must not implicitly discard an active timer.

### Security and privacy for later phases

Task names and time entries may expose customer names, work habits, and project details. Local databases and credentials should use owner-only permissions where the platform supports them. Remote sync should use TLS when it leaves a trusted machine, avoid writing secrets or task contents to logs, and provide a documented way to export and delete all tracker data.

No user-management system is planned. A remote deployment is a single-tenant tracker. If it is reachable over a network, clients can use one shared sync token. That token protects the tracker without introducing users, roles, sessions, or account records.

The initial single-writer promise simplifies conflict handling but is not a security control. The server must eventually enforce revisions and the one-active-entry rule even if two clients write by mistake.

## Repository quality setup

Set up the same workflow categories requested for this project, using independent configuration:

- Keep a repository-owned `.githooks/pre-commit` and set `core.hooksPath` to `.githooks`.
- The pre-commit hook runs `cargo fmt --all --check`. Keep it fast. Run broader checks in the validation command and CI.
- Configure Clippy to fail on warnings with:
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- Run tests with `cargo test --workspace --all-features`.
- Configure `.cargo/mutants.toml` with `minimum_test_timeout = 20.0`.
- Generate coverage with `cargo llvm-cov --workspace --all-features --lcov --output-path lcov.info`.
- Configure `.cargo-crap.toml` with a maximum CRAP score of 30, pessimistic handling of missing coverage, and failure above the threshold.
- Add `target/`, `mutants.out*`, and `lcov.info` to `.gitignore`.
- Keep the validation rules copied from the Jira adapter in `AGENTS.md`, including its pinned versions:
  - `cargo-mutants` 27.1.0
  - `cargo-llvm-cov` 0.9.0
  - `cargo-crap` 0.4.3
- Use repository hooks and local validation for now. Do not add a Forgejo Actions workflow.

Required handoff checks for each feature:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo mutants --workspace
cargo llvm-cov --workspace --all-features --lcov --output-path lcov.info
cargo crap --lcov lcov.info
```

Missed and timed-out mutants fail the check. CRAP scores above 30 fail the check. If Ratatui or Crossterm glue produces meaningless mutants, exclude only named functions and explain each exclusion in `AGENTS.md`.

## Implementation steps

### Milestone 0: settle names and bootstrap the repository

- [x] Answer the remaining blocking questions below.
- [x] Initialize Git on `main`.
- [x] Create the Cargo workspace and the two packages.
- [x] Copy and adapt the Jira adapter's `AGENTS.md`, retaining its general and validation rules.
- [x] Add `.gitignore`, quality configuration, and the pre-commit hook.
- [x] Set `git config core.hooksPath .githooks`.
- [x] Add a short README with Linux and macOS prerequisites, build, run, keybinding, and validation commands.
- [x] Run all checks that apply to the empty skeleton.
- [x] Commit as `chore(Workspace): bootstrap Rust workspace and quality checks`.
- [x] Add the Forgejo SSH remote and push `main` to create the remote repository.

### Milestone 1: render the hard-coded task list

- [x] Define the small `Task` type in `tracker-core`.
- [x] Add TUI state with arbitrary hard-coded tasks and bounded selection movement.
- [x] Render a title, task list, selected row, and one-line key help.
- [x] Add a central command map and keyboard handling for `j`, `k`, arrow-key aliases, and exit.
- [x] Reserve `h` and `l` for horizontal or parent/child navigation when a screen has that concept. Do not give them a fake action in the one-list MVP.
- [x] Add terminal setup and reliable cleanup.
- [x] Test empty, initial, moved, first-row, and last-row rendering or state behavior.
- [x] Run formatting, Clippy, tests, mutation tests, coverage, and CRAP checks.
- [x] Perform a manual terminal smoke test.
- [x] Commit core and TUI changes separately if both contain meaningful logic:
  - `feat(TrackerCore): add task model`
  - `feat(Tui): render selectable task list`

### Milestone 2: local tracking

- [x] Model reusable tasks with many time entries.
- [x] Define idle and running tracking states and commands in `tracker-core`.
- [x] Persist tasks, entries, and the active timer in SQLite through a repository trait.
- [x] Store the database in the platform application-data directory on Linux and macOS.
- [x] Add `a` to create a task, `e` to rename it, and `d` to archive it after confirmation.
- [x] Use Space to start or stop the selected task.
- [x] When another task is active, switch tasks by stopping and starting entries in one transaction.
- [x] Keep an active timer running when the TUI exits and recover it when the application restarts.
- [x] Treat each restart of a stopped task as a new time entry.
- [x] Defer manual time-entry editing.
- [x] Add schema migrations and tests around transactions, restart recovery, and clock changes.

### Milestone 3: server sync

- [ ] Write a protocol decision record covering authentication, idempotency, revisions, and handoff behavior.
- [ ] Add a server package with an HTTP API and PostgreSQL storage.
- [ ] Add a local outbox and retry policy.
- [ ] Implement an explicit stop-sync-start flow for changing computers.
- [ ] Detect stale revisions and show a useful conflict instead of overwriting data.
- [ ] Measure switch latency before adding a pushed event channel.

### Milestone 4: more clients

- [ ] Extract a versioned client API or SDK only after the TUI and server prove what callers need.
- [ ] Add a web client against the same server API.
- [ ] Define a narrow plugin command or IPC protocol for Omarchy integrations.
- [ ] Keep plugins outside the process unless a concrete use case requires in-process extensions.

## Approved subagents for milestones 1 and 2

Before each assignment, run a preflight that confirms repository access, relevant files, required Rust tools, and the ability to run tests.

The current session does not use GitHub Copilot, so no GitHub Copilot model may be used. The available catalog lists GLM 5.3 as Z.ai's latest series and GPT-5.6 as OpenAI's latest series. The implementation model is the user-selected synthetic GLM 5.3 Flash. OpenAI models are limited to independent reviews.

| Task | Model | Vendor | Provider | Thinking | Latest-series check | Reason |
|---|---|---|---|---|---|---|
| Bootstrap the workspace, hooks, and quality configuration | `hf:zai-org/GLM-5.3-Flash` | Z.ai | `synthetic` | medium | Eligible. GLM 5.3 is the latest available Z.ai series. | Focused setup work. |
| Implement the task, tracking, and SQLite layers | `hf:zai-org/GLM-5.3-Flash` | Z.ai | `synthetic` | high | Eligible. GLM 5.3 is the latest available Z.ai series. | The user selected this model for implementation. Tests must cover state and transaction invariants. |
| Implement the TUI, keybindings, and terminal lifecycle | `hf:zai-org/GLM-5.3-Flash` | Z.ai | `synthetic` | high | Eligible. GLM 5.3 is the latest available Z.ai series. | The user selected this model for implementation. |
| Review correctness and lifecycle behavior | `gpt-5.6-sol` | OpenAI | `openai-codex` | high | Eligible. GPT-5.6 is the latest available OpenAI series. | Independent review of timer, transaction, recovery, and terminal failure paths. |
| Review security and privacy | `gpt-5.6-luna` | OpenAI | `openai-codex` | medium | Eligible. GPT-5.6 is the latest available OpenAI series. | Independent review of local storage and data exposure. |

Each reviewer must return one complete report with severity, affected files, rationale, and recommended fixes. Address review findings, rerun validation, and commit the reviewed checkpoint before further work.

## Open questions

### Needed before server sync

1. When changing computers, should the owner explicitly stop on one and resume on the other, or should starting on the second computer automatically stop the first timer?
2. Is offline tracking required once sync exists? If yes, what should happen when two offline computers both record time despite the single-writer rule?
3. What does near-time mean for this product: roughly one second, ten seconds, or only when a user starts and stops tracking?
4. Where will the single-tenant sync server run, and will it be reachable outside a trusted local network? This determines whether the first sync version needs TLS and a shared token.
5. Should users edit past entries and add time manually in the first synced release?
6. Do tasks need projects, tags, issue links, or free-form notes? Which one is required first?
7. For Omarchy, what integration is expected first: a launcher command, a status-bar indicator, desktop notifications, or a plugin API?

## Approval gate

Milestones 1 and 2, their bootstrap prerequisite, the functional defaults recorded above, and the listed subagents are approved. Server sync and additional clients still require a later planning and approval round.
