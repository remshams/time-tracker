# 0007: delete completed worklogs

## Status

Accepted

## Date

2026-09-10

## Context

The history screen can correct worklog timestamps, but it could not remove an entry. A mistaken completed entry should be removable without allowing an active timer to disappear or allowing a stale screen to delete a changed row. Archived tasks still own their history and need the same operation.

This record supersedes only ADR 0004's Decision clauses that loaded history is read-only and that the first history screen has no deletion, plus the deletion part of ADR 0005's Consequences statement that manual worklog creation and deletion remain out of scope. All other decisions in ADRs 0004 and 0005 remain accepted.

## Decision

- Deletion removes the completed row with an ordinary SQLite hard delete.
- Only completed worklogs can be deleted. The application rejects active rows, and SQLite schema version 5 installs a trigger that rejects direct SQL deletion of active rows too.
- The repository deletes only when the worklog ID, task ID, start, and end exactly match the values read by the caller. Missing, changed, and active rows remain distinct outcomes.
- The delete and the affected task's latest-work aggregate run in one transaction. SQLite derives that aggregate with `MAX(start_us)` before commit, and the application adopts the returned value.
- Deletion does not change history-order cursor revisions. It removes a row but does not move any remaining row.
- In history, `d` opens a confirmation. `y` or Enter confirms it. A second `d` confirms the deletion, so `y` or Enter is not needed. `n` or Escape cancels. The dialog shows the interval and says that the action cannot be undone.
- After a successful delete, history selects the row now at the deleted index. If no row remains there, it selects the preceding final row. Only empty loaded history has no selection. Archived-task history uses the same behavior. If older pages remain after every loaded row is deleted, history says so; loading the next page selects its first row.
- If the stored row changed before confirmation, the application refreshes the newest history page and keeps the target selected when it is still present. The user must confirm again. A missing target follows the same refresh path. If this refresh fails, history is unavailable until `r` succeeds. If the application's full state recovery fails first, the confirmation remains open and reports both sanitized errors instead of claiming that history is current.

## Consequences

Deletion is safe against stale exact-value comparisons and keeps task ordering authoritative without a post-commit read. A schema-level active-delete guard protects databases from other SQLite clients, not only `tt`. Users get a deliberate confirmation and a fast `dd` path, while cancellation leaves both the screen and database unchanged.

The removed row is gone from ordinary SQLite queries and remains gone after reopening the database. SQLite journals, WAL files, backups, filesystem snapshots, and operating-system recovery may still contain old bytes. This feature does not promise forensic erasure; data disposal requires separate handling of those copies.
