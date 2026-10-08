# Time tracker plan

## Status

Milestones 0, 1, and 2, the terminal-palette checkpoint, the domain/application separation, task browsing and ordering, task search, read-only worklog history, worklog correction, completed-worklog deletion, worklog moves, and the screen-scoped TUI command refactor are complete. The `tt` TUI renders and edits tasks, tracks time in local SQLite storage, recovers the active timer across restarts, browses and restores archived tasks, orders and searches both task views, and loads one task's worklogs in bounded pages. The timestamp and ordering rules live in [ADR 0002](docs/adr/0002-task-timestamps-and-ordering.md), [ADR 0003](docs/adr/0003-canonical-timestamp-precision.md) fixes their precision at microseconds, [ADR 0004](docs/adr/0004-paginated-worklog-history.md) records pagination, [ADR 0006](docs/adr/0006-local-minute-tui-timestamps.md) records local-minute history and correction behavior, [ADR 0007](docs/adr/0007-completed-worklog-deletion.md) records completed-worklog deletion, [ADR 0008](docs/adr/0008-move-worklogs-between-tasks.md) records worklog moves, and [ADR 0009](docs/adr/0009-task-search.md) records task search.

The required Rust, Python, mutation, coverage, CRAP, and Linux PTY checks pass. A native macOS run also passes the full validation suite, including the PTY lifecycle test and file-backed SQLite tests. Detailed counts belong in each feature handoff rather than this long-lived plan. The reproducible audit covers all 17 dark and 5 light bundled Omarchy themes. Normal and selected text pass their thresholds; six known accent-role exceptions remain deferred.

## Settled decisions

| Decision | Outcome |
|---|---|
| Product and binary names | Product `Time Tracker`, binary `tt` |
| Rust packages | `tracker-domain`, `tracker-application`, `tracker-storage`, and the `tracker-tui` application |
| Initial platforms | Linux and macOS |
| Navigation | Keyboard-first. Vim-style keys are primary, with arrow-key aliases where they make sense. |
| MVP data | Any small hard-coded task list is acceptable. |
| Repository host | Forgejo at `ssh://git@forgejo/remshams/time-tracker`, using push-to-create. Use repository hooks and local validation; do not add hosted CI yet. |
| Quality setup | Use the Jira adapter's `AGENTS.md` validation rules and pinned tool versions, adapted only to describe Time Tracker. Configure the rest of this project's tooling independently. |
| Tenancy | One tracker and one timeline. There is no account or user-management system. |
| Storage selection | With no endpoint configured, `tt` uses local SQLite. With an endpoint configured, it uses that remote store exclusively and never falls back to local writes. |
| Initial remote deployment | The same `tt` binary provides TUI and server modes. Client and server may initially assume the same version and communicate inside a trusted Tailscale network. The server uses SQLite first. |
| Domain terminology | A persisted tracking record is a `Worklog`. It is active while its end time is absent and completed once its end time is set. |
| Layering direction | Use `tracker-domain` for the domain model, put repository-backed use cases in `tracker-application`, and keep local SQLite and future HTTP implementations as adapters. |
| Timestamp authority | The client creates task and tracking timestamps in local and remote modes. The initial remote design assumes client and server clocks are sufficiently synchronized; the server enforces ordinary domain checks such as end not preceding start but adds no clock-skew protocol. |
| Pre-release schema compatibility | Rewriting the baseline was limited to the completed domain/application refactor. The later task-timestamp decision preserves version 1 task and worklog data through the migration specified by [ADR 0002](docs/adr/0002-task-timestamps-and-ordering.md). |

## Product direction

Build a Rust time tracker that starts as a terminal application on Linux and macOS. It can later support a web client, Omarchy integrations, and other plugins.

The owner should eventually be able to run the TUI on one computer while the authoritative data store runs on another. Each TUI process selects exactly one store: local SQLite when no endpoint is configured, or the remote HTTP service when an endpoint is configured. Remote mode is online-only at first and never falls back to local writes. Switching between local and remote modes does not merge their databases automatically.

The same `tt` binary will provide the TUI and server process modes, while shared application workflows remain independent of either entry point. The first deployment may assume that client and server run the same version inside a trusted Tailscale network. The API should still use a `/v1` prefix and idempotent state-setting operations where retries could otherwise duplicate or reverse a change.

## Technology and target architecture

### TUI and workspace

