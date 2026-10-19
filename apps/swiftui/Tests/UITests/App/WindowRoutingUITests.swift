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

    func testOtherWindowDisablesEditorsWhileOwnerWindowShowsASheet() throws {
        let task = try fixture.create("Single sheet task")
        launch()
        openRename(task)
        let renameSheet = app.sheets.containing(.staticText, identifier: "Edit task name").firstMatch
        replaceText(renameSheet.textFields["task-name.input"], "Rename draft")

        app.typeKey("n", modifierFlags: [.command, .shift])
        waitUntil("New Window opens while rename is shown") { self.trackerWindows.count == 2 }
        let freeWindow = try XCTUnwrap(trackerWindows.allElementsBoundByIndex.first { $0.sheets.count == 0 })
        freeWindow.click()
        XCTAssertTrue(freeWindow.buttons[XCUIIdentifierCloseWindow].isEnabled)
        XCTAssertFalse(freeWindow.buttons["New task"].isEnabled)
        XCTAssertFalse(freeWindow.buttons["Edit task name"].isEnabled)
        XCTAssertFalse(freeWindow.buttons["Archive task"].isEnabled)
        XCTAssertFalse(freeWindow.buttons["bulk-archive.open"].isEnabled)
        freeWindow.descendants(matching: .any).matching(identifier: "task-sidebar.task.\(task.id)").firstMatch.rightClick()
        XCTAssertFalse(app.menuItems["New task"].isEnabled)
        XCTAssertFalse(app.menuItems["Edit name"].isEnabled)
        XCTAssertFalse(app.menuItems["Archive task..."].isEnabled)
        app.typeKey(.escape, modifierFlags: [])
        app.typeKey("n", modifierFlags: .command)
        XCTAssertEqual(app.sheets.count, 1)

        freeWindow.buttons["Start tracking"].click()
        waitUntil("Tracking starts while the other window shows rename") {
            (try? self.fixture.activeWorklog())?.taskID == task.id
        }
        XCTAssertEqual(renameSheet.textFields["task-name.input"].value as? String, "Rename draft")

        // The new window covers the owner window, so close it before using the rename sheet.
        app.typeKey("w", modifierFlags: .command)
        waitUntil("The free window closes") { self.trackerWindows.count == 1 }
        renameSheet.buttons["task-name.cancel"].click()
        waitUntil("Rename closes") { self.app.sheets.count == 0 }
        let window = trackerWindows.firstMatch
        waitUntil("Editors become available after the sheet closes") { window.buttons["New task"].isEnabled }
        window.buttons["New task"].click()
        XCTAssertTrue(window.sheets.firstMatch.waitForExistence(timeout: timeout))
        window.sheets.firstMatch.buttons["task-name.cancel"].click()
        waitUntil("Creation closes") { self.app.sheets.count == 0 }
        XCTAssertEqual(try fixture.tasks().map(\.name), [task.name])
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
