# 0029: send native requests directly

## Status

Proposed. Once accepted, it partially supersedes the Swift session clauses listed under [Superseded clauses](#superseded-clauses) in ADR 0016, ADR 0018 and ADR 0023. Their Rust, bridge and server decisions remain accepted.

Partially superseded by [ADR 0031](0031-remove-server-revisions-from-writes-and-reads.md) for the Rust adapter's preflights, which ADR 0031 removes; all other decisions remain proposed.

## Date

2026-10-18

## Context

`TrackerSession` in the native client wrapped every request in its own scheduling and guards. It ran one operation at a time in a fixed priority order and queued tracking clicks behind reads. Before each write it refreshed the task list, looked up the worklog in bounded history, and compared the reviewed task name or worklog with the refreshed values. After an uncertain write it looked for an already committed result, kept the intent in memory, and could resubmit it. Open editors watched refreshed worklogs and asked for review. One open editor kept every other editor closed and blocked connection changes. A session generation token and `shutdown()` rejected results that arrived after the app began to quit.

Most of that work repeated checks made further down. The server and the shared Rust application layer reject a write based on outdated data: rename and archive carry the reviewed name, worklog edits carry the expected task and times, and tracking commands carry the expected active worklog ID ([ADR 0023](0023-enforce-reviewed-intent-at-application-boundaries.md)). The Rust remote adapter retries a lost send once with the same request ID and marks failed writes as requiring a refresh. The serial `DispatchQueue` in `TrackerWorker` already runs bridge calls one at a time and in order.

The Swift layer still made each feature change touch the operation gate, the drain priorities and the recovery paths. [PR #79](https://github.com/remshams/time-tracker/pull/79) and [issue #77](https://github.com/remshams/time-tracker/issues/77) restructured these guards instead of removing them and were closed. Issues #40 and #31 were closed for the same reason. [Issue #95](https://github.com/remshams/time-tracker/issues/95) asks for the direct flow below.

## Decision

- Every user action sends its request right away in a Swift `Task`. The session has no operation gate (`isBusy`, `beginOperation`, `finishOperation`, the `drain…` functions), no queued control commands, no session generation token and no `shutdown()`.
- Each feature state owns its in-flight flag, such as `TaskCreationState.isSubmitting`, `TrackingState.isSubmitting`, `WorklogHistoryState.isLoading` and `ConnectionState.changing`. A flag disables only its own feature's submit button or control.
- Writes send the captured values without reading first. Start and stop send the click time and expected active worklog ID. The Rust adapter and the server reject outdated writes.
- Success resets or closes the editor. Failure shows the server's message in the feature and keeps the draft. Either way the session publishes, then reloads the task list, and today's totals when a report client exists. Worklog writes also reload the selected task's history.
- The session does not recover uncertain writes. When the remote transport times out on both attempts, the feature shows "Could not confirm the change. Check the reloaded data before trying again." followed by the bridge message. Resubmitting sends a new request with a new timestamp.
- Editors do not observe refreshed worklogs. A conflict appears when the user saves.
- The session accepts several open editor states. No editor blocks tracking controls or a connection change. The app view still allows one editor at a time, as described under Keep.
- Screen lock sends `pauseTracking` with the active worklog ID and lock time. Unlock sends `resumeTracking` only if that pause stopped the timer. Neither refreshes first.
- Sleep cancels the poll, display and rollover timers; wake re-anchors elapsed time and refreshes. Writes are not deferred while the Mac sleeps.
- `MacLifecycleObserver` no longer observes `willTerminate`. The system cleans up when the app quits.

Keep:

- The serial `DispatchQueue` in `TrackerWorker`. The Rust handle is not thread-safe, and the queue keeps calls in order, so a connection change waits for earlier calls.
- One poll at a time. `refresh()` does nothing while a refresh is in flight, and the next poll is scheduled after it finishes. Polls and post-write reloads may overlap; the serial worker orders them.
- History pages are dropped when their task is no longer selected. Move candidate and bulk archive preview responses for an outdated query are dropped by their revision or generation number.
- One owner window. `TrackerTaskPresentationCoordinator` routes every editor sheet and the tracking error alert to one tracker window. macOS shows one sheet per window, so while that window shows a sheet or the alert, the app view disables every action that opens another editor. This is a view rule, not a session guard. Separate editor windows were tried and dropped as too complex.

Accepted behavior changes:

- If a write times out on both attempts, the app reports that it could not confirm the change and reloads, even when the write landed.
- An open editor no longer warns that its item changed elsewhere. The conflict appears when the user saves.
- A write made while the Mac sleeps fails with an error instead of resuming on wake.
- Changing connections is allowed while an editor is open. A later save fails, for example with a "not found" error.

### Superseded clauses

Once accepted, this record replaces these clauses. Everything else in those records stays.

- [ADR 0018](0018-compose-resource-state-in-client-workflows.md), Decision: "Commands retain their own safe preflight dependencies" no longer applies to the Swift session, which sends commands without a preflight read. The Rust adapter's preflights, the bridge's connection write block, receipts and identical-request retry stay.
- ADR 0018, Lifecycle and failure invariants: "Operations move from queued to executing to completed; cancellation or replacement invalidates publication through the existing connection and selection generations." The session has no operation states or generations. It drops a history page whose task is no longer selected, and the serial worker orders reads.
- ADR 0018, Lifecycle and failure invariants: "publishes refreshed resources if its generation is still current". The session publishes every refresh result. Keeping confirmed values after a failed refresh and never resubmitting a saved command stay.
- [ADR 0023](0023-enforce-reviewed-intent-at-application-boundaries.md), Consequences: "Swift preflight remains a presentation and reconciliation check". Swift no longer reads or compares before rename and archive. The name guard at the write stays.
- [ADR 0016](0016-configurable-inactive-task-archiving.md), Decision: "serializes its reads and writes through the existing session". The session sends preview and archive requests directly, and the serial worker orders them. Dropping late preview responses, requiring a fresh preview after failure and never resubmitting an uncertain archive stay.

[ADR 0026](0026-use-swift-observation-for-native-presentation.md) is still Proposed and lists request controls among the `TrackerPresentation` values. This record removes the shared request-control value; each feature presentation carries its own flag.

## Consequences

`TrackerSession` loses its scheduling, recovery and review code. Each action is a request, a result and a reload. A new feature adds its own in-flight flag instead of a priority in the operation gate.

Conflicts surface on save instead of while editing. After a double timeout, the app may report a write that actually landed; the reload shows the current state, and resubmitting creates a new request. A change made while the Mac sleeps fails and must be repeated after wake.

Native E2E tests check that one window owns the open sheet, that other windows disable editor actions until it closes, and that tracking still works from another window during an editor request. Portable tests check the independent feature states.

The bridge still marks a failed remote write as requiring a refresh and rejects later writes until a task-list refresh succeeds. The reload after each failure clears that state.
