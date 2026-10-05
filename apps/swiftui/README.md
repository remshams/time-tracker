# Native macOS proof of concept

This SwiftUI app shows active and archived tasks, the running timer, and paged worklog history. You can create tasks and start, switch, and stop tracking. Choose a local database or a tracker server in Connection settings.

## Build and run on a Mac

Use macOS 13 or newer, Xcode 15 or newer, and Rust 1.88 or newer. Open `apps/swiftui/TimeTracker.xcodeproj` in Xcode, select the `TimeTracker` scheme and `My Mac`, then press Run. Xcode builds the Rust library automatically. The build phase finds Rust installed with rustup or mise in their usual locations.

Debug builds use the Mac's active architecture. For a universal Release build, install both Rust targets first:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

Xcode keeps build output in DerivedData. The project signs local builds ad hoc and does not require an Apple Developer account for the current app capabilities.

## Connect to a server

Open Time Tracker > Settings, or click the gear in the sidebar toolbar. Choose Server and enter the full HTTP or HTTPS origin, including the server's port. Click Test connection to check it, then Connect to use it. The app saves the successful selection for the next launch. A failed connection change keeps the previous data source and saved settings.

For tabit, use its Tailscale IP and the actual port configured for the tracker server, for example `http://<tabit-tailscale-ip>:<tracker-port>`. The existing server validates the HTTP Host header against its listening address. It rejects a DNS name such as `tabit` unless a reverse proxy rewrites Host to an accepted address. This integration does not change that server policy.

The Mac must be able to reach the server. If the server only listens on loopback, expose it through the project's server deployment setup first. The app checks the server protocol version before loading its tasks.

Local mode uses this Mac's secured default `tt.db`, shared with the local terminal client. Server mode uses only the configured server. Switching modes does not copy or merge their data. An unavailable server never causes a fallback to local storage, and the app does not queue offline writes.

## Create a task

In the main window, click the plus button in the sidebar toolbar. You can also right-click the task list and choose New task, or choose File > New task with Command-N. Each opens the same dialog. Enter a name and click Create. The app selects the confirmed task in Active and loads its history. Creating a task leaves any running timer unchanged. Task creation is available in local and server mode.

The sheet keeps the draft in memory. Rust validates the name and creates its permanent UUID before submitting it to the configured backend. SQLite stores that UUID as the task's primary key. Names may repeat; IDs must be unique. Names must contain text, cannot contain control characters, and cannot exceed 256 Unicode scalars.

Create is disabled during submission. Validation and request errors appear in the sheet. If a server write has an uncertain outcome, the app retains the submitted name, ID, and timestamp for Retry. It checks authoritative state before sending another creation request, so a lost response does not create a second task. Connection changes are blocked until this creation is resolved. You can close and reopen the sheet to retry, but quitting loses the pending intent. This is an in-memory recovery flow and does not queue offline writes.

## Edit a task name

Select a task and click the pencil in the toolbar, or right-click a task and choose Edit name. Right-click editing captures that task even when another task is selected. The dialog starts with the current name. Save updates that task in local or server mode, including active and archived tasks. Its ID, archive state, running timer, and worklog timestamps and durations stay unchanged. The new name appears in the sidebar, task details, timer, and menu. An open menu keeps its existing snapshot until it closes.

The dialog captures the task you clicked and keeps its draft through background refreshes. Before saving, the app refreshes authoritative state. If that read shows another client changed the task's name, it asks you to cancel and reopen the dialog to review the change. Server mode also checks the tracker revision when applying the write. Local mode retains the shared SQLite database's existing last-write behavior: a direct database client can still rename between the preflight read and the write.

Validation errors appear inline. Save is disabled during submission. An uncertain response retains the original target, name, and timestamp for Retry, and blocks changing connections until the outcome is resolved. If a fresh snapshot already confirms the requested name, the app completes without sending another rename. Pending recovery is kept in memory and does not survive quitting. Creation and renaming use one dialog at a time.

