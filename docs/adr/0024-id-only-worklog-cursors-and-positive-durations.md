# 0024: use ID-only worklog cursors and positive durations

## Status

Accepted. Partially supersedes ADR 0004's cursor fields, ADR 0005's zero-duration acceptance and cursor scope fields, and ADR 0017's protocol-3 cursor representation. All other decisions remain accepted.

## Date

2026-10-15

## Context

Worklog continuations repeat the task filter and expose an anchor timestamp that storage already owns. Corrected start times can differ from UUID creation order, so an identifier cannot replace timestamp ordering. Corrections across different tasks can also create equal starts, even when every interval has positive duration.

Allowing a completed interval with no duration complicates interval validation. Tightening that rule must preserve existing user history and leave failed tracking transitions unchanged.

## Decision

- Task and global cursors contain only `id` and the existing database `revision`. HTTP continuations submit `after_id` and `after_revision`; `task_id` remains the optional collection filter. Remove `after_start` and `after_task_id`.
- Storage checks the relevant revision, resolves the anchor's start by ID, validates its membership in the requested collection, and reads the page in one SQLite transaction. A missing, deleted, or out-of-scope anchor, or an outdated revision, returns the existing history-changed conflict. HTTP clients receive 409 with `worklog_history_changed` and must request a fresh first page.
- Retain `start_us DESC, id ASC` ordering internally and the 50-row page bound. An ID identifies the anchor; its stored start identifies its timestamp position. UUID creation order never determines chronological history order.
- Task history-order revisions still change on start corrections and moves, not inserts, end-only corrections, or deletions. Deleting the anchor now explicitly invalidates its continuation. Global revisions still change on every worklog write.
- Completed worklogs require `end > start` after canonicalization to UTC microseconds. Domain validation and SQLite insert/update guards enforce that rule. Corrections cannot collapse a completed interval to zero.
- A stop or switch at the active start fails with an equality-specific error. The active worklog and all stored rows remain unchanged. Before-start failures retain their existing errors. A repeated request to start the already active task remains an idempotent success.
- Schema migration 7 checks for zero-duration completed rows before adding guards. If it finds any, the entire migration transaction rolls back without deleting, changing, or inventing duration for any row. Opening the database fails with recovery instructions.
- Back up a blocked database before repair. Open it with the previous application version, inspect affected worklogs, and explicitly correct their timestamps to positive intervals or delete entries the user chooses to remove. Close all old clients before retrying the upgrade. New clients never repair history automatically.
- Bump the HTTP protocol to 4 while retaining `/v1` routes. Upgrade server and clients together; incompatible pairs fail the health check before resource loading. Regenerate saved CLI history and archive-preview tokens. Swift treats history cursors as opaque strings; its Rust bridge emits and accepts only the new fields.

## Consequences

Callers pass fewer cursor fields while SQLite retains chronological ordering and transactional consistency. A deleted anchor produces an explicit reload instead of continuing from an unverified timestamp supplied by the client.

Existing zero-duration records require a user decision before upgrading. This can delay an upgrade, but preserves history and avoids silently assigning time that was never recorded. Keep the pre-upgrade backup for rollback: older builds reject the newer schema version even though the new guards do not rebuild the worklog table.

The change adds no background jobs, network paths, or asynchronous state owners. Client generation checks, cancellation behavior, confirmed caches, stale-write guards, and atomic switch ownership remain unchanged.
