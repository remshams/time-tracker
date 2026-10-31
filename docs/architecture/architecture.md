# Architecture and application primitives

Start with the architecture primitives to find each crate, client, and communication boundary. The application concepts explain their shared rules. [Revisions and concurrency](#revisions-and-concurrency) explains write checks, request replay, and pagination guards. The [visual guide](architecture.html) adds diagrams; the [detailed diagrams](diagrams.html) cover method-level ownership. Dependency lists below name direct production dependencies within this repository; manifests list external libraries and test dependencies.

Source scope: the Rust core, TUI, CLI, and native macOS client at `bf93367b`, inspected on 19 October 2026, including protocol 6 from issue #94. This includes tracker-remote worklog reads and mutation recovery, first described at `cc69c715` on 19 October 2026. The tracker-remote history description, without individual worklog reads, describes `f7b0684c`, inspected on 22 October 2026. Standalone server packaging and fixtures describe commits `a18005c7` through `c431aead`, inspected on 19 October 2026. Native direct requests describe `e2dff5da`, inspected on 18 October 2026. The TrackerClient package description covers the removal of its revision fields at `1b72aefa`, inspected on 22 October 2026. The timestamp owner link describes `107cb840`, inspected on 24 October 2026. The TrackerClient worklog write values describe `201a0116`, inspected on 25 October 2026. The shared time helpers describe `581ad51b`, inspected on 31 October 2026. This catalog contains no quality measurements. Code defines implementation; the linked ADRs explain decisions, including later supersessions.

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

Defines protocol-6 JSON requests, resource reads, command receipts, and conversions at the HTTP boundary. `WriteGuard` carries only `request_id`. Task rename and archive requests can carry the reviewed task name as `expected_name`; restore carries none. `parse_worklog_cursor` holds the one cursor validation rule: the revision is nonnegative and the ID is a `WorklogId`. Reads and receipts carry no server revision. Receipts describe committed writes and do not replace current read state. This crate contains no HTTP client, server, or database connection.

Depends on: [tracker-application](#tracker-application), [tracker-domain](#tracker-domain).

Source: [crate](../../crates/tracker-protocol/Cargo.toml), [wire contracts](../../crates/tracker-protocol/src/lib.rs). Decisions: [ADR 0017](../adr/0017-separate-http-resources-from-client-refreshes.md), [ADR 0019](../adr/0019-read-coherent-workflow-state.md), [ADR 0027](../adr/0027-check-reviewed-task-names-on-the-server.md), [ADR 0031](../adr/0031-remove-server-revisions-from-writes-and-reads.md).

### tracker-server

Owns HTTP routes, server-side operation serialization, request-ID receipt replay, and its application and SQLite database. Writes run the shared application operations, which check the command's expected values; the server compares no revision. Task-list and daily-overview reads capture related resources in one transaction. Rename and archive with `expected_name` use the shared guarded operations, which compare and write in one SQLite transaction; without it, they act on the current task. `tt-server` launches this library against an explicit local or Tailscale bind address.

Depends on: [tracker-application](#tracker-application), [tracker-domain](#tracker-domain), [tracker-protocol](#tracker-protocol), [tracker-storage](#tracker-storage).

Source: [crate](../../crates/tracker-server/Cargo.toml), [entry points](../../crates/tracker-server/src/lib.rs), [resource routes](../../crates/tracker-server/src/resources.rs). Decisions: [ADR 0017](../adr/0017-separate-http-resources-from-client-refreshes.md), [ADR 0019](../adr/0019-read-coherent-workflow-state.md), [ADR 0027](../adr/0027-check-reviewed-task-names-on-the-server.md), [ADR 0031](../adr/0031-remove-server-revisions-from-writes-and-reads.md).

### tracker-server-app

The `tt-server` binary owns server argument validation, database path selection, and startup. It requires an explicit loopback or Tailscale bind address and defaults to `default_database_path()?.with_file_name("tt-server.db")`. A leading `serve` word is accepted for existing systemd units. It delegates HTTP and database ownership to `tracker-server` and has no terminal dependencies.

Depends on: [tracker-server](#tracker-server), [tracker-storage](#tracker-storage).

Source: [crate](../../apps/server/Cargo.toml), [startup](../../apps/server/src/main.rs), [arguments](../../apps/server/src/cli.rs). Decision: [ADR 0028](../adr/0028-ship-a-standalone-server-binary.md).

### tracker-remote

Used by clients in remote mode; local clients and the HTTP server use the local application and storage path. Owns asynchronous HTTP transport, response validation, confirmed task, tracking, and report observations, and command recovery. History pages are validated and returned without caching. Writes send the caller's reviewed values without a pre-write read or comparison. A start without a reviewed timer reads tracking once when no tracking observation exists. A connection that fails before sending marks the write as not attempted. After an attempted worklog edit, recovery refreshes only the coherent task list. `RemoteApplication` publishes only validated observations. Failed refreshes retain confirmed values; a saved command remains saved after a display refresh fails. Unloaded tracking differs from observed idle tracking. Report requests share canonical range, query, cooldown, and echoed-context validation in `ReportsClient`; the coordinator validates the remaining daily-overview resources before publishing them together. A restored inactive-task preview token passes the same validation as a server preview while it is deserialized. It never opens the server's database.

Depends on: [tracker-application](#tracker-application), [tracker-domain](#tracker-domain), [tracker-protocol](#tracker-protocol).

Source: [crate](../../crates/tracker-remote/Cargo.toml), [remote application](../../crates/tracker-remote/src/application/mod.rs), [transport](../../crates/tracker-remote/src/transport.rs). Decisions: [ADR 0018](../adr/0018-compose-resource-state-in-client-workflows.md), [ADR 0019](../adr/0019-read-coherent-workflow-state.md), [ADR 0031](../adr/0031-remove-server-revisions-from-writes-and-reads.md).

Report validation scope: `bf93367b`, inspected on 19 October 2026. [Report requests](../../crates/tracker-remote/src/application/reports.rs) fetch validated totals without publishing aggregate resources; the [coordinator](../../crates/tracker-remote/src/application/coordinator.rs) owns coherent adoption.

Preview token validation scope: `bf93367b`, inspected on 19 October 2026. [`InactiveTaskPreview`](../../crates/tracker-remote/src/application/mod.rs) deserializes through its protocol DTO and rejects tokens that fail [preview validation](../../crates/tracker-remote/src/application/decoding.rs).

### tracker-client

Owns shared client dispatch through the concrete `TrackerBackend` enum. It selects a local application or `RemoteApplication` and shares task, tracking, history, report, and archive operations. Task and tracking observations return plain values without revisions. Remote failure never selects local storage. Callers retain runtime, lifecycle, input, and output ownership.

Depends on: [tracker-application](#tracker-application), [tracker-domain](#tracker-domain), [tracker-protocol](#tracker-protocol), [tracker-remote](#tracker-remote), [tracker-storage](#tracker-storage).

Source: [crate](../../crates/tracker-client/Cargo.toml), [backend](../../crates/tracker-client/src/lib.rs). Decision: [ADR 0021](../adr/0021-share-concrete-client-backend.md).

### tracker-tui

The `tt` terminal client owns keyboard commands, screen state, rendering, terminal restoration, and a monotonic timer. One event loop handles input, redraw ticks, and request completions. Production requests use `TrackerBackend`; presentation tests can inject `TrackerApplicationService`. Remote task-list state comes from one aggregate response, so the TUI publishes tasks and tracking without comparing revisions. Server startup belongs to `tracker-server-app`.

Depends on: [tracker-client](#tracker-client), [tracker-application](#tracker-application), [tracker-domain](#tracker-domain), [tracker-remote](#tracker-remote), [tracker-storage](#tracker-storage).

Source: [crate](../../apps/tui/Cargo.toml), [startup](../../apps/tui/src/main.rs), [controller](../../apps/tui/src/app.rs), [runtime](../../apps/tui/src/runtime.rs). Decisions: [ADR 0011](../adr/0011-use-one-tui-event-loop-with-a-fixed-redraw-tick.md), [ADR 0021](../adr/0021-share-concrete-client-backend.md).

### tracker-cli

The `tt-cli` automation client owns argument validation, command handlers, JSON output, exit codes, and local/remote selection. It dispatches through `TrackerBackend` and reuses application search and calendar policies. `tasks rename` and `tasks archive` send no expected name and act on the current task in local and server mode. Remote preview tokens carry no revision. Invalid input fails before opening a backend. It performs no terminal rendering or raw SQL queries.

Depends on: [tracker-client](#tracker-client), [tracker-application](#tracker-application), [tracker-domain](#tracker-domain), [tracker-protocol](#tracker-protocol), [tracker-remote](#tracker-remote), [tracker-storage](#tracker-storage).

Source: [crate](../../apps/cli/Cargo.toml), [entry point](../../apps/cli/src/main.rs), [command reference](../cli.md). Decisions: [ADR 0012](../adr/0012-share-client-query-and-failure-policies.md), [ADR 0021](../adr/0021-share-concrete-client-backend.md).

### tracker-swift-bridge

Owns the Rust static library's C ABI, connection handles, JSON results, and persistent current-thread Tokio runtime. It dispatches through `TrackerBackend` and maps failures for Swift. Rename and archive exports carry the reviewed task name into the shared application operation; the bridge has no unguarded rename or archive export. Restore has no name guard. Start and stop exports take the click time and check the active worklog the app last read; resume requires an idle tracker. The bridge has no unguarded tracking export. The worklog delete export requires the expected task, start, and end, so it cannot delete a running worklog. Resource, report, and receipt JSON carries no server revision. The native app's serial `TrackerWorker` owns call ordering and handle disposal; the bridge owns neither windows nor feature state.

Depends on: [tracker-client](#tracker-client), [tracker-application](#tracker-application), [tracker-domain](#tracker-domain), [tracker-protocol](#tracker-protocol), [tracker-remote](#tracker-remote), [tracker-storage](#tracker-storage).

Source: [crate](../../apps/swiftui/bridge/Cargo.toml), [backend](../../apps/swiftui/bridge/src/backend.rs), [C interface](../../apps/swiftui/Sources/CTrackerBridge/include/TrackerBridge.h). Decisions: [ADR 0018](../adr/0018-compose-resource-state-in-client-workflows.md), [ADR 0021](../adr/0021-share-concrete-client-backend.md).

### TrackerClient Swift package

Owns portable feature state and `TrackerSession` coordination through injected client, clock, scheduler, and settings interfaces. Each user action sends its request right away; the session has no operation gate, queue, or shutdown step. Each feature state owns an in-flight flag that disables only its own submit button or control. Writes send the captured values without reading first: rename and archive carry the name the dialog started with; worklog corrections carry the original times, and moves and deletions also carry the original task; tracking carries the click time and expected active worklog ID. Rust and the server reject outdated writes. A failure shows the server's message in the feature and keeps the draft; after success or failure the session reloads the task list, and worklog writes also reload history. A double timeout reports that the change could not be confirmed; the session does not look for the write or resubmit it. One poll runs at a time, and a history page is dropped if its task is no longer selected. It composes resource observations for features and separates command success from later refresh failures. `TrackerPresentation` copies the session values views read into an `@Observable` model and assigns only changed values. Observations, reports, and command receipts have no revision fields, matching protocol 6. `Shared/` holds helpers that no feature owns, such as the time helpers `parseTimestamp`, `formatTimestamp`, and `formatClockDuration`. Rust continues to own domain rules and persistence.

Depends on: Foundation, Observation, and injected interfaces. No direct Rust, SwiftUI, AppKit, network, or database dependency.

Source: [package](../../apps/swiftui/TrackerClient/Package.swift), [session](../../apps/swiftui/TrackerClient/Sources/TrackerClient/App/TrackerSession.swift), [presentation](../../apps/swiftui/TrackerClient/Sources/TrackerClient/App/TrackerPresentation.swift), [contracts](../../apps/swiftui/TrackerClient/Sources/TrackerClient/Contracts/Dependencies.swift), [shared time helpers](../../apps/swiftui/TrackerClient/Sources/TrackerClient/Shared/TimeFormatting.swift). Decisions: [ADR 0007](../adr/0007-completed-worklog-deletion.md), [ADR 0018](../adr/0018-compose-resource-state-in-client-workflows.md), [ADR 0026](../adr/0026-use-swift-observation-for-native-presentation.md), [ADR 0029](../adr/0029-send-native-requests-directly.md).

### TimeTrackerSwiftUI macOS app

Owns SwiftUI scenes, views, observable stores, and feature action stores, plus AppKit menu-bar and lifecycle integration. Views read observable values directly, so SwiftUI redraws only views whose values changed, and controls bind to their feature's in-flight flag. Windows share the portable session. The presentation coordinator routes every editor sheet and the tracking alert to one owner window. macOS shows one sheet per window, so while that sheet or alert is open, the app view disables every action that opens another editor. The session itself accepts several open editors. The lifecycle observer handles visibility, sleep, wake, and screen lock, but not app termination. Its infrastructure supplies preferences, timers, and the serial `TrackerWorker`, which keeps bridge calls and decoding off the UI thread and runs them in order. Window interaction and error presentation stay in the app.

Depends on: [TrackerClient](#trackerclient-swift-package), [tracker-swift-bridge](#tracker-swift-bridge) through the C ABI, SwiftUI, and AppKit.

Source: [app](../../apps/swiftui/Sources/TimeTrackerSwiftUI/App/TimeTrackerApp.swift), [worker](../../apps/swiftui/Sources/TimeTrackerSwiftUI/Infrastructure/RustBridge/TrackerWorker.swift), [native architecture](../../apps/swiftui/README.md#architecture). Decisions: [ADR 0018](../adr/0018-compose-resource-state-in-client-workflows.md), [ADR 0021](../adr/0021-share-concrete-client-backend.md), [ADR 0026](../adr/0026-use-swift-observation-for-native-presentation.md), [ADR 0029](../adr/0029-send-native-requests-directly.md).

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

Owner: [timestamp canonicalization](../../crates/tracker-application/src/timestamps.rs), [calendar policy](../../crates/tracker-application/src/calendar_reports.rs), and [TUI clock](../../apps/tui/src/support/clock.rs). Decisions: [ADR 0003](../adr/0003-canonical-timestamp-precision.md), [ADR 0006](../adr/0006-local-minute-tui-timestamps.md).

### History and cursors

Worklogs load in pages of at most 50, newest start first with ID ascending as the tie-break. Task and global cursors carry only the last row ID and their database revision. Both scopes use `WorklogCursorDto` in protocol pages and CLI history tokens; the token's `scope` field records which history the cursor belongs to. The server, remote client, CLI and Swift bridge validate incoming cursors with `parse_worklog_cursor` and then build their own task or global cursor. Storage resolves the anchor start and collection membership in the same transaction as revision checks and page reads. Changing a start or moving a worklog invalidates affected task cursors. Inserts and end-only edits preserve task cursors; deleting the anchor requires a fresh first page. Global revisions change on every worklog write. History is a bounded read, not the source of report totals.

Owner: [page and cursor models](../../crates/tracker-application/src/model.rs), [cursor wire format and parsing](../../crates/tracker-protocol/src/lib.rs), and [SQLite history reads](../../crates/tracker-storage/src/sqlite/worklogs.rs). Decisions: [ADR 0004](../adr/0004-paginated-worklog-history.md), [ADR 0005](../adr/0005-worklog-correction.md), [ADR 0008](../adr/0008-move-worklogs-between-tasks.md).

### Reports

Totals clip worklogs to a requested interval, using an explicit cutoff for running work. Archived tasks remain included. Calendar policy converts inclusive local dates to a UTC interval with an exclusive end. Shared client rows carry task IDs and durations; clients resolve labels according to their output needs.

Owner: [report use cases](../../crates/tracker-application/src/service/reports.rs), [SQLite aggregation](../../crates/tracker-storage/src/sqlite/reports.rs), and [client reports](../../crates/tracker-client/src/reports.rs). Decisions: [ADR 0012](../adr/0012-share-client-query-and-failure-policies.md), [ADR 0021](../adr/0021-share-concrete-client-backend.md).

### Inactive-task archive preview

A reviewed candidate set captured at a fixed time and positive inactivity period, defaulting to 14 days. Eligibility checks task creation, metadata changes, and work in that window. Confirmation rechecks eligibility in one transaction. Local confirmation guards the candidate IDs. Remote confirmation archives the tasks that match the preview's time and period when it runs, so the result can differ from the preview. That needs another client to change tasks in between, and the trade-off is accepted. The server rejects a preview time more than 15 minutes from its clock. tracker-remote rejects a restored remote token whose fields fail server-preview validation. Running tasks remain protected and archiving keeps history.

Owner: [inactivity period](../../crates/tracker-domain/src/inactivity.rs) and [task use cases](../../crates/tracker-application/src/service/tasks.rs). Decisions: [ADR 0012](../adr/0012-share-client-query-and-failure-policies.md), [ADR 0016](../adr/0016-configurable-inactive-task-archiving.md), [ADR 0017](../adr/0017-separate-http-resources-from-client-refreshes.md), [ADR 0031](../adr/0031-remove-server-revisions-from-writes-and-reads.md).

## Revisions and concurrency

Since protocol 6, the server keeps no global revision. Commands carry the values the caller reviewed, and the shared application and storage operations compare them with stored state before writing. A request ID lets the server replay the receipt of a resent command. Database counters guard history pagination in both local and remote mode.

| # | Mechanism | Stored in | Protects against |
|---|---|---|---|
| 1 | Command expected values | The request | A write based on an outdated worklog, active timer, or task name. |
| 2 | Request ID | Server memory, at most 1,024 successful receipts | A resent command running twice while its receipt is cached. |
| 3 | Per-task history order | Each task's SQLite `history_order_revision`, initially 0 | A task history continuation after a changed worklog start; a move increments both source and destination tasks. |
| 4 | Global worklog history | One SQLite `global_worklog_revision` row, initially 0 | A global history continuation after any worklog insert, update, or delete. |

### Expected values and request replay

Correction compares the original worklog times; move and deletion also compare the original task ID. A mismatch returns HTTP 409 `worklog_changed`. Start and switch compare `expected_active`; stop rejects a replacement timer and succeeds as already idle when no timer remains. Rename and archive compare `expected_name` when the request carries it. A changed active worklog or task name returns HTTP 409 `conflict` and leaves storage unchanged. Create, restore, rename and archive without `expected_name`, and bulk archive carry no expected values. A write by another client to an unrelated task or worklog does not reject a command.

`WriteGuard.request_id` identifies a command. The server checks its receipt cache under the operation lock before it runs a new command. While the successful receipt stays cached, an identical command returns it with `replayed: true`, and the same ID with a different command returns HTTP 409 `conflict`. The cache holds at most 1,024 successful receipts in memory. A restart or eviction forgets them, and an old command can then run again if its expected values still match. For example, if an archive response is lost and another client restores the task, retrying the archive after a restart archives the task again. This is bounded replay protection, not exactly-once execution. See [ADR 0031](../adr/0031-remove-server-revisions-from-writes-and-reads.md).

### Coherent reads

Reads carry no revision. `/v1/state/task-list` and `/v1/state/daily-overview` read tasks, tracking, and totals in one SQLite transaction under the server operation lock, so clients take related values from one response instead of comparing revisions across reads. Separate task, tracking, history, and report reads stay independent.

### History cursors and database revisions

`GET /v1/worklogs?task_id=...` uses that task's history-order counter. Renames, archive/restore, worklog inserts/deletions, and end-only edits leave it unchanged because existing rows do not move in the history order. Unfiltered `GET /v1/worklogs` uses the global worklog counter, which conservatively invalidates on every worklog write. Task metadata changes leave that counter unchanged.

On the first page, storage reads the relevant counter and rows in one SQLite transaction. If more rows exist, `next_cursor` contains only the last returned ID and counter. Continuations submit `after_id` and `after_revision`, with optional `task_id` as the collection filter. In one read transaction, storage checks the revision, resolves the anchor start by ID, verifies collection membership, and reads rows after the stored start and ID. An outdated revision or missing, deleted, or out-of-scope anchor returns HTTP 409 with `worklog_history_changed`, requiring a fresh first page. Corrected starts determine chronology even when UUID creation order differs; equal starts across different tasks retain the ID tie-break.

SQLite triggers update these counters inside the worklog write transaction, including writes from other local processes. For example, a cursor ending at 10:00 would miss an unseen 09:00 worklog corrected to 11:00. Incrementing the task counter rejects that stale continuation instead of silently skipping the moved entry.

### Why the guards coexist

Pagination counters cover worklog order. Command expected values cover the worklog, timer, and task name that a command was reviewed against. A server-wide token for pagination would reject cursors after unrelated commands, such as renaming another task. The database counters also cover writes from other local processes, which never pass through the server.

Local clients have no server. The TUI, CLI, and native macOS app can open the same local SQLite database concurrently, even if each presents one view. Database counters give all clients the same transactional pagination checks, and the expected-value checks run in the same SQLite transactions in local and remote mode.

Source scope: server, protocol, remote adapter, and storage concurrency mechanisms at `bf93367b`, inspected on 19 October 2026. Code: [server command execution](../../crates/tracker-server/src/core.rs), [task routes](../../crates/tracker-server/src/tasks.rs), [server initialization](../../crates/tracker-server/src/bootstrap.rs), [wire contracts](../../crates/tracker-protocol/src/lib.rs), [remote commands](../../crates/tracker-remote/src/application/coordinator.rs), [history reads](../../crates/tracker-storage/src/sqlite/worklogs.rs), and [database triggers](../../crates/tracker-storage/src/migrate.rs). Decisions: [ADR 0005](../adr/0005-worklog-correction.md), [ADR 0008](../adr/0008-move-worklogs-between-tasks.md), [ADR 0017](../adr/0017-separate-http-resources-from-client-refreshes.md), [ADR 0019](../adr/0019-read-coherent-workflow-state.md), [ADR 0027](../adr/0027-check-reviewed-task-names-on-the-server.md), [ADR 0031](../adr/0031-remove-server-revisions-from-writes-and-reads.md).

### Positive-duration migration and protocol upgrade

Schema migration 7 deletes completed worklogs with zero duration; they cover no time. It refuses databases containing negative durations and rolls back without changing any history. Negative durations indicate invalid stored data and require explicit database repair or a valid backup. Back up the database before upgrading. Protocol 4 removes timestamp and task fields from cursors. Upgrade the server and every client together and regenerate saved history and preview tokens. Incompatible pairs fail the health check. See [ADR 0024](../adr/0024-id-only-worklog-cursors-and-positive-durations.md).

### Protocol 5 upgrade

Protocol 5 adds `expected_name` to task rename and archive requests. Upgrade the server and every client together; incompatible pairs fail the health check. No database migration is needed. Saved history tokens keep their format and remain usable. The server restart starts a new revision epoch, so regenerate saved remote archive previews. See [ADR 0027](../adr/0027-check-reviewed-task-names-on-the-server.md).

### Protocol 6 upgrade

Protocol 6 removes `expected_revision` from `WriteGuard`, the `stale_revision` error code, the `revision` field from reads and inactive previews, and `applied_revision` from receipts. `expected_name` on rename and archive becomes optional. Upgrade the server and every client together; incompatible pairs fail the health check. A protocol-5 write that still carries `expected_revision` fails with HTTP 400 `invalid_request`. No database migration is needed. Saved history tokens keep their format. Saved remote archive previews contain `revision` and fail to load, so generate a new preview. See [ADR 0031](../adr/0031-remove-server-revisions-from-writes-and-reads.md).