## Tracking

Select an active task and click the play icon in the toolbar. The icon changes to stop while that task is running. Select another task and click play to end the previous worklog and begin the new one at the same instant. Archived tasks cannot start tracking.

The menu also controls tracking. Click an active task in Today to start it or switch to it. Start tracking lists active tasks with no time logged today. The running task has a filled colored dot and is disabled; Stop tracking ends its current worklog. These actions leave the window's task selection unchanged. Archived entries show their totals and cannot start tracking.

Commands capture the selected task, expected running worklog, and UTC time when you click. The server checks its revision and active worklog before accepting a change. If another client changes tracking first, the app reports the conflict and refreshes. A Stop command never stops a replacement worklog.

While a request is running or state is stale, tracking buttons are disabled. If a write fails, the app refreshes authoritative state before allowing another write because the server may already have committed it. During an outage, the app labels its last confirmed state as unavailable. Before the first successful snapshot, the timer displays Unavailable rather than claiming Idle.

## Menu bar clicks and task colors

Right-click the menu bar dot to open its menu. Control-click also opens it. Left-click stops the current worklog, or starts the last tracked task when idle. If there is no previous task or that task was deleted or archived, left-click opens the menu. Click decisions use live confirmed state and the existing serialized commands. Busy, stale, sleeping, locked, or stopped sessions reject tracking clicks. The displayed dot never grants write permission.

The dot is filled while tracking and outlined while idle. Both use the current or last tracked task's color. An unavailable connection shows an outline and labels the task as last confirmed. Hover shows the running task's name, the last tracked task when idle, or that no task has been tracked yet. Tooltips resolve names from the current catalog, so renaming is reflected there too.

Task colors use a fixed palette selected by a stable hash of the permanent task ID. Sidebar rows and menu entries share the same mapping. Colors remain stable across restarts and devices; different tasks can share a palette color. System colors adapt to Light and Dark appearance. The last tracked task ID is saved on this Mac separately for each server URL and for the local database. It changes only from accepted snapshots. A newer completed worklog reported by another client can update the last task even when its running state was never observed.

The native AppKit status-item controller owns mouse events, tooltip, appearance updates, and menu presentation. It creates one menu snapshot per opening and holds it across the complete popup tracking loop. It reuses the portable session for tracking and does not add a network poll or display timer. Its subscriptions and status item are removed when the app terminates.

## Pause tracking when the screen locks

In Settings > Tracking, enable "Automatically pause tracking when the screen is locked". The preference is off by default and saves immediately for this Mac. It applies to the current data source, including the shared timer when connected to a server.

Locking the screen stops the current worklog at the captured notification time. Unlocking starts a new worklog for the same task after refreshing tracker state, so the locked interval does not count as work. The app resumes only after a confirmed automatic stop and only if tracking is idle and the task is available. Selecting a different task does not change the remembered task.

Disabling the preference, changing the connection, manually changing tracking, or quitting cancels automatic resume. The preference persists across launches; a paused task does not. Failed or uncertain writes require reconciliation, and an idle snapshot alone does not prove that this app paused the task. Errors remain visible instead of claiming that tracking stopped.

The app must remain running to receive lock and unlock events. A locked Mac waking from sleep does not resume until it unlocks. macOS can suspend or lose its network connection before a server write completes; automatic tracking cannot guarantee a successful stop during an outage. The feature uses distributed lock notifications with undocumented names, so notification delivery requires validation on a Mac. It adds requests on transitions and no repeating timer or polling interval.

The server's conditional idle-start guard was also strengthened. Deploy the server update on tabit to use that guard. The HTTP protocol is unchanged.

## Today's totals

Each sidebar task shows its time logged today. The menu's Today section lists tasks with time logged today, including archived tasks, and shows the combined total with seconds. The status bar shows that combined total in hours and minutes beside the tracking icon. A `~` prefix marks cached totals; `-` means the total is unavailable. Totals include the running worklog and only the portion of an overnight worklog that overlaps today.

