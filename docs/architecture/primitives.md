# Architecture and application primitives

Start with the architecture primitives to find each crate, client, and communication boundary. The application concepts explain their shared rules. [Revisions and concurrency](#revisions-and-concurrency) explains stale-write and pagination guards. The [visual guide](primitives.html) adds diagrams; the [detailed architecture](architecture.html) covers method-level ownership. Dependency lists below name direct production dependencies within this repository; manifests list external libraries and test dependencies.

Source scope: the Rust core, TUI, CLI, and native macOS client at `a7868df36673ee247e5dea5dbfd53c0dd9e8541d`, inspected on 15 October 2026. Native reviewed task-name contracts additionally describe `384069b` on 16 October 2026. This catalog contains no quality measurements. Code defines implementation; the linked ADRs explain decisions, including later supersessions.

## Architecture primitives

### tracker-domain

Owns task and worklog identities, validated names, interval rules, tracking transitions, and inactivity periods. Its models validate changes without performing database, network, or UI work. Operation instants are explicit inputs.

Depends on: no other repository component.

Source: [crate](../../crates/tracker-domain/Cargo.toml), [models](../../crates/tracker-domain/src/lib.rs). Decisions: [ADR 0005](../adr/0005-worklog-correction.md), [ADR 0016](../adr/0016-configurable-inactive-task-archiving.md).

### tracker-application

Owns synchronous use cases, calendar and search policies, and semantic failures. `TrackerApplication<R>` coordinates domain rules through one `TrackerRepository` contract. `TrackerApplicationService` remains the TUI's synchronous test boundary. Storage adapters implement persistence; clients own presentation and scheduling.

Depends on: [tracker-domain](#tracker-domain).

Source: [crate](../../crates/tracker-application/Cargo.toml), [use cases](../../crates/tracker-application/src/service/mod.rs), [repository contract](../../crates/tracker-application/src/repository.rs). Decisions: [ADR 0012](../adr/0012-share-client-query-and-failure-policies.md), [ADR 0022](../adr/0022-consolidate-application-and-repository-contracts.md).

### tracker-storage

Used by clients in local mode and by the HTTP server. Remote clients use HTTP instead of opening SQLite. Owns database connections, paths, migrations, coherent reads, and transactional writes. `SqliteRepository` implements the application repository contract. Database constraints also enforce single active tracking, same-task overlap rules, and active-worklog deletion protection for direct SQL writes.

Depends on: [tracker-application](#tracker-application), [tracker-domain](#tracker-domain).

Source: [crate](../../crates/tracker-storage/Cargo.toml), [SQLite adapter](../../crates/tracker-storage/src/sqlite/mod.rs), [migrations](../../crates/tracker-storage/src/migrate.rs). Decisions: [ADR 0005](../adr/0005-worklog-correction.md), [ADR 0007](../adr/0007-completed-worklog-deletion.md).

### tracker-protocol

Defines protocol-4 JSON requests, resource observations, command receipts, and conversions at the HTTP boundary. Observations pair read values with revisions. Receipts describe committed writes and do not replace current read state. This crate contains no HTTP client, server, or database connection.

Depends on: [tracker-application](#tracker-application), [tracker-domain](#tracker-domain).

Source: [crate](../../crates/tracker-protocol/Cargo.toml), [wire contracts](../../crates/tracker-protocol/src/lib.rs). Decisions: [ADR 0017](../adr/0017-separate-http-resources-from-client-refreshes.md), [ADR 0019](../adr/0019-read-coherent-workflow-state.md).

### tracker-server

Owns HTTP routes, server-side operation serialization, revision guards, command receipts, and its application and SQLite database. Task-list and daily-overview reads capture related resources in one transaction. `tt serve` launches this library against an explicit local or Tailscale bind address.

Depends on: [tracker-application](#tracker-application), [tracker-domain](#tracker-domain), [tracker-protocol](#tracker-protocol), [tracker-storage](#tracker-storage).

Source: [crate](../../crates/tracker-server/Cargo.toml), [entry points](../../crates/tracker-server/src/lib.rs), [resource routes](../../crates/tracker-server/src/resources.rs). Decisions: [ADR 0017](../adr/0017-separate-http-resources-from-client-refreshes.md), [ADR 0019](../adr/0019-read-coherent-workflow-state.md).

### tracker-remote

Used by clients in remote mode; local clients and the HTTP server use the local application and storage path. Owns asynchronous HTTP transport, response validation, confirmed resource caches, and guarded command recovery. `RemoteApplication` publishes only validated observations. Failed refreshes retain confirmed values; a saved command remains saved after a display refresh fails. Unloaded tracking differs from observed idle tracking. Report requests share canonical range, query, cooldown, and echoed-context validation in `ReportsClient`; the coordinator validates the remaining daily-overview resources before publishing them together. It never opens the server's database.

Depends on: [tracker-application](#tracker-application), [tracker-domain](#tracker-domain), [tracker-protocol](#tracker-protocol).

Source: [crate](../../crates/tracker-remote/Cargo.toml), [remote application](../../crates/tracker-remote/src/application/mod.rs), [transport](../../crates/tracker-remote/src/transport.rs). Decisions: [ADR 0018](../adr/0018-compose-resource-state-in-client-workflows.md), [ADR 0019](../adr/0019-read-coherent-workflow-state.md).

Report validation scope: `a1e1da396889f63aa65a1fc3ef21c5af3068eea1`, inspected on 16 October 2026. [Report requests](../../crates/tracker-remote/src/application/reports.rs) fetch validated totals without publishing aggregate resources; the [coordinator](../../crates/tracker-remote/src/application/coordinator.rs) owns coherent adoption.

### tracker-client

Owns shared client dispatch through the concrete `TrackerBackend` enum. It selects a local application or `RemoteApplication` and shares task, tracking, history, report, and archive operations. Remote failure never selects local storage. Callers retain runtime, lifecycle, input, and output ownership.

Depends on: [tracker-application](#tracker-application), [tracker-domain](#tracker-domain), [tracker-protocol](#tracker-protocol), [tracker-remote](#tracker-remote), [tracker-storage](#tracker-storage).

Source: [crate](../../crates/tracker-client/Cargo.toml), [backend](../../crates/tracker-client/src/lib.rs). Decision: [ADR 0021](../adr/0021-share-concrete-client-backend.md).

### tracker-tui

The `tt` terminal client owns keyboard commands, screen state, rendering, terminal restoration, and a monotonic timer. One event loop handles input, redraw ticks, and request completions. Production requests use `TrackerBackend`; presentation tests can inject `TrackerApplicationService`. The same binary launches `tracker-server` through `tt serve`.

Depends on: [tracker-client](#tracker-client), [tracker-application](#tracker-application), [tracker-domain](#tracker-domain), [tracker-remote](#tracker-remote), [tracker-server](#tracker-server), [tracker-storage](#tracker-storage).

Source: [crate](../../apps/tui/Cargo.toml), [startup](../../apps/tui/src/main.rs), [controller](../../apps/tui/src/app.rs), [runtime](../../apps/tui/src/runtime.rs). Decisions: [ADR 0011](../adr/0011-use-one-tui-event-loop-with-a-fixed-redraw-tick.md), [ADR 0021](../adr/0021-share-concrete-client-backend.md).

### tracker-cli

The `tt-cli` automation client owns argument validation, command handlers, JSON output, exit codes, and local/remote selection. It dispatches through `TrackerBackend` and reuses application search and calendar policies. Invalid input fails before opening a backend. It performs no terminal rendering or raw SQL queries.

Depends on: [tracker-client](#tracker-client), [tracker-application](#tracker-application), [tracker-domain](#tracker-domain), [tracker-protocol](#tracker-protocol), [tracker-remote](#tracker-remote), [tracker-storage](#tracker-storage).

Source: [crate](../../apps/cli/Cargo.toml), [entry point](../../apps/cli/src/main.rs), [command reference](../cli.md). Decisions: [ADR 0012](../adr/0012-share-client-query-and-failure-policies.md), [ADR 0021](../adr/0021-share-concrete-client-backend.md).

### tracker-swift-bridge

Owns the Rust static library's C ABI, connection handles, JSON results, and persistent current-thread Tokio runtime. It dispatches through `TrackerBackend` and maps failures for Swift. Additive guarded rename and archive exports carry the reviewed task name into the shared application operation. Existing desired-state exports remain available. The native app's serial `TrackerWorker` owns call ordering and handle disposal; the bridge owns neither windows nor feature state.

Depends on: [tracker-client](#tracker-client), [tracker-application](#tracker-application), [tracker-domain](#tracker-domain), [tracker-protocol](#tracker-protocol), [tracker-remote](#tracker-remote), [tracker-storage](#tracker-storage).

Source: [crate](../../apps/swiftui/bridge/Cargo.toml), [backend](../../apps/swiftui/bridge/src/backend.rs), [C interface](../../apps/swiftui/Sources/CTrackerBridge/include/TrackerBridge.h). Decisions: [ADR 0018](../adr/0018-compose-resource-state-in-client-workflows.md), [ADR 0021](../adr/0021-share-concrete-client-backend.md).

### TrackerClient Swift package

Owns portable feature state and `TrackerSession` coordination through injected client, clock, scheduler, and settings interfaces. Connection and selection generations reject obsolete results. It composes resource observations for features and separates command success from later refresh failures. Queued rename and archive intents retain their reviewed names through background updates and pass them through the client contract. Rust compares the name and writes in one transaction; a conflict requires refresh and review. Rust continues to own domain rules and persistence.

Depends on: Foundation and injected interfaces. No direct Rust, SwiftUI, AppKit, network, or database dependency.

Source: [package](../../apps/swiftui/TrackerClient/Package.swift), [session](../../apps/swiftui/TrackerClient/Sources/TrackerClient/App/TrackerSession.swift), [contracts](../../apps/swiftui/TrackerClient/Sources/TrackerClient/Contracts/Dependencies.swift). Decision: [ADR 0018](../adr/0018-compose-resource-state-in-client-workflows.md).

### TimeTrackerSwiftUI macOS app

Owns SwiftUI scenes, views and observable adapters, plus AppKit menu-bar and lifecycle integration. Windows share the portable session. Its infrastructure supplies preferences, timers, and the serial `TrackerWorker`, which keeps bridge calls and decoding off the UI thread. Window interaction and error presentation stay in the app.

Depends on: [TrackerClient](#trackerclient-swift-package), [tracker-swift-bridge](#tracker-swift-bridge) through the C ABI, SwiftUI, and AppKit.

Source: [app](../../apps/swiftui/Sources/TimeTrackerSwiftUI/App/TimeTrackerApp.swift), [worker](../../apps/swiftui/Sources/TimeTrackerSwiftUI/Infrastructure/RustBridge/TrackerWorker.swift), [native architecture](../../apps/swiftui/README.md#architecture). Decisions: [ADR 0018](../adr/0018-compose-resource-state-in-client-workflows.md), [ADR 0021](../adr/0021-share-concrete-client-backend.md).

## Application concepts

### Task

A reusable activity identified by `TaskId`, with a validated name, archive flag, and creation/update timestamps. Archiving preserves its worklogs. Rename, archive, and restore advance metadata time without moving it backward; tracking leaves it unchanged. Latest work is derived from worklog starts and drives the default task order. Fuzzy search ranks matches by the newer of metadata update and latest work.

Owner: [task model](../../crates/tracker-domain/src/task.rs), [task list model](../../crates/tracker-application/src/model.rs), and [search policy](../../crates/tracker-application/src/task_search.rs). Decisions: [ADR 0002](../adr/0002-task-timestamps-and-ordering.md), [ADR 0009](../adr/0009-task-search.md).

### Worklog

A recorded interval identified by `WorklogId` and owned by one task. A missing end means it is active. Completed intervals include their start and exclude their end. The end must be strictly later than the start after microsecond canonicalization; touching intervals remain valid. Same-task intervals cannot overlap; different tasks may overlap. Correction preserves identity, ownership, and active/completed state. Moving changes only ownership. Deletion requires a completed row. These writes compare the original stored values to reject stale edits.

Owner: [worklog model](../../crates/tracker-domain/src/worklog.rs) and [worklog use cases](../../crates/tracker-application/src/service/worklogs.rs). Decisions: [ADR 0005](../adr/0005-worklog-correction.md), [ADR 0007](../adr/0007-completed-worklog-deletion.md), [ADR 0008](../adr/0008-move-worklogs-between-tasks.md).

### Active tracking

The tracker is idle or running one worklog across the selected database. Starting a previously stopped task creates a new worklog. Switching stops the old worklog and starts the new one atomically. A stop or switch at the active start fails and leaves tracking unchanged. Archived tasks cannot start tracking, and a running task cannot be archived. Closing a client leaves tracking running for the next client or restart.

Owner: [tracking model](../../crates/tracker-domain/src/tracking.rs), [tracking use cases](../../crates/tracker-application/src/service/tracking.rs), and [SQLite tracking](../../crates/tracker-storage/src/sqlite/tracking.rs).

### Time and elapsed duration

Clients supply operation instants; the application canonicalizes them to UTC microseconds before validation and persistence. Calendar boundaries use timezone rules. The TUI anchors its live elapsed timer to a monotonic clock so wall-clock changes do not change displayed or persisted running duration. Its timestamp display and correction inputs use local minute precision while unchanged values retain stored precision.

Owner: [application service](../../crates/tracker-application/src/service/mod.rs), [calendar policy](../../crates/tracker-application/src/calendar_reports.rs), and [TUI clock](../../apps/tui/src/support/clock.rs). Decisions: [ADR 0003](../adr/0003-canonical-timestamp-precision.md), [ADR 0006](../adr/0006-local-minute-tui-timestamps.md).

### History and cursors

Worklogs load in pages of at most 50, newest start first with ID ascending as the tie-break. Task and global cursors carry only the last row ID and their database revision. Storage resolves the anchor start and collection membership in the same transaction as revision checks and page reads. Changing a start or moving a worklog invalidates affected task cursors. Inserts and end-only edits preserve task cursors; deleting the anchor requires a fresh first page. Global revisions change on every worklog write. History is a bounded read, not the source of report totals.

Owner: [page and cursor models](../../crates/tracker-application/src/model.rs) and [SQLite history reads](../../crates/tracker-storage/src/sqlite/worklogs.rs). Decisions: [ADR 0004](../adr/0004-paginated-worklog-history.md), [ADR 0005](../adr/0005-worklog-correction.md), [ADR 0008](../adr/0008-move-worklogs-between-tasks.md).

### Reports

Totals clip worklogs to a requested interval, using an explicit cutoff for running work. Archived tasks remain included. Calendar policy converts inclusive local dates to a UTC interval with an exclusive end. Shared client rows carry task IDs and durations; clients resolve labels according to their output needs.

Owner: [report use cases](../../crates/tracker-application/src/service/reports.rs), [SQLite aggregation](../../crates/tracker-storage/src/sqlite/reports.rs), and [client reports](../../crates/tracker-client/src/reports.rs). Decisions: [ADR 0012](../adr/0012-share-client-query-and-failure-policies.md), [ADR 0021](../adr/0021-share-concrete-client-backend.md).

### Inactive-task archive preview

A reviewed candidate set captured at a fixed time and positive inactivity period, defaulting to 14 days. Eligibility checks task creation, metadata changes, and work in that window. Confirmation rechecks eligibility in one transaction. Local confirmation guards the candidate IDs; remote confirmation guards the revision and preview expiry. Running tasks remain protected and archiving keeps history.

Owner: [inactivity period](../../crates/tracker-domain/src/inactivity.rs) and [task use cases](../../crates/tracker-application/src/service/tasks.rs). Decisions: [ADR 0012](../adr/0012-share-client-query-and-failure-policies.md), [ADR 0016](../adr/0016-configurable-inactive-task-archiving.md), [ADR 0017](../adr/0017-separate-http-resources-from-client-refreshes.md).

## Revisions and concurrency

The server token guards remote commands and coherent resource reads. Database counters guard history pagination in both local and remote mode. These are separate version spaces, not independent general-purpose versions on every task and worklog.

| # | Revision | Stored in | Advances on |
|---|---|---|---|
| 1 | Server token, `epoch:sequence` | Server memory | New successful commands, including accepted no-ops; also server errors where a write may have committed. Replayed receipts retain their original revision. |
| 2 | Per-task history order | Each task's SQLite `history_order_revision`, initially 0 | A changed worklog start; a move increments both source and destination tasks. |
| 3 | Global worklog history | One SQLite `global_worklog_revision` row, initially 0 | Every worklog insert, update, or delete, across all tasks. |

### Server command and resource revision

The server creates a new epoch at startup and starts its sequence at 0. Reads return the token captured under the operation lock. Writes submit `expected_revision`; the server checks equality under that same lock before executing a new command. A mismatch returns HTTP 409. This prevents a stale rename from silently overwriting another client's edit and rejects stale archive previews. Matching read tokens also let clients check that related resource observations agree.

The token covers server-mediated commands, not direct SQLite writes. It is not persisted. A restart changes the epoch, so old tokens remain invalid even though the sequence resets. Identical successful request replays are checked before the revision guard and return their original receipt.

### History cursors and database revisions

`GET /v1/worklogs?task_id=...` uses that task's history-order counter. Renames, archive/restore, worklog inserts/deletions, and end-only edits leave it unchanged because existing rows do not move in the history order. Unfiltered `GET /v1/worklogs` uses the global worklog counter, which conservatively invalidates on every worklog write. Task metadata changes leave that counter unchanged.

On the first page, storage reads the relevant counter and rows in one SQLite transaction. If more rows exist, `next_cursor` contains only the last returned ID and counter. Continuations submit `after_id` and `after_revision`, with optional `task_id` as the collection filter. In one read transaction, storage checks the revision, resolves the anchor start by ID, verifies collection membership, and reads rows after the stored start and ID. An outdated revision or missing, deleted, or out-of-scope anchor returns HTTP 409 with `worklog_history_changed`, requiring a fresh first page. Corrected starts determine chronology even when UUID creation order differs; equal starts across different tasks retain the ID tie-break.

SQLite triggers update these counters inside the worklog write transaction, including writes from other local processes. For example, a cursor ending at 10:00 would miss an unseen 09:00 worklog corrected to 11:00. Incrementing the task counter rejects that stale continuation instead of silently skipping the moved entry.

### Why the guards coexist

Pagination counters do not cover stale task metadata writes. Conversely, using the server token for remote pagination would reject cursors after unrelated commands, such as renaming another task. It could work for server-mediated requests, and its new epoch would safely invalidate cursors after a restart. In-memory storage itself is not the correctness blocker.

Local clients have no server token. The TUI, CLI, and native macOS app can open the same local SQLite database concurrently, even if each presents one view. Database counters give all clients the same transactional pagination checks. The current split keeps task-history invalidation narrow while retaining the server's broader write and read-consistency guard.

Source scope: server, protocol, and storage revision mechanisms at `a7868df36673ee247e5dea5dbfd53c0dd9e8541d`, inspected on 15 October 2026. Code: [server guards](../../crates/tracker-server/src/core.rs), [server initialization](../../crates/tracker-server/src/bootstrap.rs), [cursor contracts](../../crates/tracker-protocol/src/lib.rs), [history reads](../../crates/tracker-storage/src/sqlite/worklogs.rs), and [database triggers](../../crates/tracker-storage/src/migrate.rs). Decisions: [ADR 0005](../adr/0005-worklog-correction.md), [ADR 0008](../adr/0008-move-worklogs-between-tasks.md), [ADR 0017](../adr/0017-separate-http-resources-from-client-refreshes.md), [ADR 0019](../adr/0019-read-coherent-workflow-state.md).

### Positive-duration migration and protocol upgrade

Schema migration 7 refuses databases containing zero-duration completed worklogs and rolls back without changing any history. Back up the database, use the previous client to inspect and explicitly correct or delete affected entries, close old clients, then retry. Protocol 4 removes timestamp and task fields from cursors. Upgrade the server and every client together and regenerate saved history and preview tokens. Incompatible pairs fail the health check. See [ADR 0023](../adr/0023-id-only-worklog-cursors-and-positive-durations.md).
