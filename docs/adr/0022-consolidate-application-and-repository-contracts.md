# 0022: consolidate application and repository contracts

## Status

Accepted for [issue #33](https://github.com/remshams/time-tracker/issues/33).

## Date

11 October 2026.

## Context

The application exposed seven capability traits and an empty umbrella trait. Persistence exposed five capability traits and another empty umbrella. Production consumers did not select smaller contracts. They imported capability traits to call methods on concrete types.

The consumer inventory identified two useful substitution boundaries:

| # | Consumer | Contract needed | Test seam |
|---|---|---|---|
| 1 | `TrackerApplication<R>` | All task, tracking, worklog, report and inactivity persistence methods | `MemoryRepository` injects reads, writes and recovery failures in application tests. |
| 2 | TUI `App<S>` and local request execution | Synchronous task, tracking, history and report use cases | `TestService` records commands and controls outcomes without opening SQLite. |
| 3 | SQLite concurrency tests | The complete repository contract | `SynchronizingRepository` intercepts coherent reads and coordinates another connection. |
| 4 | CLI, server and Swift bridge | Concrete `TrackerApplication<SqliteRepository>` methods | Existing integration tests exercise persistence, commands and transport behavior. |
| 5 | Remote application | Shared domain results and error types, with asynchronous concrete methods | Transport tests control server responses. It implements no local service or repository trait. |

Individual task lookups and move candidates remain inherent methods. The TUI resolves these values from its own TaskCatalog, so its service trait does not include them.

Configurable inactivity operations have concrete server and Swift bridge consumers. The TUI uses the existing 14-day operations, so its service contract does not require the configurable methods.

## Decision

Use one `TrackerRepository` persistence contract. Include configurable inactivity methods because both production storage and the application test adapter already implement them. The synchronization adapter forwards them to SQLite while retaining its read interception.

Expose application use cases as inherent `TrackerApplication<R>` methods. Keep their bodies and private helpers in the task, tracking, worklog and report modules. Clients can call these methods without importing capability traits.

Retain one `TrackerApplicationService` trait for the TUI and its `TestService`. Its explicit adapter calls the same inherent production methods. It contains methods rather than capability bounds, so it describes the actual test boundary. Retain the default no-op refresh for test services whose catalog is already authoritative. The production adapter always delegates refresh to the coherent repository read.

Remove the capability traits, their blanket umbrella implementations and the extra configurable-inactivity bound. Do not introduce new backend traits. The shared client backend work can expose a concrete asynchronous API.

## Consequences

There are two trait contracts. Domain rules, SQLite transactions, cache adoption, errors, recovery, timestamp canonicalization and history precision remain unchanged. Atomic switching and exact worklog comparisons still occur in the repository.

This changes Rust source compatibility inside this unpublished workspace. It changes no HTTP, CLI, C ABI or UI behavior. Existing E2E requirements and tests remain unchanged. Application tests, SQLite concurrency tests and the TUI fake keep their established substitution boundaries.
