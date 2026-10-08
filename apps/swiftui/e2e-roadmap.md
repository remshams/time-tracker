# SwiftUI E2E coverage roadmap

Cover every shipped macOS workflow through the native UI and verify writes against the real backend. Start with task management and tracking, then add worklog editing, server recovery, menu actions, and macOS integration.

The roadmap covers shipped application behavior. Baseline reviewed on 2026-10-08 at revision `360430c`.

## Current coverage

The [native UI runner](check-native-ui.sh) builds `tt-cli` and runs `TimeTrackerUITests` with XCUITest against the Debug app. Tests use a temporary SQLite database, an isolated preferences suite, English UI text, and the production Swift-to-Rust bridge. Test execution is serial.

[BulkTaskArchivingUITests.swift](Tests/UITests/BulkTaskArchivingUITests.swift) currently has seven tests. They cover the default preview, period changes and refresh, loading and dialog geometry, invalid input and recovery, an empty preview, cancellation, confirmation, running-worklog preservation, and persistence after relaunch. The fixture also verifies stored task IDs and archive states through the CLI.

The [macOS CI job](../../.github/workflows/ci.yml) already runs this suite and uploads logs and `.xcresult` diagnostics. [Native layout checks](Tests/NativeLayout/PaneLayoutChecks.swift) exercise the shared split layout in fixture windows. Portable `TrackerClient` tests cover client state, and Rust tests cover application rules and the bridge. These checks do not establish native UI coverage for the other workflows. The portable Swift coverage report excludes the native app.

## Coverage rules

- Drive user actions through XCUITest. Use the CLI to seed data, simulate another client, and verify stored results. Never use the CLI to perform the action a UI test claims to cover.
- Check both the rendered result and backend state for writes. Assert IDs, timestamps, archive state, and active-worklog identity where the action promises to preserve them.
- Give each feature a successful journey and applicable cancellation, validation, loading, failure, and recovery cases. Check persistence only for state the app actually saves.
- Run a successful journey for each backend operation in both local and server mode. Run exhaustive input variants locally and add server cases where transport, revisions, or reconciliation change the outcome.
- Keep clock arithmetic, exhaustive validation boundaries, and scheduler permutations in portable Swift and Rust tests. Native tests must still prove that the controls, errors, and results reach the user.
- Use stable accessibility identifiers for repeated task and worklog rows, scoped by their permanent IDs. Prefer native labels for unique controls. Add identifiers when an existing query is ambiguous, without changing interaction behavior.
- Wait for observable UI or backend conditions with bounded timeouts. Use explicit request gates for slow or concurrent operations. Avoid fixed sleeps and unbounded process waits.
- Use English fixtures. Include duplicate names and long names so queries cannot accidentally rely on text uniqueness. Keep seed timestamps away from day and inactivity boundaries unless a test targets that boundary.
- Preserve the existing bulk archive cases while extracting shared support. Capture all windows, the accessibility hierarchy, CLI errors, and server or proxy logs on failure.

## Delivery order

Each milestone should produce a reviewable change with its own tests and acceptance evidence. Milestones 3 and 5 can proceed independently once milestone 2 is complete. Milestone 4 needs milestone 3's write scenarios; milestone 6 needs milestones 4 and 5.

### Milestone 1: share the existing test support

Extract common launch, cleanup, wait, screenshot, and CLI helpers from the archive suite into `Tests/UITests/Support`. Keep feature-specific seeds separate. Extend fixture commands to read history and reports and to create completed worklogs, archived tasks, empty storage, and histories longer than 50 rows.

Keep `-tt-ui-testing`, `TT_UI_TEST_DATABASE_PATH`, and `TT_UI_TEST_DEFAULTS_SUITE` as the isolated launch contract. Relaunch with the same fixture for persistence checks; use a new fixture for each independent test. Bound every child process and clean up preferences, databases, SQLite sidecars, and processes after failures too.

Establish queries for the task heading, history rows and pagination, tracking controls, connection status, editors, and settings. Add a smoke case that opens an empty app and another that selects a seeded task and reads its history.

Acceptance: all seven archive tests still pass, both smoke cases pass, repeated runs leave no fixture data or processes, and failure diagnostics identify the test and its backend.

### Milestone 2: cover tasks, tracking, and history

Add `TaskCatalogUITests`, `TaskCreationUITests`, `TrackingUITests`, and `WorklogHistoryUITests`.

