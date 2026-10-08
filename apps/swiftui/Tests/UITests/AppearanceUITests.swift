import AppKit
import XCTest

@MainActor
final class AppearanceUITests: TrackerUITestCase {
    func testLightAndDarkAppearanceKeepTaskNamesErrorsAndControlsReachable() throws {
        let task = try fixture.create("Planning with a long task name that wraps across the narrow sidebar")
        for appearance in ["Light", "Dark"] {
            app.launchEnvironment["TT_UI_TEST_APPEARANCE"] = appearance
            launch()
            let resolvedAppearance = element("tracker.window.appearance")
            XCTAssertTrue(resolvedAppearance.waitForExistence(timeout: timeout))
            waitUntil("Window resolves the requested appearance") {
                resolvedAppearance.value as? String == appearance
            }
            XCTAssertEqual(resolvedAppearance.value as? String, appearance)
            select(task)
            XCTAssertTrue(taskRow(task).isHittable)
            XCTAssertTrue(app.buttons["Start tracking"].isHittable)
            XCTAssertTrue(app.buttons["Edit task name"].isHittable)
            let screenshot = XCTAttachment(screenshot: trackerWindow.screenshot())
            screenshot.name = "\(appearance) appearance"
            screenshot.lifetime = .keepAlways
            add(screenshot)
            openSettings()
            let recorder = element("menu.shortcut.open")
            recorder.click()
            app.typeKey("x", modifierFlags: [])
            let error = element("menu.shortcut.error")
            XCTAssertTrue(error.waitForExistence(timeout: timeout))
            XCTAssertTrue(error.isHittable)
            app.terminate()
        }
    }

    func testMinimumAndExpandedWindowLayoutsKeepControlsInsideContent() throws {
        let task = try fixture.create("Layout task")
        launch()
        select(task)
        resizeWindow(to: CGSize(width: 760, height: 480))
        XCTAssertGreaterThanOrEqual(trackerWindow.frame.width, 760)
        XCTAssertLessThanOrEqual(trackerWindow.frame.width, 780)
        XCTAssertGreaterThanOrEqual(trackerWindow.frame.height, 480)
        XCTAssertLessThanOrEqual(trackerWindow.frame.height, 540)
        let minimumHeight = trackerWindow.frame.height
        assertControlsInsideWindow()
        resizeWindow(to: CGSize(width: 1100, height: 720))
        XCTAssertGreaterThan(trackerWindow.frame.width, 900)
        XCTAssertGreaterThan(trackerWindow.frame.height, minimumHeight + 100)
        assertControlsInsideWindow()
    }

    func testSidebarCollapsesAndRestoresAndRunningStatusHasText() throws {
        let task = try fixture.create("Accessible running task")
        _ = try fixture.start(task)
        launch()
        select(task)
        XCTAssertEqual(element("tracking.active-task").label, "Tracking")
        let color = try XCTUnwrap(element("tracking.active-task").value as? String)
        XCTAssertTrue(color.hasPrefix("Task color: "))
        XCTAssertEqual(statusButton.label, "Tracking: \(task.name)")
        openStatusMenu()
        XCTAssertEqual(menuTask(task).value as? String, color)
        dismissStatusMenu()
        let sidebar = trackerWindow.buttons["Hide Sidebar"]
        XCTAssertTrue(sidebar.waitForExistence(timeout: timeout))
        sidebar.click()
        waitUntil("The sidebar collapses") {
            let list = self.element("task-sidebar.list")
            return !list.exists || !list.isHittable
        }
        XCTAssertTrue(app.buttons["Stop tracking"].isHittable)
        let restore = trackerWindow.buttons["Show Sidebar"]
        XCTAssertTrue(restore.waitForExistence(timeout: timeout))
        restore.click()
        XCTAssertTrue(taskRow(task).waitForExistence(timeout: timeout))
        XCTAssertTrue(taskRow(task).isHittable)
    }

    private func resizeWindow(to size: CGSize) {
        DistributedNotificationCenter.default().postNotificationName(
            Notification.Name("\(fixture.defaultsSuite).resize"), object: nil,
            userInfo: ["width": size.width, "height": size.height], deliverImmediately: true
        )
        let width = min(size.width, NSScreen.main?.visibleFrame.width ?? size.width)
        waitUntil("The real window reaches the requested layout width") {
            abs(self.trackerWindow.frame.width - width) <= 2
        }
    }

    private func assertControlsInsideWindow(file: StaticString = #filePath, line: UInt = #line) {
        let frame = trackerWindow.frame
        for control in [app.buttons["Start tracking"], app.buttons["Edit task name"],
                        app.buttons["bulk-archive.open"], element("task-sidebar.tab.Active")] {
            XCTAssertTrue(control.isHittable, file: file, line: line)
            XCTAssertTrue(frame.contains(control.frame), file: file, line: line)
        }
    }
}
