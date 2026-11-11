# 0037: let daily totals load and roll over in their store

## Status

Accepted. It partially supersedes the clauses of ADR 0030 and ADR 0035 listed under [Superseded clauses](#superseded-clauses). All other decisions in those records remain accepted.

Implemented by #383.

Partially superseded by [ADR 0039](0039-let-the-stores-keep-their-own-values-current.md) for `DailyTotalsStore` publishing through a closure and for the display timer staying in `TrackerSync`; all other decisions remain accepted.

## Date

2026-11-10

## Context

After [ADR 0035](0035-put-user-actions-on-feature-stores.md), `DailyTotalsStore` held today's totals but none of the logic that loads them. `TrackerSync` called the store's `begin`, `validate`, `accept` and `fail` in order, ran the rollover timer at midnight, and switched the day inside the display timer tick. The store was a passive container driven from `App/`, the pattern ADR 0035 removed for the other stores.

## Decision

`DailyTotalsStore` gets the report client, the connection store, the clock and the scheduler. It sends the report request, validates the reply, records a failure, switches the day and runs the rollover timer. `TrackerSync` still decides when to load, and calls the store's `start()`, `sleep()`, `wake()`, `load()` and `acceptTaskList(reportRequested:)` like any other store.

The report call also returns the task list (`refreshTaskListWithTotals`). The store hands that list, read failures, the refresh after a rollover and publishing back to `TrackerSync` through closures that `TrackerFeatures` wires, as ADR 0035 describes for calls from a store up into `App/`.

`DailyTotalsStore` and `TrackingStore` hold their clock, so their methods no longer take a `clock:` parameter.

The new folder dependency, DailyTotals → Connection, forms no cycle.

### Superseded clauses

This record replaces these clauses:

- ADR 0030, "Who writes the stores": "`TrackerSync` owns polling, the display and rollover timers, …". The rollover timer moves to `DailyTotalsStore`. Polling and the display timer stay in `TrackerSync`.
- ADR 0035, "`TrackerSync` keeps the orchestration": "`TrackerSync` keeps … the display, rollover and menu bar timers". The rollover timer moves to `DailyTotalsStore`.
- ADR 0035, "Status": "`DailyTotalsStore` has none, because whether to invalidate the report depends on loading details that `TrackerSync` owns." The store now owns those details and has an `acceptTaskList(reportRequested:)` method.

## Consequences

`TrackerSync` no longer holds the report client or the clock, and no longer reads the report request or the day.

`TrackerSync` used to switch the day before every reload. Without that call, the totals explanation hides a report error once its day has ended, so it reads the same as before in most cases. In the short window after midnight, it now says "Today's total is unavailable" instead of the previous day's error.