Today follows the Mac's local calendar and time zone. Calendar boundaries account for daylight saving changes. A missing report shows an unavailable value instead of zero; failed updates label retained totals as cached. Connecting to a different data source clears the previous totals.

Settings > Menu bar > Show today's total next to the icon controls the status bar total. It is enabled by default, applies immediately, and is saved on this Mac. Turn it off to show just the tracking icon. The Today section inside the menu still shows the total. Hiding the status bar total stops its minute display clock.

The Rust application report query aggregates the full day independently of paged worklog history. Its response includes totals and an authoritative tracker snapshot from the same read. Healthy synchronization uses this report instead of a separate snapshot request. Swift advances the matching running worklog's contribution locally between reports. Ordinary display ticks make no network request. One local day-boundary timer clears yesterday's totals and requests today's report, even while tracking is idle.

## Unit tests

The `TrackerClient` package contains Foundation-only client state and XCTest tests. It has no Rust, SwiftUI, AppKit, database, or network dependency. Run it from the repository root on a Mac or Linux machine with Swift 5.9 or newer:

```sh
swift test --package-path apps/swiftui/TrackerClient
```

The tests use an in-memory settings repository, a manually advanced wall and monotonic clock, a manual scheduler, and an async client whose responses the test controls. They cover connection rollback and persistence, command serialization and captured click times, task creation and renaming, write reconciliation, selection and pagination, stale history responses, polling, display ticks, sleep/wake, and shutdown.

You can also open `apps/swiftui/TrackerClient/Package.swift` in Xcode and run its package tests. The app remains a native Xcode project and links the local package. Native UI E2E tests are deferred; package tests do not exercise macOS windows, menus, notification delivery, or the rendered appearance.

## Coverage and mutation testing

Run these commands from the repository root on Linux or macOS. The scripts require Python 3.12 or newer. Coverage uses the selected Swift toolchain's `llvm-cov`, with `xcrun` as a fallback on macOS:

```sh
python3 scripts/swift-coverage.py
```

The command runs the package tests with instrumentation and requires at least 95% executable source line coverage. It exports `coverage.json`, `lcov.info`, `summary.json`, and `html/index.html` under `.build/swift-coverage`. Tests, generated code, and the native app are outside this report. The protocol-only dependency contract has no executable lines; the script checks its compiler parse tree before listing it separately. Missing coverage for an executable source is an error. Use `--minimum-line-coverage` to choose a stricter minimum.

Build the pinned Muter tool once with Swift 6.1 or newer, then run the mutation check:

```sh
python3 scripts/install-swift-muter.py
python3 scripts/swift-mutations.py
```

The installer downloads public source and dependencies, verifies the source archive checksum, and uses the upstream dependency lockfile. It installs under `.build/swift-tools` without changing system tools. Muter is pinned to revision `7f1f2584e0a27fc05c952a5c8cdd52b10cc9513f`. Compatibility patches supply the missing Linux autorelease-pool function, match reparsed syntax by position and text, insert nested mutations before enclosing ones, and group source mappings by path. These fix upstream defects that reported mutations without inserting them. The runner rejects duplicate source basenames because upstream mutation IDs and log names still use basenames.

Muter's side-effect removal already excludes concurrency primitives such as recursive locks and semaphores. The pinned tool patch adds the missing `NSLock` to that list. Lock acquisition is synchronization, and removing it can cause undefined unlocking behavior or produce a race that sequential tests cannot reliably detect. The package's formatter calls keep their lock; formatter output and timer behavior remain covered by unit tests.

Mutation testing copies only the package manifest, source, and tests into a temporary directory. It runs the unmodified tests first, then applies Muter's operators across production Swift sources. Coverage filtering is disabled so uncovered mutation points also run. Baseline and mutant tests use four isolated XCTest workers. Each mutant has a 180-second test timeout to allow the suite's bounded failure waits to finish on slower machines. The runner limits the baseline to 180 seconds and the full Muter process to 1,800 seconds; use `--timeout` to change the latter. Timeout or interruption stops compiler and test descendants before removing temporary files.

