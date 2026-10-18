import XCTest

@MainActor
final class WindowRoutingUITests: TrackerUITestCase {
    func testClosingLastWindowLeavesAppAndStatusItemAndReopensThroughMenu() throws {
        _ = try fixture.create("Window task")
        launch()
        trackerWindow.buttons[XCUIIdentifierCloseWindow].click()
        waitUntil("The tracker window closes") { self.trackerWindows.count == 0 }
        XCTAssertEqual(app.state, .runningForeground)
        XCTAssertTrue(statusButton.exists)
        openStatusMenu()
        app.menuItems["Open Time Tracker"].click()
        XCTAssertTrue(trackerWindow.waitForExistence(timeout: timeout))
        trackerWindow.buttons[XCUIIdentifierCloseWindow].click()
        showTrackerViaShortcut()
    }

    func testNewTaskFromSettingsOpensOneSheetInTrackerWindow() throws {
        launch()
        openSettings()
        app.typeKey("n", modifierFlags: .command)
        XCTAssertTrue(app.sheets.firstMatch.waitForExistence(timeout: timeout))
        XCTAssertEqual(app.sheets.count, 1)
        XCTAssertTrue(trackerWindow.exists)
        app.typeKey("n", modifierFlags: .command)
        XCTAssertEqual(app.sheets.count, 1)
        app.typeKey(.escape, modifierFlags: [])
        waitUntil("Cancel closes the only task sheet") { self.app.sheets.count == 0 }
        XCTAssertTrue(try fixture.tasks().isEmpty)
    }

    func testAdditionalWindowSharesTrackingStateAndEditorHasOneOwner() throws {
        let task = try fixture.create("Shared window task")
        launch()
        select(task)
        app.typeKey("n", modifierFlags: [.command, .shift])
        waitUntil("New Window creates a second tracker window") {
            self.trackerWindows.count == 2
        }
        let windows = trackerWindows
        let owner = windows.element(boundBy: 1)
        owner.click()
        let row = owner.descendants(matching: .any).matching(identifier: "task-sidebar.task.\(task.id)").firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: timeout))
        row.click()
        owner.buttons["Start tracking"].click()
        assertRunning(task)
        for index in 0..<2 {
            XCTAssertTrue(windows.element(boundBy: index).buttons["Stop tracking"].waitForExistence(timeout: timeout))
        }
        app.typeKey("n", modifierFlags: .command)
        XCTAssertTrue(app.sheets.firstMatch.waitForExistence(timeout: timeout))
        XCTAssertEqual(app.sheets.count, 1)
        let actualOwner = app.windows.containing(.any, identifier: "task-name.input").firstMatch
        XCTAssertTrue(actualOwner.exists)
        XCTAssertFalse(actualOwner.buttons[XCUIIdentifierCloseWindow].isEnabled)
        app.typeKey(.escape, modifierFlags: [])
        XCTAssertEqual(try fixture.tasks().map(\.id), [task.id])
    }

    func testDistinctEditorsUseSeparateWindowsAndLeaveOtherFeaturesAvailable() throws {
        let task = try fixture.create("Concurrent editor task")
        launch()
        openRename(task)
        let renameSheet = app.sheets.containing(.staticText, identifier: "Edit task name").firstMatch
        replaceText(renameSheet.textFields["task-name.input"], "Rename draft")

        app.typeKey("n", modifierFlags: .command)
        let creationSheet = app.sheets.containing(.staticText, identifier: "New task").firstMatch
        XCTAssertTrue(creationSheet.waitForExistence(timeout: timeout))
        waitUntil("Creation opens another tracker window beside rename") {
            self.trackerWindows.count == 2 && self.app.sheets.count == 2
        }
        creationSheet.textFields["task-name.input"].typeText("Creation draft")
        app.typeKey("n", modifierFlags: .command)
        XCTAssertEqual(app.sheets.count, 2)
        XCTAssertEqual(trackerWindows.count, 2)
        for window in trackerWindows.allElementsBoundByIndex {
            XCTAssertEqual(window.sheets.count, 1)
            XCTAssertFalse(window.buttons[XCUIIdentifierCloseWindow].isEnabled)
        }

        app.typeKey("n", modifierFlags: [.command, .shift])
        waitUntil("New Window remains available with two editors open") { self.trackerWindows.count == 3 }
        let freeWindow = try XCTUnwrap(trackerWindows.allElementsBoundByIndex.first { $0.sheets.count == 0 })
        freeWindow.click()
        XCTAssertTrue(freeWindow.buttons[XCUIIdentifierCloseWindow].isEnabled)
        XCTAssertFalse(freeWindow.buttons["New task"].isEnabled)
        XCTAssertFalse(freeWindow.buttons["Edit task name"].isEnabled)
        XCTAssertTrue(freeWindow.buttons["bulk-archive.open"].isEnabled)
        freeWindow.buttons["Start tracking"].click()
        waitUntil("Tracking starts while distinct editors stay open") {
            (try? self.fixture.activeWorklog())?.taskID == task.id
        }
        freeWindow.buttons["bulk-archive.open"].click()
        let bulkSheet = app.sheets.containing(.button, identifier: "bulk-archive.cancel").firstMatch
        XCTAssertTrue(bulkSheet.waitForExistence(timeout: timeout))
        XCTAssertEqual(app.sheets.count, 3)
        XCTAssertEqual(trackerWindows.count, 3)
        XCTAssertEqual(renameSheet.textFields["task-name.input"].value as? String, "Rename draft")
        XCTAssertEqual(creationSheet.textFields["task-name.input"].value as? String, "Creation draft")

        bulkSheet.buttons["bulk-archive.cancel"].click()
        creationSheet.buttons["task-name.cancel"].click()
        XCTAssertTrue(renameSheet.exists)
        XCTAssertEqual(renameSheet.textFields["task-name.input"].value as? String, "Rename draft")
        renameSheet.buttons["task-name.cancel"].click()
        waitUntil("Each editor closes independently") { self.app.sheets.count == 0 }
        XCTAssertEqual(try fixture.tasks().map(\.name), [task.name])
    }

    func testAnotherEditorReusesAnExistingFreeTrackerWindow() throws {
        let task = try fixture.create("Free window task")
        launch()
        select(task)
        app.typeKey("n", modifierFlags: [.command, .shift])
        waitUntil("A second tracker window opens") { self.trackerWindows.count == 2 }
        let window = trackerWindows.element(boundBy: 0)
        window.click()
        window.buttons["Edit task name"].click()
        XCTAssertTrue(app.sheets.firstMatch.waitForExistence(timeout: timeout))
        app.typeKey("n", modifierFlags: .command)
        waitUntil("Creation uses the free window while rename keeps its owner") { self.app.sheets.count == 2 }
        XCTAssertEqual(trackerWindows.count, 2)
        for window in trackerWindows.allElementsBoundByIndex {
            XCTAssertEqual(window.sheets.count, 1)
        }
    }

    func testQuitLeavesExactRunningWorklogAndRelaunchRestoresIt() throws {
        let task = try fixture.create("Persistent running task")
        launch()
        select(task)
        app.buttons["Start tracking"].click()
        assertRunning(task)
        let worklog = try XCTUnwrap(fixture.activeWorklog())
        app.typeKey("q", modifierFlags: .command)
        waitUntil("Command Q terminates the app") { self.app.state == .notRunning }
        XCTAssertEqual(try fixture.activeWorklog(), worklog)
        launch()
        select(task)
        XCTAssertTrue(app.buttons["Stop tracking"].waitForExistence(timeout: timeout))
        XCTAssertEqual(try fixture.activeWorklog(), worklog)
    }
}

