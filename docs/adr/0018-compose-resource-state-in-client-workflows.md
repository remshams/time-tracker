# 0018: compose resource state in client workflows

## Status

Accepted and implemented on 9 October 2026. This implements [issue #18](https://github.com/remshams/time-tracker/issues/18) on top of the validated protocol-3 route refactoring in [PR #17](https://github.com/remshams/time-tracker/pull/17). The implementation request authorizes this stack before the base PR merges.

## Context

[ADR 0017](0017-separate-http-resources-from-client-refreshes.md) split HTTP resources, but internal tracker snapshots still joined task catalogs and tracking. Reports, history pages and command responses carried that state across the native bridge. The TUI attached the same catalog and tracking state to every completed request. Those attachments hid which reads each workflow needed.

## Decision

Remove the broad Rust and Swift tracker snapshot types, the protocol snapshot DTO, the bridge snapshot export and the TUI application snapshot. Keep protocol 3, HTTP routes, response shapes and the database schema unchanged.

The remote client retains separate task and tracking observations. An observation pairs its value with the opaque revision returned by its own read. An absent observation means unloaded; a loaded empty catalog and loaded idle tracking remain distinct. Reports and history keep their scoped data and revision.

Clients select their refresh dependencies. `Tasks` and `Tracking` read independently. `TaskList` selects a coherent catalog and tracking pair. `DailyTotals` selects report and tracking; `TaskListWithTotals` also selects the catalog. The Rust coordinator stages selected responses, validates them and compares revisions before adopting them together. It retries reconciliation once. Failed reads or exhausted reconciliation retain all previously confirmed observations.

Commands retain their own safe preflight dependencies, reviewed values, timestamps and request IDs. A caller cannot remove a command guard by selecting fewer display resources. Original receipts describe committed domain outcomes and never advance resource revisions. A validated successful receipt remains successful when a subsequent recovery read fails. The backend retains its write block until the required coherent recovery succeeds. Lost or invalid receipts remain uncertain, with the existing identical-request retry and guarded recovery.

The C bridge exports separate resource reads and refresh status. Commands return affected task or worklog values and optional original receipt metadata. Swift's worker performs a selected refresh and captures each required observation in one serialized operation. Feature workflows compose task-list or daily-totals state in Swift; reports, history and commands carry no task-list attachment. A report remains valid when task labels are missing. Running projection requires a matching report and tracking revision and the expected active-worklog anchor.

The TUI completes requests with their focused outcome. Its controller publishes the resources required by that request before applying the outcome. Returning to the task list explicitly refreshes the catalog and tracking pair. Report rows use task IDs and optional labels, preserving totals if label resolution fails. The CLI resolves labels explicitly where its established JSON output requires them.

Local initialization and recovery return task items and active tracking as separate values from one SQLite transaction. A selected task-list totals read also uses one transaction. Reports, archive operations and history retain only the transaction values their adoption requires. Correction, deletion and move results continue adopting committed worklog and activity values without a post-commit read. Independent remote history has no local transaction adoption state; an unloaded tracking cache must never become a synthetic idle observation.

## Lifecycle and failure invariants

The Swift session owns publication, while its worker owns serialized bridge access and disposal. Operations move from queued to executing to completed; cancellation or replacement invalidates publication through the existing connection and selection generations. Closing the worker queues disposal after its operations. Older connection candidates, history responses and command refreshes cannot replace a newer session's resources.

A saved command and a later display refresh have separate results. The session completes the saved edit, then publishes refreshed resources if its generation is still current. If the refresh fails, it retains confirmed display values and reports stale connection state. It does not resubmit the saved command.

Resource isolation must preserve active-task protection, exact worklog comparisons, archive preview expiry, atomic switches and history cursor invalidation. Shared transport limits, sanitized errors and the existing connection policy remain in force. The change adds no network destination, persistent receipt store or exported credentials.

## Consequences

Resource dependencies and freshness are explicit. Timer and report reads avoid loading unrelated catalogs. Controllers have more typed reads and some feature-specific composition, while Rust continues owning reconciliation and command recovery.

The main regression risks are missing an implicit refresh dependency, publishing only part of a selected group, or treating a receipt as current state. Tests cover independent reads, failed refresh retention, revision races, missing metadata, stale intent and committed outcomes after recovery failures. The [architecture diagrams and bridge method table](../architecture.html) describe the implemented ownership and retain the stated scope of historical quality measurements.
