# 0038: move the remaining feature rules out of TrackerSync

## Status

Accepted. It partially supersedes the clauses of ADR 0032 and ADR 0035 listed under [Superseded clauses](#superseded-clauses). All other decisions in those records remain accepted.

Implemented by #384.

Partially superseded by [ADR 0039](0039-let-the-stores-keep-their-own-values-current.md) for `MenuBarStore` running the menu bar clock with a scheduler and a publish closure, and for `TrackerSync` keeping the display timer and publishing; all other decisions remain accepted.

## Date

2026-11-10

## Context

After [ADR 0035](0035-put-user-actions-on-feature-stores.md) and [ADR 0037](0037-let-daily-totals-load-and-roll-over-in-their-store.md), `TrackerSync` still held four rules that each belong to one feature:

- the menu bar clock, which republishes once a minute while the menu bar shows the daily total and a timer runs,
- the rule for which read failures mark the connection as failed,
- `showTask(_:)`, which changes the tab and selection to a created task and reloads the history once,
- the connection change reset, which called internals of six stores in turn.

## Decision

### Menu bar clock

`MenuBarStore` runs the clock. It gets the scheduler and a publish closure that `TrackerFeatures` wires to `TrackerSync`. The app's `MenuBarPreferencesStore` turns the clock on and off with `setClockEnabled(_:)` and no longer gets `TrackerSync`.

`TrackerSync` calls the store's `start()`, `sleep()` and `wake()` with the other stores, and hands it each loaded task list after `TrackingStore`, because the clock runs only while a worklog does.

### Connection recovery rule

`ConnectionStore.requiresRecovery(after:)` decides which read failures mark the connection as failed. `TrackerSync` asks it, then records the failure, schedules polling and publishes as before.

### Showing a created task

`TaskCatalogStore.showTask(_:)` changes the tab and selection and reloads the history through the catalog's existing `reloadHistory` closure. `TrackerFeatures` passes it to `TaskCreationStore` through the existing `showTask` closure. ADR 0035 lets a store call another store directly when that forms no cycle, and `TaskCreation` → `TaskCatalog` forms none. The closure stays to keep this change small, and replacing it with a direct catalog dependency is a later choice.

### Connection change reset

Each store that holds state of the previous connection gets an `acceptConnection` method: `TrackingAutomationStore`, `ConnectionStore` (which takes the new settings), `DailyTotalsStore`, `TaskCatalogStore`, `WorklogHistoryStore` and `TrackingStore`. `TrackerSync` calls them in the old order, saves the settings between the daily totals and the catalog as before, and then takes in the task list, as it calls `acceptTaskList`.

### What stays in TrackerSync

`TrackerSync` keeps loading, refreshing, polling, the display timer, visibility, sleep and wake, recording connection failures, the order in which stores take in a connection or a task list, and publishing.

The new folder dependencies are on `Contracts/` only, so neither target gains a folder cycle.

### Superseded clauses

This record replaces these clauses:

- ADR 0032, "Reload closures": "So #327 adds `TrackerSync.showTask(_:)`". `TaskCatalogStore.showTask(_:)` holds the logic. `TaskCreationStore` keeps its `showTask` closure.
- ADR 0035, "Environment first": "Inputs that drive the orchestration rather than a feature, such as window and menu visibility, sleep and wake, and the menu bar clock setting, still go to `TrackerSync`." The menu bar clock setting goes to `MenuBarStore`.
- ADR 0035, "`TrackerSync` keeps the orchestration": "`TrackerSync` keeps loading, failure recovery, publishing, polling and the display, rollover and menu bar timers". The menu bar timer moves to `MenuBarStore`, and the rule for which read failures need recovery moves to `ConnectionStore`.
- ADR 0035, "Superseded clauses", on ADR 0030: "`ConnectionStore.connect(_:)` starts the connection change, and `TrackerSync` keeps resetting the other stores and reloading." Each store resets its own state in `acceptConnection`. `TrackerSync` calls those methods in order and reloads.
- ADR 0035, "Superseded clauses", on ADR 0032: "A selection change still reloads the history through `TrackerSync`, so `showTask` stays." A selection change reloads the history through the catalog's `reloadHistory` closure, so `showTask` moves to `TaskCatalogStore`.
- ADR 0035, "Superseded clauses", on ADR 0034: "The app classes still send visibility, sleep and wake, and the menu bar clock setting to `TrackerSync`." The menu bar clock setting goes to `MenuBarStore`.

## Consequences

A store's rules can be read and tested next to its state. `TrackerSync` reads as a list of calls in a fixed order.

`TrackerSync` must hand `MenuBarStore` the task list after `TrackingStore`. The `acceptTaskList` comment on `MenuBarStore` says so.

`TrackerSync` still saves the connection settings, because `ConnectionStore` doesn't hold the settings repository.
