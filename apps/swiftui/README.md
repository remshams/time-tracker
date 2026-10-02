# Native macOS proof of concept

This SwiftUI app reads the same local `tt.db` as the terminal client. It shows active and archived tasks, the running timer, and paged worklog history. The app is read-only. It refreshes database state once a second and also refreshes when macOS activates its window.

The Xcode app target calls a small Rust static library through a bridging header. Its Build Rust bridge phase compiles that library before Xcode links the app. Rust opens the secured default database, seeds a new empty database once, and uses `TrackerApplication` for task and worklog reads. Swift never opens SQLite directly.

## Build and run on a Mac

Use macOS 13 or newer, Xcode 15 or newer, and Rust 1.88 or newer. Open `apps/swiftui/TimeTracker.xcodeproj` in Xcode, select the `TimeTracker` scheme and `My Mac`, then press Run. Xcode builds the Rust library automatically. The build phase finds Rust installed with rustup or mise in their usual locations.

Debug builds use the Mac's active architecture. For a universal Release build, install both Rust targets first:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

Xcode keeps build output in DerivedData. The project signs local builds ad hoc and does not require an Apple Developer account for the current app capabilities.

The task list uses the native sidebar. The segmented control switches between Active and Archived; each tab remembers its selected task. The details pane shows the selected task's worklogs, 50 per page. The macOS menu bar has a clock item with the current task, elapsed time, a command to open the window, and Quit. Closing the window leaves the menu bar item running.

If the build or launch fails, send the error text from Xcode's Report navigator. The Build Rust bridge phase is listed there separately from Swift compilation and linking.
