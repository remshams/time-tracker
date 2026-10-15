# 0023: enforce reviewed intent at application boundaries

## Status

Accepted

## Date

2026-10-11

## Context

The TUI captures the task name or active worklog displayed when a user queues a command. Remote execution checks those values, but local execution discarded them. The server also compared tracking against its cached state before refreshing from SQLite. A second writer could replace the timer and let a stale command affect the replacement.

Application errors included a TUI keyboard instruction, and task-list presentation retained an HTTP preview DTO. Several adapters duplicated timestamp truncation. Report validation sometimes used nanoseconds even though persistence and remote responses use microseconds. Domain tracking transitions generated a worklog ID that the application discarded when reconstructing the worklog with a client-supplied ID.

## Decision

Add shared application operations that accept reviewed task names and expected active worklog IDs. The shared TrackerBackend passes the captured values for local and remote commands, following [ADR 0021](0021-share-concrete-client-backend.md). TrackerRepository compares the reviewed name and mutates the task atomically; SQLite uses an Immediate transaction. Tracking operations refresh authoritative state before comparing the expected active ID. Persistence retains insert constraints and exact worklog comparisons to reject later races. The server uses the same guarded operation with the request's worklog ID. Existing convenience operations remain available for callers that explicitly request desired state.

Keep application error messages independent of keyboard controls. The TUI adds its refresh instruction when rendering a history conflict. Task-list presentation holds an opaque confirmation object with count and sample names. Its request adapter owns backend-specific confirmation state. The remote adapter wraps its private wire DTO and resolves candidates into domain tasks; CLI token serialization retains its existing shape.

Enforce [ADR 0003](0003-canonical-timestamp-precision.md) through one application timestamp module. Truncate timestamps to UTC microseconds before validation, including report bounds and cutoffs. Adapters reuse the helper and echo the canonical report context.

The application chooses worklog identity. Domain start, switch and toggle transitions accept that typed ID and return the worklog that persistence stores. A remote request can supply the same ID without reconstructing a domain result.

## Consequences

Local and remote reviewed commands reject stale intent. A rejected timer command refreshes the display and leaves the replacement worklog unchanged. Existing TUI status wording stays intact, and three stale-client E2E scenarios now assert the guarded behavior.

No database migration or HTTP protocol change is required. Swift task rename and archive commands still supply only task IDs; adding reviewed names to those native contracts remains separate work. Guarded archive retains the existing post-commit recovery behavior. Resource observation ownership and saved-command versus display-refresh handling follow [ADR 0018](0018-compose-resource-state-in-client-workflows.md).
