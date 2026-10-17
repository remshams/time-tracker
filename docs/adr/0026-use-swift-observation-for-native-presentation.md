# 0026: use Swift Observation for native presentation

## Status

Proposed

## Date

2026-10-17

## Context

`TrackerSession` has one `onChange` callback for every state change. To keep draft edits, clock ticks, and unchanged server polls from redrawing the task and worklog lists, the native app added `TrackerPresentationObserver`. It kept a previous copy of each feature's presentation, compared it after every change, and called a per-feature callback. Ten app classes, including `TrackerStore`, the presentation coordinator, and eight feature stores, forwarded those callbacks to `objectWillChange`. Three more stores did the same for request controls, the elapsed timer, and per-task totals. Each new feature added observer fields and another forwarding store. No record explained why the app did not use Swift's Observation framework.

Observation needs macOS 14. The app targets macOS 15; the portable `TrackerClient` package declared macOS 13. The Swift 6.1 Linux toolchain that runs the portable tests in CI includes the `Observation` module.

`@Observable` notifies observers on every assignment, including an equal value. Many views also read derived values, such as the formatted elapsed time and each task's formatted daily total, which change at a coarser rate than the clock.

## Decision

- Add `TrackerPresentation` to the portable package. It is an `@Observable` model of the session values that views read, including each feature's presentation, its availability, request controls, and the elapsed text. `TrackerStore` calls `update(from:)` after each session change.
- Assign a presentation property only when its value changes. This is the one comparison Observation cannot express.
- Give each task total its own observable object, so a ticking timer notifies only the running task's label.
- Keep each sheet's last presented content while it closes, because SwiftUI renders the sheet during its dismissal animation.
- Keep `TrackerSession` a plain class with its `onChange` callback. Its internal state changes at many call sites, and derived values still need the change filter above.
- Make `TrackerStore`, `ConnectionDraftStore`, and `TrackerTaskPresentationCoordinator` observable. Views read stores directly, with `@Bindable` where they bind a draft. Feature stores remain as action facades without change publishers. Remove the forwarding stores for request controls, the timer, and per-task totals.
- The coordinator observes presentations with `withObservationTracking`. Observation reports a change before the new value is stored, so the coordinator reads it on the next main-actor turn.
- Raise the portable package platform to macOS 14.
- Keep the AppKit status menu's own snapshot observer. AppKit does not use SwiftUI's dependency tracking.

## Consequences

A new feature adds properties to `TrackerPresentation` and a facade store. It no longer needs observer fields, callbacks, or a forwarding publisher. SwiftUI redraws a view only when a property it read changed, which is finer than the previous per-feature callbacks.

Portable tests use `withObservationTracking` to check that draft edits, clock ticks, and unchanged polls do not notify the task and worklog lists. A test harness checks every presentation property against the session after each update.

The sheet owner is chosen one main-actor turn after a sheet opens, instead of within the same session change. Values a view reads must come from the observable presentation or a store's own observable state. A value read directly from the session would not redraw the view.
