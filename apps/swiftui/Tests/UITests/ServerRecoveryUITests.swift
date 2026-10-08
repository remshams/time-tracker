import XCTest

final class ServerRecoveryUITests: ConnectionUITestCase {
    override var fixtureSource: TrackerFixture.Source { .server }

    func testInitialOutageShowsUnavailableAndRetryRecoversWithoutLocalFallback() throws {
        let task = try fixture.create("Server outage task")
        fixture.stopServer()
        launch()
        assertConnectionStatus("Unavailable")
        XCTAssertFalse(taskRow(task).exists)
        XCTAssertFalse(app.buttons["New task"].isEnabled)
        try fixture.startServer()
        app.buttons["connection-summary.retry"].click()
        assertConnectionStatus("Connected")
        select(task)
        XCTAssertTrue(app.buttons["Start tracking"].isEnabled)
    }

    func testLaterOutageRetainsConfirmedTasksAndCachedTotalsAndDisablesWrites() throws {
        let task = try fixture.create("Cached server task")
        let now = Date()
        let log = try fixture.completed(task, start: now.addingTimeInterval(-1800), end: now.addingTimeInterval(-600))
        launch()
        select(task)
        XCTAssertTrue(element("worklog-history.row.\(log.id)").waitForExistence(timeout: timeout))
        fixture.stopServer()
        assertConnectionStatus("Unavailable")
        XCTAssertTrue(taskRow(task).exists)
        XCTAssertTrue(app.staticTexts["Today, cached"].exists)
        XCTAssertFalse(app.buttons["New task"].isEnabled)
        XCTAssertFalse(app.buttons["Start tracking"].isEnabled)
        try fixture.startServer()
        app.buttons["connection-summary.retry"].click()
        assertConnectionStatus("Connected")
        XCTAssertEqual(try fixture.worklogs(task), [log])
        XCTAssertTrue(app.buttons["Start tracking"].isEnabled)
    }

    func testDelayedHistoryFromPreviousSelectionCannotReplaceCurrentRows() throws {
        let first = try fixture.create("Delayed history A")
        let second = try fixture.create("Current history B")
        let now = Date()
        let firstLog = try fixture.completed(first, start: now.addingTimeInterval(-3600), end: now.addingTimeInterval(-3000))
        let secondLog = try fixture.completed(second, start: now.addingTimeInterval(-1800), end: now.addingTimeInterval(-1200))
        let proxy = try fixture.enableProxy()
        launch()
        select(second)
        XCTAssertTrue(element("worklog-history.row.\(secondLog.id)").waitForExistence(timeout: timeout))
        try proxy.arm(method: "GET", path: "/v1/tasks/\(first.id)/worklogs", mode: .holdAfter)
        select(first)
        _ = try proxy.waitForHeldRequest()
        select(second)
        try proxy.release()
        XCTAssertTrue(element("worklog-history.row.\(secondLog.id)").waitForExistence(timeout: timeout))
        XCTAssertFalse(element("worklog-history.row.\(firstLog.id)").exists)
        XCTAssertFalse(element("worklog-history.error").exists)
    }

    func testFailedHistoryReadOffersRetryAndRecoversStoredRows() throws {
        let task = try fixture.create("History retry")
        let other = try fixture.create("Other selection")
        let log = try fixture.completed(task, start: Date().addingTimeInterval(-3600), end: Date().addingTimeInterval(-1800))
        let proxy = try fixture.enableProxy()
        launch()
        select(other)
        try proxy.arm(method: "GET", path: "/v1/tasks/\(task.id)/worklogs", mode: .fail)
        select(task)
        XCTAssertTrue(element("worklog-history.error").waitForExistence(timeout: timeout))
        app.buttons["worklog-history.retry"].click()
        XCTAssertTrue(element("worklog-history.row.\(log.id)").waitForExistence(timeout: timeout))
        XCTAssertFalse(element("worklog-history.error").exists)
        XCTAssertEqual(try fixture.worklogs(task), [log])
    }

    func testTrackingActionQueuedBehindRefreshKeepsItsCapturedTaskAndRunsOnce() throws {
        let first = try fixture.create("Captured start target")
        let second = try fixture.create("Later selection")
        let proxy = try fixture.enableProxy()
        launch()
        select(first)
        try proxy.arm(method: "GET", path: "/v1/snapshot", mode: .holdAfter)
        _ = try proxy.waitForHeldRequest()
        app.buttons["Start tracking"].click()
        select(second)
        try proxy.release()
        waitUntil("Captured task starts") { (try? self.fixture.activeWorklog())?.taskID == first.id }
        XCTAssertEqual(try proxy.requests(method: "PUT", path: "/v1/tracking").count, 1)
        XCTAssertEqual(element("task-details.name").value as? String, second.name)
    }

