# 0027: check reviewed task names on the server

## Status

Proposed

## Date

2026-10-18

## Context

[ADR 0023](0023-enforce-reviewed-intent-at-application-boundaries.md) made local rename and archive compare the reviewed task name and write in one SQLite transaction. Over HTTP, the request carried only the new name or the archive time. The remote adapter read the task and compared its name before sending the write, and the server's global revision check covered the time between that read and the write.

Issue #94 removes the revision check from task writes, and with it the client-side comparison. Without a name in the request, the server could then apply a rename or archive to a task that another client renamed after the user reviewed it.

## Decision

- `TaskChangeRequest::Rename` and `TaskChangeRequest::Archive` require `expected_name`, the task name the user reviewed. Restore carries no expected name, because restoring a task twice changes nothing.
- `PATCH /v1/tasks/{id}` calls `rename_task_with_expected_name` and `archive_task_with_expected_name`, the operations local mode already uses. SQLite compares the name and writes in one transaction. A stored name that differs returns HTTP 409 with the existing `conflict` code and leaves the task unchanged. The server adds no comparison of its own.
- `tracker-remote` sends the reviewed name from its guarded operations. Its unguarded rename and archive send the name they read from the server just before the write. The client-side `task_guard` comparison stays until #94 removes it.
- Bump the HTTP protocol from 4 to 5 and keep the `/v1` routes. A version 4 request without `expected_name` fails JSON validation.

## Consequences

The server rejects a stale rename or archive even when the request carries the current revision, so removing the revision check in #94 keeps that protection. Local and remote mode share one name check in `tracker-storage`.

The server and every remote client upgrade together. Mismatched versions fail the health check before any resource loads. No database migration is needed, and saved CLI history and preview tokens stay valid.

Guarded archive checks the name before the active-task trigger. Archiving a running task whose name another client changed therefore reports a conflict instead of an active-task error. Either error leaves the task unchanged.
