import XCTest

class WorklogMoveUITests: TrackerUITestCase {
    func testCompletedMoveFromArchivedHistoryPreservesIdentityTimesAndSelection() throws {
        let source = try fixture.create("Source archive")
        let destination = try fixture.create("Destination development")
        let log = try fixture.completed(source, start: Date().addingTimeInterval(-3600.375), end: Date().addingTimeInterval(-1800.625))
        try fixture.archive(source)
        launch()
        showTab("Archived")
        select(source)
        openMove(log)
        replaceText(app.textFields["worklog-move.search"], "dstdvl")
        XCTAssertTrue(element("worklog-move.candidate.\(destination.id)").waitForExistence(timeout: timeout))
        app.typeKey(.return, modifierFlags: [])
        waitUntil("Move sheet closes") { !self.element("worklog-move.search").exists }
        XCTAssertTrue(element("task-details.task.\(source.id)").exists)
        XCTAssertEqual(taskDetailsNameText, source.name)
        XCTAssertTrue(element("worklog-history.empty").waitForExistence(timeout: timeout))
        XCTAssertTrue(try fixture.worklogs(source).isEmpty)
        let moved = try XCTUnwrap(fixture.worklogs(destination).first)
        XCTAssertEqual(moved.id, log.id)
        XCTAssertEqual(moved.start, log.start)
        XCTAssertEqual(moved.end, log.end)
        XCTAssertEqual(moved.taskID, destination.id)
        relaunch()
        showTab("Active")
        select(destination)
        XCTAssertTrue(element("worklog-history.row.\(log.id)").waitForExistence(timeout: timeout))
    }

    func testRunningMoveTransfersExactTimerToDestinationWithoutChangingSelection() throws {
        let source = try fixture.create("Running source")
        let destination = try fixture.create("Running destination")
        let log = try fixture.start(source, at: Date().addingTimeInterval(-120.375))
        launch()
        select(source)
        openMove(log)
        element("worklog-move.candidate.\(destination.id)").click()
        app.buttons["worklog-move.confirm"].click()
        waitUntil("Running move closes") { !self.element("worklog-move.search").exists }
        let running = try XCTUnwrap(fixture.activeWorklog())
        XCTAssertEqual(running.id, log.id)
        XCTAssertEqual(running.start, log.start)
        XCTAssertNil(running.end)
        XCTAssertEqual(running.taskID, destination.id)
        XCTAssertTrue(element("task-details.task.\(source.id)").exists)
        XCTAssertEqual(taskDetailsNameText, source.name)
        select(destination)
        XCTAssertTrue(app.buttons["Stop tracking"].waitForExistence(timeout: timeout))
        relaunch()
        XCTAssertEqual(try fixture.activeWorklog(), running)
    }

    func testSearchExcludesSourceAndArchivedTasksAndSupportsArrowsAndCancel() throws {
        let source = try fixture.create("Move source")
        let first = try fixture.create("Destination alpha")
        let second = try fixture.create("Destination beta")
        let archived = try fixture.create("Destination archived")
        try fixture.archive(archived)
        let log = try fixture.completed(source, start: Date().addingTimeInterval(-3600), end: Date().addingTimeInterval(-1800))
        launch()
        select(source)
        openMove(log)
        XCTAssertFalse(element("worklog-move.candidate.\(source.id)").exists)
        XCTAssertFalse(element("worklog-move.candidate.\(archived.id)").exists)
        XCTAssertTrue(element("worklog-move.candidate.\(first.id)").waitForExistence(timeout: timeout))
        XCTAssertTrue(element("worklog-move.candidate.\(second.id)").exists)
        app.textFields["worklog-move.search"].typeKey(.downArrow, modifierFlags: [])
        app.textFields["worklog-move.search"].typeKey(.upArrow, modifierFlags: [])
        replaceText(app.textFields["worklog-move.search"], "No matching destination")
        XCTAssertTrue(element("worklog-move.empty").waitForExistence(timeout: timeout))
        XCTAssertFalse(app.buttons["worklog-move.confirm"].isEnabled)
        app.typeKey(.escape, modifierFlags: [])
        waitUntil("Escape cancels move") { !self.element("worklog-move.search").exists }
        XCTAssertEqual(try fixture.worklogs(source), [log])
    }

    func testOverlappingDestinationRejectsMoveWithoutChangingEitherWorklog() throws {
        let source = try fixture.create("Overlap source")
        let destination = try fixture.create("Overlap destination")
        let start = Date().addingTimeInterval(-3600)
        let log = try fixture.completed(source, start: start, end: start.addingTimeInterval(600))
        let existing = try fixture.completed(destination, start: start, end: start.addingTimeInterval(600))
        launch()
        select(source)
        openMove(log)
        XCTAssertTrue(element("worklog-move.candidate.\(destination.id)").waitForExistence(timeout: timeout))
        element("worklog-move.candidate.\(destination.id)").click()
        app.buttons["worklog-move.confirm"].click()
        XCTAssertTrue(element("worklog-move.error").waitForExistence(timeout: timeout))
        XCTAssertEqual(try fixture.worklogs(source), [log])
        XCTAssertEqual(try fixture.worklogs(destination), [existing])
        app.buttons["worklog-move.cancel"].click()
    }

    func testExternallyCorrectedWorklogRequiresReviewAndRetainsDestination() throws {
        let source = try fixture.create("Conflict source")
        let destination = try fixture.create("Conflict destination")
        let start = Date().addingTimeInterval(-3600)
        let end = start.addingTimeInterval(600)
        let log = try fixture.completed(source, start: start, end: end)
        launch()
        select(source)
        openMove(log)
        element("worklog-move.candidate.\(destination.id)").click()
        try fixture.correct(log, start: start.addingTimeInterval(60), end: end)
        app.buttons["worklog-move.confirm"].click()
        XCTAssertTrue(app.buttons["worklog-move.review"].waitForExistence(timeout: timeout))
        XCTAssertFalse(app.buttons["worklog-move.confirm"].isEnabled)
        XCTAssertTrue(try fixture.worklogs(destination).isEmpty)
        app.buttons["worklog-move.review"].click()
        app.buttons["worklog-move.confirm"].click()
        waitUntil("Reviewed move closes") { !self.element("worklog-move.search").exists }
        XCTAssertEqual(try fixture.worklogs(destination).first?.id, log.id)
    }
}

final class ServerWorklogMoveUITests: WorklogMoveUITests {
    override var fixtureSource: TrackerFixture.Source { .server }
}
