# 0036: let dialog stores claim a presentation gate

## Status

Accepted. It partially supersedes the clauses of ADR 0029 listed under [Superseded clauses](#superseded-clauses). All other decisions in that record remain accepted.

Implemented by #385.

## Date

2026-11-10

## Context

The macOS tracker window shows one sheet or alert at a time. Seven dialog stores own the sheets, and `TrackingStore` owns the tracking error alert. `TrackerTaskPresentationCoordinator` in the app's `App/` watched all eight stores and set `hasPresentation`, and the toolbar buttons and the New Task command checked `store.canOpen && !presentation.hasPresentation`. The dialog list was written out four times in `App/`, so a new dialog meant editing four or five places.

Most of those checks did nothing. An attached sheet or alert already blocks the window, including its toolbar and context menus, as `OpenDialogUITests` shows. The main menu isn't blocked: without a check, the New Task command (⌘N) would queue a second sheet behind the first.

[ADR 0035](0035-put-user-actions-on-feature-stores.md) rejected a shared hub in `Shared/`, because it would read every feature store and only move the folder cycle. A gate that the stores write to doesn't read any store, so it doesn't have that problem.

## Decision

### A presentation gate in `Shared/`

`Shared/PresentationGate` is a small `@Observable` class. `TrackerFeatures` creates one and passes it to each dialog store and to `TrackingStore`. A dialog store claims it while its sheet shows, and `TrackingStore` claims it as an alert while its error shows. Both release it when the value goes away. The stores do this in a `didSet` on `isPresented` and `error`, so every path that shows or hides a dialog, including a failed unarchive that shows the archive dialog, updates the gate.

The gate depends on no feature, and the features depend on `Shared/` only, so it forms no folder cycle.

### The rule lives in each store's `canOpen`

Each dialog store's `canOpen`, and `TaskArchivingStore`'s `canArchive` and `canUnarchive`, are false while the gate is claimed, and `open()` refuses then. The New Task command uses `.disabled(!creation.canOpen)` and needs no check of its own.

### No checks at call sites

Views and commands read `canOpen` and call `open()`. They don't read the gate, and button actions don't repeat the `canOpen` check, because a disabled button can't run its action and `open()` checks again. Buttons whose store doesn't check the gate, such as Start tracking, rely on the window: an attached sheet blocks them. `OpenDialogUITests` covers the toolbar too.

### The coordinator only reveals the window

`TrackerTaskPresentationCoordinator` observes the gate's `showsSheet` and brings up the tracker window when a sheet opens. A tracking error alone still doesn't open the window. `PresentationSources` and `hasPresentation` are gone.

macOS doesn't close a window that has an attached sheet, so the window no longer sets `windowDismissBehavior`. `OpenDialogUITests` checks that the close button and ⌘W leave the window open while a sheet shows.

### Rejected alternatives

- **Keeping the checks in the views.** The coordinator would keep a list of every dialog, and each new dialog would need edits in several `App/` files.
- **A closure from each store into `App/`.** The stores would ask the app whether a dialog shows. That adds eight closures to `TrackerFeatures` for a rule that needs no orchestration.

### Superseded clauses

This record replaces these clauses:

- ADR 0029, "Decision": "The session accepts several open editor states." The dialog stores refuse to open while another dialog or the tracking error alert shows. A failed unarchive still shows the archive dialog.
- ADR 0029, "Decision", under Keep: "while that window shows a sheet or the alert, the app view disables every action that opens another editor. This is a view rule, not a session guard." The dialog stores enforce the rule through the gate, and the window blocks the rest of its controls.

## Consequences

A new dialog store takes the gate and claims it from `isPresented`. `App/` needs no change.

The gate changes in the same run as the store, so buttons update without waiting for an `Observations` loop.

Unit tests can check that one dialog blocks the others without a running app.
