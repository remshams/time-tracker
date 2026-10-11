import AppKit
import ApplicationServices
import XCTest

@MainActor
final class WindowRoutingUITests: TrackerUITestCase {
    func testExternalAccessibilityCanFocusBackgroundTrackerWindow() throws {
        XCTAssertTrue(AXIsProcessTrusted(),
                      "The UI test runner needs macOS Accessibility permission to test external window focus.")
        launch()
        let applications = NSRunningApplication.runningApplications(withBundleIdentifier: "com.timetracker.swiftui")
        XCTAssertEqual(applications.count, 1, "The test requires exactly one Time Tracker process.")
        let running = try XCTUnwrap(applications.first)
        let application = AXUIElementCreateApplication(running.processIdentifier)
        assertAXSuccess(AXUIElementSetMessagingTimeout(application, 2), operation: "Set AX messaging timeout")
        let windows = try XCTUnwrap(try axValue(application, attribute: kAXWindowsAttribute) as? [AXUIElement])
        XCTAssertEqual(windows.count, 1, "A fresh launch must expose exactly one tracker AX window.")
        let window = try XCTUnwrap(windows.first)
        XCTAssertEqual(try axValue(window, attribute: kAXRoleAttribute) as? String, kAXWindowRole)
        XCTAssertEqual(try axValue(window, attribute: kAXSubroleAttribute) as? String, kAXStandardWindowSubrole)
        _ = try axValue(window, attribute: kAXTitleAttribute)
        _ = try axValue(window, attribute: kAXPositionAttribute)
        _ = try axValue(window, attribute: kAXSizeAttribute)
        var actions: CFArray?
        assertAXSuccess(AXUIElementCopyActionNames(window, &actions), operation: "Read AX window actions")
        XCTAssertTrue((actions as? [String] ?? []).contains(kAXRaiseAction), "The tracker window must support AXRaise.")
        var mainSettable = DarwinBoolean(false)
        assertAXSuccess(AXUIElementIsAttributeSettable(window, kAXMainAttribute as CFString, &mainSettable),
                        operation: "Check whether AXMain is writable")
        XCTAssertTrue(mainSettable.boolValue, "The tracker window must permit an external client to make it main.")

        XCUIApplication(bundleIdentifier: "com.apple.finder").activate()
        waitUntil("Another app becomes active before the external switch") { !running.isActive }
        assertAXSuccess(AXUIElementPerformAction(window, kAXRaiseAction as CFString), operation: "Raise tracker AX window")
        assertAXSuccess(AXUIElementSetAttributeValue(window, kAXMainAttribute as CFString, kCFBooleanTrue),
                        operation: "Make tracker AX window main")
        assertAXSuccess(AXUIElementSetAttributeValue(application, kAXFrontmostAttribute as CFString, kCFBooleanTrue),
                        operation: "Activate Time Tracker through AX")
        waitUntil("The external AX client focuses the selected existing tracker window") {
            var focused: CFTypeRef?
            let error = AXUIElementCopyAttributeValue(application, kAXFocusedWindowAttribute as CFString, &focused)
            return running.isActive && error == .success && focused.map { CFEqual($0, window) } == true
        }
        XCTAssertEqual(try axValue(window, attribute: kAXMainAttribute) as? Bool, true)
        XCTAssertTrue(CFEqual(try axValue(application, attribute: kAXFocusedWindowAttribute), window))
        let remaining = try XCTUnwrap(try axValue(application, attribute: kAXWindowsAttribute) as? [AXUIElement])
        XCTAssertEqual(remaining.count, 1, "An external switch must focus the existing window without opening another one.")
        XCTAssertTrue(CFEqual(try XCTUnwrap(remaining.first), window))
    }

    private func axValue(_ element: AXUIElement, attribute: String,
                         file: StaticString = #filePath, line: UInt = #line) throws -> CFTypeRef {
        var value: CFTypeRef?
        assertAXSuccess(AXUIElementCopyAttributeValue(element, attribute as CFString, &value),
                        operation: "Read \(attribute)", file: file, line: line)
        return try XCTUnwrap(value, "\(attribute) did not return a value.", file: file, line: line)
    }

    private func assertAXSuccess(_ error: AXError, operation: String,
                                 file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertEqual(error, .success,
                       "\(operation) returned Apple AXError \(error.rawValue), \(error). This is not a Raycast error number.",
                       file: file, line: line)
    }

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
