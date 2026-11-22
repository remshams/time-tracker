import AppKit
import XCTest

@MainActor
extension TrackerUITestCase {
    var quickTrackSearch: XCUIElement { element("quick-track.search") }

    func quickTrackRow(_ task: FixtureTask) -> XCUIElement { element("quick-track.task.\(task.id)") }

    /// Presses the default Quick Track shortcut, ⌃⌥Space, and waits for the search field.
    func openQuickTrack() {
        app.typeKey(" ", modifierFlags: [.control, .option])
        XCTAssertTrue(quickTrackSearch.waitForExistence(timeout: timeout), "The Quick Track dialog did not open.")
    }

    func waitForQuickTrackToClose(_ description: String = "The Quick Track dialog closes") {
        waitUntil(description) { !self.quickTrackSearch.exists }
    }

    /// Waits until the dialog's header shows `task` running, which means the app finished its tracking request.
    func waitForQuickTrackHeader(tracking task: FixtureTask) {
        let header = element("quick-track.header")
        waitUntil("The header shows \(task.name) running") { header.label == "Tracking \(task.name)" }
    }

    /// Waits until `task`'s row can start, which means no tracking request is running.
    func waitForStartableQuickTrackRow(_ task: FixtureTask) {
        let row = quickTrackRow(task)
        waitUntil("\(task.name) can start") { row.exists && !row.label.hasSuffix(", unavailable") }
    }
}

@MainActor
class QuickTrackUITests: TrackerUITestCase {
    func testTypingAndReturnStartTheFirstMatchAndClose() throws {
        _ = try fixture.create("Planning")
        let review = try fixture.create("Code review")
        launch()
        openQuickTrack()
        quickTrackSearch.typeText("revi")
        let row = quickTrackRow(review)
        waitUntil("The first match is highlighted") { row.exists && (row.value as? String) == "Highlighted" }
        app.typeKey(.return, modifierFlags: [])
        assertRunning(review)
        waitForQuickTrackToClose()
    }

    func testArrowKeysSwitchTasksAndTheHeaderStopsTracking() throws {
        let running = try fixture.create("Running task")
        let other = try fixture.create("Other task")
        _ = try fixture.start(running)
        launch()
        openQuickTrack()
        XCTAssertFalse(quickTrackRow(running).exists, "The running task shows only in the header.")
        XCTAssertTrue(quickTrackRow(other).waitForExistence(timeout: timeout))
        app.typeKey(.return, modifierFlags: [])
        // Give a wrongly handled Return time to act before checking that nothing changed.
        Thread.sleep(forTimeInterval: 1)
        XCTAssertTrue(quickTrackSearch.exists, "Return does nothing while no row is highlighted.")
        XCTAssertEqual(try fixture.activeWorklog()?.taskID, running.id)
        app.typeKey(.downArrow, modifierFlags: [])
        app.typeKey(.return, modifierFlags: [])
        assertRunning(other)
        waitForQuickTrackToClose()

        openQuickTrack()
        waitForQuickTrackHeader(tracking: other)
        app.typeKey(.upArrow, modifierFlags: [])
        app.typeKey(.return, modifierFlags: [])
        assertStopped()
        waitForQuickTrackToClose()
    }

    func testStopButtonAndRowClickWork() throws {
        let running = try fixture.create("Running task")
        let other = try fixture.create("Other task")
        _ = try fixture.start(running)
        launch()
        openQuickTrack()
        let stop = element("quick-track.stop")
        XCTAssertTrue(stop.waitForExistence(timeout: timeout))
        stop.click()
        assertStopped()
        waitForQuickTrackToClose()

        openQuickTrack()
        let row = quickTrackRow(other)
        waitForStartableQuickTrackRow(other)
        row.click()
        assertRunning(other)
        waitForQuickTrackToClose()
    }

    func testEscapeClearsTheSearchThenCloses() throws {
        let task = try fixture.create("Planning")
        launch()
        openQuickTrack()
        quickTrackSearch.typeText("zzz")
        XCTAssertTrue(element("quick-track.create").waitForExistence(timeout: timeout))
        app.typeKey(.escape, modifierFlags: [])
        waitUntil("Escape clears the search") { (self.quickTrackSearch.value as? String ?? "").isEmpty }
        XCTAssertTrue(quickTrackRow(task).waitForExistence(timeout: timeout))
        app.typeKey(.escape, modifierFlags: [])
        waitForQuickTrackToClose()
        XCTAssertNil(try fixture.activeWorklog())
    }