- Use stable Rust with edition 2024.
- Keep four packages: `tracker-domain`, `tracker-application`, `tracker-storage`, and the `tracker-tui` application.
- Use `ratatui` for widgets and deterministic test rendering.
- Use `crossterm` for terminal input, alternate-screen handling, and raw mode.
- Keep the current synchronous event loop during the domain/application refactor. Do not add Tokio or networking in that checkpoint.
- Keep semantic command mapping centralized per screen so keybindings, footer help, and tests remain consistent. The event loop receives a global wrapper around those screen-owned payloads.

The domain/application split gives the future server and clients reusable use cases without moving presentation or infrastructure into the domain. Keep one time-tracking bounded context; separate crates for individual entities would add no useful boundary.

### Storage and remote-access direction after the MVP

- Keep persistence behind repository ports owned by the application layer.
- Put a backend-neutral application service between every UI and its selected store.
- In local mode, the application service uses `SqliteRepository` directly.
- In remote mode, an HTTP client calls resource-oriented REST endpoints. It does not expose raw repository or SQL operations over the network.
- Let the same `tt` binary run as a TUI or as an HTTP server through separate composition paths.
- Start the server with SQLite. PostgreSQL remains an option only if measured server load warrants it.
- Let clients query tasks, current tracking state, and worklogs through resource-oriented endpoints. Any batching optimization remains internal to the HTTP adapter and is not a TUI concept.
- Represent tracking as desired state: setting an active task starts or switches atomically, and clearing the active task stops it. Retrying the same request must not toggle the state back.
- Let the client create UTC timestamps in both local and remote modes. A remote command carries the timestamp captured by the client, and a retry reuses that value.
- Assume client and server clocks are sufficiently synchronized initially. The server rejects ordinary invalid intervals such as an end before its start but adds no clock-skew detection, correction, or conflict flow.
- Keep stable UUIDv7 record identifiers and UTC timestamps.
- Never switch a configured remote client to local writes after a connection failure.
- Do not merge an existing local database into a remote tracker automatically. Any future import must be an explicit operation.
- Add pushed updates only after polling or refresh-on-focus proves insufficient.

A central HTTP service is simpler than file sharing or synchronization for the first remote release. It gives future clients one API while preserving the one-active-timer rule. Offline synchronization remains a separate, later feature.

## Completed MVP scope

The first executable milestone did only this:

- Open a full-screen terminal UI.
- Render a fixed list of tasks from Rust data.
- Show which row is selected.
- Move the selection with `j` and `k`, with the up and down arrow keys as aliases.
- Exit with `q` or Escape.
- Establish the keybinding pattern used by later screens: `h`, `j`, `k`, and `l` navigate; mnemonic keys such as `a` invoke actions; the footer shows available keys for the current screen.
- Restore the terminal after normal exit and after errors.
- Test the rendered list and selection movement without requiring an interactive terminal.

That milestone did not track time, persist data, call a server, authenticate users, or run background sync. Milestone 2 subsequently added local tracking and persistence.

A suggested initial list is:

1. Write release notes
2. Fix the coffee machine
3. Plan Friday's demo

The exact text is easy to replace once the intended demo is clear.

## Current architecture

The architecture refactors are complete without observable behavior changes. The domain, application, and SQLite adapter remain separate. The TUI now splits global presentation state into task, tracking, and shell owners behind one App controller and immutable render projection.

Responsibilities and current module boundaries:

