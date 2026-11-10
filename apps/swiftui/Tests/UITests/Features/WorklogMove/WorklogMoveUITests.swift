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

    func testExternallyCorrectedWorklogShowsConflictOnConfirmAndReopenedMoveSucceeds() throws {
        let source = try fixture.create("Conflict source")
        let destination = try fixture.create("Conflict destination")
        let start = Date().addingTimeInterval(-3600)
        let end = start.addingTimeInterval(600)
        let log = try fixture.completed(source, start: start, end: end)
        launch()
        select(source)
        openMove(log)
        let candidate = element("worklog-move.candidate.\(destination.id)")
        XCTAssertTrue(candidate.waitForExistence(timeout: timeout))
        candidate.click()
        let external = try fixture.correct(log, start: start.addingTimeInterval(60), end: end)
        app.buttons["worklog-move.confirm"].click()
        waitForText("worklog-move.error", containing: "changed")
        XCTAssertTrue(app.buttons["worklog-move.confirm"].isEnabled)
        XCTAssertFalse(app.buttons["worklog-move.review"].exists)
        XCTAssertTrue(try fixture.worklogs(destination).isEmpty)
        XCTAssertEqual(try fixture.worklogs(source), [external])
        app.buttons["worklog-move.cancel"].click()
        waitUntil("Move closes") { !self.element("worklog-move.search").exists }
        waitForHistoryDuration(of: external)
        openMove(external)
        XCTAssertTrue(candidate.waitForExistence(timeout: timeout))
        candidate.click()
        app.buttons["worklog-move.confirm"].click()
        waitUntil("Reopened move closes") { !self.element("worklog-move.search").exists }
        XCTAssertEqual(try fixture.worklogs(destination).first?.id, log.id)
    }
}

final class ServerWorklogMoveUITests: WorklogMoveUITests {
    override var fixtureSource: TrackerFixture.Source { .server }

    func testDelayedSearchKeepsPreviousResultsDisabledUntilTheQueryCompletes() throws {
        let source = try fixture.create("Search source")
        let destination = try fixture.create("Search destination")
        let log = try fixture.completed(source, start: Date().addingTimeInterval(-3600), end: Date().addingTimeInterval(-1800))
        let proxy = try fixture.enableProxy()
        launch()
        select(source)
        openMove(log)
        let candidate = element("worklog-move.candidate.\(destination.id)")
        XCTAssertTrue(candidate.waitForExistence(timeout: timeout))
        let frame = app.sheets.firstMatch.frame
        // A held poll blocks the bridge's serial worker, so the new search waits behind it.
        try proxy.arm(method: "GET", path: "/v1/state/daily-overview", mode: .holdAfter)
        _ = try proxy.waitForHeldRequest()
        replaceText(app.textFields["worklog-move.search"], "No matching destination")
        XCTAssertTrue(element("worklog-move.search-progress").waitForExistence(timeout: timeout))
        XCTAssertTrue(candidate.exists)
        XCTAssertFalse(app.buttons["worklog-move.confirm"].isEnabled)
        XCTAssertEqual(app.sheets.firstMatch.frame.height, frame.height, accuracy: 1)
        try proxy.release()
        XCTAssertTrue(element("worklog-move.empty").waitForExistence(timeout: timeout))
        XCTAssertFalse(candidate.exists)
        XCTAssertFalse(element("worklog-move.search-progress").exists)
        app.buttons["worklog-move.cancel"].click()
        XCTAssertEqual(try fixture.worklogs(source), [log])
    }

    func testHeldHistoryReloadKeepsRowsUntilItRemovesMovedEntry() throws {
        let source = try fixture.create("Source with two entries")
        let destination = try fixture.create("Move destination")
        let now = Date()
        let retained = try fixture.completed(source, start: now.addingTimeInterval(-7200), end: now.addingTimeInterval(-5400))
        let moved = try fixture.completed(source, start: now.addingTimeInterval(-3600), end: now.addingTimeInterval(-1800))
        let proxy = try fixture.enableProxy()
        launch()
        select(source)
        XCTAssertTrue(element("worklog-history.row.\(retained.id)").waitForExistence(timeout: timeout))
        openMove(moved)
        let candidate = element("worklog-move.candidate.\(destination.id)")
        XCTAssertTrue(candidate.waitForExistence(timeout: timeout))
        candidate.click()
        let baseline = try proxy.requests().count
        try proxy.arm(method: "GET", path: "/v1/worklogs?task_id=\(source.id)", mode: .holdAfter)
        app.buttons["worklog-move.confirm"].click()
        let held = try proxy.waitForHeldRequest()
        let requests = try proxy.requests()
        try assertWriteSentWithoutPriorRead(proxy, after: baseline, method: "PATCH", path: "/v1/worklogs/\(moved.id)")
        let write = try XCTUnwrap(requests.firstIndex { $0.method == "PATCH" && $0.path.contains("/v1/worklogs/\(moved.id)") })
        let reload = try XCTUnwrap(requests.firstIndex { $0.id == held.id })
        XCTAssertLessThan(write, reload)
        waitUntil("Committed move sheet closes") { !self.element("worklog-move.search").exists }
        XCTAssertTrue(element("worklog-history.row.\(retained.id)").exists)
        XCTAssertTrue(element("worklog-history.row.\(moved.id)").exists, "Rows change only when the history arrives.")
        XCTAssertFalse(element("worklog-history.empty").exists)
        try proxy.release()
        waitUntil("History reload removes the moved row") { !self.element("worklog-history.row.\(moved.id)").exists }
        XCTAssertTrue(element("worklog-history.row.\(retained.id)").exists)
        XCTAssertFalse(element("worklog-history.empty").exists)
        XCTAssertEqual(try fixture.worklogs(source).map(\.id), [retained.id])
        XCTAssertEqual(try fixture.worklogs(destination).map(\.id), [moved.id])
    }
}
