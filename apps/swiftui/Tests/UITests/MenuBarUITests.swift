import AppKit
import XCTest

@MainActor
class MenuBarUITests: TrackerUITestCase {
    func testRightClickControlClickAndShortcutOpenMenuAndEscapeDismisses() throws {
        _ = try fixture.create("Planning")
        launch()
        XCTAssertTrue(statusButton.waitForExistence(timeout: timeout))
        statusButton.rightClick()
        XCTAssertTrue(app.menuItems["Open Time Tracker"].waitForExistence(timeout: timeout))
        dismissStatusMenu()
        XCUIElement.perform(withKeyModifiers: .control) { statusButton.click() }
        XCTAssertTrue(app.menuItems["Open Time Tracker"].waitForExistence(timeout: timeout))
        dismissStatusMenu()
        openStatusMenu()
        app.typeKey("t", modifierFlags: [.control, .option])
        waitUntil("The shortcut toggles the menu closed") { !self.app.menuItems["Open Time Tracker"].exists }
        openStatusMenu()
        dismissStatusMenu()
    }

    func testPrimaryClickWithoutLastTaskOpensMenuAndOutsideClickDismisses() throws {
        _ = try fixture.create("Planning")
        launch()
        statusButton.click()
        XCTAssertTrue(app.menuItems["Open Time Tracker"].waitForExistence(timeout: timeout))
        trackerWindow.coordinate(withNormalizedOffset: CGVector(dx: 0.7, dy: 0.6)).click()
        waitUntil("Clicking outside dismisses the menu") { !self.app.menuItems["Open Time Tracker"].exists }
        XCTAssertNil(try fixture.activeWorklog())
    }

    func testTodayRowStartsSwitchesAndStopsWithoutChangingWindowSelection() throws {
        let selected = try fixture.create("Selected task")
        let first = try fixture.create("First tracked task")
        let second = try fixture.create("Second tracked task")
        try completedToday(first)
        try completedToday(second)
        launch()
        select(selected)
        openStatusMenu()
        menuTask(first).click()
        assertRunning(first)
        let initial = try XCTUnwrap(fixture.activeWorklog())
        openStatusMenu()
        menuTask(second).click()
        assertRunning(second)
        let replacement = try XCTUnwrap(fixture.activeWorklog())
        let stopped = try XCTUnwrap(fixture.worklogs(first).first { $0.id == initial.id })
        XCTAssertEqual(stopped.end, replacement.start)
        openStatusMenu()
        menuTask(second).click()
        assertStopped()
        XCTAssertEqual(taskDetailsNameText, selected.name)
    }

    func testStartTrackingSubmenuAndKeyboardReturnStartUnusedTask() throws {
        let task = try fixture.create("Unused task")
        launch()
        openStatusMenu()
        app.typeKey(.downArrow, modifierFlags: [])
        app.typeKey(.rightArrow, modifierFlags: [])
        XCTAssertTrue(menuTask(task).waitForExistence(timeout: timeout))
        app.typeKey(.rightArrow, modifierFlags: [])
        app.typeKey(.return, modifierFlags: [])
        assertRunning(task)
    }

    func testPrimaryClickStopsAndRestartsRememberedTaskAcrossRelaunch() throws {
        let task = try fixture.create("Remembered task")
        launch()
        select(task)
        app.buttons["Start tracking"].click()
        assertRunning(task)
        let first = try XCTUnwrap(fixture.activeWorklog())
        statusButton.click()
        assertStopped()
        relaunch()
        statusButton.click()
        assertRunning(task)
        XCTAssertNotEqual(try fixture.activeWorklog()?.id, first.id)
    }

    func testArchivedTodayTaskHasDisabledTrackingAndAvailableCopyActions() throws {
        let task = try fixture.create("Archived review")
        try completedToday(task)
        try fixture.archive(task)
        launch()
        openStatusMenu()
        let row = menuTask(task)
        XCTAssertTrue(row.waitForExistence(timeout: timeout))
        row.hover()
        XCTAssertFalse(app.menuItems["Start tracking"].firstMatch.isEnabled)
        XCTAssertTrue(app.menuItems["Copy task name"].isEnabled)
        dismissStatusMenu()
        XCTAssertNil(try fixture.activeWorklog())
    }

