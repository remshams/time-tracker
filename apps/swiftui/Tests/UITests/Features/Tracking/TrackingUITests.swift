import XCTest

class TrackingUITests: TrackerUITestCase {
    func testStartSwitchAndStopPreserveIdentityAndAtomicBoundary() throws {
        let first = try fixture.create("Implementation")
        let second = try fixture.create("Review")
        launch()
        select(first)
        app.buttons["Start tracking"].click()
        XCTAssertTrue(app.buttons["Stop tracking"].waitForExistence(timeout: timeout))
        let firstLog = try XCTUnwrap(fixture.activeWorklog())
        XCTAssertEqual(firstLog.taskID, first.id)
        XCTAssertTrue(element("worklog-history.row.\(firstLog.id)").waitForExistence(timeout: timeout))
        startTracking(fromContextMenuOf: second)
        assertRunning(second)
        let secondLog = try XCTUnwrap(fixture.activeWorklog())
        XCTAssertNotEqual(secondLog.id, firstLog.id)
        XCTAssertEqual(secondLog.taskID, second.id)
        let completedFirst = try XCTUnwrap(fixture.worklogs(first).first)
        XCTAssertEqual(completedFirst.id, firstLog.id)
        XCTAssertEqual(completedFirst.start, firstLog.start)
        XCTAssertEqual(completedFirst.end, secondLog.start)
        app.buttons["Stop tracking"].click()
        XCTAssertTrue(app.buttons["Start tracking"].waitForExistence(timeout: timeout))
        XCTAssertNil(try fixture.activeWorklog())
        XCTAssertEqual(try fixture.worklogs(second).count, 1)
        XCTAssertNotNil(try fixture.worklogs(second).first?.end)
    }

    func testPillStartsAndStopsTheSelectedTask() throws {
        let task = try fixture.create("Pill task")
        launch()
        select(task)
        let start = app.buttons["Start tracking"]
        XCTAssertTrue(start.waitForExistence(timeout: timeout))
        XCTAssertEqual(start.value as? String, task.name)
        start.click()
        let stop = app.buttons["Stop tracking"]
        XCTAssertTrue(stop.waitForExistence(timeout: timeout))
        XCTAssertTrue((stop.value as? String)?.hasPrefix(task.name) == true)
        assertRunning(task)
        stop.click()
        XCTAssertTrue(app.buttons["Start tracking"].waitForExistence(timeout: timeout))
        assertStopped()
    }

    func testContextMenuSwitchMovesRunningIndicatorToTheNewTask() throws {
        let first = try fixture.create("Running first")
        let second = try fixture.create("Switch target")
        _ = try fixture.start(first)
        launch()
        let indicator = "tracking.active-task"
        func indicatorIn(_ task: FixtureTask) -> XCUIElement {
            taskRow(task).descendants(matching: .any).matching(identifier: indicator).firstMatch
        }
        XCTAssertTrue(indicatorIn(first).waitForExistence(timeout: timeout))
        XCTAssertFalse(indicatorIn(second).exists)
        startTracking(fromContextMenuOf: second)
        assertRunning(second)
        XCTAssertTrue(indicatorIn(second).waitForExistence(timeout: timeout))
        waitUntil("The indicator leaves the first task") { !indicatorIn(first).exists }
    }

    func testRunningTimerRetainsExactWorklogAcrossRelaunch() throws {
        let task = try fixture.create("Persistent tracking")
        let log = try fixture.start(task, at: Date().addingTimeInterval(-90.375))
        launch()
        select(task)
        XCTAssertTrue(app.buttons["Stop tracking"].waitForExistence(timeout: timeout))
        XCTAssertTrue(element("tracking.active-task").exists)
        XCTAssertTrue(element("worklog-history.row.\(log.id)").exists)
        relaunch()
        select(task)
        XCTAssertTrue(app.buttons["Stop tracking"].waitForExistence(timeout: timeout))
        XCTAssertEqual(try fixture.activeWorklog(), log)
        XCTAssertEqual(try fixture.worklogs(task), [log])
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

    func testArchivedTaskOffersNoTrackingAction() throws {
        let task = try fixture.create("Archived task")
        try fixture.archive(task)
        launch()
        showTab("Archived")
        select(task)
        XCTAssertFalse(app.buttons["Start tracking"].exists)
        XCTAssertFalse(app.buttons["Stop tracking"].exists)
        XCTAssertTrue(app.staticTexts["Nothing running"].waitForExistence(timeout: timeout))
        XCTAssertNil(try fixture.activeWorklog())
    }
}

final class ServerTrackingUITests: TrackingUITests {
    override var fixtureSource: TrackerFixture.Source { .server }
}
