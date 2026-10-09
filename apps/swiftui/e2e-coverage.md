# Native E2E coverage

The suite drives the shipped macOS application through XCUITest and the production Rust bridge. The CLI creates fixtures, acts as another client, and verifies stored results. It does not perform the UI action under test.

## Test layout

Native suites follow the application's `App`, `Features`, and `Infrastructure` folders. Feature-specific UI helpers live beside their suites, including creation, renaming, worklog editing, menu actions, and connection settings. Window helpers live in `App`; lifecycle suites live in `Infrastructure/MacLifecycle`.

`Support` owns the shared test base, database fixtures, process management, proxy controls, and assertions used across features. Local and server variants remain together, and the six CI groups select the same test classes independently of their file paths. Portable `TrackerClientTests` follow that package's `App`, `Contracts`, and `Features` folders, with shared fakes in `Support`.

Implementation checkpoint `72b2154`, 9 October 2026, builds on `360430c`. There are 159 native test executions, including inherited local and server cases. This count includes the seven retained bulk archive scenarios. Native execution and final acceptance evidence are recorded with the implementation PR.

The menu bar display change adds three cases to both `MenuBarUITests` and its server subclass, for six additional executions. They cover immediate Time/Task name/None selection, saved Task name selection across relaunch, start/rename/switch/stop updates, ellipsis, and full native hover tooltips in every display mode. The settings test retains saved None and automatic-pause checks. Native execution evidence for these additions is recorded in [PR 3](https://github.com/remshams/time-tracker/pull/3).

## Run the tests

```sh
apps/swiftui/check-native-ui.sh
apps/swiftui/check-native-ui.sh --smoke
apps/swiftui/check-native-ui.sh --group server
```

The full suite requires macOS, Xcode 27, and a desktop session. The runner builds `tt-cli` and `tt`, supplies an absolute Python 3 path, and disables parallel XCTest execution. CI runs six required groups on separate macOS runners. Every group remains serial because status items, menus, shortcuts, and the clipboard share its desktop. The CI gate requires all groups to pass. The Return correction case temporarily enables system button navigation through the global Core Foundation preference domain and verifies AppKit's mode before launch. It then Tabs to the native Save button and presses Return. Teardown restores and verifies the exact prior key value or absence. Bounded fresh AppKit processes verify the original mode, enabled navigation and restored mode, independently of the long-lived runner's cached getter. Window layout cases resize the real AppKit window through an isolated DEBUG fixture event.

The previous seven-test native UI step took 7 minutes 20 seconds in [CI run 37833088852](https://github.com/remshams/time-tracker/actions/runs/37833088852). The expanded suite is grouped to stay within the existing 60-minute job limit. Logs and result bundles are retained for every group.

| Index | Group | Suites |
| --- | --- | --- |
| 1 | `core` | Smoke, catalog, creation, tracking, history, and their server variants |
| 2 | `tasks` | Rename, individual archive, existing and extended bulk archive, and server variants |
| 3 | `worklogs` | Correction and move, with server variants |
| 4 | `server` | Connection, outages, concurrency, pagination races, and uncertain-write recovery |
| 5 | `menu` | Menu actions, clipboard, per-source remembered tasks, shortcuts, and settings |
| 6 | `mac` | Window routing, daily totals, lifecycle, and appearance |

## Fixtures and failure evidence

Each test has a private temporary directory and UUID preferences suite. The app keeps the isolated launch contract `-tt-ui-testing`, `TT_UI_TEST_DATABASE_PATH`, and `TT_UI_TEST_DEFAULTS_SUITE`. Relaunch uses the same fixture; an independent test gets a new fixture. Cleanup stops server/proxy processes, removes preferences, and deletes the whole directory, including SQLite sidecars.

`TrackerFixture` uses direct CLI commands for local or real server state. `ControlledProxy` forwards UI traffic to that server while fixture writes bypass the proxy. Fault rules match in order, once per rule, using a method and path substring. A new plan releases gates from its previous plan. Gates can hold before a write, hold a committed response, drop a committed response, fail a read, or return incompatible protocol data. Native cases assert backend identity and proxy request counts after recovery.

Child processes, health checks, gates, and UI waits have deadlines. Failure attachments include every app window, the accessibility hierarchy, CLI output, and server/proxy logs. Clipboard contents are restored on normal exit and test teardown. Shortcut changes live in the fixture preferences suite; a temporary Carbon registration exercises a real conflict and is released immediately.

DEBUG lifecycle notifications use the isolated suite name and require the UI-test flag. They reach the native lifecycle observer and real application session. They establish notification delivery and application behavior, not evidence that macOS generated a real screen-lock or sleep event.

## Coverage map

Paths below are relative to this directory. Local feature suites also have real-server subclasses unless the workflow concerns only macOS presentation.

| Index | Workflow | Native evidence |
| --- | --- | --- |
| 1 | Launch, isolation, cleanup, diagnostics | [SmokeUITests](Tests/UITests/App/SmokeUITests.swift) and [shared support](Tests/UITests/Support/TrackerUITestCase.swift) |
| 2 | Active/Archived, ordering, ID selection | [TaskCatalogUITests](Tests/UITests/Features/TaskCatalog/TaskCatalogUITests.swift) |
| 3 | Creation entry points, validation, cancellation, timer preservation | [TaskCreationUITests](Tests/UITests/Features/TaskCreation/TaskCreationUITests.swift) |
| 4 | Start, switch, stop, exact running identity after relaunch | [TrackingUITests](Tests/UITests/Features/Tracking/TrackingUITests.swift) |
| 5 | History order, 50-row pagination, empty/running states, selection and response ownership | [WorklogHistoryUITests](Tests/UITests/Features/WorklogHistory/WorklogHistoryUITests.swift), [ServerRecoveryUITests](Tests/UITests/Features/Connection/ServerRecoveryUITests.swift) |
| 6 | Rename, clicked target, preserved history/timer, conflicts | [TaskRenameUITests](Tests/UITests/Features/TaskRename/TaskRenameUITests.swift) |
| 7 | Archive/unarchive, cancellation, fallback selection, running guard | [TaskArchivingUITests](Tests/UITests/Features/TaskArchiving/TaskArchivingUITests.swift) |
| 8 | Bulk preview, period, geometry, confirmation, concurrent candidates and lost response | [retained cases](Tests/UITests/Features/BulkTaskArchiving/BulkTaskArchivingUITests.swift), [extended cases](Tests/UITests/Features/BulkTaskArchiving/BulkTaskArchivingExtendedUITests.swift) |
| 9 | Completed/running correction, precision, validation, conflicts | [WorklogCorrectionUITests](Tests/UITests/Features/WorklogHistory/WorklogCorrectionUITests.swift) |
| 10 | Completed/running moves, search, archived source, overlap, conflict | [WorklogMoveUITests](Tests/UITests/Features/WorklogHistory/WorklogMoveUITests.swift) |
| 11 | Test/adopt connection, distinct sources, rollback, saved settings | [ConnectionUITests](Tests/UITests/Features/Connection/ConnectionUITests.swift) |
| 12 | Initial/later outage, cached state, disabled writes, Retry, incompatible endpoint | [ConnectionUITests](Tests/UITests/Features/Connection/ConnectionUITests.swift), [ServerRecoveryUITests](Tests/UITests/Features/Connection/ServerRecoveryUITests.swift) |
| 13 | Captured commands, other-client replacement, uncertain writes and retained editor intent | [ServerRecoveryUITests](Tests/UITests/Features/Connection/ServerRecoveryUITests.swift) |
| 14 | Native menu opening, navigation, Return, dismissal, display choices, label updates, ellipsis and hover tooltips | [MenuBarUITests](Tests/UITests/Features/MenuBar/MenuBarUITests.swift) |
| 15 | Menu tracking, primary clicks, remembered task per source | `MenuBarUITests`, `ServerMenuBarUITests`, and `MenuSourceUITests` in the same file |
| 16 | Clipboard, zero/archived/cached values, menu identity during refresh and ticks | [MenuBarUITests](Tests/UITests/Features/MenuBar/MenuBarUITests.swift) |
| 17 | Saved menu bar display and automatic pause, shortcut recording/cancellation/defaults, registration conflict | [SettingsUITests](Tests/UITests/Features/Connection/SettingsUITests.swift) |
| 18 | Windows, editor ownership, reopening, quit with running timer | [WindowRoutingUITests](Tests/UITests/App/WindowRoutingUITests.swift) |
| 19 | Daily totals against CLI reports, overnight/live totals, source clearing | [DailyTotalsUITests](Tests/UITests/Features/DailyTotals/DailyTotalsUITests.swift), [ConnectionUITests](Tests/UITests/Features/Connection/ConnectionUITests.swift) |
| 20 | Visibility, controlled lock/unlock/sleep/wake, display ticks without requests | [LifecycleUITests](Tests/UITests/Infrastructure/MacLifecycle/LifecycleUITests.swift); real-event checks below remain open |
| 21 | Light/Dark identity, size, sidebar, text status, task color identity | [AppearanceUITests](Tests/UITests/App/AppearanceUITests.swift); platform checks below remain open |

## Dedicated-Mac acceptance

Record the macOS/Xcode version, application revision, backend, steps, result, and stored worklog evidence for these checks. Hosted CI and simulated notifications do not close them.

| Index | Check | Evidence status |
| --- | --- | --- |
| 1 | Real lock/unlock with pause enabled and disabled; prove the locked interval is excluded | Open |
| 2 | Lock followed by sleep, wake while locked, and a competing remote timer before unlock | Open |
| 3 | Real sleep/wake with pause disabled and shutdown during a pending command | Open |
| 4 | Multiple displays and menu-bar auto-hiding | Open |
| 5 | Keyboard-layout changes while the saved global shortcut is registered | Open |
| 6 | Real day rollover and DST transition, alongside portable calendar tests | Open |
| 7 | Visual readability and control geometry across supported macOS appearances | Automated geometry and identity checks; visual acceptance open |
| 8 | Drag native window borders to the minimum size and expand them | Open. Hosted pointer drags did not resize the window; CI checks real AppKit window sizing and resulting control geometry through a guarded DEBUG fixture notification. |

While a native popup tracks, its label and actions retain the captured task. Live durations continue updating, and the sidebar reconciles current state. Reopening the popup captures the current task.

Changing the data source through Settings closes an open native menu. Source-switch/reopen journeys therefore cover the reachable native interaction. Captured-source invalidation while a menu snapshot exists remains covered by portable state tests rather than an artificial native session hook.

## Local validation

At checkpoint `a80d4d8`, the Python suite passes 64 tests. Rust workspace tests and coverage pass, and all 1,174 reported functions are below the CRAP threshold of 12. Scoped `tracker-swift-bridge` mutations produce 87 caught and 43 unviable cases, with zero missed or timed-out cases. The bridge crate's unit and integration tests remain enabled.

The change adds native Swift tests, accessibility observations, and native menu shortcut delivery during AppKit tracking. It does not change portable Swift state or Rust production behavior. Portable Swift mutations are therefore unchanged; the Rust bridge check covers the production boundary exercised by the native journeys. Native Swift line coverage is not inferred from the portable package's coverage percentage.