    func testCapturedStopCannotStopAnotherClientsReplacementTimer() throws {
        let first = try fixture.create("Original running task")
        let second = try fixture.create("Replacement task")
        let original = try fixture.start(first)
        let proxy = try fixture.enableProxy()
        launch()
        select(first)
        try proxy.arm(method: "GET", path: "/v1/snapshot", mode: .holdAfter)
        _ = try proxy.waitForHeldRequest()
        app.buttons["Stop tracking"].click()
        let replacement = try fixture.start(second)
        try proxy.release()
        waitUntil("Replacement survives captured Stop") {
            self.element("connection-summary.status").exists && self.app.buttons["Start tracking"].exists
        }
        XCTAssertEqual(try fixture.activeWorklog(), replacement)
        XCTAssertNotEqual(replacement.id, original.id)
        XCTAssertNotNil(try fixture.worklogs(first).first?.end)
    }

    func testLostCreationResponseReconcilesWithoutCreatingAnotherTask() throws {
        let proxy = try fixture.enableProxy()
        launch()
        openCreation()
        app.textFields["task-name.input"].typeText("Committed creation")
        try proxy.arm(method: "POST", path: "/v1/tasks", mode: .dropAfter)
        app.buttons["task-name.submit"].click()
        XCTAssertTrue(element("task-name.error").waitForExistence(timeout: timeout))
        XCTAssertFalse(app.textFields["task-name.input"].isEnabled)
        app.buttons["task-name.submit"].click()
        waitUntil("Accepted creation is reconciled") { !self.element("task-name.input").exists }
        let tasks = try fixture.tasks()
        XCTAssertEqual(tasks.count, 1)
        XCTAssertEqual(tasks.first?.name, "Committed creation")
        XCTAssertEqual(try proxy.requests(method: "POST", path: "/v1/tasks").count, 1)
        select(try XCTUnwrap(tasks.first))
    }

    func testLostRenameResponseReconcilesAndPreservesRunningWorklog() throws {
        let task = try fixture.create("Original remote name")
        let running = try fixture.start(task)
        let proxy = try fixture.enableProxy()
        launch()
        openRename(task)
        replaceText(app.textFields["task-name.input"], "Confirmed remote name")
        try proxy.arm(method: "PATCH", path: "/v1/tasks/\(task.id)", mode: .dropAfter)
        app.buttons["task-name.submit"].click()
        waitUntil("Accepted rename is reconciled") { !self.element("task-name.input").exists }
        XCTAssertEqual(try fixture.tasks().first?.name, "Confirmed remote name")
        XCTAssertEqual(try fixture.activeWorklog(), running)
        XCTAssertEqual(try proxy.requests(method: "PATCH", path: "/v1/tasks/\(task.id)").count, 1)
    }

    func testLostArchiveResponseReconcilesWithoutRepeatingTheMutation() throws {
        let task = try fixture.create("Remote archive target")
        let proxy = try fixture.enableProxy()
        launch()
        select(task)
        app.buttons["Archive task"].click()
        XCTAssertTrue(app.buttons["task-archive.confirm"].waitForExistence(timeout: timeout))
        try proxy.arm(method: "PATCH", path: "/v1/tasks/\(task.id)", mode: .dropAfter)
        app.buttons["task-archive.confirm"].click()
        waitUntil("Accepted archive is reconciled") { !self.app.buttons["task-archive.confirm"].exists }
        XCTAssertTrue(try XCTUnwrap(fixture.tasks().first).archived)
        XCTAssertEqual(try proxy.requests(method: "PATCH", path: "/v1/tasks/\(task.id)").count, 1)
        showTab("Archived")
        select(task)
    }

