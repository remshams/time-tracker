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
        XCTAssertFalse(app.buttons["Start tracking"].isEnabled)
        try fixture.startServer()
        app.buttons["connection-summary.retry"].click()
        assertConnectionStatus("Connected")
        XCTAssertEqual(try fixture.tasks().map(\.id), [task.id])
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
        try proxy.arm(method: "GET", path: "/v1/worklogs?task_id=\(first.id)", mode: .holdAfter)
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
        try proxy.arm(method: "GET", path: "/v1/worklogs?task_id=\(task.id)", mode: .fail)
        select(task)
        XCTAssertTrue(element("worklog-history.error").waitForExistence(timeout: timeout))
        app.buttons["worklog-history.retry"].click()
        XCTAssertTrue(element("worklog-history.row.\(log.id)").waitForExistence(timeout: timeout))
        XCTAssertFalse(element("worklog-history.error").exists)
        XCTAssertEqual(try fixture.worklogs(task), [log])
    }

    func testStartDuringBackgroundReadTargetsClickedTaskAndWritesOnce() throws {
        let first = try fixture.create("Captured start target")
        let second = try fixture.create("Later selection")
        let proxy = try fixture.enableProxy()
        launch()
        select(first)
        try proxy.arm(method: "GET", path: "/v1/state/daily-overview", mode: .holdAfter)
        _ = try proxy.waitForHeldRequest()
        let start = app.buttons["Start tracking"]
        start.click()
        // The start waits behind the held poll in the bridge's serial worker; task creation stays available.
        waitUntil("The start waits for its response") { !start.isEnabled }
        XCTAssertTrue(app.buttons["New task"].isEnabled)
        XCTAssertTrue(app.buttons["bulk-archive.open"].isEnabled)
        select(second)
        try proxy.release()
        waitUntil("Captured task starts") { (try? self.fixture.activeWorklog())?.taskID == first.id }
        XCTAssertEqual(try proxy.requests(method: "PUT", path: "/v1/tracking").count, 1)
        XCTAssertEqual(taskDetailsNameText, second.name)
    }

    func testTrackingWriteInFlightDisablesOnlyTrackingControls() throws {
        let task = try fixture.create("In-flight start")
        let proxy = try fixture.enableProxy()
        launch()
        select(task)
        try proxy.arm(method: "PUT", path: "/v1/tracking", mode: .holdBefore)
        app.buttons["Start tracking"].click()
        _ = try proxy.waitForHeldRequest()
        XCTAssertFalse(app.buttons["Start tracking"].isEnabled)
        XCTAssertTrue(app.buttons["New task"].isEnabled)
        XCTAssertTrue(app.buttons["Edit task name"].isEnabled)
        XCTAssertTrue(app.buttons["bulk-archive.open"].isEnabled)
        try proxy.release()
        assertRunning(task)
        waitUntil("Tracking controls become available after success") {
            self.app.buttons["Stop tracking"].exists && self.app.buttons["Stop tracking"].isEnabled
        }
    }

    func testCapturedStopCannotStopAnotherClientsReplacementTimer() throws {
        let first = try fixture.create("Original running task")
        let second = try fixture.create("Replacement task")
        let original = try fixture.start(first)
        let proxy = try fixture.enableProxy()
        launch()
        select(first)
        try proxy.arm([
            .init(method: "GET", path: "/v1/state/daily-overview", mode: .holdAfter),
            .init(method: "GET", path: "/v1/tracking", mode: .holdAfter),
        ])
        _ = try proxy.waitForHeldRequest()
        app.buttons["Stop tracking"].click()
        try proxy.release()
        // Let the adapter finish its read before the competing client changes tracking.
        let preflight = try proxy.waitForHeldRequest()
        XCTAssertTrue(preflight.path.contains("/v1/tracking"))
        let replacement = try fixture.start(second)
        try proxy.release()
        dismissTrackingFailure(containing: "changed")
        waitUntil("The reload shows the replacement timer") {
            self.taskRow(second).descendants(matching: .any).matching(identifier: "tracking.active-task").firstMatch.exists
        }
        XCTAssertEqual(try fixture.activeWorklog(), replacement)
        XCTAssertNotEqual(replacement.id, original.id)
        XCTAssertNotNil(try fixture.worklogs(first).first?.end)
        let writes = try proxy.requests(method: "PUT", path: "/v1/tracking")
        XCTAssertEqual(writes.count, 1)
        XCTAssertEqual(try XCTUnwrap(writes.first?.status), 409)
    }

    func testUnconfirmedCreationReportsFailureAndReloadShowsCommittedTask() throws {
        let proxy = try fixture.enableProxy()
        launch()
        openCreation()
        app.textFields["task-name.input"].typeText("Committed creation")
        try proxy.dropWriteResponses(method: "POST", path: "/v1/tasks")
        let baseline = try proxy.requests().count
        app.buttons["task-name.submit"].click()
        waitForText("task-name.error", containing: unconfirmedChangeMessage)
        XCTAssertTrue(app.textFields["task-name.input"].isEnabled)
        XCTAssertEqual(app.textFields["task-name.input"].value as? String, "Committed creation")
        XCTAssertEqual(app.buttons["task-name.submit"].label, "Create")
        XCTAssertTrue(app.buttons["task-name.submit"].isEnabled)
        let tasks = try fixture.tasks()
        XCTAssertEqual(tasks.count, 1)
        let committed = try XCTUnwrap(tasks.first)
        XCTAssertEqual(committed.name, "Committed creation")
        XCTAssertTrue(taskRow(committed).waitForExistence(timeout: timeout))
        try assertAcceptedWriteIntent(proxy, method: "POST", path: "/v1/tasks")
        try assertWriteSentWithoutPriorRead(proxy, after: baseline, method: "POST", path: "/v1/tasks")
        app.buttons["task-name.cancel"].click()
        waitUntil("Creation closes") { !self.element("task-name.input").exists }
        openCreation()
        XCTAssertEqual(app.textFields["task-name.input"].value as? String, "")
        app.buttons["task-name.cancel"].click()
        try assertAcceptedWriteIntent(proxy, method: "POST", path: "/v1/tasks")
        XCTAssertEqual(try fixture.tasks().map(\.id), [committed.id])
    }

    func testUnconfirmedRenameReportsFailureAndReloadShowsCommittedName() throws {
        let task = try fixture.create("Original remote name")
        let running = try fixture.start(task)
        let proxy = try fixture.enableProxy()
        launch()
        openRename(task)
        replaceText(app.textFields["task-name.input"], "Confirmed remote name")
        try proxy.dropWriteResponses(method: "PATCH", path: "/v1/tasks/\(task.id)")
        let baseline = try proxy.requests().count
        app.buttons["task-name.submit"].click()
        waitForText("task-name.error", containing: unconfirmedChangeMessage)
        XCTAssertTrue(app.textFields["task-name.input"].isEnabled)
        XCTAssertEqual(app.textFields["task-name.input"].value as? String, "Confirmed remote name")
        XCTAssertEqual(app.buttons["task-name.submit"].label, "Save")
        XCTAssertTrue(app.buttons["task-name.submit"].isEnabled)
        waitUntil("The reload shows the committed name") { self.taskDetailsNameText == "Confirmed remote name" }
        XCTAssertEqual(try fixture.tasks().first?.name, "Confirmed remote name")
        XCTAssertEqual(try fixture.activeWorklog(), running)
        try assertAcceptedWriteIntent(proxy, method: "PATCH", path: "/v1/tasks/\(task.id)")
        try assertWriteSentWithoutPriorRead(proxy, after: baseline, method: "PATCH", path: "/v1/tasks/\(task.id)")
        app.buttons["task-name.cancel"].click()
    }

    func testUnconfirmedArchiveReportsFailureAndReloadMovesTaskToArchived() throws {
        let task = try fixture.create("Remote archive target")
        let proxy = try fixture.enableProxy()
        launch()
        select(task)
        XCTAssertTrue(element("worklog-history.empty").waitForExistence(timeout: timeout))
        app.buttons["Archive task"].click()
        XCTAssertTrue(app.buttons["task-archive.confirm"].waitForExistence(timeout: timeout))
        try proxy.dropWriteResponses(method: "PATCH", path: "/v1/tasks/\(task.id)")
        let baseline = try proxy.requests().count
        app.buttons["task-archive.confirm"].click()
        waitForText("task-archive.error", containing: unconfirmedChangeMessage)
        XCTAssertTrue(app.buttons["task-archive.confirm"].isEnabled)
        XCTAssertEqual(app.buttons["task-archive.confirm"].label, "Archive")
        XCTAssertFalse(app.buttons["task-archive.review"].exists)
        waitUntil("The reload removes the task from Active") { !self.taskRow(task).exists }
        XCTAssertTrue(try XCTUnwrap(fixture.tasks().first).archived)
        try assertAcceptedWriteIntent(proxy, method: "PATCH", path: "/v1/tasks/\(task.id)")
        try assertWriteSentWithoutPriorRead(proxy, after: baseline, method: "PATCH", path: "/v1/tasks/\(task.id)")
        app.buttons["task-archive.cancel"].click()
        waitUntil("Archive confirmation closes") { !self.app.buttons["task-archive.confirm"].exists }
        showTab("Archived")
        select(task)
    }

    func testUnconfirmedMoveReportsFailureAndReloadRemovesRowFromSource() throws {
        let source = try fixture.create("Remote source")
        let destination = try fixture.create("Remote destination")
        let log = try fixture.start(source, at: Date().addingTimeInterval(-90))
        let proxy = try fixture.enableProxy()
        launch()
        select(source)
        openMove(log)
        XCTAssertTrue(element("worklog-move.candidate.\(destination.id)").waitForExistence(timeout: timeout))
        element("worklog-move.candidate.\(destination.id)").click()
        try proxy.dropWriteResponses(method: "PATCH", path: "/v1/worklogs/\(log.id)")
        let baseline = try proxy.requests().count
        app.buttons["worklog-move.confirm"].click()
        waitForText("worklog-move.error", containing: unconfirmedChangeMessage)
        XCTAssertTrue(app.textFields["worklog-move.search"].isEnabled)
        XCTAssertTrue(app.buttons["worklog-move.confirm"].isEnabled)
        XCTAssertEqual(app.buttons["worklog-move.confirm"].label, "Move")
        XCTAssertFalse(app.buttons["worklog-move.review"].exists)
        waitUntil("The history reload removes the moved row") {
            !self.element("worklog-history.row.\(log.id)").exists
        }
        let moved = try XCTUnwrap(fixture.activeWorklog())
        XCTAssertEqual(moved.id, log.id)
        XCTAssertEqual(moved.start, log.start)
        XCTAssertEqual(moved.end, log.end)
        XCTAssertEqual(moved.taskID, destination.id)
        try assertAcceptedWriteIntent(proxy, method: "PATCH", path: "/v1/worklogs/\(log.id)")
        try assertWriteSentWithoutPriorRead(proxy, after: baseline, method: "PATCH", path: "/v1/worklogs/\(log.id)")
        app.buttons["worklog-move.cancel"].click()
    }

    func testUnconfirmedCorrectionReportsFailureAndReloadShowsCommittedTimes() throws {
        let task = try fixture.create("Remote correction")
        // Half a minute past the minute, so the corrected start changes the displayed duration.
        let now = Date(timeIntervalSince1970: floor(Date().timeIntervalSince1970 / 60) * 60 + 30)
        let log = try fixture.completed(task, start: now.addingTimeInterval(-3600), end: now.addingTimeInterval(-1800.375))
        let proxy = try fixture.enableProxy()
        launch()
        select(task)
        openCorrection(log)
        stepMinute("worklog-correction.start")
        try proxy.dropWriteResponses(method: "PATCH", path: "/v1/worklogs/\(log.id)")
        let baseline = try proxy.requests().count
        app.buttons["worklog-correction.save"].click()
        waitForText("worklog-correction.error", containing: unconfirmedChangeMessage)
        XCTAssertTrue(app.buttons["worklog-correction.save"].isEnabled)
        XCTAssertEqual(app.buttons["worklog-correction.save"].label, "Save")
        XCTAssertFalse(app.buttons["worklog-correction.review"].exists)
        let corrected = try XCTUnwrap(fixture.worklogs(task).first)
        XCTAssertEqual(corrected.id, log.id)
        XCTAssertEqual(corrected.end, log.end)
        XCTAssertNotEqual(corrected.start, log.start)
        waitForHistoryDuration(of: corrected)
        try assertAcceptedWriteIntent(proxy, method: "PATCH", path: "/v1/worklogs/\(log.id)")
        try assertWriteSentWithoutPriorRead(proxy, after: baseline, method: "PATCH", path: "/v1/worklogs/\(log.id)")
        app.buttons["worklog-correction.cancel"].click()
    }

    func testUnconfirmedDeletionReportsFailureAndReloadRemovesCommittedRow() throws {
        let task = try fixture.create("Remote deletion")
        let log = try fixture.completed(task, start: Date().addingTimeInterval(-3600), end: Date().addingTimeInterval(-1800))
        let proxy = try fixture.enableProxy()
        launch()
        select(task)
        openWorklogAction(log, title: "Delete...", sheetID: "worklog-deletion.summary")
        try proxy.dropWriteResponses(method: "DELETE", path: "/v1/worklogs/\(log.id)")
        let baseline = try proxy.requests().count
        app.buttons["worklog-deletion.confirm"].click()
        waitForText("worklog-deletion.error", containing: unconfirmedChangeMessage)
        XCTAssertTrue(app.buttons["worklog-deletion.confirm"].isEnabled)
        XCTAssertEqual(app.buttons["worklog-deletion.confirm"].label, "Delete")
        XCTAssertFalse(app.buttons["worklog-deletion.review"].exists)
        XCTAssertTrue(element("worklog-deletion.summary").exists)
        waitUntil("The history reload removes the deleted row") {
            !self.element("worklog-history.row.\(log.id)").exists && self.element("worklog-history.empty").exists
        }
        XCTAssertTrue(try fixture.worklogs(task).isEmpty)
        try assertAcceptedWriteIntent(proxy, method: "DELETE", path: "/v1/worklogs/\(log.id)")
        try assertWriteSentWithoutPriorRead(proxy, after: baseline, method: "DELETE", path: "/v1/worklogs/\(log.id)")
        app.buttons["worklog-deletion.cancel"].click()
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
        try proxy.arm(method: "GET", path: "/v1/worklogs?task_id=\(task.id)&", mode: .holdBefore)
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
        try proxy.arm([
            .init(method: "GET", path: "/v1/state/daily-overview", mode: .holdAfter),
            .init(method: "GET", path: "/v1/tasks/\(selected.id)", mode: .holdAfter),
        ])
        _ = try proxy.waitForHeldRequest()
        app.buttons["Start tracking"].click()
        try proxy.release()
        let preflight = try proxy.waitForHeldRequest()
        XCTAssertTrue(preflight.path.contains("/v1/tasks/\(selected.id)"))
        let timer = try fixture.start(competing)
        try proxy.release()
        dismissTrackingFailure(containing: "changed")
        waitUntil("The reload shows the competing timer") {
            self.taskRow(competing).descendants(matching: .any).matching(identifier: "tracking.active-task").firstMatch.exists
        }
        XCTAssertEqual(try fixture.activeWorklog(), timer)
        let writes = try proxy.requests(method: "PUT", path: "/v1/tracking")
        XCTAssertEqual(writes.count, 1)
        XCTAssertEqual(try XCTUnwrap(writes.first?.status), 409)
    }
}