- Browse Active and Archived, remember selection within each tab, show empty states, and keep the selected task after activity changes its order.
- Create from the toolbar, list context menu, File menu, and Command-N. Verify initial focus, Return submission, Escape cancellation, trimmed names, duplicate names with distinct IDs, invalid names, and preservation of an existing timer.
- Start task A, switch to task B, and stop B. Verify exactly one running worklog, the switch's shared stop/start instant, history updates, and disabled or absent tracking actions for archived tasks. Relaunch while running and verify the same worklog survives.
- Read newest-first history, load a second page from a seed of at least 51 worklogs, and verify no duplicate or missing IDs. Select another task and verify it owns the displayed rows. Check the running row and the no-worklogs state.

Acceptance: the local create, track, switch, stop, and read-history journey passes through the real bridge; cancellation leaves storage unchanged; each alternate creation entry point opens exactly one sheet. Request-dependent history races are completed in milestone 4.

### Milestone 3: cover editors and archive actions

Add `TaskRenameUITests`, `TaskArchivingUITests`, `WorklogCorrectionUITests`, and `WorklogMoveUITests`. Extend bulk archive coverage for concurrent changes and editor ownership.

- Rename active, archived, and running tasks. Rename an unselected row from its context menu and verify the clicked task changes. Check Cancel, unchanged and invalid names, confirmed labels after refresh, and preservation of IDs, worklogs, and running start.
- Archive from toolbar and context menu, cancel confirmation, and unarchive immediately. Verify history survives, the current tab stays selected, selection falls back when its task leaves, and the restored task is remembered when switching to Active. Reject archiving the running task.
- Correct completed worklog start and end times and a running worklog's start. Check the duration preview, displayed time zone, minute precision for changed fields, preservation of untouched timestamp fractions, totals and history refresh, and continued tracking. Exercise future times, reversed intervals, overlap, Cancel, Return, and Escape.
- Move completed and running worklogs, including one from archived-task history. Check fuzzy destination search, arrow navigation, Return, Escape, no matches, and exclusion of the source and archived destinations. Verify exact worklog ID and timestamps survive, a running timer moves to the destination, and the selected task stays selected. Exercise overlap and an unavailable destination.
- Change a task or worklog through the fixture while its editor is open. Verify conflict or review UI and retained drafts. Check that one editor owns the interaction and blocks competing editors or connection changes where required.
- Change archive eligibility after preview and confirm the local candidate guard reports failure. Require a fresh preview before resubmission. Check bulk archiving also covers tasks outside the current tab.

Acceptance: each successful edit has a backend assertion for changed and preserved fields; each cancelled or rejected edit proves storage did not change. Confirmed edits survive relaunch. Server-specific conflicts and uncertain writes follow in milestone 4.

### Milestone 4: add server journeys and recovery

Add a managed server fixture using the real `tt serve --bind ADDRESS:PORT --db PATH` process and temporary storage. Extend the runner to build `tt` as well as `tt-cli`. Allocate a loopback endpoint, wait for protocol readiness, and terminate the server during teardown. Seed and verify server state with `tt-cli --server URL`; do concurrent writes through that endpoint so revision checks remain meaningful.

Add a controlled HTTP proxy for delaying selected responses, failing reads, returning a protocol mismatch, and dropping a response after a real server write commits. Forward the Host header accepted by the server. Record request counts and release gates explicitly. Keep the real server and Rust bridge in the path for operation tests.

Add `ConnectionUITests` and `ServerRecoveryUITests`, and run the successful operations from milestones 2 and 3 against this fixture.

- Test connection without adopting it, connect successfully, relaunch to verify saved settings, and switch between distinct local and server datasets. Verify no copied data and no totals or history from the previous source.
- Reject empty, invalid, unreachable, and incompatible endpoints. A failed candidate connection keeps the previous backend and saved settings. An unavailable configured server never exposes local data as a fallback.
- Fail the initial read and verify Unavailable rather than Idle. Fail a later read and verify last-confirmed state, cached totals, disabled writes, Retry, and recovery after restarting the server.
- Hold history for task A, select B, and release A's response. Verify A cannot overwrite B's rows or errors. Delay pagination and invalidate its cursor through another client, then verify refreshed history without duplicates.
- Hold a background read and click a tracking action. Verify its captured target survives selection changes, it runs once after a valid refresh, and a changed timer causes a visible conflict. Replace the running worklog from another client and verify a captured Stop cannot stop its replacement.
- For creation, rename, archive, correction, and move, drop the response after commit. Verify the editor's recovery state and connection blocking, then reconcile without a duplicate write. Check retained intent after closing and reopening editors where supported. Pending intent does not survive quitting.
- Drop the bulk archive response and verify authoritative refresh without automatic resubmission. Exercise remote revision conflicts and require a fresh preview after a failed confirmation.

