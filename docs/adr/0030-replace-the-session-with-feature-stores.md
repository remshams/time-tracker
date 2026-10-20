# 0030: replace the session with feature stores

## Status

Proposed. Once accepted, it partially supersedes the clauses of ADR 0026 listed under [Superseded clauses](#superseded-clauses). The other ADR 0026 decisions remain accepted.

## Date

2026-10-20

## Context

`TrackerSession` in the portable `TrackerClient` package holds every feature's state: 14 feature state objects, the display, poll and rollover timers, sleep and visibility, the connection change flow and the dialog flows. Views can't read that state directly. After each change the session calls `publish()`, which runs its one `onChange` callback. `TrackerStore` then calls `TrackerPresentation.update(from:)`, which copies each value views read into an `@Observable` model, and updates the menu bar snapshot. A new value is declared in `TrackerPresentation`, set in its initializer and set again in `update(from:)`, and usually gets a session accessor too. The availability checks, such as `canOpenTaskRename`, live in the session because only the session sees every feature at once.

[ADR 0026](0026-use-swift-observation-for-native-presentation.md) kept the session as a plain class, because its state changed at many call sites and derived values needed a change filter. [ADR 0029](0029-send-native-requests-directly.md) has since removed the session's operation gate, queues and recovery code. Each feature state now owns its in-flight flag, and each action is a request, a result and a reload. What remains in the session is mostly state that features could own themselves, plus the code that passes it on.

The user plans to move the app to a single window plus the macOS menu bar, which is separate work. With one window, state that is per window today, such as the task selection, can be app-wide.

The app targets macOS 15 and Swift 5 mode, the portable package declares macOS 14 and Swift 5 mode, and Linux CI runs Swift 6.1.3. `Observations`, an `AsyncSequence` over observable properties, needs Swift 6.2 and macOS 26. Every Mac the app runs on has the current macOS.

## Decision

### Stores

- Replace the session's state with app-wide `@Observable` stores in the portable package: `ConnectionStore`, `TaskCatalogStore` (tasks, tab and selection), `WorklogHistoryStore` (pages of the selected task), `TrackingStore` and `DailyTotalsStore`. Views read them directly; `TrackerPresentation` and `publish()` go away step by step.
- A store's state lives as long as the app, so it survives closing the window.
- An existing `…State` class becomes the `…Store` class in the same feature folder. No new folder collects stores.
- `WorklogHistoryStore` holds one task's pages. Worklogs are not cached per task in `TaskCatalogStore`. Reopening a task reloads its first page, as today.
- Keep the ADR 0026 rules: assign a property only when its value changes, give each task total its own observable object, and keep a sheet's last content while it closes.

### Who writes the stores

Stores are passive. A store doesn't fetch data or call other stores. A new `TrackerSync` class and the orchestration code in `App/` write into stores through methods such as `apply(...)`. Views read store properties, and SwiftUI redraws a view when a value it read changes. Views call store methods only for user actions, such as submitting a rename. They don't start loads.

- One refresh response holds tasks, the active worklog and daily totals. `TrackerSync` writes them into the catalog, tracking and totals stores in one pass. If views fetched their own parts, the task list, the timer and the menu bar would each send requests on every poll and could show results from different points in time.
- Polling, auto-pause and the menu bar keep running while the window is closed. Loading tied to a view's lifetime would stop with the view.
- Follow-up loads have a fixed order. History reloads after the catalog applied a refresh, and only when the selection, the task's `latestStart` or the active worklog changed. `TaskCatalogState.apply(_:previousActive:active:)` already reports that as a `Bool`. The code in `App/` checks it and then asks `WorklogHistoryStore` to reload; the catalog store never calls the history store.
- Stores and `TrackerSync` live in the portable package, where `swift test` on Linux and the Swift mutation check cover them. View code in the app target is covered only by E2E tests.

`WorklogHistoryStore` sends its own page requests when asked to reload or load more, and drops a page whose task is no longer selected. `TrackerSync` owns polling, the display and rollover timers, sleep and wake, auto-pause and the connection change flow.

A view may load data with `.task(id:)` only when the data is needed just while that view is visible. No current view needs that.

### Coordination

- Cross-feature follow-ups are explicit calls in `App/` in a fixed order (orchestration). Stores don't observe each other (choreography), because the order of follow-ups and the place to debug them would then depend on observation timing.
- Code outside SwiftUI that reacts to store changes uses `Observations`: the AppKit status menu observer and `TrackerTaskPresentationCoordinator`. `Observations` delivers the value after a change, merges changes made in one synchronous run, and keeps tracking without re-registration. That removes the coordinator's re-registration loop and its read on the next main-actor turn.

### Injection

- Views get stores from the SwiftUI environment with `@Environment(TaskCatalogStore.self)`.
- Rows get plain values from their parent, so a row redraws only when its own values change.
- The AppKit status menu gets stores through its initializer.

### Feature dependencies

- Features may depend on other features as long as the dependencies form no cycle.
- Only `App/` composes features at runtime.
- `Shared/` holds helpers that no feature owns. The first are the time helpers `timestamp`, `commandTimestamp` and `clockDuration`, which currently live in `Features/Tracking/TrackingState.swift`. `hasPositiveWorklogDuration` stays in `WorklogHistory/`, `TaskEditing/` stays a feature, and `MenuBar/` stays one folder in each target.
- `Contracts/` holds protocols and wire models.

Swift compiles a target as one module and doesn't reject cycles between folders in it, so the rule is enforced in review. `AGENTS.md` states it. It is not an architecture primitive, so `docs/architecture/primitives.md` doesn't list it.

### Platform

- Raise the app and the portable package to macOS 27 and Swift 6.4 (Xcode 27), and the Linux CI jobs to Swift 6.4. Step 1 checks that `setup-swift` provides Swift 6.4 on Linux and what the Swift Build default changes.
- After the raise, turn on default `@MainActor` isolation. A draft PR also turns on Swift 6 language mode and counts the errors in CI. Fixes that need no design change, such as annotations, `Sendable` conformances or `sending` parameters, go into that step. If a fix needs a design change, such as how `TrackerWorker` hands work to its queue, strict mode gets its own issue after the slices.

### Steps

[Issue #125](https://github.com/remshams/time-tracker/issues/125) tracks the steps. Each gets its own issue and PR:

0. This record and the `AGENTS.md` rule.
1. The platform raise.
2. Default isolation, and Swift 6 mode if the fixes are mechanical.
3. `Shared/`.
4. Slices: TaskCatalog, WorklogHistory, Tracking, DailyTotals, Connection, MenuBar, `TrackerSync`, tracking actions and auto-pause, the connection change flow.

Each slice moves one store with its views, its part of the menu observer and its `TrackerPresentation` fields. During the migration the session holds the new stores and forwards to them, so the remaining session code keeps working. Each slice adds store tests. Session tests stay until the code they cover moves. E2E tests stay unchanged, because the app's behavior doesn't change.

### Rejected alternatives

- **Worklogs cached per task in the catalog store.** Worklogs belong to tasks, but the app shows one task's history at a time. A cache would need invalidation after every write and poll and gives the user nothing that reloading the first page doesn't.
- **Stores per window.** Needed only while several tracker windows exist. The single-window move makes app scope and window scope the same.
- **Shared code moved out of features.** Banning feature-to-feature dependencies would push most code into `Shared/` and leave thin features. Allowing dependencies without cycles keeps code with the feature that owns it.

### Out of scope

Dialog controllers, sheets, `TrackerTaskPresentationCoordinator`'s routing, the single-window move, merging task creation and rename, and deleting `TrackerSession`. A later issue covers those. Until then, the session keeps the dialog flows and the availability checks that depend on several features.

### Superseded clauses

Once accepted, this record replaces these ADR 0026 clauses. The change filter, the per-task total objects, the kept sheet content, the observable `ConnectionDraftStore` and `@Bindable` drafts stay.

- "Add `TrackerPresentation` to the portable package … `TrackerStore` calls `update(from:)` after each session change." Views read feature stores instead.
- "Keep `TrackerSession` a plain class with its `onChange` callback." The session shrinks to the dialog flows and then goes away; stores replace `onChange`.
- "Feature stores remain as action facades without change publishers." The facades become observable feature stores that own their state.
- "The coordinator observes presentations with `withObservationTracking` … reads it on the next main-actor turn." The coordinator uses `Observations`.
- "Raise the portable package platform to macOS 14." The package moves to macOS 27.
- "Keep the AppKit status menu's own snapshot observer." The menu observer reads stores through `Observations`.

## Consequences

A new feature adds a store in its feature folder and reads it in its views, without changes to `TrackerSession`, `TrackerPresentation` or `publish()`. Cross-feature follow-ups are visible as calls in `App/`.

The app and the portable package need macOS 27, and Linux CI needs Swift 6.4.

During the migration, state lives in two places: moved features in stores, the rest in the session, which forwards to the stores.

`scripts/swift-mutation-scope.py` maps test suites to production files, so each slice that renames or adds a file updates that map. A slice that changes `TestSupport.swift` together with `Contracts/Dependencies.swift` also needs a reviewed `REVIEWED_SUPPORT_ADAPTATIONS` entry, removed once the PR has merged.

The feature rule depends on review, because the compiler doesn't check folder cycles inside a target.