- `tracker-domain` defines task identity and domain invariants. `Worklog` owns reusable same-task half-open overlap semantics, including touching and zero-duration rules.
- `tracker-application` is split into `error`, `model`, `repository`, and task, tracking, and worklog service modules. Presentation consumes semantic application failure classifications instead of matching repository errors. Successful commands return exact domain values when there is no alternative outcome.
- Application repository ports expose only the queries and writes used by application workflows. SQLite may retain additional diagnostic queries.
- `tracker-storage` keeps its SQLite adapter in `mapping`, `tasks`, `tracking`, and `worklogs` modules. `SqliteRepository` still owns one `Connection`, and transaction-aware helpers continue to accept an explicit `&Connection`.
- Large application and SQLite tests are split by concern.
- `apps/tui/src/app.rs` is the global presentation controller. Its four private fields are the application service, `TaskCatalog`, `TrackingSession`, and `ShellState`; only App orchestration calls the service.
- `TaskCatalog` owns both task collections, ordering, and lookup. `TrackingSession` keeps `TrackingState` and its clock together. `ShellState` owns the exclusive `ScreenState`, status, lifecycle, and one startup `chrono_tz::Tz`.
- The folders under `apps/tui/src/screens` contain screen state, commands, keymaps, rendering, and tests. `ScreenState` owns `TaskListState`. Worklog history moves and retains that exact return state, while `TaskCatalog` keeps task data separate from it.
- Each screen owns its semantic command enum. The event loop sees only `Command::TaskList(TaskListCommand)`, `Command::WorklogHistory(WorklogHistoryCommand)`, or global `Command::Quit`. Screen keymaps return a typed local payload or a mode-sensitive quit request. Shared key handling filters event kinds, handles Ctrl+C, applies the screen tag, and normalizes quit. `App::handle` has one mismatch guard that leaves dormant state untouched and never calls the application service.
- Shared stateless rendering functions live under `components`. Clock, timestamp, and error-presentation helpers live under `support`. `ui.rs` composes a frame from immutable `AppView` data and has no application-service dependency.
- `terminal.rs` owns terminal lifecycle. `main.rs` wires the SQLite adapter, application service, event loop, and TUI.

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

- The single tracker has at most one active worklog across all connected clients.
- A completed worklog has an end time that is not earlier than its start time.
- Retrying a remote request has the same effect as its first successful execution.
- A configured remote client never writes to local SQLite as a fallback.
- The server validates every command and enforces one active timer regardless of client state.
- The client never invents elapsed time from a negative wall-clock difference.
- Setting a different active task is one atomic server operation that stops the old worklog and starts the new one.
- Closing a local or remote TUI must not implicitly discard an active timer.

### Security and privacy for later phases

Task names and worklogs may expose customer names, work habits, and project details. Local databases and credentials should use owner-only permissions where the platform supports them. The initial server may trust a restricted Tailscale network and must bind only to an explicitly selected localhost or Tailscale address. If the API later leaves that trusted network, add TLS and a shared token before exposing it. Never write credentials or task contents to logs, and provide a documented way to export and delete tracker data.

No user-management system is planned. A remote deployment is a single-tenant tracker. Network placement is not a substitute for database invariants: the server must enforce the one-active-worklog rule even when two clients write concurrently.

## Repository quality setup

Set up the same workflow categories requested for this project, using independent configuration:

- Keep a repository-owned `.githooks/pre-commit` and set `core.hooksPath` to `.githooks`.
- The pre-commit hook runs `cargo fmt --all --check`. Keep it fast. Run broader checks in the validation command and CI.
- Configure Clippy to fail on warnings with:
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- Run tests with `cargo test --workspace --all-features`.
- Configure `.cargo/mutants.toml` with `minimum_test_timeout = 20.0`.
- Generate coverage with `cargo llvm-cov --workspace --all-features --lcov --output-path lcov.info`.
- Configure `.cargo-crap.toml` with a maximum CRAP score of 12, pessimistic handling of missing coverage, and failure above the threshold.
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
cargo llvm-cov --workspace --all-features --lcov --output-path lcov.info
cargo crap --workspace --lcov lcov.info
```

Run focused mutation checks using the file and crate scope rules in `AGENTS.md`. Include production code exercised by changed tests and expand the scope for shared code or build input changes. Documentation-only changes skip mutation checks. Routine full mutation runs are deferred until CI is set up.

Missed and timed-out mutants fail the check. CRAP scores above 12 fail the check. If Ratatui or Crossterm glue produces meaningless mutants, exclude only named functions and explain each exclusion in `AGENTS.md`.

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

- [x] Define the small `Task` type in `tracker-core`, which was later renamed to `tracker-domain`.
- [x] Add TUI state with arbitrary hard-coded tasks and bounded selection movement.
- [x] Render a title, task list, selected row, and one-line key help.
- [x] Add a central command map and keyboard handling for `j`, `k`, arrow-key aliases, and exit.
- [x] Reserve `h` and `l` for horizontal or parent/child navigation when a screen has that concept. Do not give them a fake action in the one-list MVP. (True at this milestone; the reservation ended when archived-task navigation later gave them the view-switching role.)
- [x] Add terminal setup and reliable cleanup.
- [x] Test empty, initial, moved, first-row, and last-row rendering or state behavior.
- [x] Run formatting, Clippy, tests, mutation tests, coverage, and CRAP checks.
- [x] Perform a manual terminal smoke test.
- [x] Commit core and TUI changes separately if both contain meaningful logic:
  - `feat(TrackerCore): add task model`
  - `feat(Tui): render selectable task list`

### Milestone 2: local tracking

- [x] Model reusable tasks with many worklogs.
- [x] Define idle and running tracking states and commands in the original `tracker-core` package.
- [x] Persist tasks, worklogs, and the active timer in SQLite through a repository trait.
- [x] Store the database in the platform application-data directory on Linux and macOS.
- [x] Add `a` to create a task, `e` to rename it, and `d` to archive it after confirmation.
- [x] Use Space to start or stop the selected task.
- [x] When another task is active, switch tasks by stopping and starting worklogs in one transaction.
- [x] Keep an active timer running when the TUI exits and recover it when the application restarts.
- [x] Treat each restart of a stopped task as a new worklog.
- [x] Defer manual worklog editing.
- [x] Add schema migrations and tests around transactions, restart recovery, and clock changes.

### Interim checkpoint: terminal palette compatibility

- [x] Keep TUI styling independent of Omarchy-specific files and fixed RGB colors.
- [x] Centralize non-default visual roles in the TUI styles module.
- [x] Use reversed terminal foreground and background colors for selected rows.
- [x] Use default terminal colors for important status text, hints, and the input cursor.
- [x] Use named ANSI accents for titles, focused borders, active markers, and the redundant error label.
- [x] Mark errors with a textual prefix so color is never the only indicator.
- [x] Verify important-text contrast and decorative-accent visibility against all 17 dark and 5 light bundled Omarchy themes.
- [x] Keep the palette audit reproducible with a repository script that reads an Omarchy themes checkout.
- [x] Show the accent border only around the widget that owns focus.
- [x] Cover emitted ANSI roles and terminal restoration in a real-binary PTY test.
- [x] Retain Linux PTY coverage and the full quality checks.

### Interim checkpoint: domain and application separation

1. [x] Capture the passing baseline and map every old worklog type, repository, clock, and storage dependency before moving code.
2. [x] Rename the domain package to `tracker-domain` and adopt `Worklog`, `ActiveWorklog`, and `WorklogId` throughout the domain.
3. [x] Add `tracker-application`, depending only on `tracker-domain`. Define backend-neutral task operations, tracking operations, worklog queries, outcomes, repository errors, and repository ports there.
4. [x] Move repository-backed workflows from `apps/tui/src/app.rs` into application use cases. Accept explicit client-created timestamps, expose `set_active_task` and `clear_active_task`, and keep atomic switching and conflict recovery out of presentation code.
5. [x] Adapt `tracker-storage` to the application repository ports and rename schema, query, index, and trigger terminology to worklogs. Update the baseline schema directly and recreate test or development databases; do not add a compatibility migration before the first production release.
6. [x] Adapt `tracker-tui` to application operations and errors. Keep the monotonic client clock, task list, selection, modes, input buffers, key handling, status text, and rendering in the TUI; remove direct dependencies on repository ports, `SqliteRepository`, and `StorageError` from TUI state.
7. [x] Reduce `apps/tui/src/main.rs` to composition: create the SQLite adapter, application service, and TUI. Preserve the path, seeding, terminal, and startup-error behavior.
8. [x] Remove transitional aliases and the old domain repository port. Verify the final dependency direction and prove `tracker-domain` and `tracker-application` have no Ratatui, Crossterm, rusqlite, or HTTP dependency.
9. [x] Run the full Rust, Python, mutation, coverage, CRAP, theme-audit, and real-binary PTY checks. Recreate the local test database and confirm all user-visible behavior remains unchanged.

This checkpoint does not add worklog browsing, HTTP, asynchronous execution, server modes, remote-request states, revisions, or synchronization metadata.

### Interim checkpoint: task browsing and ordering

The timestamp and ordering decisions for this checkpoint are recorded in [ADR 0002](docs/adr/0002-task-timestamps-and-ordering.md), with canonical precision in [ADR 0003](docs/adr/0003-canonical-timestamp-precision.md).

- [x] Add a TUI view for archived tasks while keeping them out of the default active-task list.
- [x] Decide which actions, if any, are available from the archived-task view before implementing them.
- [x] Add explicit task creation and update timestamps. Creation sets both; rename, archive, and restore advance `updated_at`; idempotent metadata operations and tracking never touch it.
- [x] Add the recently worked, recently updated, and recently created orderings from ADR 0002, with deterministic tie-breakers.
- [x] Add TUI controls for choosing the ordering. The choice is session-only, applies to both views, and cycles only in normal mode.
- [x] Migrate existing tasks without losing their archive state or worklogs.
- [x] Cover active and archived views, each ordering, migration behavior, and empty states in tests.

### Interim checkpoint: read-only worklog history

The pagination and display decisions for this checkpoint are recorded in [ADR 0004](docs/adr/0004-paginated-worklog-history.md).

- [x] Open the selected task's history from the active or archived task view and return to the same selection.
- [x] Load worklogs on demand in newest-first pages of at most 50 records.
- [x] Use a `(start, WorklogId)` cursor so equal starts and concurrent newer inserts do not disturb page boundaries.
- [x] Show local start and end timestamps as `YYYY-MM-DD HH:MM`, with completed durations and a live active duration from the timer's monotonic clock. [ADR 0006](docs/adr/0006-local-minute-tui-timestamps.md) supersedes the earlier explicit-offset display choice.
- [x] Add explicit older-page and refresh commands while keeping history read-only.
- [x] Preserve loaded state after read failures and keep selections by stable identifier.
- [x] Cover completed, active, archived, empty, and paginated histories in real-binary E2E scenarios that do not depend on the production seed.

### Interim checkpoint: worklog correction

The correction decisions are recorded in [ADR 0005](docs/adr/0005-worklog-correction.md) and [ADR 0006](docs/adr/0006-local-minute-tui-timestamps.md).

- [x] Encapsulate `Worklog` and `ActiveWorklog` fields before exposing update operations.
- [x] Edit the start and end of a completed worklog while preserving its identity and task.
- [x] Edit only the start of the active worklog and re-anchor the visible monotonic timer.
- [x] Canonicalize edited timestamps to UTC microseconds and reject an end before its start.
- [x] Detect stale edits, including another client stopping or switching the active worklog.
- [x] Reject overlap between worklogs for the same task while allowing touching and zero-duration intervals, with SQLite enforcing the rule.
- [x] Use compare-and-set correction writes and reload authoritative task and tracking state after stale writes.
- [x] Keep worklog corrections separate from task `updated_at`; refresh recently-worked ordering after a changed start.
- [x] Defer manual worklog creation and deletion unless they receive separate approval.

### Interim checkpoint: screen-scoped TUI commands

- [x] Replace the flat event-loop command enum with screen-tagged task-list and worklog-history payloads plus global quit.
- [x] Move semantic command enums into their owning screen modules and make both screen handlers exhaustive.
- [x] Keep event-kind filtering and Ctrl+C in shared key handling while screen keymaps retain mode-sensitive quit and cancel behavior.
- [x] Dispatch task-list and history transitions from their owning typed handlers.
- [x] Keep one `App::handle` mismatch guard so a command tagged for the dormant screen cannot mutate state or call the application service.
- [x] Preserve all keybindings, modes, status text, transitions, timer behavior, narrow-terminal behavior, and test names without changing E2E tests.

### Completed checkpoint: move worklogs between tasks

The move decisions are recorded in [ADR 0008](docs/adr/0008-move-worklogs-between-tasks.md). This is a separate command from timestamp correction. Correction preserves the worklog's task; move changes only its owning task.

- [x] Move completed and active worklogs from their source history without changing the worklog ID, timestamps, or active state.
- [x] Search destinations fuzzily while excluding the source task and all archived tasks.
- [x] Keep search and result focus separate. Tab and Shift+Tab switch focus, Up and Down navigate destination results even while search has focus, and `j` / `k` navigate only in the result list.
- [x] Commit the move atomically, return both task aggregates, reject same-task overlap and active-worklog conflicts, preserve the source history on failure or cancellation, and reload it after success.
- [x] Increment both tasks' history revisions without changing either task's `updated_at`.
- [x] Cover completed and active moves, filtering, focus-sensitive navigation, cancellation, stale state, overlap, and identity/timestamp preservation.

### Completed checkpoint: task search

- [x] Add fuzzy name search to active and archived task views with a filter that can be kept for normal task actions.
- [x] Rank matching tasks by the newer of their metadata update and latest worklog start, regardless of the normal list ordering.
- [x] Keep fuzzy matching and eligible-destination rules in the worklog move dialog while ranking its matches by the same activity rule.
- [x] Preserve task selection by identifier and restore the unfiltered view when search is cleared or cancelled.
- [x] Cover search, ranking, navigation, and destination restrictions in E2E tests.

### Milestone 3: exclusive local or remote storage

- [x] Add explicit process modes to the same binary: local TUI, remote TUI with a configured endpoint, and `tt serve`.
- [x] Add a versioned resource-oriented REST API for tasks, worklogs, and the singleton active-tracking state under `/v1`.
- [x] Keep raw repository methods and storage commands out of the public HTTP contract.
- [x] Run the same `tracker-application` workflows behind local and server adapters.
- [x] Use SQLite as the initial server store and serialize or pool access safely for concurrent requests.
- [x] Bind the initial server only through an explicitly selected localhost or Tailscale address. Client and server may assume the same application version.
- [x] Send client-created UTC timestamps with remote task and tracking operations. Assume sufficiently synchronized clocks and add no clock-skew-specific error or conflict handling in the initial version.
- [x] Keep server validation to domain and persistence invariants, including rejecting a worklog end before its start and enforcing atomic start, stop, and switch behavior.
- [x] Make state-setting and record-creation requests safe to retry without reversing state or duplicating records.
- [x] Load the required task and current-tracking queries on connection and refresh affected resources after stale or conflicting commands.
- [x] Keep remote network work off the TUI rendering and input thread, with visible loading, saving, and unavailable states.
- [x] Fail remote operations when the endpoint is unavailable; never fall back to a local database.
- [x] Keep local and remote databases separate. Defer explicit import unless a concrete migration need is approved.
- [ ] Measure polling and refresh behavior before adding Server-Sent Events or WebSockets.

### Deferred checkpoint: offline synchronization

- [ ] Revisit synchronization only when offline operation against a remote tracker is required.
- [ ] Prefer immutable completed worklogs merged by stable identifier rather than last-write-wins updates.
- [ ] Define logical ordering, tombstones, and deterministic conflict handling before synchronizing mutable tasks or worklogs.
- [ ] Define an explicit conflict outcome for two offline active timers; record-level timestamp comparison alone is insufficient.
- [ ] Add an outbox and incremental pull protocol only after those rules are approved.

### Milestone 4: more clients

- [x] Add a noninteractive Rust CLI for all current TUI domain operations, with local and remote backends, JSON output, caller-supplied concurrency guards, and bounded history pagination.
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
| Review correctness and lifecycle behavior | `gpt-5.6-terra` | OpenAI | `openai-codex` | high | Eligible. GPT-5.6 is the latest available OpenAI series. | Escalated from GPT-5.6 Sol after that reviewer exceeded its time limit. The replacement reviewed timer, transaction, recovery, and terminal failure paths. |
| Review security and privacy | `gpt-5.6-luna` | OpenAI | `openai-codex` | medium | Eligible. GPT-5.6 is the latest available OpenAI series. | Independent review of local storage and data exposure. |

Each reviewer must return one complete report with severity, affected files, rationale, and recommended fixes. Address review findings, rerun validation, and commit the reviewed checkpoint before further work.

## Open questions

### Needed before remote storage

1. Should configuring an endpoint leave existing local data untouched, or should the first remote release include an explicit one-time import?
2. May several remote clients issue task and tracking commands concurrently, beyond the server already enforcing one active timer?
3. How often should a connected TUI refresh remote state: on focus, on a fixed polling interval, or only after its own commands?
4. Should completed worklogs remain immutable through the first remote release?
5. Should users edit past worklogs and add time manually in the first remote release?
6. Do tasks need projects, tags, issue links, or free-form notes? Which one is required first?
7. For Omarchy, what integration is expected first: a launcher command, a status-bar indicator, desktop notifications, or a plugin API?

## Approval gate

The Rust CLI is authorized by the user's request to expose the TUI's current operations to external agents and implement it on a new branch. The implementation adds a separate `tt-cli` binary and reuses the existing local and remote application services.

Milestones 1 and 2, their bootstrap prerequisite, the functional defaults recorded above, and the listed subagents are approved. The `Worklog` terminology, domain/application separation, exclusive local-or-remote direction, same-binary server mode, initial same-version, synchronized-clock and Tailscale assumptions, client-created timestamps, resource-oriented REST API, and deferred offline synchronization are approved architectural direction. The domain/application refactor has an approved implementation and review workflow. Milestone 3 implementation and additional clients require a later planning and approval round.