Acceptance: every backend mutation has a successful server journey and its applicable conflict or uncertain-response case. Storage and proxy request counts prove recovery does not repeat an accepted write. All server and proxy resources are isolated per test.

### Milestone 5: cover the menu, settings, and windows

Add `MenuBarUITests`, `SettingsUITests`, and `WindowRoutingUITests`. First verify how XCUITest exposes the status item, custom menu rows, shortcut recorder, and clipboard on the CI desktop. Add accessibility identifiers if needed; keep mouse coordinates as a documented last resort.

- Open the native menu with right-click, Control-click, and the global shortcut. Navigate with arrows, open and close submenus, invoke commands with Return, and dismiss with Escape and an outside click.
- Start, switch, and stop from Today, Start tracking, and task submenus. Verify archived and stale tracking actions are unavailable and menu actions preserve window selection. Primary-click the status icon to stop and restart the last task; with no valid last task, verify it opens the menu.
- Copy task names, exact durations, and rounded durations. Cover confirmed zero totals, archived tasks with work today, cached totals, and unavailable values. Read the clipboard after invocation and restore the previous clipboard during teardown.
- Keep a submenu open through timer updates and server refreshes. Verify existing values advance while item order, action targets, and keyboard selection stay stable. Replace the timer or data source and verify captured values and actions become unavailable as required. Reopening shows the latest task list and names.
- Toggle the status bar total and automatic screen-lock pause in Settings. Verify immediate application and persistence after relaunch. Record a shortcut, cancel recording, restore the default, and verify the saved binding opens the menu. Use a controlled registration conflict to check the error and retention of the old binding.
- Close all tracker windows and verify the app and status item remain running. Reopen through the menu and Show Time Tracker. Command-N from Settings alone opens a tracker with one creation sheet. Create a second window with Command-Shift-N and verify shared state, preferred-window routing, single-sheet ownership, and close restrictions during presentation.
- Exercise a failed action after its window closes and verify the pending review or retry action appears when the tracker reopens. Quit through the menu and verify app exit and status-item removal without stopping a running worklog.

Acceptance: each shipped menu command and persisted setting has an automated native interaction check. Tests restore shortcuts and clipboard state and run serially. Window tests distinguish shared application state from per-window layout.

### Milestone 6: cover macOS integration and close remaining gaps

Add `DailyTotalsUITests`, `LifecycleUITests`, and `AppearanceUITests`. Keep the existing native layout checks as a separate check of the shared split layout.

- Compare sidebar, menu, and status totals with CLI reports for completed, running, archived, zero-total, and overnight worklogs. Check updates after correction and move, cached and unavailable labels, and clearing on connection changes. Assert exact totals for completed work and bounded values for live timers.
- Minimize, hide, close, and reopen windows with a local server request log. Verify visibility reaches the session, reopening refreshes, and display ticks do not produce extra requests. Keep exact backoff and day-boundary timing in the existing controlled-clock tests.
- On a dedicated Mac, verify real screen lock and unlock with automatic pause enabled and disabled, lock followed by sleep, wake while still locked, and another client starting before unlock. Inspect stored worklogs to prove the locked interval is excluded and a competing timer survives. Also verify sleep and wake with pause disabled and pending-command cancellation during shutdown.
- If hosted CI cannot deliver real lock or sleep events reliably, add a separate native notification-delivery check with controlled events and retain the real event journeys as explicit Mac acceptance checks. A simulated notification is not evidence that macOS delivered the real event. Record the result and environment for each release; keep the gap open until checked.
- Check the full app in Light and Dark appearance, at the 760 by 480 minimum window size and a larger size, with long names, a narrow sidebar, and collapsed and restored sidebars. Verify controls remain reachable, headings and errors remain readable, running state has a text label, and task colors match between sidebar and menu. Add focused geometry assertions and screenshots for failures.
- Keep multiple displays, menu bar auto-hiding, keyboard-layout changes, real day rollover, and DST boundaries on the dedicated-Mac checklist until reliable native automation exists. Preserve automated calendar boundary coverage in portable tests.

