# 0035: put user actions on feature stores

## Status

Accepted. It partially supersedes the clauses of ADR 0030, ADR 0032 and ADR 0034 listed under [Superseded clauses](#superseded-clauses). All other decisions in those records remain accepted.

## Date

2026-11-09

## Context

Views in the macOS app read state from the feature stores, but many of their actions go through two classes in the package's `App/`. `TrackerSync` takes the tab and task selection, history paging, connecting, refreshing and the auto-pause setting. `TrackingActions` starts and stops tracking, dismisses the tracking error and runs the menu's primary action. Both classes collect actions from many features, so they grow with every feature, and a view needs a store for its state and `TrackerSync` or `TrackingActions` for its buttons.

[ADR 0034](0034-allow-no-cycles-between-folders-in-a-target.md) lets app views read `TrackerSync` and `TrackingActions` directly. The same record explains why a package store can't call `TrackerSync`: `TrackerFeatures` in the package's `App/` creates every store, so the call would form a folder cycle. The dialog stores already work around that with closures ([ADR 0032](0032-move-the-dialogs-into-feature-stores.md), [ADR 0033](0033-report-bulk-archive-preview-failures-through-a-closure.md)).

`TrackerSync` also holds the screen-lock auto-pause, and its `acceptTaskList` and `finishConnection` decide how each store takes in a loaded task list.

## Decision

### Actions live on the owning feature store

Each user action is a method on the store of the feature that owns it, for example `TaskCatalogStore.select(_:)`, `WorklogHistoryStore.loadOlder()` and `ConnectionStore.connect(_:)`. A view then reads state and calls actions on the same feature stores.

### Environment first

Views read feature stores from the SwiftUI environment, as they already do for state. App classes that SwiftUI can't inject, such as AppKit controllers, get the stores through `init`.

Inputs that drive the orchestration rather than a feature, such as window and menu visibility, sleep and wake, and the menu bar clock setting, still go to `TrackerSync`.

### Closures where the environment can't reach

A package store that has to trigger orchestration in the package's `App/`, such as reloading the history after the selection changes, gets a closure. `TrackerFeatures` creates the store and passes a `TrackerSync` method, the same way the dialog stores get `reloadAfterWrite`. A store may read or call another feature's store directly when that forms no cycle.

### TrackingActions is deleted

Starting, stopping and dismissing the tracking error move to `Features/Tracking`. The menu's primary action moves to `MenuBarStore`. Its folder, `Features/MenuBar`, already depends on Tracking and TrackingAutomation.

Cancelling a pending auto-resume, reloading and publishing reach `Features/Tracking` through closures. TrackingAutomation reads `TrackingStore`, so a direct call from Tracking into TrackingAutomation would form a cycle.

### Screen-lock auto-pause

Pausing on lock and resuming on unlock move to `Features/TrackingAutomation`. It may read `TrackingStore` directly. Reloading, recording a tracking failure and publishing come in through closures.

### TrackerSync keeps the orchestration

`TrackerSync` keeps loading, failure recovery, publishing, polling and the display, rollover and menu bar timers, along with visibility, sleep and wake. Each store gets an `accept(_:)` method for its part of a loaded task list, and `TrackerSync` calls them in order and decides on follow-up reloads. Values one store needs from another, such as the active worklog before and after the update, are arguments.

Once no view reads `TrackerSync` from the environment, it no longer needs to be observable.

### Rejected alternatives

- **Protocols in `Contracts/`.** A store would depend on a protocol that `TrackerSync` implements. That breaks the cycle too, but each protocol adds a file and a test double, and the package would have two mechanisms for one job.
- **A shared hub in `Shared/`.** A class that collects actions and reads the feature stores would still depend on those stores, so the cycle stays and only moves. `Shared/` holds helpers no feature owns, and this hub would belong to all of them.
- **Keeping the actions on `TrackerSync` and `TrackingActions`.** Both would keep growing with each feature, and views would keep reading two kinds of objects.

### Steps

[Issue #369](https://github.com/remshams/time-tracker/issues/369) moves the actions in order. Until the last step merges, some views still call `TrackerSync` and `TrackingActions`.

- [#370](https://github.com/remshams/time-tracker/issues/370) moves the tab and task selection onto `TaskCatalogStore` and history paging onto `WorklogHistoryStore`.
- [#371](https://github.com/remshams/time-tracker/issues/371) moves connecting and refreshing onto `ConnectionStore` and the auto-pause setting onto `TrackingAutomationStore`.
- [#372](https://github.com/remshams/time-tracker/issues/372) moves starting and stopping into `Features/Tracking`, the menu's primary action into `MenuBarStore`, and deletes `TrackingActions`.
- [#373](https://github.com/remshams/time-tracker/issues/373) moves the screen-lock auto-pause into `Features/TrackingAutomation`.
- [#374](https://github.com/remshams/time-tracker/issues/374) adds the `accept(_:)` methods.

### Superseded clauses

This record replaces these clauses:

- ADR 0030, "Who writes the stores": "Stores are passive. A store doesn't fetch data or call other stores, with one exception: `WorklogHistoryStore` sends its own page requests". A feature store now also sends the requests for its own actions, and may call another store or a closure from `TrackerFeatures`.
- ADR 0030, "Who writes the stores": "A new `TrackerSync` class and the orchestration code in `App/` write into stores through methods such as `apply(...)`." A feature store also writes its own state when it runs an action, and `TrackerSync` writes a loaded task list through each store's `accept(_:)`.
- ADR 0030, "Who writes the stores": `WorklogHistoryStore` "sends a page request when `App/` asks it to reload or the user loads more". It also reloads when the user retries.
- ADR 0030, "Who writes the stores": "When the failure means the connection needs recovery, the store reports it to its caller in `App/`". The store reports it through a closure that `TrackerFeatures` wires to `TrackerSync`. The history store still doesn't call the connection store.
- ADR 0030, "Who writes the stores": "`TrackerSync` owns polling, the display and rollover timers, sleep and wake, auto-pause and the connection change flow." Auto-pause moves to `Features/TrackingAutomation`. `ConnectionStore.connect(_:)` starts the connection change, and `TrackerSync` keeps resetting the other stores and reloading.
- ADR 0030, "Coordination": "Cross-feature follow-ups are explicit calls in `App/` in a fixed order (orchestration)." A follow-up may also be a direct call from one feature store to another when that forms no cycle, such as the menu's primary action starting tracking, or a closure from `TrackerFeatures` into `App/`. The order stays fixed, and stores still don't observe each other. The same goes for the ADR 0030 consequence "Cross-feature follow-ups are visible as calls in `App/`."
- ADR 0032, "Reload closures": "Only `TrackerSync` can reload the history". `WorklogHistoryStore` also reloads itself when the user retries. A selection change still reloads the history through `TrackerSync`, so `showTask` stays.
- ADR 0032, "Reloads and selection": "`reloadAfterTaskWrite(then:)`, `reloadAfterWorklogWrite()`, `changeTab(_:)` and `selectTask(_:)` move from the session to `TrackerSync`." The two reloads stay in `TrackerSync`. Changing the tab and the selection move to `TaskCatalogStore`.
- ADR 0032, "TrackerFeatures": it "creates and holds the stores, `TrackerSync`, `TrackingActions` and the dialog stores". `TrackingActions` goes. `TrackerFeatures` also passes the new closures.
- ADR 0032, "TrackingAvailability": "`TrackingActions`, `TrackerMenuSources` and the `TrackerStore` forwarders use it." The starting and stopping code in `Features/Tracking`, `MenuBarStore` and `TrackerMenuSources` uses it.
- ADR 0034, "Rule": "The app's `Features/Tracking` may use the package's `App/TrackingActions`". The rule it illustrates stays; `TrackingActions` goes.
- ADR 0034, "Removing the existing cycle": "#362 lets views and the status item use `TrackerSync` and `TrackingActions` directly." Views and the status item call user actions on feature stores. The app classes still send visibility, sleep and wake, and the menu bar clock setting to `TrackerSync`.
- ADR 0034, "Superseded clauses": "After #362, views call `changeTab(_:)` and `selectTask(_:)` on `TrackerSync`." Views call `changeTab(_:)` and `select(_:)` on `TaskCatalogStore`.
- ADR 0034, Consequences: "A feature in the app target can read `TrackerSync` and `TrackingActions` from the package without creating a cycle". That reading still forms no cycle, but views read feature stores instead.

## Consequences

A view reads one kind of object for both state and actions. A new action goes to its feature's store, and `TrackerSync` and `TrackerFeatures` change only when it needs orchestration.

Tests for an action move to the store tests, next to the state they check.

A store with orchestration follow-ups takes more closures, and its initializer grows. `TrackerFeatures` is the one place that wires them, and a reader has to look there to see what a closure does.

Splitting the timers and polling out of `TrackerSync` is a later decision.
