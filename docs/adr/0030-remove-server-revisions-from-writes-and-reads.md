# 0030: remove server revisions from writes and reads

## Status

Proposed

## Date

2026-10-19

## Context

Protocol 5 required every server write to carry the global revision in `WriteGuard.expected_revision`. `Core::execute` returned HTTP 409 `stale_revision` when it differed from the current `epoch:sequence` token. Before each write, `tracker-remote` read the affected task, timer or worklog through `task_guard`, `tracking_guard` or `worklog_guard`, compared it with the reviewed values and sent the revision it had just read. The check covered only the time between that read and the write. Because the revision was global, a write by another client to an unrelated task or worklog in that time also rejected the command.

Commands already carry expected values, and `tracker-application` and `tracker-storage` check them in local and remote mode. Correction compares the original worklog times; move and deletion also compare the original task ID. Start, switch and stop compare the expected active worklog. [ADR 0027](0027-check-reviewed-task-names-on-the-server.md) added the reviewed task name to remote rename and archive. Create, restore and bulk archive carry no expected values.

Reads also returned the revision so clients could check that combined reads agree, as [ADR 0019](0019-read-coherent-workflow-state.md) describes. Every combined view now comes from one aggregate response, `/v1/state/task-list` or `/v1/state/daily-overview`, so those comparisons could not fail. One comparison could: `resolve_preview_tasks` compared the inactive preview's revision with separate task reads, so any write, server restart or server error between them failed the preview with "Tracker state changed". History cursors use separate database counters.

## Decision

