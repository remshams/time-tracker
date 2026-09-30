# Native macOS proof of concept

This SwiftUI app reads the same local `tt.db` as the terminal client. It shows active and archived tasks, the running timer, and paged worklog history. The app is read-only. It refreshes database state once a second and also refreshes when macOS activates its window.

The Swift package calls a small Rust static library through C functions. Rust opens the secured default database, seeds a new empty database once, and uses `TrackerApplication` for task and worklog reads. Swift never opens SQLite directly.

## Build and run on a Mac

Use macOS 13 or newer, Xcode with its command-line tools selected, Swift 5.9 or newer, and Rust 1.88 or newer. From the repository root:

```sh
./apps/swiftui/build.sh
open "apps/swiftui/.build/Time Tracker.app"
```

The script builds the Rust library, links the Swift executable, creates a local `.app` bundle, and gives it an ad hoc signature. Build output stays under `apps/swiftui/.build/`.

The task list uses the native sidebar. The segmented control switches between Active and Archived; each tab remembers its selected task. The details pane shows the selected task's worklogs, 50 per page. The macOS menu bar has a clock item with the current task, elapsed time, a command to open the window, and Quit. Closing the window leaves the menu bar item running.

If the build fails, please send the full `build.sh` output. If it builds but the app fails to open, launch `apps/swiftui/.build/Time Tracker.app/Contents/MacOS/TimeTrackerSwiftUI` from Terminal and send its output.