Acceptance: every row below has an automated test or a named Mac acceptance check with recorded evidence. Required manual checks remain visible; portable tests and screenshots alone do not count as completed native interaction coverage.

## CI and completion

Use `apps/swiftui/check-native-ui.sh` as the entry point throughout. Keep tests serial because status items, menus, shortcuts, and the clipboard share the desktop. Update the CI step's bulk-archive-only name when other suites land.

Keep a small smoke group for rapid feedback and the full automated suite as the required macOS check. Measure suite runtime before introducing test plans or a separate job. Split build and UI jobs if growth approaches the existing job's 60-minute limit; retain logs and result bundles from every group. Do not make recovery tests optional because they are slow.

For each milestone, run its focused native cases, then the full native suite and existing layout check on a Mac. Run the repository's required Python tests, Rust tests, Rust coverage, and CRAP checks. Run portable Swift tests for client changes and mutation checks at the scope required by [AGENTS.md](../../AGENTS.md). Do not claim native app line coverage from the portable package's 95% threshold.

Track coverage by user-visible behavior, backend, and failure path. Record the test method or Mac acceptance evidence for each matrix row as implementation proceeds. Complete the roadmap when every shipped control and workflow is mapped, every backend write has local and server coverage, failures prove safe recovery, and no required native acceptance check remains unverified. Recheck the matrix when the app adds behavior.

## Coverage matrix

All planned rows are open at the baseline revision. Bulk archiving is partial because server and concurrent-change cases remain open. Proposed suite names describe implementation batches, not files that already exist.

| Index | Workflow | Existing native coverage | Planned coverage | Milestone |
| --- | --- | --- | --- | --- |
| 1 | Isolated launch, cleanup, diagnostics | Archive-specific support | Shared support, empty and seeded smoke cases | 1 |
| 2 | Active and Archived catalog, ordering, selection | Tab and row checks after bulk archive | `TaskCatalogUITests` | 2 |
| 3 | Creation, all entry points, validation and cancellation | None | `TaskCreationUITests` | 2 |
| 4 | Start, switch, stop and running persistence | Running-session preservation during archive | `TrackingUITests` | 2 |
| 5 | History, pagination, empty state and selection | None | `WorklogHistoryUITests`, delayed responses | 2, 4 |
| 6 | Rename and preservation of tracking and history | None | `TaskRenameUITests` | 3 |
| 7 | Individual archive, unarchive and selection fallback | None | `TaskArchivingUITests` | 3 |
| 8 | Bulk archive preview, validation and confirmation | Seven local cases | Extend `BulkTaskArchivingUITests` for concurrent changes and server outcomes | 3, 4 |
| 9 | Completed and running worklog correction | None | `WorklogCorrectionUITests` | 3 |
| 10 | Worklog move and destination search | None | `WorklogMoveUITests` | 3 |
| 11 | Connection testing, switching, rollback and persistence | Isolated local launch only | `ConnectionUITests` | 4 |
| 12 | Initial outage, stale state, protocol mismatch and Retry | None | `ServerRecoveryUITests` | 4 |
| 13 | Concurrent clients, captured commands and uncertain writes | None | Server cases for each affected feature | 4 |
| 14 | Menu opening, keyboard navigation and dismissal | None | `MenuBarUITests` | 5 |
| 15 | Menu tracking, primary clicks and last-task persistence per source | None | `MenuBarUITests`, distinct-source relaunch cases | 5 |
| 16 | Clipboard commands and live menu values | None | `MenuBarUITests` | 5 |
| 17 | Settings toggles, shortcut recording and persistence | None | `SettingsUITests` | 5 |
| 18 | Multiple windows, sheet ownership, reopen and quit | Shared layout fixture only | `WindowRoutingUITests` | 5 |
| 19 | Daily totals, overnight work, cached and unavailable values | None | `DailyTotalsUITests` | 6 |
| 20 | Visibility, sleep, wake, lock, unlock and shutdown | None | `LifecycleUITests` and real-event Mac acceptance checks | 6 |
| 21 | Appearance, resizing, accessibility and platform variations | Split-layout fixture and bulk archive geometry | `AppearanceUITests` and dedicated-Mac checklist | 6 |
