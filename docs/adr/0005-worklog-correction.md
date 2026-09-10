# 0005: correct worklog timestamps without overlap

## Status

Partially superseded by ADR 0006 and ADR 0007; all other decisions remain accepted.

## Date

2026-09-05

## Context

Worklog history records when tracking started and stopped, but clock mistakes and late corrections can leave those timestamps wrong. Correcting a start may also change history order and the task's recently-worked position. An active worklog needs different handling because it has no end and its visible elapsed time comes from a monotonic clock.

Two clients may open the same worklog before either saves. A correction must not overwrite a change made by the other client. The database also needs an interval rule that permits work recorded for different tasks at the same time while preventing contradictory entries for one task.

## Decision

### Editable values

- A correction preserves the worklog identifier, task identifier, and active or completed state.
- A completed worklog may change its start and end. An active worklog may change only its start.
- Corrected timestamps are client-created values canonicalized to UTC microseconds.
- A completed end may equal its start but cannot precede it.
- Corrected completed timestamps and a corrected active start cannot be later than the client-created correction time.
- Corrections do not change the task's metadata `updated_at`. A changed start does refresh the derived latest-work value used by recently-worked ordering.

### Interval bounds

- Worklogs for one task must not overlap.
- Intervals are half-open. A completed worklog covers `[start, end)`, and an active worklog covers `[start, +infinity)`.
- One worklog may start exactly when another worklog for the same task ends.
- A zero-duration worklog covers no time and overlaps nothing.
- Worklogs for different tasks may overlap.
- SQLite enforces these rules on inserts and updates. Schema migration 3 checks existing data before installing overlap triggers. Migration 4 replaces their linear scans with indexed predecessor checks and rejects direct moves onto archived tasks. If existing worklogs for one task overlap, migration fails without changing the schema or records.

### Concurrent correction

- A correction write compares the stored start and end with the original values read by the client.
- The comparison and update run in one transaction. A missing worklog, changed worklog, overlap, and storage failure remain distinct outcomes.
- A stale correction never overwrites the newer values. The application reloads authoritative task and tracking state in one backend snapshot, while the TUI keeps the user's draft and tells them to cancel and refresh.
- Each task has a history-order revision. Changing an existing worklog's start increments it; inserts and end-only changes do not. Continuation cursors carry their task identifier and revision, so a cursor cannot be reused for another task and another client's start correction makes an older cursor fail instead of omitting or duplicating a moved row. The TUI then reloads the newest page.
- Stop and switch writes compare the active worklog's stored start as well as its identifier. If another client corrects that start, the write fails, authoritative state is reloaded, and the monotonic timer is re-anchored before the user retries.

### TUI behavior

- `e` opens correction for the selected history row from either the active or archived task view.
- The editor shows local RFC 3339 timestamps with an explicit UTC offset and six fractional digits.
- `Tab` and `Shift+Tab` switch between a completed worklog's start and end. An active worklog has only the start field.
- `j` and `k` move the focused timestamp forward and backward by five minutes. `J` and `K` move it forward and backward by one hour.
- Direct timestamp input, cursor movement, Backspace, and Delete remain available.
- Enter saves and Escape cancels. Ctrl+C quits without writing.
- A failed parse or write leaves the editor and draft open.
- A successful correction discards loaded older pages and reloads the newest page because changing a start changes the pagination key. Selection follows the corrected identifier when that row remains in the page.
- If that reload fails after the correction commits, history is marked unavailable until `r` succeeds. Rows and cursors from the previous ordering snapshot are not shown as current.
- Correcting an active start re-anchors the header and history-row timer to the new start. Correcting another worklog preserves an unchanged active timer's monotonic anchor. The elapsed display remains monotonic and never becomes negative.

## Consequences

`Worklog` and `ActiveWorklog` expose their values through read-only accessors. Application operations accept timestamp-only correction values, so callers cannot move a worklog to another task or change its identity. SQLite schema version 3 adds overlap and history-revision triggers, a task history-order revision, and an optimistic compare-and-set update. Schema version 4 adds indexed overlap checks, task-bound cursor invalidation for direct moves, and archived-task move protection.

Cross-task overlap is now possible through corrections even though ordinary tracking still permits only one active worklog across the tracker. Same-task overlap remains impossible through both application and direct repository writes.

The TUI correction editor has its own input mode and footer. Successful corrections reload bounded history rather than merging pages with different ordering snapshots. Each page carries the requested and active tasks' latest-work aggregates plus global tracking from the same read transaction, so adopting another client's correction cannot combine stale ordering with current tracking or scan every task's history. Cross-client start corrections also invalidate loaded continuation cursors. Manual worklog creation and deletion remain out of scope.