Surviving, timed-out, skipped, or failed-to-build mutants fail the command. An unexplained runtime error also fails; accepting a crash as a killed mutant requires a matching test log with evidence that a test started and crashed. A missing, empty, or inconsistent report fails. The mutation score alone does not decide success. Unit fixtures bound asynchronous waits so removed callbacks fail promptly.

Results and logs go to `.build/swift-mutations`, including individual test logs under `test-logs` when Muter fails or times out. A focused diagnostic run accepts package-relative source paths:

```sh
python3 scripts/swift-mutations.py --files Sources/TrackerClient/Features/Connection/ConnectionState.swift
```

Focused runs label their scope and do not replace a full package check. All commands accept `--swift` for a specific toolchain. Coverage also accepts `--llvm-cov`; mutation testing accepts `--muter` for another tool binary. Generated artifacts are ignored by Git.

These checks cover the portable client state. Native UI E2E tests will run on macOS and remain deferred. Rust coverage, mutation testing, and CRAP checks continue to cover the Rust application and bridge separately.

## Architecture

The Xcode target links a Rust static library through a C bridging header. The app's `TrackerStore` publishes changed content on the main actor. `TrackerPresentationObserver` compares content, request controls, task creation, formatted elapsed time, and daily totals separately. The creation sheet, timer labels, daily totals, and request controls have their own observable adapters, so draft edits, clock ticks, and unchanged server polls do not invalidate the task and worklog lists. A serial background queue owns every bridge operation, JSON decode, and handle release. HTTP requests and database work do not block the UI thread.

```mermaid
flowchart TD
    UI[SwiftUI window, settings and menu] --> Store[TrackerStore observable adapter]
    Store --> Session[TrackerClient package session]
    Session --> Features[Connection, task catalog, task creation, task renaming, tracking, daily totals and history state]
    Lifecycle[AppKit notifications] --> Session
    Session --> Port[Injected TrackerClient and ReportClient interfaces]
    Port --> Worker[Serial background connection worker]
    Worker --> ABI[C functions and JSON]
    ABI --> Backend[Rust bridge backend]
    Backend --> Local[TrackerApplication]
    Local --> DB[Local SQLite tt.db]
    Backend --> Remote[RemoteApplication and persistent Tokio runtime]
    Remote --> HTTP[HTTP client]
    HTTP --> Server[Configured tracker-server]
    Server --> ServerDB[Server SQLite tt.db]
```

A candidate connection must return a valid snapshot before replacing the current backend. The session coordinates operations one at a time. Selection and connection generations prevent older history results from replacing the current selection. The Rust remote backend also blocks writes after failures until an explicit snapshot refresh succeeds.

The package organizes state under `Features/Connection`, `Features/TaskCatalog`, `Features/TaskCreation`, `Features/TaskRename`, `Features/Tracking`, `Features/DailyTotals`, and `Features/WorklogHistory`. `App/TrackerSession` coordinates them through injected client, clock, scheduler, and settings interfaces. Shared task name editing rules live under `Features/TaskEditing`. `Features/TaskIndicators` owns the deterministic color and dot presentation, and `Features/MenuBar` owns remembered-task state and immutable menu snapshots. The remembered-task repository stores identifiers without duplicating tasks or worklogs. The macOS app organizes its views by the same capabilities and shares the name form between creation and renaming. Its `Infrastructure` directory owns Rust bridge calls, real timers, preferences, and AppKit lifecycle notifications. Rust remains responsible for domain rules and persistence.

A session moves once from idle to running, then to stopped on shutdown. A running session distinguishes unconfirmed, confirmed, and stale snapshots. Writes require a confirmed current snapshot and an idle operation gate. Failed writes trigger an authoritative read before another write can proceed. Successful connection changes replace feature state and persist settings; failed changes retain the previous source.

