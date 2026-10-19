# 0019: read coherent workflow state

## Status

Accepted for [issue #30](https://github.com/remshams/time-tracker/issues/30). Partially supersedes ADRs [0017](0017-separate-http-resources-from-client-refreshes.md) and [0018](0018-compose-resource-state-in-client-workflows.md) for selected task-list and totals refreshes. Their independent resources, write contracts, resource observations and lifecycle decisions remain accepted.

Partially superseded by [ADR 0030](0030-remove-server-revisions-from-writes-and-reads.md) for the common read revision and Rust remote command preflights; all other decisions remain accepted.

## Date

2026-10-11

## Context

The task-list workflow needs tasks and active tracking together. Ordinary macOS synchronization also needs the current interval's totals. Independent requests force the shared remote adapter to stage responses, compare global revisions and retry once. A write between requests can invalidate an otherwise valid refresh.

The existing SQLite repository already reads the task list and tracking in one transaction, and can include interval totals in that transaction. The server can expose those existing reads without changing domain rules or storage contracts. The reason to add routes is simpler client coordination. The loopback measurements below do not establish a real-network performance benefit.

## Decision

Add two purpose-based read routes. Both hold the server operation lock while one existing SQLite read transaction captures all included resources and while the server captures its common revision.

| # | Route | Included values | Transactional application read |
|---|---|---|---|
| 1 | `GET /v1/state/task-list` | Complete task catalog, active worklog or null, one revision | `refresh_task_list` |
| 2 | `GET /v1/state/daily-overview?start=...&end=...&now=...` | Complete task catalog, active worklog or null, interval totals and echoed context, one revision | `task_list_report_totals` |

The daily overview accepts explicit RFC 3339 UTC interval boundaries and cutoff. It uses the same half-open clipping, archived-task inclusion, completed-work end, running-work cutoff, canonical microsecond precision and positive-duration rows as the independent report. Its name identifies the demonstrated daily workflow; the caller supplies the interval. Presentation choices, selection and dialogs remain client-owned.

Retain all independent resource routes. Reports and paginated worklog history keep their focused responses. Add no general composition query, per-platform route, implicit command attachment or history aggregate.

`RemoteApplication` uses the task-list route for `TaskList` and coherent task command refreshes. `DailyTotals` and `TaskListWithTotals` use the daily overview. The adapter validates the complete response before adopting any selected observation. DailyTotals leaves the catalog observation unchanged, even though the response includes its catalog for validation. This deliberate extra transfer avoids a third aggregate contract. Independent report-only and tracking-only reads remain available and avoid catalog transfer.

The TUI and native bridge keep their existing Rust workflow selections and public contracts. Commands retain their own preflights, expected values, request identities, receipts and recovery. This change removes cross-request revision reconciliation from selected refreshes. It does not weaken write guards or replace reviewed intent with newly read values.

Loaded idle tracking is an observation containing Idle; unloaded tracking remains an absent observation. A failed read or invalid aggregate preserves all previously confirmed observations. A validated successful command remains successful when its subsequent aggregate refresh fails. Receipts never advance read revisions or cause an automatic duplicate submission.

Keep protocol 3 because existing resource and command shapes remain compatible. Deploy the upgraded server before aggregate-dependent clients. Older protocol-3 clients still use the independent routes. A new client talking to an older server receives an explicit route error when it requests aggregate state and adopts no state from that failure. Health checks alone do not prove aggregate route availability. Roll back clients before rolling back the server; no schema migration is needed.

## Measurements and concurrent writes

Measured on 11 October 2026 against an isolated debug-build server over loopback. The temporary database had 100 tasks and one running worklog. Each row reports 50 sequential samples after five warmups, using one reused HTTP client. Byte counts include JSON bodies only, not HTTP headers. Other validation work ran on the same machine, so these timings are illustrative.

| # | Workflow | Requests | JSON bytes | Median ms | 95th percentile ms |
|---|---|---|---|---|---|
| 1 | Independent task list | 2 | 19,292 | 1.779 | 1.821 |
| 2 | Aggregate task list | 1 | 19,237 | 1.514 | 1.585 |
| 3 | Independent daily overview | 3 | 19,541 | 2.203 | 2.413 |
| 4 | Aggregate daily overview | 1 | 19,431 | 1.636 | 1.706 |

Run the reproducible fixture with `cargo test -p tracker-remote --test server_roundtrip measure_aggregate_reads_and_intervening_writes -- --ignored --nocapture`. It uses temporary storage and starts its own loopback server. This compares the retained independent routes with the new aggregates in the same implementation. It is not a historical before-and-after benchmark or a CI latency threshold.

An injected rename between the independent task and tracking requests produced differing revisions. The next aggregate returned the renamed task and tracking at one revision. The ordinary integration suite also runs 20 tracking switches concurrently with 40 task-list and daily-overview reads. Each read keeps the active task's latest-work timestamp equal to the active worklog's start; daily totals and both observations share one revision. Existing SQLite concurrent-writer tests verify that the reused repository reads hold one read version across their queries.

## Consequences

Selected refreshes make one request and either adopt the full validated group or retain their prior observations. Independent resource caches, focused command results, write safety and history boundaries remain. Timer-only and report-only callers keep their narrow reads.

The daily-only selection transfers a catalog it does not adopt. If measurements show that catalog size matters for a demonstrated timer-and-totals workflow, reconsider a smaller named aggregate. Do not expand this response for hypothetical clients.

Source scope: aggregate protocol DTOs, server routes, shared remote refresh workflows and their integration tests at e9514dc. Retained architecture quality figures keep their stated historical scope and revision; these measurements do not replace them.
