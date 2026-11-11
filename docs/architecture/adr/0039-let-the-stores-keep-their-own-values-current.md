# 0039: let the stores keep their own values current

## Status

Accepted. It partially supersedes the clauses of ADR 0032, ADR 0035, ADR 0037 and ADR 0038 listed under [Superseded clauses](#superseded-clauses). All other decisions in those records remain accepted.

Implemented by #395.

## Date

2026-11-11

## Context

After [ADR 0038](0038-move-the-remaining-feature-rules-out-of-tracker-sync.md), four stores still showed some of their values only when `TrackerSync.publish()` ran. `TrackingStore`, `TrackingAutomationStore`, `DailyTotalsStore` and `MenuBarStore` each held a `publishChanges` closure that called back into `TrackerSync`, which then pushed values into the stores:

- values derived from the clock: the timer text, today's total, each task's total and duration. `TrackerSync`'s one-second display timer and `MenuBarStore`'s one-minute menu bar clock called `publish()`.
- state a store had recorded earlier: the tracking error, the auto-pause status and the totals status and explanation. They appeared only at the next publish, usually the one after a reload.

`TrackerSync` passed the inputs one store needed from another, such as the task list and the running worklog, as arguments to `publish`. A new feature that shows a live value, such as search results with durations, would have needed another call in `publish()`.

## Decision

`TrackerSync.publish()` and the `publishChanges` closures go. Each store updates its observed values when their inputs change.

### Display clock

`Shared/DisplayClock` drives the values derived from the clock. It ticks every second while a window or the menu is visible, and every minute while the menu bar shows today's total, both only while the Mac is awake and a worklog runs. It also refreshes once when the window or the menu shows or hides, because the values may be stale while nothing showed them.

`TrackerSync` reports start, sleep, wake and visibility to it, `TrackingStore` reports whether a worklog runs, and `MenuBarStore` reports the menu bar clock setting. `TrackingStore` refreshes the timer text on each tick, and `DailyTotalsStore` checks for a new day and refreshes its totals. `MenuBarStore` no longer runs a timer.

`TrackerFeatures` creates the clock and passes it to these stores and to `TrackerSync`. Views don't read it.

### Stored values

The observed values stay stored and change-guarded. Computing them from an observable clock would be less code, but every view that reads a timer or a total would redraw each second, and the per-task `TaskDailyTotal` objects would lose their purpose.

### When values change

- `TrackingStore` refreshes the timer text on a tick, on a new task list and on wake. It reads `ConnectionStore.confirmed` itself.
- A failed tracking request, auto-pause or auto-resume still records its error first. The store that sent the request shows it once its reload returns, so the alert appears over the reloaded state as before. Starting a request or dismissing the error hides it at once.
- `TrackingAutomationStore` shows the paused status as soon as it holds or drops a task.
- `DailyTotalsStore` reads the tasks from `TaskCatalogStore` and the running worklog from `TrackingStore`. It refreshes on a tick, a task list, a report request, a failure, wake and the rollover. `TrackerSync` hands it each task list after the catalog and the tracking store.

The new folder dependencies, DailyTotals → TaskCatalog and DailyTotals → Tracking, form no cycle. Every folder may depend on `Shared/`.

### Rejected alternatives

- **Each store runs its own timer.** Every store would repeat the visibility and sleep rules.
- **`TrackerSync` keeps the timer and calls each store's refresh.** `TrackerSync` would still have to know which stores show time, and a new live value would still add a call there.

### Superseded clauses

This record replaces these clauses:

- ADR 0032, "publish() stays": "`TrackerSync.publish()` refreshes values derived from the clock (`timerText`, today's totals, the auto-pause status) and recorded errors, which the stores then assign. … `publish()` keeps its name." `publish()` is removed. `DisplayClock` drives the clock values, and each store shows its recorded state itself.
- ADR 0035, "TrackingActions is deleted": "Cancelling a pending auto-resume, reloading and publishing reach `Features/Tracking` through closures." Cancelling and reloading still do; there is no publishing closure.
- ADR 0035, "Screen-lock auto-pause": "Reloading, recording a tracking failure and publishing come in through closures." Reloading comes in through a closure. The store records and shows tracking failures on `TrackingStore` directly.
- ADR 0035, "`TrackerSync` keeps the orchestration", as amended by ADR 0038: "`TrackerSync` keeps loading, failure recovery, publishing, polling and the display, rollover and menu bar timers". `TrackerSync` keeps loading, failure recovery and polling. `DisplayClock` runs the display and menu bar timers, and `DailyTotalsStore` runs the rollover timer, as ADR 0037 decided.
- ADR 0037, "Decision": "The store hands that list, read failures, the refresh after a rollover and publishing back to `TrackerSync` through closures". The store hands back the list, read failures and the refresh; it refreshes its own values.
- ADR 0038, "Menu bar clock": "`MenuBarStore` runs the clock. It gets the scheduler and a publish closure that `TrackerFeatures` wires to `TrackerSync`." `MenuBarStore` turns the `DisplayClock`'s minute ticks on and off, and has no scheduler or publish closure. `TrackerSync` no longer calls its `start()`, `sleep()` and `wake()`, and the order in which it receives the task list no longer matters for the clock.
- ADR 0038, "Connection recovery rule": "`TrackerSync` asks it, then records the failure, schedules polling and publishes as before." `TrackerSync` records the failure and schedules polling; it doesn't publish.
- ADR 0038, "What stays in TrackerSync": "… the order in which stores take in a connection or a task list, and publishing." `TrackerSync` keeps the order and doesn't publish.

## Consequences

A store's observed values change only through its own methods. A new feature with a live value registers a refresh with `DisplayClock` and adds no code to `TrackerSync`.

The paused status and the totals "Loading" status may appear a moment earlier than before, when the store changes them instead of at the next publish.

`DisplayClock` merges the display and menu bar timers into one owner, but keeps two timers so the menu bar clock still ticks once a minute while no window or menu shows.

A store that shows a value derived from another store must refresh when that store changes. `DailyTotalsStore` relies on `TrackerSync` handing it the task list after the catalog and the tracking store; the `acceptTaskList` comment says so.