History responses carry the selection generation captured at request time. Timer callbacks carry their scheduling generation. Shutdown invalidates both, cancels scheduled timers, and rejects late results. An already running C call can finish on its queue and release its handle there. Sleep cancels timers while allowing an in-flight operation to finish; wake resets the elapsed anchor and refreshes.

Refreshing history for the same task retains its rows and pagination cursor until the replacement page arrives. Selecting another task or connecting to another data source clears that history immediately. Rows stay visible during synchronization without displaying another task's cached rows.

The bridge passes JSON snapshots, daily reports, and history pages across an in-process function call. Its costs are serialization and decoding, with no separate bridge process or IPC. Server response time and network activity need measurement on a Mac before making battery or latency claims.

## Refresh and battery behavior

The app refreshes state every 5 seconds while a window is visible or an app menu is open. With windows hidden, closed, minimized, or fully covered and menus closed, it refreshes every 60 seconds. The intervals begin after the previous request finishes, so requests cannot overlap.

The native menu uses a separate immutable presentation snapshot. The controller holds it through the synchronous [AppKit popup tracking loop](https://developer.apple.com/documentation/appkit/nsmenu/popup(positioning:at:in:)), including nested menus. Elapsed labels, task totals, action availability, and the status bar label stay fixed while the menu is open, and the controller keeps the same native menu items during navigation. Tracking, polling, and the window's timers continue. Closing the menu publishes the latest state; opening it captures fresh values. Commands still validate task eligibility and the captured worklog ID against the live session. The status label compares only its displayed minutes and tracking status, so second ticks do not redraw it. The app owns its adapters without observing session changes at the scene level.

After connection failures, visible polling backs off to 5, 10, 20, 40, then 60 seconds. Background polling stays at least 60 seconds. A protocol mismatch stops automatic retries until you reopen the UI or click Retry. Opening the UI and waking the Mac request an immediate refresh. Sleep pauses scheduled polling; an already running request may finish.

The elapsed timer redraws once a second only while UI is visible and a timer is active. That tick makes no network request. The native daily totals adapter owns a separate one-minute clock while tracking runs and the status bar total is enabled, so the status bar total advances when windows and menus are closed. It reads the locally advancing total and does not enable foreground polling or the session's one-second display timer. The minute clock stops when tracking is idle and allows five seconds of tolerance for macOS to coalesce wakeups. These rules reduce unnecessary work, but battery impact has not been measured.

## Appearance and history

The app follows macOS light and dark mode. Text, window backgrounds, worklog cards, and borders use system colors. The sidebar keeps macOS's native selection appearance. The task heading stays above the scrolling history and wraps to three lines. Hover over a task name to read its full text.

The Active and Archived tabs remember their selections. Worklogs load 50 at a time. The status bar shows today's combined total. Its menu shows the current task, elapsed time, connection status, today's task totals, tracking actions, and commands to open the window or quit. Closing the window leaves the menu bar item running.

## Check on a Mac

1. Build and run in Xcode. Check readable text in Light and Dark appearance and at the minimum window size.
2. Test and connect to the tracker server on tabit. Confirm its tasks and timer appear. Quit and reopen to confirm the connection setting persists.
3. Start a task, switch to another, then stop it. Confirm history and running state from another client. Change tracking in that client before clicking Stop in the Mac app and confirm the new timer is preserved.
4. Try an invalid or unreachable address in Settings. Confirm Connect keeps the existing connection. Stop the configured server, then confirm stale state is labeled and tracking is disabled. Restart it and confirm recovery.
5. Use a slow connection and change the selected task while history loads. Confirm the window stays responsive and old history cannot replace the new selection.
6. Watch server requests with the window visible, then close or minimize it and keep the menu closed. Confirm the refresh interval changes from about 5 to about 60 seconds. Right-click the status dot to open its menu and confirm it refreshes. Sleep and wake the Mac and confirm the elapsed time includes sleep.
7. Switch to Local, then back to Server. Confirm each data source retains its own tasks and history.
8. Enable automatic pause in Settings > Tracking, start a task, lock the Mac for about 30 seconds, then unlock it. Confirm the same task resumes in a new worklog and the locked interval is excluded. Repeat with the app window closed and with lock followed by sleep. Wake while still locked and confirm tracking remains paused. Start a timer from another client before unlocking and confirm the Mac preserves it. Disable the setting and confirm locking leaves tracking unchanged.
9. Keep a running task open for several server polls. Confirm elapsed labels advance while task and worklog rows remain stable. Change tracking from another client and confirm the updated history appears without briefly showing an empty list. Repeat with a slow connection; selecting a different task must clear the old task's history.
10. Check sidebar totals and the combined menu total against all of today's worklogs, including a running worklog and one crossing midnight. Confirm yesterday's time is excluded. Check the status bar matches the combined total in hours and minutes and advances with the window and menu closed. Check the menu includes archived tasks with time today as informational entries. Stop the server and confirm totals are labelled cached and the status bar uses `~`. Change the data source and confirm old totals disappear. Repeat in Light and Dark appearance and with the sidebar narrowed.
11. Start an active task from the Today section, then click a different task to switch tracking. Check Start tracking includes tasks with no time today. Confirm the running entry is disabled and Stop tracking ends the current worklog. Keep a different task selected in the window and confirm menu actions do not change that selection. Repeat while requests are slow or state is stale and confirm commands are rejected, even if an open menu still displays an older enabled entry.

12. In Settings > Menu bar, turn Show today's total next to the icon off and on. Confirm the status bar switches immediately between icon-only and icon with total, the total inside the menu stays available, and the choice survives quitting and reopening the app.

13. With tracking running, leave Start tracking open for at least 15 seconds and move between submenu entries. Confirm the submenu stays open at a stable size across timer ticks and server polls. Menu values remain fixed until closing. Reopen and confirm fresh elapsed time and totals. Repeat with the main window visible and closed, and with the status bar total enabled and disabled. While the menu is open, change tracking from another client, then confirm an old Stop tracking entry cannot stop the new worklog and reopening shows the latest state.

14. Create a task using the toolbar and Command-N in local and server mode. Check field focus, Return to create, Escape to cancel, inline validation errors, and readable Light and Dark appearance. Confirm the returned task is selected and a running timer stays unchanged. Try a repeated name, a name over 256 Unicode scalars, and an unavailable server. During a slow submission, confirm repeated clicks do not submit again. After an uncertain write, restore the server and retry; confirm only one task exists. Close and reopen the sheet before retrying and confirm the submitted name is retained.

15. Use the single plus in the sidebar toolbar and New task in the context menu; confirm each opens one creation dialog. Select an active task, click the toolbar pencil, and save a changed name. Repeat with an archived task, with tracking running, and in local and server mode. Check the sidebar, heading, timer, and reopened menu show the new name, while the running worklog and its elapsed time remain intact. Check Cancel, unchanged names, invalid names, and slow or failed requests. Rename the same task from another client while editing and confirm Save asks you to reopen and review the latest name. After a lost response, retry and confirm a name already accepted by the server completes without another write. Repeat in Light and Dark appearance and at the minimum window size.

16. Check the sidebar toolbar contains one plus and the settings gear, with edit and start/stop icons at the right and elapsed time beside the title. Right-click an unselected task and rename it; confirm the clicked task changes. Check all dots have matching colors in Light and Dark appearance. Left-click the status dot to stop and restart the last task without changing window selection. Right-click and Control-click to open its menu and keep a submenu open across polls. Check hover text while running, stopped, and offline. Quit and reopen while idle, then switch servers; confirm each source remembers its own task. Check no previous task and deleted/archived previous tasks open the menu.

Xcode compilation, native layout, and the lifecycle checks above require a Mac. Foundation package tests can run on Linux. If the build fails, send the error text from Xcode's Report navigator. The Build Rust bridge phase appears separately from Swift compilation and linking.
