# 0017: separate HTTP resources from client refreshes

## Status

Accepted on 9 October 2026. The server, CLI, TUI and macOS clients will switch together to the new contract under `/v1`, with protocol compatibility number 3. Production still implements the previous `/v1` contract with protocol number 2. Implementation steps and acceptance checks are in the [route migration plan](../api-route-migration-plan.md).

## Date

2026-10-09

## Context

The current API combines resource reads with client refreshes. `/v1/snapshot` returns the task catalog and active tracking. Reports, global worklog pages and mutation responses include the same snapshot. Each report row also repeats its task's metadata. Task history carries global tracking and latest-work aggregates alongside individual worklogs. These contracts are defined in [tracker-protocol](../../crates/tracker-protocol/src/lib.rs).

These bundles let the existing clients refresh several views from one response. The macOS app deliberately uses reports for normal synchronization, as documented in [its daily totals behavior](../../apps/swiftui/README.md#todays-totals). A timer widget, historical reporting tool or future web UI may need a different combination. Expanding the shared bundles for each client would repeat more data and make unrelated response contracts change together.

A report remains a distinct domain query. It calculates time within a requested interval across tasks, including clipped overnight work and running worklogs. Individual history pages cannot replace that query without transferring and aggregating the worklogs. The current storage layer reads totals and tracker state in [one transaction](../../crates/tracker-storage/src/sqlite/reports.rs).

The server also has useful guarantees that must survive the split. It serializes operations, rejects stale global revisions, checks expected active or worklog values, deduplicates successful commands, and enforces transactional domain rules. [Server guards](../../crates/tracker-server/src/lib.rs), [repository contracts](../../crates/tracker-application/src/repository.rs).

## Decision

### Independent resources

Replace the current `/v1` HTTP contract with independent task, tracking, worklog and report resources in one coordinated server-and-client release. Each read returns the requested resource or calculation and its revision. Writes return their committed outcome. Responses do not automatically attach the task catalog or global tracking. `/v1/snapshot` does not exist.

Use task IDs to connect resources. Report rows refer to tasks without embedding names, archive status or timestamps. Clients obtain metadata from task reads or their task cache. Keep coherent internal repository reads and transaction boundaries where they protect domain behavior.

| # | Read endpoint | Response fields | Responsibility |
|---|---|---|---|
| 1 | `GET /v1/health` | `status`, `protocol_version` | Availability and protocol compatibility |
| 2 | `GET /v1/tasks` | `tasks`, `revision` | Task catalog and recent-activity metadata |
| 3 | `GET /v1/tasks/{id}` | `task`, `revision` | One task, including archived tasks |
| 4 | `GET /v1/tracking` | `active_worklog`, `revision` | Current global tracking, with `null` while idle |
| 5 | `GET /v1/worklogs` | `worklogs`, `next_cursor`, `revision` | Global history or task history through optional `task_id` |
| 6 | `GET /v1/worklogs/{id}` | `worklog`, `revision` | One worklog for inspection or an edit preflight |
| 7 | `GET /v1/reports/task-totals` | `start`, `end`, `now`, `rows`, `total_us`, `revision` | Time per task in the requested interval |
| 8 | `GET /v1/tasks/inactive-preview` | `as_of`, `inactive_days`, `count`, `candidate_task_ids`, `revision` | Complete preview for fixed or configurable bulk archive |

Successful reads return HTTP 200. Missing task or worklog IDs return HTTP 404 with the existing error envelope. Health has no resource revision.

Tasks include `id`, `name`, `archived`, `created_at`, `updated_at` and nullable `latest_work_start`. Flatten the task representation instead of retaining `TaskItemDto`. Task reads include active and archived tasks by default; optional `archived=false` or `archived=true` filters the catalog. Retain the complete catalog and current creation ordering initially. Clients retain the shared search and sorting policies.

Worklogs include `id`, `task_id`, `start` and nullable `end`. Use one collection route. Omitting `task_id` requests global history; supplying it requests that task's history. Do not introduce `/v1/tasks/{id}/worklogs` as an alias. A nonexistent filtered task returns HTTP 404; an existing task without worklogs returns an empty page.

Retain 50-entry pages, newest-first ordering and the existing `after_start`, `after_id` and `after_revision` continuation parameters. The integer history revision remains distinct from the opaque server revision. Continuation metadata identifies the original collection scope. Filtered continuations submit `after_task_id` matching their `task_id`; unfiltered continuations omit it. Reject incomplete or mismatched scope. Continuations stay bound to their original task filter or unfiltered collection. Invalidated cursors return HTTP 409. History can contain a running entry, but carries no additional global tracking or task aggregates. `/v1/tracking` remains authoritative for the active worklog.

Name the existing report `/v1/reports/task-totals` to leave room for additional report types. There is no generic `/v1/reports` alias. Require RFC 3339 `start`, `end` and `now` parameters and echo their normalized values. Preserve half-open interval clipping, archived-task inclusion and integer microsecond durations. Running worklogs contribute through `now`; completed worklogs use their stored end even when it is later than `now`. Rows contain only `task_id` and positive `duration_us`. `total_us` is their sum. An absent row means zero time in the interval, not a missing task. Reports contain no task metadata, active worklog or history entries.

Use one inactive-task preview for both existing archive workflows. Require `as_of`; optional positive `inactive_days` defaults to 14. Echo the chosen period and return all candidate task IDs, their count and the preview revision. Resolve names and other task metadata through task reads. Confirm with the preview's original `as_of`, `inactive_days` and revision, then recompute eligibility under the write lock. Keep expiry, active-task protection and checked cutoff arithmetic. `/v1/tasks/inactive-candidates` and `/v1/tasks/archive-inactive-candidates` do not exist as separate aliases.

### Guarded writes and original command receipts

Keep existing domain commands, client timestamps, expected values and UUID request IDs. Remove the automatic full snapshot from write responses.

| # | Write endpoint | Command and result |
|---|---|---|
| 1 | `POST /v1/tasks` | Create with a client-generated task ID; return the committed task |
| 2 | `PATCH /v1/tasks/{id}` | Rename, archive or restore; return the committed task |
| 3 | `PUT /v1/tracking` | Start, switch or stop; return the affected worklog or tracking outcome |
| 4 | `PATCH /v1/worklogs/{id}` | Correct or move using original expected values; return the committed worklog |
| 5 | `DELETE /v1/worklogs/{id}` | Delete an exactly matched completed worklog; return the removed worklog |
| 6 | `POST /v1/tasks/archive-inactive` | Confirm the fixed or configurable preview; return the committed count |

Successful writes retain HTTP 200 and return `request_id`, `applied_revision`, `replayed` and the tagged domain `result`. Preserve switch, already-active and already-idle outcomes. Task outcomes use the same task representation as reads. A switch returns its stopped and started worklogs, not unrelated resources.

Cache the original successful receipt. An identical retry returns the original result and `applied_revision`, with `replayed: true`. Never label an old result with a fresh server revision. Clients must not overwrite newer reads with delayed receipts. After an uncertain write or replay, clients explicitly recover the affected resources.

Retain the 1,024-receipt memory bound and identical-byte transport retry. Restart and eviction remove deduplication history. Keep command fingerprints and guarded access to one command history. The coordinated cutover restarts the server and clears the in-memory deduplication cache; there is no parallel contract to namespace or replay across. No durable exactly-once guarantee is introduced.

Preserve one active worklog, atomic switches, archived-task protection, overlap checks, exact expected worklog values, microsecond precision, inactive-preview expiry, structured errors and recovery after a possibly committed write. No task-deletion or manual-worklog-creation operation is added.

### Revisions and client composition

Retain the existing global revision as an opaque equality token. Every read captures its data and revision under the same server operation lock. Preserve the existing revision changes after successful commands and uncertain failures, and the new epoch on restart. The token covers server-mediated changes, not direct database writes.

Cache each resource with its own revision. A report read cannot advance a task or tracking cache's write guard. Commands use matching revisions for all resources that form their intent; the server checks the resulting `expected_revision` again when executing the command. Refreshing inputs must preserve the user's original reviewed values and timestamps. A fresh guard must not silently authorize an old draft against changed relevant state.

Clients compose the resources their features need. Separate reads are not atomic across requests. A feature requiring coherence compares the relevant revisions, retries reconciliation once, then keeps an explicitly cached view if changes continue. Preserve exact source checks for worklog edits and the original preview revision for bulk archive.

Report validation is independent of task-cache membership. Resolve missing metadata separately without discarding valid totals. Project running totals only when tracking and report revisions agree and the active worklog matches the projection anchor. Use the report's echoed `now` as the cutoff. Otherwise retain reported totals until a coherent pair arrives. An independent timer may continue displaying its confirmed tracking state.

A client adapter may compose several HTTP reads into an internal view model. Local-mode clients can retain their coherent application models. No server route is named after a UI or platform.

### Compatibility and convenience endpoints

Keep the `/v1` URI prefix and bump `tracker-protocol::VERSION` from 2 to 3. Deploy the server and every supported remote client together. New clients require protocol number 3 before adopting resource data; old clients reject the upgraded server, and new clients reject a protocol-2 server. Preserve the existing health response shape so both incompatibilities remain detectable. Strict decoding stays enabled for the new DTOs.

Remove the old snapshot route, nested task-history route, generic report route and separate configurable-archive aliases at cutover. Replace the remaining response shapes in place. Maintain one set of DTOs, handlers and caches, with no compatibility adapter for protocol 2. Stop remote activity, replace the server and clients, and restart them as one release. Roll back the server and clients together if necessary; stored task and worklog data do not require a schema migration.

Add no convenience endpoints initially. Consider a documented aggregate read only after request counts, response bytes, latency and concurrent-write behavior show a need. Any aggregate must identify its included resources and common revision.

This decision replaces remote HTTP expectations in ADRs [0004](0004-paginated-worklog-history.md), [0005](0005-worklog-correction.md) and [0008](0008-move-worklogs-between-tasks.md) for implicit tracking refresh, bundled recovery metadata and source/destination activity aggregates. Their domain rules, coherent repository reads and local-mode behavior remain. ADR [0013](0013-remove-remote-archive-candidate-fingerprints.md) still governs archive preview guards. ADR [0016](0016-configurable-inactive-task-archiving.md) retains its configurable-period domain and native-dialog decisions; this decision replaces its separate HTTP routes and embedded candidate task metadata. The earlier records are marked partially superseded; their local-mode and domain rules remain accepted.

## Consequences

Clients can request tasks, tracking, history and calculations independently. Reports and writes stop returning the full catalog and unrelated tracking state. Future UIs can select their own combinations without expanding shared response contracts.

Clients become responsible for cache freshness and coherent combinations. Some views require more requests. Global revision conflicts remain conservative, and frequent writes can delay matching revisions. No performance improvement is assumed.

Some repeated data remains intentional. A task appears in collection, individual and mutation responses. Tracking and history can both return the same running worklog. `latest_work_start` summarizes history, `total_us` summarizes report rows, and inactive preview counts summarize their candidate IDs. These do not require full-catalog attachments to unrelated responses.

The coordinated release intentionally breaks compatibility with protocol-2 clients. It removes parallel-version maintenance but requires the server and all supported clients to be updated together.

This decision adds no resource-version database migration, task pagination, server-side search, push subscription or convenience endpoint. The [migration plan](../api-route-migration-plan.md) contains implementation order, examples and acceptance checks.

Source basis: production code and client documentation on `main` at revision `2d8b96cff1d097c18c31e06eadbd665616c0a54d`, inspected on 9 October 2026. The [historical communication diagrams](https://github.com/remshams/time-tracker/blob/2ee9ce88ef7fe431044cb77f24b9df9301dffab2/docs/client-server-communication.html) describe `/v1` protocol 2 at source snapshot `3806193` and predate configurable bulk archiving. Implementation will update the current architecture diagrams for protocol 3.
