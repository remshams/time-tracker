import AppKit
import XCTest

@MainActor
class LifecycleUITests: TrackerUITestCase {
    func testControlledScreenNotificationsPauseAndResumeSameTask() throws {
        let task = try fixture.create("Lock notification task")
        launch()
        openSettings()
        element("tracking.pause-on-lock").click()
        showTrackerWindow()
        select(task)
        app.buttons["Start tracking"].click()
        assertRunning(task)
        let original = try XCTUnwrap(fixture.activeWorklog())
        postScreenEvent("lock")
        assertStopped()
        let stopped = try XCTUnwrap(fixture.worklogs(task).first { $0.id == original.id })
        XCTAssertEqual(stopped.start, original.start)
        XCTAssertNotNil(stopped.end)
        postScreenEvent("unlock")
        assertRunning(task)
        XCTAssertNotEqual(try fixture.activeWorklog()?.id, original.id)
    }

    func testControlledScreenNotificationsDoNotPauseWhenPreferenceDisabled() throws {
        let task = try fixture.create("Manual tracking task")
        _ = try fixture.start(task)
        launch()
        let original = try XCTUnwrap(fixture.activeWorklog())
        postScreenEvent("lock")
        postScreenEvent("unlock")
        let deadline = Date().addingTimeInterval(1)
        waitUntil("The lifecycle notification queue drains") { Date() >= deadline }
        XCTAssertEqual(try fixture.activeWorklog(), original)
    }

    func testUnlockDoesNotReplaceTimerStartedByAnotherClient() throws {
        let first = try fixture.create("Paused task")
        let second = try fixture.create("Other client task")
        launch()
        openSettings()
        element("tracking.pause-on-lock").click()
        showTrackerWindow()
        select(first)
        app.buttons["Start tracking"].click()
        assertRunning(first)
        postScreenEvent("lock")
        assertStopped()
        let external = try fixture.start(second)
        postScreenEvent("unlock")
        waitUntil("Unlock reconciles the other client's timer") {
            self.statusButton.label == "Tracking: \(second.name)"
        }
        XCTAssertEqual(try fixture.activeWorklog(), external)
    }

    func testMinimizeHideAndReopenRefreshExternalChangesWithoutStoppingTimer() throws {
        let task = try fixture.create("Visible task")
        _ = try fixture.start(task)
        launch()
        let original = try XCTUnwrap(fixture.activeWorklog())
        trackerWindow.buttons[XCUIIdentifierMinimizeWindow].click()
        let added = try fixture.create("Created while minimized")
        showTrackerWindow()
        XCTAssertTrue(taskRow(added).waitForExistence(timeout: timeout))
        app.typeKey("h", modifierFlags: .command)
        waitUntil("Command H hides the app") { self.app.state == .runningBackground }
        let hidden = try fixture.create("Created while hidden")
        showTrackerWindow()
        XCTAssertTrue(taskRow(hidden).waitForExistence(timeout: timeout))
        XCTAssertEqual(try fixture.activeWorklog(), original)
    }

    func postScreenEvent(_ event: String) {
        DistributedNotificationCenter.default().postNotificationName(
            Notification.Name("\(fixture.defaultsSuite).\(event)"), object: nil,
            userInfo: nil, deliverImmediately: true
        )
    }
}

@MainActor
final class ServerLifecycleUITests: LifecycleUITests {
    override var fixtureSource: TrackerFixture.Source { .server }

    func testHiddenWindowUsesBackgroundPollingAndReopenRefreshes() throws {
        let task = try fixture.create("Polling task")
        _ = try fixture.start(task)
        let proxy = try fixture.enableProxy()
        launch()
        select(task)
        trackerWindow.buttons[XCUIIdentifierCloseWindow].click()
        waitUntil("Closing the window removes the visible UI") { !self.element("tracker.window.content").exists }
        let baseline = try proxy.requests(method: "GET", path: "/v1/snapshot").count
        let deadline = Date().addingTimeInterval(6)
        waitUntil("A visible polling interval passes while the window is closed") { Date() >= deadline }
        XCTAssertEqual(try proxy.requests(method: "GET", path: "/v1/snapshot").count, baseline)
        let added = try fixture.create("Added while the window was closed")
        showTrackerViaShortcut()
        XCTAssertTrue(taskRow(added).waitForExistence(timeout: timeout))
        XCTAssertGreaterThan(try proxy.requests(method: "GET", path: "/v1/snapshot").count, baseline)
    }

    func testControlledSleepSuspendsPollingAndWakeRefreshesWithoutStoppingTimer() throws {
        let task = try fixture.create("Sleep notification task")
        let original = try fixture.start(task)
        let proxy = try fixture.enableProxy()
        launch()
        postScreenEvent("sleep")
        let delivery = Date().addingTimeInterval(1)
        waitUntil("The controlled sleep notification is delivered") { Date() >= delivery }
        let baseline = try proxy.requests(method: "GET", path: "/v1/snapshot").count
        let added = try fixture.create("Created during controlled sleep")
        let deadline = Date().addingTimeInterval(6)
        waitUntil("A visible polling interval passes during controlled sleep") { Date() >= deadline }
        XCTAssertEqual(try proxy.requests(method: "GET", path: "/v1/snapshot").count, baseline)
        postScreenEvent("wake")
        XCTAssertTrue(taskRow(added).waitForExistence(timeout: timeout))
        XCTAssertGreaterThan(try proxy.requests(method: "GET", path: "/v1/snapshot").count, baseline)
        XCTAssertEqual(try fixture.activeWorklog(), original)
    }

    func testDisplayTickUpdatesElapsedWithoutAnHTTPRequest() throws {
        let task = try fixture.create("Display-only tick task")
        _ = try fixture.start(task, at: Date().addingTimeInterval(-60))
        let proxy = try fixture.enableProxy()
        launch()
        select(task)
        let elapsed = element("tracking.elapsed")
        XCTAssertTrue(elapsed.waitForExistence(timeout: timeout))
        try proxy.arm(method: "GET", path: "/v1/snapshot", mode: .holdBefore)
        _ = try proxy.waitForHeldRequest()
        defer { try? proxy.release() }
        let initial = elapsed.label
        let baseline = try proxy.requests().count
        let tick = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            elapsed.exists && elapsed.label != initial
        }, object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [tick], timeout: 2), .completed)
        XCTAssertEqual(try proxy.requests().count, baseline)
    }
}
