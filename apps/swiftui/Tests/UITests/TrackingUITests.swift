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
        select(second)
        app.buttons["Start tracking"].click()
        XCTAssertTrue(app.buttons["Stop tracking"].waitForExistence(timeout: timeout))
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

    func testArchivedTaskOffersNoTrackingAction() throws {
        let task = try fixture.create("Archived task")
        try fixture.archive(task)
        launch()
        showTab("Archived")
        select(task)
        XCTAssertFalse(app.buttons["Start tracking"].exists)
        XCTAssertFalse(app.buttons["Stop tracking"].exists)
        XCTAssertNil(try fixture.activeWorklog())
    }
}

final class ServerTrackingUITests: TrackingUITests {
    override var fixtureSource: TrackerFixture.Source { .server }
}
