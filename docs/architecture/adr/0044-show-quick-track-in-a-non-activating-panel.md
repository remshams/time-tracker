# 0044: show Quick Track in a non-activating panel

## Status

Accepted

Implemented by #427.

## Date

2026-11-22

## Context

Starting or switching a task from another app means opening the status menu with `⌃⌥T`, finding the task and opening a submenu or moving through the list with `j`/`k`. With a few dozen tasks that's slow, and the menu can't filter by name. Quick Track (#427) adds a dialog that opens over the current app with its own global shortcut: type part of a task name, press Return, and the task starts.

The first idea was to reuse the status item's `NSMenu`, pop it up in the middle of the screen and add a search field as a custom item view. Two things rule that out:

- AppKit allows custom views in menu items, but it doesn't document typing into them. Whether a text field in a menu gets keystrokes while the app is in the background depends on undocumented menu tracking.
- `NSMenu` has no public API that moves the highlighted item. Keeping the highlight on the first search match, and moving it with the arrow keys while focus stays in the field, can't be built on it.

The dialog must also not bring Time Tracker or its main window forward, because the user is working in another app.

## Decision

- Quick Track is a borderless `NSPanel` with the `.nonactivatingPanel` style, hosting a SwiftUI view. It overrides `canBecomeKey`, so it becomes the key window and gets the typed keys without activating the app. It floats on the current Space, also over an app in full screen (`.canJoinAllSpaces`, `.fullScreenAuxiliary`). The status menu stays an `NSMenu` and doesn't change.
- The dialog's state and rules live in the portable `TrackerClient` package, in `Features/QuickTrack`. `QuickTrackStore` holds whether the dialog shows, the query, the search result and the highlighted row, and starts or stops tracking through `TrackingStore`. `QuickTrackContent` computes the rows from `QuickTrackInputs`, which `QuickTrackSources` reads from the stores itself. Quick Track shares only three things with the menu bar: the start and stop rules in `TrackingAvailability`, the copy values in `TaskCopying` and the duration text in `DurationLineText`. It doesn't depend on the `MenuBar` folder. The app's `Features/QuickTrack` holds only the panel and the view.
- Task search goes through the client to the application's search rule (ADR 0009), like the move dialog's destinations. `tracker-application` gains `search_active_tasks`, and the bridge exports it as `tt_bridge_search_tasks`, which returns ranked task IDs. A result for an older query is dropped.
- The dialog counts as visible UI for `TrackerSync`, like the window and the open menu, so polling and the display clock run while it shows.
- The panel closes when it loses key status, which covers a click into another app and switching apps.

### Rejected alternatives

- **The status menu in the middle of the screen.** See the context: no documented text input in menu item views and no way to move the highlight.
- **An activating window.** Activating the app brings its other windows forward, which is the problem the dialog avoids.
- **Ranking tasks in Swift.** It would copy the ADR 0009 rule that `tracker-application` owns for every client (ADR 0012).

## Consequences

A new kind of row no longer needs work in both the menu and the dialog, and the menu and the dialog can change independently. The cost is that the presentation helpers are copied: the running task's name and the elapsed, total and task duration text exist in both `TrackerMenuInputs` and `QuickTrackInputs`, so they can drift. The data and the start and stop rules stay in one place. The dialog's keyboard handling, accessibility and native tests are its own. The panel never activates the app, so a tracking error alert from a dialog request shows in the main window the next time it's open, as it does for the status menu.
