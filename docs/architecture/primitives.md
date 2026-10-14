# Application primitives

This catalog names the application's main concepts and the components that implement them. Use it to find the owner of a rule before changing code. The [visual guide](primitives.html) adds diagrams; the [detailed architecture](architecture.html) covers method-level ownership.

Source scope: the Rust core, TUI, CLI, and native macOS client at `e0f570ade9dd633046cb784d9c546a2d34516f35`, inspected on 14 October 2026. This catalog contains no quality measurements. Code defines implementation; the linked ADRs explain decisions, including later supersessions.

## Application concepts

### Task

A reusable activity identified by `TaskId`, with a validated name, archive flag, and creation/update timestamps. Archiving preserves its worklogs. Rename, archive, and restore advance metadata time without moving it backward; tracking leaves it unchanged. Latest work is derived from worklog starts and drives the default task order. Fuzzy search ranks matches by the newer of metadata update and latest work.

Owner: [task model](../../crates/tracker-domain/src/task.rs), [task list model](../../crates/tracker-application/src/model.rs), and [search policy](../../crates/tracker-application/src/task_search.rs). Decisions: [ADR 0002](../adr/0002-task-timestamps-and-ordering.md), [ADR 0009](../adr/0009-task-search.md).

### Worklog

A recorded interval identified by `WorklogId` and owned by one task. A missing end means it is active. Completed intervals include their start and exclude their end; touching intervals and zero duration are valid. Same-task intervals cannot overlap; different tasks may overlap. Correction preserves identity, ownership, and active/completed state. Moving changes only ownership. Deletion requires a completed row. These writes compare the original stored values to reject stale edits.

Owner: [worklog model](../../crates/tracker-domain/src/worklog.rs) and [worklog use cases](../../crates/tracker-application/src/service/worklogs.rs). Decisions: [ADR 0005](../adr/0005-worklog-correction.md), [ADR 0007](../adr/0007-completed-worklog-deletion.md), [ADR 0008](../adr/0008-move-worklogs-between-tasks.md).

### Active tracking

The tracker is idle or running one worklog across the selected database. Starting a previously stopped task creates a new worklog. Switching stops the old worklog and starts the new one atomically. Archived tasks cannot start tracking, and a running task cannot be archived. Closing a client leaves tracking running for the next client or restart.

Owner: [tracking model](../../crates/tracker-domain/src/tracking.rs), [tracking use cases](../../crates/tracker-application/src/service/tracking.rs), and [SQLite tracking](../../crates/tracker-storage/src/sqlite/tracking.rs).

### Time and elapsed duration

Clients supply operation instants; the application canonicalizes them to UTC microseconds before validation and persistence. Calendar boundaries use timezone rules. The TUI anchors its live elapsed timer to a monotonic clock so wall-clock changes do not change displayed or persisted running duration. Its timestamp display and correction inputs use local minute precision while unchanged values retain stored precision.

Owner: [application service](../../crates/tracker-application/src/service/mod.rs), [calendar policy](../../crates/tracker-application/src/calendar_reports.rs), and [TUI clock](../../apps/tui/src/support/clock.rs). Decisions: [ADR 0003](../adr/0003-canonical-timestamp-precision.md), [ADR 0006](../adr/0006-local-minute-tui-timestamps.md).

### History and cursors

Worklogs load in pages of at most 50, newest start first with an identifier tie-break. A task cursor carries the task and history-order revision. Changing a start or moving a worklog invalidates affected task cursors; inserts, end-only edits, and deletions leave them valid. Global history has a separate cursor. History is a bounded read, not the source of report totals.

Owner: [page and cursor models](../../crates/tracker-application/src/model.rs) and [SQLite history reads](../../crates/tracker-storage/src/sqlite/worklogs.rs). Decisions: [ADR 0004](../adr/0004-paginated-worklog-history.md), [ADR 0005](../adr/0005-worklog-correction.md), [ADR 0008](../adr/0008-move-worklogs-between-tasks.md).

### Reports

Totals clip worklogs to a requested interval, using an explicit cutoff for running work. Archived tasks remain included. Calendar policy converts inclusive local dates to a UTC interval with an exclusive end. Shared client rows carry task IDs and durations; clients resolve labels according to their output needs.

