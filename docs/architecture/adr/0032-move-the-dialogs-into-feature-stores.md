# 0032: move the dialogs into feature stores

## Status

Accepted. It partially supersedes the clauses of ADR 0026 and ADR 0030 listed under [Superseded clauses](#superseded-clauses). All other decisions in those records remain accepted.

Partially superseded by [ADR 0033](0033-report-bulk-archive-preview-failures-through-a-closure.md); all other decisions remain accepted.

Partially superseded by [ADR 0034](0034-allow-no-cycles-between-folders-in-a-target.md) for the reasons given for the reload closures and against the first rejected alternative, though both decisions stay, for views calling `changeTab(_:)` and `selectTask(_:)` through `TrackerStore`, and for the coordinator and app menus reading the dialog stores through `TrackerStore`, which keeps its Mac settings and forwarders; all other decisions remain accepted.

Partially superseded by [ADR 0035](0035-put-user-actions-on-feature-stores.md) for the reason that only `TrackerSync` can reload the history, for moving `changeTab(_:)` and `selectTask(_:)` to `TrackerSync`, for `TrackerFeatures` holding `TrackingActions`, and for `TrackingActions` using `TrackingAvailability`; all other decisions remain accepted.

## Date

2026-11-08

## Context

[ADR 0030](0030-replace-the-session-with-feature-stores.md) moved the task catalog, history, connection, tracking and totals into stores and left a later issue to cover the dialogs and the deletion of `TrackerSession`. Seven dialog flows remain in the session. A dialog value passes through a `*State` class, a `*Presentation` struct, a session forwarder, `TrackerPresentation` and an app store that only forwards.

Once the dialogs move, `TrackerPresentation` only copies five values that other stores can answer themselves: `canStopTracking`, `canStartTracking(taskID:)`, the running task name, and the archivable and unarchivable task IDs. Every `publish()` recomputes them. The menu reads the same start and stop checks, but `TrackingActions` can't answer them for it: `TrackingActions` sits in `App/` and depends on `MenuBarStore`, so `TrackerMenuSources` reading it would form a folder cycle.

[Issue #324](https://github.com/remshams/time-tracker/issues/324) covers the dialogs and the session. [Issue #334](https://github.com/remshams/time-tracker/issues/334) covers `TrackerPresentation`. This record decides both, so #334 has no record of its own.

## Decision

### Dialog stores

Each dialog gets one public `@Observable` store in its folder under `Features/`, with individual properties (`name`, `isSubmitting`, `error`, a computed `canSubmit`) and methods (`open`, `setName`, `cancel`, `submit`). The `*State` class, the `*Presentation` struct and the app's forwarding store are deleted. `canOpen...` and per-task checks such as `canArchive(taskID:)` move to the dialog store.

A dialog store gets the dependencies it uses through its initializer, such as the client, the clock and the stores it reads.

### Reload closures

Features don't depend on `App/`, so a dialog store can't call `TrackerSync`. Each dialog store takes a reload closure instead:

```swift
// Task dialogs. `then` runs after the reload attempt and before the last publish.
reloadAfterWrite: @MainActor (_ then: () -> Void) async -> Void

// Worklog dialogs.
reloadAfterWrite: @MainActor () async -> Void
```

`TrackerSync` has a method for each, and the code that creates a store passes it by name:

```swift
TaskRenameStore(client: client, reloadAfterWrite: sync.reloadAfterTaskWrite(then:))
WorklogDeletionStore(client: client, reloadAfterWrite: sync.reloadAfterWorklogWrite)
```

The `@MainActor` is part of the type. Default isolation doesn't apply to function types, so without it the closure type isn't isolated to the main actor, and the non-Sendable `then` would cross an isolation boundary. A store captures the method strongly. That forms no cycle, because `TrackerSync` doesn't hold dialog stores.

`then` holds the follow-up to the reload, and it runs in the dialog store. It also runs when the reload failed, so it can't assume the task list changed. Restoring a task calls `catalog.rememberRestoredTask`.

Creating a task also has to show it: change the tab and selection, then reload the history once if either changed. Only `TrackerSync` can reload the history, and calling `changeTab(_:)` and `selectTask(_:)` in a row could send two requests. So #327 adds `TrackerSync.showTask(_:)`, and `TaskCreationStore` is the one store with a second closure, `showTask: @MainActor (TaskItem) -> Void`, which it calls in `then`. When the catalog doesn't hold the created task yet, `showTask` still picks the tab from it, as today, and that tab restores its remembered selection or its first task.

### Reloads and selection

`reloadAfterTaskWrite(then:)`, `reloadAfterWorklogWrite()`, `changeTab(_:)` and `selectTask(_:)` move from the session to `TrackerSync`. Views call the last two through `TrackerStore`.

### TrackerFeatures

`TrackerFeatures` is a public type in the package's `App/` with no methods, so the app target can create it. It replaces the session's `init`: it creates and holds the stores, `TrackerSync`, `TrackingActions` and the dialog stores, and it passes the reload closures. `TrackerStore` and the test fixture both use it. The session's read-only forwarders go, and callers read the stores.

### Views

Sheets and context menus read dialog stores from the SwiftUI environment. The coordinator and the app menus read them through `TrackerStore` properties that point at `features`. `TrackerStore` otherwise keeps its Mac settings and forwarders.

### Closing sheets

SwiftUI renders a sheet during its dismissal animation, so its content must stay until the sheet is gone. The separate `sheetContent` copies go. Closing sets `isPresented` and `isSubmitting` to false and leaves the rest of the content until the next `open`. `canSubmit` is false while closed, so the kept content can't send a second request.

### TrackingAvailability

A `TrackingAvailability` struct in `Features/Tracking` reads `ConnectionStore`, `TaskCatalogStore` and `TrackingStore` and offers `canStart(taskID:)`, `canStop` and `canSend`. `TrackingActions`, `TrackerMenuSources` and the `TrackerStore` forwarders use it. Features may depend on each other, so the menu can read it without depending on `App/`.

### Removing TrackerPresentation

`TrackerMenuSources` computes the running task name, because only the menu reads it, and `startableTaskIDs` becomes a local value in its `read()`. `TaskContextMenu` calls `TaskArchivingStore.canArchive(taskID:)` and `canUnarchive(taskID:)`, and the ID sets go. `TrackerPresentation` and `TrackerSync.onChange` are then deleted.

`TrackerTaskPresentationCoordinator` moves to `Observations`, as ADR 0030 decided.

### publish() stays

`TrackerSync.publish()` refreshes values derived from the clock (`timerText`, today's totals, the auto-pause status) and recorded errors, which the stores then assign. Observation reports changes to stored properties, and the clock isn't one, so something has to recompute these values on each tick and after each write. `onChange` goes, because stores notify their readers themselves. Calls to `publish()` that no longer affect those values are removed in their own PR. `publish()` keeps its name.

### Rejected alternatives

- **Features depending on `App/`, or a shared folder for `TrackerSync`.** Either would let a dialog store call the sync directly. The first breaks the rule that only `App/` composes features, and the second would hold one class that isn't shared. A closure gives each store only the calls it needs.
- **`TrackingActions` as the owner of the start and stop checks.** See the folder cycle under Context.
- **Keeping `TrackerPresentation` for the five remaining values.** It would keep one more copy and one more recomputation per `publish()` for values the stores can answer when asked.

### Steps

Issues #324 and #334 list the steps. During the migration the session creates each new store and passes its closures.

### Superseded clauses

This record replaces these clauses:

- ADR 0026: "Keep each sheet's last presented content while it closes, because SwiftUI renders the sheet during its dismissal animation." The reason stays. The store now keeps its own content and no copy is made.
- ADR 0030: "`TrackerPresentation` and `publish()` go away step by step." `TrackerPresentation` goes and `publish()` stays.
- ADR 0030: "Stores are passive. A store doesn't fetch data or call other stores, with one exception: `WorklogHistoryStore` sends its own page requests". Dialog stores also send their own requests, read the stores they depend on and run their reload closure, and their `then` can write to the catalog.
- ADR 0030: "A new `TrackerSync` class and the orchestration code in `App/` write into stores". Dialog stores also write their own state, and a `then` step writes to the catalog.
- ADR 0030: "Cross-feature follow-ups are explicit calls in `App/` in a fixed order". A follow-up to a dialog write is an explicit call in that store's `then`, and the order is still fixed. Stores still don't observe each other. The same goes for the ADR 0030 consequence "Cross-feature follow-ups are visible as calls in `App/`."
- ADR 0030: "A store keeps a value that views read and that it derives from its own state … as a stored property." This no longer applies to a dialog store's `canSubmit` and `canOpen`, which are computed. One sheet or menu entry reads them, so the extra redraws don't matter.
- ADR 0030: "Rows get plain values from their parent, so a row redraws only when its own values change." A row may call `TrackingAvailability.canStart(taskID:)` and `TaskArchivingStore.canArchive(taskID:)`. The consequences below cover the redraws.
- ADR 0030: "keep a sheet's last content while it closes", and the kept sheet content in its list of ADR 0026 decisions that stay. The closing rule above replaces them.

## Consequences

A dialog's state, checks and request live in one file in its feature folder. A new dialog adds one store, and its creator passes a reload closure.

Views can redraw a little more often, because a row that calls `canStart` also tracks `catalog.tasks`. These values also update when a store changes, not at the next `publish()`. The list is small and one person uses the app, so neither matters in practice.

A closed dialog store still holds its last content. Code that reads it must check `isPresented` or `canSubmit`.