    func testClipboardCopiesNameExactAndRoundedCompletedTime() throws {
        let task = try fixture.create("Clipboard task")
        try completedToday(task)
        let exact = try exactTodayDuration(task)
        launch()
        withRestoredClipboard {
            for (action, expected) in [("Copy task name", task.name), ("Copy exact duration", exact),
                                       ("Copy rounded duration", "0m")] {
                openStatusMenu()
                copyFromMenu(action, task: task)
                XCTAssertEqual(NSPasteboard.general.string(forType: .string), expected)
            }
        }
    }

    func testUnusedTaskClipboardReportsZeroTime() throws {
        let task = try fixture.create("Zero total")
        launch()
        withRestoredClipboard {
            openStatusMenu()
            app.menuItems["Start tracking"].firstMatch.hover()
            copyFromMenu("Copy exact duration", task: task)
            XCTAssertEqual(NSPasteboard.general.string(forType: .string), "0s")
            openStatusMenu()
            app.menuItems["Start tracking"].firstMatch.hover()
            copyFromMenu("Copy rounded duration", task: task)
            XCTAssertEqual(NSPasteboard.general.string(forType: .string), "0m")
        }
    }

    func testOpenMenuPreservesTaskFramesAndTargetsWhileTimerTicks() throws {
        let first = try fixture.create("Running task")
        let second = try fixture.create("Completed task")
        try completedToday(second)
        _ = try fixture.start(first)
        launch()
        openStatusMenu()
        let firstFrame = menuTask(first).frame
        let secondFrame = menuTask(second).frame
        let initialRunningLabel = menuTask(first).label
        menuTask(first).hover()
        XCTAssertTrue(menuTask(first).menuItems["Stop tracking"].waitForExistence(timeout: timeout))
        let deadline = Date().addingTimeInterval(2)
        waitUntil("A real display tick passes while the submenu remains open") { Date() >= deadline }
        XCTAssertNotEqual(menuTask(first).label, initialRunningLabel)
        XCTAssertEqual(menuTask(first).frame, firstFrame)
        XCTAssertEqual(menuTask(second).frame, secondFrame)
        XCTAssertTrue(menuTask(first).isEnabled)
        menuTask(first).click()
        assertStopped()
    }
}

@MainActor
final class ServerMenuBarUITests: MenuBarUITests {
    override var fixtureSource: TrackerFixture.Source { .server }

    func testCapturedStopCannotStopReplacementTimerAndReopeningShowsNewTask() throws {
        let first = try fixture.create("Captured timer")
        let second = try fixture.create("Replacement timer")
        _ = try fixture.start(first)
        let proxy = try fixture.enableProxy()
        launch()
        try proxy.arm(method: "GET", path: "/v1/reports", mode: .holdBefore)
        openStatusMenu()
        menuTask(first).hover()
        XCTAssertTrue(menuTask(first).menuItems["Stop tracking"].waitForExistence(timeout: timeout))
        _ = try proxy.waitForHeldRequest()
        let replacement = try fixture.start(second)
        let baseline = try proxy.requests(method: "PUT", path: "/v1/tracking").count
        try proxy.release()
        waitUntil("The open menu receives the replacement timer") {
            self.taskRow(second).descendants(matching: .any)
                .matching(identifier: "tracking.active-task").firstMatch.exists
        }
        XCTAssertEqual(statusButton.label, "Tracking: \(first.name)")
        menuTask(first).hover()
        let stop = menuTask(first).menuItems["Stop tracking"]
        waitUntil("The captured Stop action is reachable") { stop.exists && stop.isHittable }
        stop.click()
        XCTAssertEqual(try fixture.activeWorklog(), replacement)
        XCTAssertEqual(try proxy.requests(method: "PUT", path: "/v1/tracking").count, baseline)
        openStatusMenu()
        XCTAssertTrue(menuTask(second).waitForExistence(timeout: timeout))
        XCTAssertTrue(menuTask(second).label.hasSuffix(", tracking"))
        dismissStatusMenu()
    }

    func testOutageDisablesMenuTrackingAndRetainsCachedClipboardTotals() throws {
        let task = try fixture.create("Cached menu task")
        try completedToday(task)
        let exact = try exactTodayDuration(task)
        launch()
        fixture.stopServer()
        waitUntil("The native status item reports unavailable tracking") {
            self.statusButton.label.contains("unavailable")
        }
        withRestoredClipboard {
            openStatusMenu()
            menuTask(task).hover()
            XCTAssertFalse(app.menuItems["Start tracking"].firstMatch.isEnabled)
            XCTAssertTrue(self.app.staticTexts["Today, cached"].exists)
            app.menuItems["Copy exact duration"].click()
            XCTAssertEqual(NSPasteboard.general.string(forType: .string), exact)
        }
        try fixture.restartServer()
        XCTAssertNil(try fixture.activeWorklog())
        XCTAssertEqual(try fixture.worklogs(task).count, 1)
    }

