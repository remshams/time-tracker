# 0026: move native feature workflows out of TrackerSession

## Status

Proposed. This implements [issue #77](https://github.com/remshams/time-tracker/issues/77). It changes no decision in an earlier record.

## Date

2026-10-17

## Context

`TrackerSession` in the portable `TrackerClient` package held every native feature workflow and had grown to 2,164 lines. It owned each feature state and each drain function, from task creation to worklog deletion and tracking automation.

Each editor gate listed every other editor by hand; `blocksConnectionChange` appeared 25 times. Adding worklog deletion in [PR #60](https://github.com/remshams/time-tracker/pull/60) meant editing every editor gate, `isBlockingControls`, wake, connect, shutdown, and the history updates.

Correction, move, and deletion repeated one flow: refresh the task list, find the worklog in bounded history, compare it with the reviewed intent, write, and reconcile after an uncertain failure. Each copy repeated the sleep, generation-token, and recovery handling.

The orchestration stayed in the session because it held the shared pieces: the single operation gate, the session generation, sleep and wake, the shared catalog, tracking, and history state, and the rule that an open editor blocks other editors and connection changes.

## Decision

- Each feature owns its workflow in its feature folder, for example `Features/WorklogDeletion/WorklogDeletionWorkflow.swift`. A workflow owns its feature state and its drains. Task creation, renaming, archiving, bulk archiving, worklog correction, move, deletion, and tracking automation each have one.
- Workflows depend on narrow interfaces instead of the session:
  - `OperationRunner` takes the operation gate, passes the session generation as a token, reports sleep, and publishes.
  - `WorkflowResources` refreshes and accepts the task list, reconciles after a failed write, finds a worklog in bounded history, and updates history, selection, and the tracking error.
  - `EditorRegistry` answers whether any editor blocks controls, other editors, or connection changes, and drains pending writes and reads in registration order.

  The session implements `OperationRunner` and `WorkflowResources`. Workflows hold them as unowned references, because the session owns every workflow and keeps itself alive while an operation runs.
- Each feature state declares its own blocking rules through `TrackerEditor`: whether it blocks controls, whether it blocks other editors, the message for a blocked connection change, whether a write waits for wake, and its sleep, connection-change, worklog-observation, and reset hooks. A new editor adds its rules to its own state and one registration line to the session. The gates of the other editors stay unchanged.
- `GuardedWorklogWrite` runs the shared guarded worklog write once for correction, move, and deletion. Each workflow supplies its history lookup, its preflight decision, its write and response check, and how it finishes a committed intent. The preflight returns apply, committed, review, or reject; the shared write applies the matching state transition.
- `TrackerSession` composes the workflows and keeps lifecycle, connection changes, timers, refresh and report scheduling, history paging, publication, and the control queue. Manual start and stop, history pages, and connection access stay in the session because they share the control queue and its generation. The session's public properties, commands, and `onChange` keep their signatures, so the macOS app needs no changes.
- Behavior does not change. Every editor gate checks all other editors, so at most one editor blocks at a time. The registry order therefore decides neither which connection message appears nor which write drains first. The shared write publishes after a worklog-changed review and after the final failure for all three editors, as correction already did. It does not publish when sleep interrupts recovery. In each case the operation gate publishes the same state as soon as the work returns, so observers see no new state.

## Consequences

A new editor needs a state that conforms to `TrackerEditor`, a workflow that conforms to `EditorWorkflow`, and one registry entry. A new guarded worklog write implements `GuardedWorklogEditor` instead of copying the recovery flow.

The session shrank from 2,164 to about 1,300 lines. It still owns the shared resources, so `WorkflowResources` grows when a workflow needs another shared value. Review those additions so the interface stays limited to shared state.

The existing portable tests cover the workflows through the session's public commands and stayed unchanged. One correction test was added, because active and history observations now use separate hooks. The Swift mutation scope maps each workflow to the suites that exercise its state, and maps the registry to every session suite.