@MainActor
final class ServerWindowRoutingUITests: TrackerUITestCase {
    override var fixtureSource: TrackerFixture.Source { .server }

    func testInFlightRenameKeepsOtherWindowsAndEditorsUsable() throws {
        let task = try fixture.create("Pending rename task")
        let other = try fixture.create("Independent tracking task")
        let proxy = try fixture.enableProxy()
        launch()
        openRename(task)
        let renameSheet = app.sheets.containing(.staticText, identifier: "Edit task name").firstMatch
        replaceText(renameSheet.textFields["task-name.input"], "Confirmed rename")
        try proxy.arm(method: "PATCH", path: "/v1/tasks/\(task.id)", mode: .holdBefore)
        defer { try? proxy.release() }
        renameSheet.buttons["task-name.submit"].click()
        _ = try proxy.waitForHeldRequest()
        XCTAssertFalse(renameSheet.textFields["task-name.input"].isEnabled)

        app.typeKey("n", modifierFlags: [.command, .shift])
        waitUntil("A free window opens during the rename request") { self.trackerWindows.count == 2 }
        let freeWindow = try XCTUnwrap(trackerWindows.allElementsBoundByIndex.first { $0.sheets.count == 0 })
        freeWindow.click()
        freeWindow.descendants(matching: .any).matching(identifier: "task-sidebar.task.\(other.id)").firstMatch.click()
        waitUntil("Selection remains usable during the rename request") {
            freeWindow.descendants(matching: .any).matching(identifier: "task-details.task.\(other.id)").firstMatch.exists
        }
        XCTAssertTrue(freeWindow.buttons["Start tracking"].isEnabled)
        XCTAssertTrue(freeWindow.buttons["New task"].isEnabled)
        XCTAssertTrue(freeWindow.buttons["bulk-archive.open"].isEnabled)
        XCTAssertFalse(freeWindow.buttons["Edit task name"].isEnabled)
        freeWindow.buttons["Start tracking"].click()
        waitUntil("Tracking submission disables its own control while rename is held") {
            !freeWindow.buttons["Start tracking"].isEnabled
        }
        XCTAssertTrue(freeWindow.buttons["New task"].isEnabled)
        XCTAssertTrue(freeWindow.buttons["bulk-archive.open"].isEnabled)
        freeWindow.buttons["New task"].click()
        let creationSheet = app.sheets.containing(.staticText, identifier: "New task").firstMatch
        XCTAssertTrue(creationSheet.waitForExistence(timeout: timeout))
        creationSheet.textFields["task-name.input"].typeText("Independent creation")
        creationSheet.buttons["task-name.submit"].click()
        waitUntil("Creation submission disables only its own editor while rename is held") {
            !creationSheet.textFields["task-name.input"].isEnabled
        }
        XCTAssertEqual(app.sheets.count, 2)
        XCTAssertTrue(renameSheet.exists)

        try proxy.release()
        waitUntil("Queued editor requests complete after rename is released") { self.app.sheets.count == 0 }
        waitUntil("Queued tracking completes after rename is released") {
            (try? self.fixture.activeWorklog())?.taskID == other.id
        }
        XCTAssertEqual(try fixture.tasks().first { $0.id == task.id }?.name, "Confirmed rename")
        XCTAssertEqual(try fixture.tasks().filter { $0.name == "Independent creation" }.count, 1)
        XCTAssertEqual(try fixture.activeWorklog()?.taskID, other.id)
    }
}