    func testCapturedCopyKeepsItsTaskAfterAnotherClientReplacesTimer() throws {
        let first = try fixture.create("Captured clipboard task")
        let second = try fixture.create("Replacement clipboard task")
        _ = try fixture.start(first, at: Date().addingTimeInterval(-65))
        let proxy = try fixture.enableProxy()
        launch()
        try proxy.arm(method: "GET", path: "/v1/reports", mode: .holdBefore)
        withRestoredClipboard {
            openStatusMenu()
            menuTask(first).hover()
            XCTAssertTrue(menuTask(first).menuItems["Copy task name"].waitForExistence(timeout: timeout))
            do {
                _ = try proxy.waitForHeldRequest()
                _ = try fixture.start(second)
                try proxy.release()
            } catch { XCTFail("Could not replace the timer through the real server: \(error)"); return }
            waitUntil("The open menu reconciles the replacement") {
                self.taskRow(second).descendants(matching: .any)
                    .matching(identifier: "tracking.active-task").firstMatch.exists
            }
            XCTAssertEqual(self.statusButton.label, "Tracking: \(first.name)")
            menuTask(first).hover()
            let copyName = menuTask(first).menuItems["Copy task name"]
            waitUntil("The captured Copy action is reachable") { copyName.exists && copyName.isHittable }
            copyName.click()
            XCTAssertEqual(NSPasteboard.general.string(forType: .string), first.name)
            openStatusMenu()
            menuTask(first).hover()
            let copyDuration = menuTask(first).menuItems["Copy exact duration"]
            waitUntil("The captured duration Copy action is reachable") {
                copyDuration.exists && copyDuration.isHittable
            }
            copyDuration.click()
            do {
                let now = Date()
                let day = try XCTUnwrap(Calendar.current.dateInterval(of: .day, for: now))
                let report = try fixture.reports(start: day.start, end: day.end, at: now)
                let row = try XCTUnwrap(report.rows.first { $0.task.id == first.id })
                let seconds = row.durationMicroseconds / 1_000_000
                let expected = seconds >= 60 ? "\(seconds / 60)m \(seconds % 60)s" : "\(seconds)s"
                XCTAssertEqual(NSPasteboard.general.string(forType: .string), expected)
            } catch { XCTFail("Could not verify the completed timer: \(error)") }
        }
    }
}

@MainActor
final class MenuSourceUITests: TrackerUITestCase {
    func testPrimaryActionRemembersDifferentTasksForLocalAndServerAcrossRelaunch() throws {
        let localTask = try fixture.create("Local remembered task")
        let remote = try TrackerFixture(source: .server)
        defer { try? remote.cleanup() }
        let serverTask = try remote.create("Server remembered task")
        launch()
        select(localTask)
        app.buttons["Start tracking"].click()
        assertRunning(localTask)
        statusButton.click()
        assertStopped()

        connectToServer(try XCTUnwrap(remote.serverURL))
        closeConnectionSettings()
        select(serverTask)
        app.buttons["Start tracking"].click()
        waitUntil("The server starts its selected task") { (try? remote.activeWorklog())?.taskID == serverTask.id }
        statusButton.click()
        waitUntil("The server timer stops") {
            do { return try remote.activeWorklog() == nil } catch { return false }
        }

        openSettings()
        element("connection.mode").click()
        app.menuItems["Local database"].click()
        app.buttons["connection.connect"].click()
        waitUntil("The local data source is adopted") { self.elementText("connection.result").contains("Connected.") }
        closeConnectionSettings()
        XCTAssertTrue(statusButton.label.contains(localTask.name))
        statusButton.click()
        assertRunning(localTask)
        statusButton.click()
        assertStopped()

        connectToServer(try XCTUnwrap(remote.serverURL))
        closeConnectionSettings()
        relaunch()
        XCTAssertTrue(statusButton.label.contains(serverTask.name))
        statusButton.click()
        waitUntil("Relaunch remembers the server's own task") { (try? remote.activeWorklog())?.taskID == serverTask.id }
        XCTAssertNil(try fixture.activeWorklog())
        XCTAssertEqual(try fixture.worklogs(localTask).count, 2)
        XCTAssertEqual(try remote.worklogs(serverTask).count, 2)
    }

}