    func testShortcutTogglesAndReopeningStartsWithAnEmptySearch() throws {
        _ = try fixture.create("Planning")
        launch()
        openQuickTrack()
        quickTrackSearch.typeText("plan")
        app.typeKey(" ", modifierFlags: [.control, .option])
        waitForQuickTrackToClose("The shortcut closes the dialog")
        openQuickTrack()
        XCTAssertEqual(quickTrackSearch.value as? String ?? "", "")
    }

    func testOpensOverAnotherAppWithoutActivatingTimeTracker() throws {
        let task = try fixture.create("Planning")
        launch()
        let finder = XCUIApplication(bundleIdentifier: "com.apple.finder")
        finder.activate()
        waitUntil("Finder is in front") { finder.state == .runningForeground }
        finder.typeKey(" ", modifierFlags: [.control, .option])
        XCTAssertTrue(quickTrackSearch.waitForExistence(timeout: timeout), "The Quick Track dialog did not open.")
        XCTAssertEqual(finder.state, .runningForeground, "Opening Quick Track must not activate Time Tracker.")
        XCTAssertNotEqual(app.state, .runningForeground)
        finder.typeText("plan")
        finder.typeKey(.return, modifierFlags: [])
        assertRunning(task)
        waitForQuickTrackToClose()
        XCTAssertEqual(finder.state, .runningForeground, "Starting a task must leave the other app in front.")
    }

    func testClickingOutsideCloses() throws {
        _ = try fixture.create("Planning")
        launch()
        openQuickTrack()
        trackerWindow.coordinate(withNormalizedOffset: CGVector(dx: 0.05, dy: 0.95)).click()
        waitForQuickTrackToClose("Clicking outside closes the dialog")
        XCTAssertNil(try fixture.activeWorklog())
    }

    func testCopyKeysAndContextMenuCopyTheTasksValues() throws {
        let task = try fixture.create("Planning")
        _ = try completedToday(task, duration: 3_725)
        launch()
        try withRestoredClipboard {
            openQuickTrack()
            app.typeKey(.downArrow, modifierFlags: [])
            app.typeKey("c", modifierFlags: .command)
            waitUntil("⌘C copies the task name") { NSPasteboard.general.string(forType: .string) == task.name }
            app.typeKey("t", modifierFlags: .command)
            waitUntil("⌘T copies the exact duration") { NSPasteboard.general.string(forType: .string) == "1h 2m 5s" }
            XCTAssertTrue(quickTrackSearch.exists, "Copying keeps the dialog open.")
            quickTrackRow(task).rightClick()
            let rounded = element("quick-track.copy.\(task.id).copyRounded")
            XCTAssertTrue(rounded.waitForExistence(timeout: timeout))
            rounded.click()
            waitUntil("The context menu copies the rounded duration") {
                NSPasteboard.general.string(forType: .string) == "1h 15m"
            }
        }
    }

    func testCommandCWithSelectedSearchTextCopiesTheText() throws {
        _ = try fixture.create("Planning")
        launch()
        try withRestoredClipboard {
            openQuickTrack()
            quickTrackSearch.typeText("plan")
            app.typeKey("a", modifierFlags: .command)
            app.typeKey("c", modifierFlags: .command)
            waitUntil("⌘C copies the selected search text") { NSPasteboard.general.string(forType: .string) == "plan" }
        }
    }

    func testCreateAndStartRowCreatesTheTaskAndStartsIt() throws {
        _ = try fixture.create("Planning")
        launch()
        openQuickTrack()
        quickTrackSearch.typeText("Quarterly report")
        let create = element("quick-track.create")
        waitUntil("The create row is highlighted") { create.exists && (create.value as? String) == "Highlighted" }
        app.typeKey(.return, modifierFlags: [])
        waitForQuickTrackToClose()
        waitUntil("The new task exists") { ((try? self.fixture.tasks()) ?? []).contains { $0.name == "Quarterly report" } }
        let created = try XCTUnwrap(try fixture.tasks().first { $0.name == "Quarterly report" })
        assertRunning(created)
    }

    func testRejectedNameKeepsTheDialogOpenWithTheError() throws {
        _ = try fixture.create("Planning")
        launch()
        openQuickTrack()
        let name = String(repeating: "x", count: 257)
        quickTrackSearch.typeText(name)
        XCTAssertTrue(element("quick-track.create").waitForExistence(timeout: timeout))
        app.typeKey(.return, modifierFlags: [])
        let status = element("quick-track.status")
        waitUntil("The dialog shows the rejected name") {
            status.exists && (status.label + (status.value as? String ?? "")).contains("256")
        }
        XCTAssertEqual(quickTrackSearch.value as? String, name)
        XCTAssertEqual(try fixture.tasks().count, 1)
    }
}

final class ServerQuickTrackUITests: QuickTrackUITests {
    override var fixtureSource: TrackerFixture.Source { .server }
}
