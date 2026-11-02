# 0002: task timestamps and list ordering

## Status

Accepted

## Date

2026-09-04

## Context

Tasks have no persisted timestamps today, and the task list sorts by `TaskId`. That order is deterministic but tells the user nothing about what they were last doing. The TUI needs a default order that puts recent work on top, plus alternatives, and a stable rule for which events count as an update. Timestamps also feed the future remote protocol, where the client already creates timestamps per the settled timestamp-authority decision in `plan.md`.

## Decision

### Timestamps

- A task stores `created_at` and `updated_at` as client-created UTC timestamps. Creating a task sets both to the same value.
- A successful semantic change advances `updated_at` to the operation's timestamp. Semantic changes are rename, archive, and restore. The stored value never moves backward: if a client clock reports a time earlier than the stored `updated_at`, the stored value stays.
- Idempotent metadata operations do not change `updated_at`. Renaming a task to its current name, archiving an archived task, and restoring an active task are no-ops for the timestamp.
- Tracking operations never change `updated_at`. Starting, stopping, and switching worklogs leave the task's metadata untouched.

### Latest work

- The latest work moment is derived as `MAX(worklog start)` per task. It is not stored on the task row. Deriving it keeps the value correct no matter which client wrote the worklog and avoids keeping a denormalized field in sync.
- The task list read model carries an optional latest worklog start next to each task. The list query does not load full worklogs; the value comes from a per-task `MAX(start)` aggregate in the storage query, or is absent when the task has no worklogs.

### Ordering

- The default ordering is recently worked: latest worklog start descending, tasks without worklogs last, then created_at descending, then `TaskId` ascending.
- Two alternative orderings exist, both newest-first with a deterministic `TaskId` ascending tie-break:
  - Recently updated: `updated_at` descending, then `created_at` descending, then `TaskId` ascending.
  - Recently created: `created_at` descending, then `TaskId` ascending.
- One ordering choice lasts for the current session only and applies to both the active and the archived view. It does not persist across runs.
- The `s` key cycles through the three modes and works only in normal mode, never while a text input has focus.
- Selection follows `TaskId`. When the list re-sorts or refreshes, the selection stays on the same task by identifier rather than on the same row position.

### Migration

- Existing v1 tasks get both timestamps backfilled with one fixed migration timestamp. The migration does not infer times from UUIDv7 bits. One shared value is honest about what the data says, which is nothing, and keeps the backfill deterministic.

## Consequences

The schema gains two task columns and a migration that backfills them. Storage needs a `MAX(start)` aggregate in the task list query, ideally backed by an index on worklog start. The TUI gains a cycling control, session state for the chosen mode, and TaskId-based selection across re-sorts.

Restricting `updated_at` to semantic changes means tracking activity does not churn the timestamp, so recently-updated ordering stays meaningful. It also means `updated_at` says nothing about work activity; recently worked covers that.

Tests must cover all three orderings and their ties, the null-worklog case, selection stability across re-sorts, the forward-only `updated_at` rule, the idempotent-operation rule, the tracking-exclusion rule, the migration backfill, and empty states in both views.
