# 0008: move worklogs between tasks

## Status

Accepted

## Date

2026-09-20

## Context

Worklog correction fixes timestamps without changing which task owns a record. A separate move operation is needed when time was recorded against the wrong task. It must work for both completed and active worklogs without changing the worklog's identity or timestamps.

The move starts in a worklog's source history. The user needs to find a destination task without leaving that history, while the database must preserve the single-active-worklog rule and same-task interval rules.

## Decision

- Moving a worklog preserves its `WorklogId`, start, end, and active or completed state. It changes only the task that owns the worklog.
- A move starts from the selected worklog in its source task's history. The source task is excluded from destination results.
- Destination search is fuzzy and includes only non-archived tasks. The search results are navigable independently from the search input.
- `Tab` and `Shift+Tab` switch between the search field and the result list. Up and Down navigate destination results even while the search field has focus. `j` and `k` navigate only when the result list has focus.
- `Enter` moves the selected worklog to the selected destination. `Escape` cancels without writing.
- The move rejects any destination that would overlap another worklog for that task. It also rejects a destination that violates the active-worklog rule. A rejected move keeps the dialog open and leaves the source history and database unchanged.
- A successful move increments the history-order revision for both the source and destination tasks and returns both task aggregates. It does not change either task's `updated_at`. The UI remains in the source history and reloads it; both histories are invalidated by their revisions.
- The move uses an atomic compare-and-set write against the source worklog values read by the UI. A stale or missing source row does not overwrite newer state; the application reloads authoritative state and reports the conflict.
- Moving an active worklog keeps it active and keeps the timer anchored to its unchanged start timestamp.

Worklog correction remains a timestamp-only operation. As recorded in ADR 0005, correction preserves the worklog's task identifier. Moving a worklog is a separate command with its own validation and transaction.

## Consequences

Users can repair a worklog assigned to the wrong task without recreating it or losing its identity. History pagination must treat a move as an ordering change for both affected tasks. The source history remains visible and reloads after success, while both histories are invalidated by their new revisions. Task metadata ordering remains independent from worklog ownership changes because `updated_at` does not advance.

The destination picker needs a search field, fuzzy matching, a result-list focus, and explicit focus-sensitive key handling. Tests must cover completed and active moves, source exclusion, archived-task filtering, overlap and stale-write rejection, cancellation, and preservation of IDs and timestamps.