    func testLostMoveResponsePreservesExactWorklogAndDoesNotRepeatTheWrite() throws {
        let source = try fixture.create("Remote source")
        let destination = try fixture.create("Remote destination")
        let log = try fixture.start(source, at: Date().addingTimeInterval(-90))
        let proxy = try fixture.enableProxy()
        launch()
        select(source)
        openMove(log)
        XCTAssertTrue(element("worklog-move.candidate.\(destination.id)").waitForExistence(timeout: timeout))
        element("worklog-move.candidate.\(destination.id)").click()
        try proxy.arm(method: "PATCH", path: "/v1/worklogs/\(log.id)", mode: .dropAfter)
        app.buttons["worklog-move.confirm"].click()
        waitUntil("Accepted move is reconciled") { !self.element("worklog-move.search").exists }
        let moved = try XCTUnwrap(fixture.activeWorklog())
        XCTAssertEqual(moved.id, log.id)
        XCTAssertEqual(moved.start, log.start)
        XCTAssertEqual(moved.end, log.end)
        XCTAssertEqual(moved.taskID, destination.id)
        XCTAssertEqual(try proxy.requests(method: "PATCH", path: "/v1/worklogs/\(log.id)").count, 1)
    }

    func testUncertainCreationRetainsIntentAcrossEditorReopeningAndBlocksConnectionChanges() throws {
        let proxy = try fixture.enableProxy()
        launch()
        openCreation()
        app.textFields["task-name.input"].typeText("Retained creation intent")
        try proxy.arm([
            .init(method: "POST", path: "/v1/tasks", mode: .dropAfter),
            .init(method: "GET", path: "/v1/snapshot", mode: .fail)
        ])
        app.buttons["task-name.submit"].click()
        XCTAssertTrue(element("task-name.error").waitForExistence(timeout: timeout))
        XCTAssertFalse(app.textFields["task-name.input"].isEnabled)
        let committed = try XCTUnwrap(fixture.tasks().first)
        app.buttons["task-name.cancel"].click()
        openConnectionSettings()
        XCTAssertFalse(app.buttons["connection.connect"].isEnabled)
        closeConnectionSettings()
        openCreation()
        XCTAssertEqual(app.textFields["task-name.input"].value as? String, committed.name)
        XCTAssertFalse(app.textFields["task-name.input"].isEnabled)
        app.buttons["task-name.submit"].click()
        waitUntil("Reopened creation reconciles its original intent") { !self.element("task-name.input").exists }
        XCTAssertEqual(try fixture.tasks().map(\.id), [committed.id])
        XCTAssertEqual(try proxy.requests(method: "POST", path: "/v1/tasks").count, 1)
        openConnectionSettings()
        XCTAssertTrue(app.buttons["connection.connect"].isEnabled)
    }

    func testQuittingDiscardsUncertainIntentButPreservesTheCommittedTask() throws {
        let proxy = try fixture.enableProxy()
        launch()
        openCreation()
        app.textFields["task-name.input"].typeText("Committed before quitting")
        try proxy.arm(method: "POST", path: "/v1/tasks", mode: .dropAfter)
        app.buttons["task-name.submit"].click()
        XCTAssertTrue(element("task-name.error").waitForExistence(timeout: timeout))
        let committed = try XCTUnwrap(fixture.tasks().first)
        app.buttons["task-name.cancel"].click()
        app.typeKey("q", modifierFlags: .command)
        waitUntil("Quit ends the process") { self.app.state == .notRunning }
        launch()
        select(committed)
        openCreation()
        XCTAssertEqual(app.textFields["task-name.input"].value as? String, "")
        XCTAssertTrue(app.textFields["task-name.input"].isEnabled)
        XCTAssertEqual(try fixture.tasks().map(\.id), [committed.id])
        XCTAssertEqual(try proxy.requests(method: "POST", path: "/v1/tasks").count, 1)
    }

    func testLostCorrectionResponseReconcilesWithoutRepeatingTheWrite() throws {
        let task = try fixture.create("Remote correction")
        let log = try fixture.completed(task, start: Date().addingTimeInterval(-3600), end: Date().addingTimeInterval(-1800.375))
        let proxy = try fixture.enableProxy()
        launch()
        select(task)
        openCorrection(log)
        stepMinute("worklog-correction.start")
        try proxy.arm(method: "PATCH", path: "/v1/worklogs/\(log.id)", mode: .dropAfter)
        app.buttons["worklog-correction.save"].click()
        waitUntil("Accepted correction is reconciled") { !self.element("worklog-correction.start").exists }
        let corrected = try XCTUnwrap(fixture.worklogs(task).first)
        XCTAssertEqual(corrected.id, log.id)
        XCTAssertEqual(corrected.end, log.end)
        XCTAssertNotEqual(corrected.start, log.start)
        XCTAssertEqual(try proxy.requests(method: "PATCH", path: "/v1/worklogs/\(log.id)").count, 1)
    }