Owner: [report use cases](../../crates/tracker-application/src/service/reports.rs), [SQLite aggregation](../../crates/tracker-storage/src/sqlite/reports.rs), and [client reports](../../crates/tracker-client/src/reports.rs). Decisions: [ADR 0012](../adr/0012-share-client-query-and-failure-policies.md), [ADR 0021](../adr/0021-share-concrete-client-backend.md).

### Inactive-task archive preview

A reviewed candidate set captured at a fixed time and positive inactivity period, defaulting to 14 days. Eligibility checks task creation, metadata changes, and work in that window. Confirmation rechecks eligibility in one transaction. Local confirmation guards the candidate IDs; remote confirmation guards the revision and preview expiry. Running tasks remain protected and archiving keeps history.

Owner: [inactivity period](../../crates/tracker-domain/src/inactivity.rs) and [task use cases](../../crates/tracker-application/src/service/tasks.rs). Decisions: [ADR 0012](../adr/0012-share-client-query-and-failure-policies.md), [ADR 0016](../adr/0016-configurable-inactive-task-archiving.md), [ADR 0017](../adr/0017-separate-http-resources-from-client-refreshes.md).

## Implementation components

### Application and repository contracts

`TrackerApplication<R>` owns synchronous use cases and calls one `TrackerRepository` persistence contract. Domain models hold reusable validation rules. The TUI retains `TrackerApplicationService` for its synchronous presentation tests; production clients use concrete backend dispatch. Neither domain nor synchronous application contracts depend on Tokio or UI code.

Owner: [application service](../../crates/tracker-application/src/service/mod.rs) and [repository contract](../../crates/tracker-application/src/repository.rs). Decision: [ADR 0022](../adr/0022-consolidate-application-and-repository-contracts.md).

### SQLite storage

`SqliteRepository` owns the connection and implements transactional reads and guarded writes. Database constraints enforce single active tracking, same-task overlap rules, and protection against deleting active worklogs, including direct SQL writes. Schema migrations preserve existing records. Local clients and the server select their own database paths.

Owner: [SQLite adapter](../../crates/tracker-storage/src/sqlite/mod.rs) and [migrations](../../crates/tracker-storage/src/migrate.rs). Decisions: [ADR 0005](../adr/0005-worklog-correction.md), [ADR 0007](../adr/0007-completed-worklog-deletion.md).

### Shared client backend

`TrackerBackend` selects a local application or `RemoteApplication` and shares task, tracking, history, report, and archive dispatch across the TUI, CLI, and Swift bridge. Remote failure never selects local storage. Each client keeps input, presentation, runtime, and lifecycle ownership.

Owner: [tracker-client](../../crates/tracker-client/src/lib.rs). Decision: [ADR 0021](../adr/0021-share-concrete-client-backend.md).

### HTTP resources, observations, and receipts

The server owns its application and database. Protocol-3 resource reads return values with observed revisions; command receipts describe a write, not current display state. The remote adapter owns confirmed caches. Task-list and daily-overview aggregates capture related resources in one server transaction. A failed refresh retains confirmed values; a saved command remains saved when its later refresh fails. Unloaded tracking is distinct from observed idle tracking.

Owner: [protocol](../../crates/tracker-protocol/src/lib.rs), [server resources](../../crates/tracker-server/src/resources.rs), and [remote application](../../crates/tracker-remote/src/application/mod.rs). Decisions: [ADR 0017](../adr/0017-separate-http-resources-from-client-refreshes.md), [ADR 0018](../adr/0018-compose-resource-state-in-client-workflows.md), [ADR 0019](../adr/0019-read-coherent-workflow-state.md).

### Clients and Swift bridge

The TUI controller owns screen state and request publication; one event loop handles input, redraws, and completions. The CLI validates arguments and emits JSON and exit codes. The native macOS session composes feature state, while its serial worker owns C bridge access and disposal. All clients use the same Rust rules through the shared backend. Views format values and own interaction state.

Owner: [TUI controller](../../apps/tui/src/app.rs), [CLI](../../apps/cli/src/main.rs), [Swift session](../../apps/swiftui/TrackerClient/Sources/TrackerClient/App/TrackerSession.swift), and [Rust bridge](../../apps/swiftui/bridge/src/backend.rs). Decisions: [ADR 0011](../adr/0011-use-one-tui-event-loop-with-a-fixed-redraw-tick.md), [ADR 0018](../adr/0018-compose-resource-state-in-client-workflows.md), [ADR 0021](../adr/0021-share-concrete-client-backend.md).
