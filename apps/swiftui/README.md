# Native macOS proof of concept

This SwiftUI app uses the same local `tt.db` as the terminal client. It shows active and archived tasks, the running timer, and paged worklog history. You can start, switch, and stop tracking. It refreshes database state once a second and also refreshes when macOS activates its window.

The Xcode app target calls a small Rust static library through a bridging header. Its Build Rust bridge phase compiles that library before Xcode links the app. Rust opens the secured default database, seeds a new empty database once, and uses `TrackerApplication` for reads and tracking commands. Swift never opens SQLite directly.

## Build and run on a Mac

Use macOS 13 or newer, Xcode 15 or newer, and Rust 1.88 or newer. Open `apps/swiftui/TimeTracker.xcodeproj` in Xcode, select the `TimeTracker` scheme and `My Mac`, then press Run. Xcode builds the Rust library automatically. The build phase finds Rust installed with rustup or mise in their usual locations.

Debug builds use the Mac's active architecture. For a universal Release build, install both Rust targets first:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

Xcode keeps build output in DerivedData. The project signs local builds ad hoc and does not require an Apple Developer account for the current app capabilities.

The task list uses the native sidebar. The segmented control switches between Active and Archived; each tab remembers its selected task. The details pane shows the selected task's worklogs, 50 per page. The macOS menu bar has a clock item with the current task, elapsed time, a command to open the window, and Quit. Closing the window leaves the menu bar item running.

## Tracking

Select an active task and click Start tracking in its details. The button changes to Stop tracking while that task is running. Select another task and click Switch tracking to end the previous worklog and begin the new one at the same instant. Archived tasks cannot start tracking.

The timer and history update after each command. Tracking errors appear in an alert and refresh the displayed state. If another client switched tasks before your Stop click, the app reports the change and leaves that client's new timer running.

To check on a Mac, start a task, confirm its timer and running worklog appear, then select a different task and switch tracking. Confirm the first task's worklog is stopped. Stop the second task and confirm the timer shows Idle and its worklog has an end time. Relaunch the app to confirm the saved history remains.

## Appearance

The app follows macOS light and dark mode. Text, window backgrounds, worklog cards, and borders use system colors. The sidebar keeps macOS's native selection appearance. The task heading stays above the scrolling history and wraps to three lines. Hover over a task name to read its full text.

To check the layout on a Mac, keep the app open and switch between Light and Dark in System Settings > Appearance. Check the window title, timer, selected task, worklog dates, and durations in both modes. Resize the window to its minimum size, scroll a task's history, and confirm the heading remains fully visible. Also check the menu bar item and the Archived tab.

If the build or launch fails, send the error text from Xcode's Report navigator. The Build Rust bridge phase is listed there separately from Swift compilation and linking.