    func testDelayedPaginationRefreshesAnInvalidatedCursorWithoutLosingNewestHistory() throws {
        let task = try fixture.create("Invalidated pagination")
        let now = Date()
        var logs: [FixtureWorklog] = []
        for index in 0..<51 {
            let start = now.addingTimeInterval(-Double(53 - index) * 600)
            logs.append(try fixture.completed(task, start: start, end: start.addingTimeInterval(300)))
        }
        let proxy = try fixture.enableProxy()
        launch()
        select(task)
        let older = app.buttons["worklog-history.load-older"]
        let scroll = element("worklog-history.scroll")
        for _ in 0..<30 {
            if older.isHittable { break }
            scroll.scroll(byDeltaX: 0, deltaY: -450)
        }
        XCTAssertTrue(older.isHittable)
        try proxy.arm(method: "GET", path: "/v1/tasks/\(task.id)/worklogs?", mode: .holdBefore)
        older.click()
        _ = try proxy.waitForHeldRequest()
        let added = try fixture.completed(task, start: now.addingTimeInterval(-500), end: now.addingTimeInterval(-200))
        try proxy.release()
        let newest = element("worklog-history.row.\(added.id)")
        for _ in 0..<30 {
            if newest.isHittable { break }
            scroll.scroll(byDeltaX: 0, deltaY: 450)
        }
        XCTAssertTrue(newest.waitForExistence(timeout: timeout))
        XCTAssertFalse(element("worklog-history.error").exists)
        let rows = historyRows.allElementsBoundByIndex.map(\.identifier)
        XCTAssertEqual(rows.count, Set(rows).count)
        XCTAssertEqual(Set(try fixture.worklogs(task).map(\.id)), Set(logs.map(\.id) + [added.id]))
    }

    func testBackgroundTimerChangeShowsConflictInsteadOfApplyingCapturedStart() throws {
        let selected = try fixture.create("Captured idle task")
        let competing = try fixture.create("Competing timer")
        let proxy = try fixture.enableProxy()
        launch()
        select(selected)
        try proxy.arm(method: "GET", path: "/v1/snapshot", mode: .holdBefore)
        _ = try proxy.waitForHeldRequest()
        app.buttons["Start tracking"].click()
        let timer = try fixture.start(competing)
        try proxy.release()
        waitUntil("Changed timer is shown as a conflict") {
            self.app.staticTexts.matching(NSPredicate(format: "value CONTAINS[c] 'changed' OR label CONTAINS[c] 'changed'"))
                .firstMatch.exists
        }
        XCTAssertEqual(try fixture.activeWorklog(), timer)
        XCTAssertEqual(try proxy.requests(method: "PUT", path: "/v1/tracking").count, 0)
    }

    func testUncertainRenameBlocksConnectionUntilReopenedIntentIsReconciled() throws {
        let task = try fixture.create("Pending rename original")
        let proxy = try fixture.enableProxy()
        launch()
        openRename(task)
        replaceText(app.textFields["task-name.input"], "Pending rename confirmed")
        try proxy.arm([
            .init(method: "PATCH", path: "/v1/tasks/\(task.id)", mode: .dropAfter),
            .init(method: "GET", path: "/v1/snapshot", mode: .fail)
        ])
        app.buttons["task-name.submit"].click()
        XCTAssertTrue(element("task-name.error").waitForExistence(timeout: timeout))
        app.buttons["task-name.cancel"].click()
        assertPendingConnectionIsBlocked()
        openRename(task)
        XCTAssertEqual(app.textFields["task-name.input"].value as? String, "Pending rename confirmed")
        XCTAssertFalse(app.textFields["task-name.input"].isEnabled)
        app.buttons["task-name.submit"].click()
        waitUntil("Pending rename reconciles") { !self.element("task-name.input").exists }
        XCTAssertEqual(try fixture.tasks().first?.name, "Pending rename confirmed")
        XCTAssertEqual(try proxy.requests(method: "PATCH", path: "/v1/tasks/\(task.id)").count, 1)
    }