- `WriteGuard` keeps only `request_id`. Remove `expected_revision`, the `stale_revision` error code, the server's revision comparison, and the `epoch` and `sequence` fields of `Core`.
- Read DTOs, including the inactive preview, carry no `revision`. `MutationDto` carries no `applied_revision`.
- Bump `tracker_protocol::VERSION` from 5 to 6 and keep the `/v1` routes. Upgrade the server and every remote client together. Health checks reject mismatched versions. A protocol-5 write body that still contains `expected_revision` fails strict decoding with HTTP 400 `invalid_request`. No database migration is needed.
- Keep the expected-value checks in `tracker-application` and `tracker-storage` unchanged. The server runs them through `TrackerApplication<SqliteRepository>`. An outdated worklog in a move, correction or deletion returns HTTP 409 `worklog_changed`. A changed active worklog or task name returns HTTP 409 `conflict`. A stop succeeds as already idle when no timer remains.
- Remove `task_guard`, `tracking_guard` and `worklog_guard` from `tracker-remote`, with their reads and comparisons, including the destination check before a move. Writes send the caller's reviewed values directly. An outdated value reaches the server and fails with the errors above.
- Make `expected_name` on rename and archive optional. Guarded calls send the reviewed name. The unguarded `rename_task` and `archive_task`, which the CLI uses, send none, and the server renames or archives the task whatever its current name, as local mode does.
- Keep one baseline read for an unguarded start, which the CLI uses. When the adapter has no tracking observation, it reads tracking once and sends the running worklog as `expected_active`. An omitted `expected_active` already means "expected idle", so dropping the read would need a new protocol marker and a new application function.
- Validate receipts only against values the command carries. Stop no longer compares the cached start time, and correction no longer expects the task ID that the removed pre-write read returned.
- Keep request-ID deduplication. `Core::execute` checks its receipt cache under the operation lock before it runs a new command. An identical command returns its stored receipt with `replayed: true`; a reused ID with a different command returns HTTP 409 `conflict`. The cache stays in memory and holds at most 1,024 successful receipts. Add no durable store.
- Keep the transport's single resend of a failed write with the same body and request ID. A resent pending creation keeps its request ID. A fresh creation no longer reads the catalog first; only pending-creation recovery does. A connection failure before sending marks the write as not attempted. Writes use an HTTP client that keeps no idle connections, so a stopped server refuses the connection instead of failing the write after it was sent.
- Remote bulk archive sends the preview's `as_of` and `inactive_days` and archives the tasks that match them when the command runs. It reports the committed count. `resolve_preview_tasks` no longer compares revisions and still rejects an archived candidate. The 15-minute server-clock limit and the local candidate-ID check stay. [Issue #108](https://github.com/remshams/time-tracker/issues/108) restores the exact previewed set by sending the previewed IDs.
- The TUI no longer compares task and tracking revisions before publishing them. The bridge JSON drops `revision` and `appliedRevision`. Swift drops `CommandReceipt.appliedRevision` and keeps its optional revision fields for now. They decode as nil, so `DailyTotalsState` compares nil with nil, and running-total projection depends on the active-worklog anchor alone. Removing those fields follows [issue #95](https://github.com/remshams/time-tracker/issues/95) and [PR #104](https://github.com/remshams/time-tracker/pull/104).
- History cursors keep their database counters. `history_order_revision`, `global_worklog_revision`, the cursor `revision` and `after_revision` do not change.

This decision supersedes these clauses. The other decisions in those records remain in effect.

- [ADR 0010](0010-share-tui-application-requests.md): "Retain the existing revision checks" for remote writes. Serialized writes, stable retry data and remote-only storage behavior remain.
- [ADR 0012](0012-share-client-query-and-failure-policies.md): "Remote confirmation also retains its server revision". Preview expiry remains.
- [ADR 0013](0013-remove-remote-archive-candidate-fingerprints.md): "Retain server revision checks" and the consequence "A server revision change still rejects an archive". Request deduplication, the recomputed atomic archive and the reported count remain.
- [ADR 0016](0016-configurable-inactive-task-archiving.md): the server revision in the preview and "checks the revision" on confirmation. The time, period and recomputed eligibility remain.
- [ADR 0017](0017-separate-http-resources-from-client-refreshes.md): the `revision` on every read, including the read endpoint table and the preview; `applied_revision` in receipts and replays; the preview revision in "Confirm with the preview's original `as_of`, `inactive_days` and revision"; and, under "Revisions and client composition", the global revision token, per-resource revisions, matching command revisions, the server's `expected_revision` check, the preview revision for bulk archive, running-total projection only when tracking and report revisions agree, and the common revision required of any aggregate. The consequence "Global revision conflicts remain conservative" no longer applies. Independent resources, expected values, the 1,024-receipt bound and history cursor parameters remain.
- [ADR 0018](0018-compose-resource-state-in-client-workflows.md): the revision paired with each observation, the revision on reports and history, "Commands retain their own safe preflight dependencies" and the exact-worklog and destination guards for correction and move in the Rust remote adapter, and the matching report and tracking revision for running projection. Separate observations, reviewed values, receipts, recovery and lifecycle remain.
- [ADR 0019](0019-read-coherent-workflow-state.md): the common revision that aggregate reads capture, "one revision" in the route table, "Commands retain their own preflights" and "Receipts never advance read revisions". The aggregate routes, their single read transaction and the recorded measurements remain.
- [ADR 0027](0027-check-reviewed-task-names-on-the-server.md): the required `expected_name`, the unguarded rename and archive that read the name just before the write, the client-side `task_guard` comparison, and restart invalidation of saved previews through `stale_revision`. The server's transactional name check remains, and the protocol-5 account stays as history.

[ADR 0023](0023-enforce-reviewed-intent-at-application-boundaries.md) remains in effect. The shared operations still take reviewed names and expected active IDs, and the Swift session's presentation preflights do not change. The TUI stale-client E2E scenarios in `apps/tui/tests/e2e/scenarios/stale.rs` are unchanged.

## Consequences

A write by another client to an unrelated task or worklog no longer rejects a command. A change to a value the command carries still fails at the shared application and storage boundary. A move or start can succeed after another client renames the destination or target task, because those commands carry no task name. CLI rename and archive in server mode no longer conflict with another client's rename. Restore applies to the task's current state.

Remote bulk archive no longer fails when another client writes after the preview. It archives the tasks that match when it runs, and that set can differ from the preview. A candidate that another client renames or starts drops out, and a task whose recent worklog another client deletes can join. Clients show the committed count. A server restart no longer invalidates a saved preview. Protocol-5 preview tokens contain `revision` and fail to load, so regenerate them. History tokens keep their format.

Receipt replay has a limit. Suppose request A archives the task `Planning` and commits, but its response is lost. Another client then restores the task. While A's receipt stays cached, resending A returns the old receipt and the task stays active. After a server restart, or after 1,024 newer successful commands evict the receipt, resending A archives the task again: its `expected_name` still matches, and no revision rejects the old command. Any command whose expected values still match behaves the same way. This trade-off is accepted. Durable replay protection needs a separate decision.

A command can also commit before a server error and leave no receipt. Clients keep their uncertain-outcome reconciliation.

[Issue #109](https://github.com/remshams/time-tracker/issues/109) and [issue #110](https://github.com/remshams/time-tracker/issues/110) simplify the remote coordinator further and stay separate.

Source scope: protocol, server, remote adapter, shared client, TUI, CLI and bridge behavior for issue #94 at `bb63c29`, inspected on 19 October 2026.