    func testUncertainArchiveReopensReviewAndDoesNotRepeatAcceptedWrite() throws {
        let task = try fixture.create("Pending archive")
        let proxy = try fixture.enableProxy()
        launch()
        select(task)
        app.buttons["Archive task"].click()
        XCTAssertTrue(app.buttons["task-archive.confirm"].waitForExistence(timeout: timeout))
        try proxy.arm([
            .init(method: "PATCH", path: "/v1/tasks/\(task.id)", mode: .dropAfter),
            .init(method: "GET", path: "/v1/snapshot", mode: .fail)
        ])
        app.buttons["task-archive.confirm"].click()
        XCTAssertTrue(element("task-archive.error").waitForExistence(timeout: timeout))
        app.buttons["task-archive.cancel"].click()
        assertPendingConnectionIsBlocked()
        let review = app.buttons["Review task action"]
        XCTAssertTrue(review.waitForExistence(timeout: timeout))
        review.click()
        app.buttons["task-archive.confirm"].click()
        waitUntil("Pending archive reconciles") { !self.app.buttons["task-archive.confirm"].exists }
        XCTAssertTrue(try XCTUnwrap(fixture.tasks().first).archived)
        XCTAssertEqual(try proxy.requests(method: "PATCH", path: "/v1/tasks/\(task.id)").count, 1)
    }

    func testUncertainCorrectionReopensOriginalIntentAndPreservesUntouchedEnd() throws {
        let task = try fixture.create("Pending correction")
        let log = try fixture.completed(task, start: Date().addingTimeInterval(-3600), end: Date().addingTimeInterval(-1800.375))
        let proxy = try fixture.enableProxy()
        launch()
        select(task)
        openCorrection(log)
        stepMinute("worklog-correction.start")
        try proxy.arm([
            .init(method: "PATCH", path: "/v1/worklogs/\(log.id)", mode: .dropAfter),
            .init(method: "GET", path: "/v1/snapshot", mode: .fail)
        ])
        app.buttons["worklog-correction.save"].click()
        XCTAssertTrue(element("worklog-correction.error").waitForExistence(timeout: timeout))
        app.buttons["worklog-correction.cancel"].click()
        assertPendingConnectionIsBlocked()
        let reopen = app.buttons["Retry worklog edit"]
        XCTAssertTrue(reopen.waitForExistence(timeout: timeout))
        reopen.click()
        app.buttons["worklog-correction.save"].click()
        waitUntil("Pending correction reconciles") { !self.element("worklog-correction.start").exists }
        let saved = try XCTUnwrap(fixture.worklogs(task).first)
        XCTAssertEqual(saved.id, log.id)
        XCTAssertEqual(saved.end, log.end)
        XCTAssertNotEqual(saved.start, log.start)
        XCTAssertEqual(try proxy.requests(method: "PATCH", path: "/v1/worklogs/\(log.id)").count, 1)
    }

    func testUncertainMoveReopensDestinationIntentWithoutRepeatingTheWrite() throws {
        let source = try fixture.create("Pending move source")
        let destination = try fixture.create("Pending move destination")
        let log = try fixture.start(source)
        let proxy = try fixture.enableProxy()
        launch()
        select(source)
        openMove(log)
        XCTAssertTrue(element("worklog-move.candidate.\(destination.id)").waitForExistence(timeout: timeout))
        element("worklog-move.candidate.\(destination.id)").click()
        try proxy.arm([
            .init(method: "PATCH", path: "/v1/worklogs/\(log.id)", mode: .dropAfter),
            .init(method: "GET", path: "/v1/snapshot", mode: .fail)
        ])
        app.buttons["worklog-move.confirm"].click()
        XCTAssertTrue(element("worklog-move.error").waitForExistence(timeout: timeout))
        app.buttons["worklog-move.cancel"].click()
        assertPendingConnectionIsBlocked()
        let reopen = app.buttons["Retry worklog move"]
        XCTAssertTrue(reopen.waitForExistence(timeout: timeout))
        reopen.click()
        app.buttons["worklog-move.confirm"].click()
        waitUntil("Pending move reconciles") { !self.element("worklog-move.search").exists }
        let moved = try XCTUnwrap(fixture.activeWorklog())
        XCTAssertEqual(moved.id, log.id)
        XCTAssertEqual(moved.start, log.start)
        XCTAssertEqual(moved.taskID, destination.id)
        XCTAssertEqual(try proxy.requests(method: "PATCH", path: "/v1/worklogs/\(log.id)").count, 1)
    }

    private func assertPendingConnectionIsBlocked() {
        openConnectionSettings()
        XCTAssertFalse(app.buttons["connection.connect"].isEnabled)
        closeConnectionSettings()
    }
}
