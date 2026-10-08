import XCTest

class WorklogCorrectionUITests: TrackerUITestCase {
    func testCompletedStartCorrectionUsesMinutePrecisionAndPreservesEndFractions() throws {
        let task = try fixture.create("Completed correction")
        let now = Date()
        let log = try fixture.completed(task, start: now.addingTimeInterval(-3600.375), end: now.addingTimeInterval(-1800.625))
        launch()
        select(task)
        openCorrection(log)
        XCTAssertTrue(element("worklog-correction.timezone").exists)
        let originalPreview = element("worklog-correction.duration").value as? String
        stepMinute("worklog-correction.start")
        XCTAssertNotEqual(element("worklog-correction.duration").value as? String, originalPreview)
        app.buttons["worklog-correction.save"].click()
        waitUntil("Completed correction closes") { !self.element("worklog-correction.start").exists }
        let saved = try XCTUnwrap(fixture.worklogs(task).first)
        XCTAssertEqual(saved.id, log.id)
        XCTAssertEqual(saved.taskID, log.taskID)
        XCTAssertNotEqual(saved.start, log.start)
        XCTAssertEqual(saved.end, log.end)
        assertMinutePrecision(saved.start)
        let originalDate = try parsed(log.start)
        XCTAssertEqual(try parsed(saved.start).timeIntervalSince1970,
                       floor(originalDate.timeIntervalSince1970 / 60) * 60 + 60, accuracy: 0.001)
        relaunch()
        select(task)
        XCTAssertTrue(element("worklog-history.row.\(saved.id)").waitForExistence(timeout: timeout))
        XCTAssertEqual(try fixture.worklogs(task), [saved])
    }

    func testCompletedEndCorrectionPreservesUntouchedStart() throws {
        let task = try fixture.create("End correction")
        let log = try fixture.completed(task, start: Date().addingTimeInterval(-3600.375), end: Date().addingTimeInterval(-1800.625))
        launch()
        select(task)
        openCorrection(log)
        stepMinute("worklog-correction.end", direction: .downArrow)
        app.typeKey(.return, modifierFlags: [])
        waitUntil("Return saves corrected end") { !self.element("worklog-correction.start").exists }
        let saved = try XCTUnwrap(fixture.worklogs(task).first)
        XCTAssertEqual(saved.id, log.id)
        XCTAssertEqual(saved.start, log.start)
        XCTAssertNotEqual(saved.end, log.end)
        assertMinutePrecision(try XCTUnwrap(saved.end))
    }

    func testRunningStartCorrectionKeepsExactTimerIdentityAndTracking() throws {
        let task = try fixture.create("Running correction")
        let log = try fixture.start(task, at: Date().addingTimeInterval(-3600.375))
        launch()
        select(task)
        openCorrection(log)
        XCTAssertFalse(element("worklog-correction.end").exists)
        stepMinute("worklog-correction.start")
        app.buttons["worklog-correction.save"].click()
        waitUntil("Running correction closes") { !self.element("worklog-correction.start").exists }
        let saved = try XCTUnwrap(fixture.activeWorklog())
        XCTAssertEqual(saved.id, log.id)
        XCTAssertEqual(saved.taskID, log.taskID)
        XCTAssertNil(saved.end)
        XCTAssertNotEqual(saved.start, log.start)
        XCTAssertTrue(app.buttons["Stop tracking"].isEnabled)
        XCTAssertTrue(element("tracking.active-task").exists)
    }

    func testCancelAndEscapeLeaveStoredTimesUnchanged() throws {
        let task = try fixture.create("Cancelled correction")
        let log = try fixture.completed(task, start: Date().addingTimeInterval(-3600), end: Date().addingTimeInterval(-1800))
        launch()
        select(task)
        for escape in [false, true] {
            openCorrection(log)
            stepMinute("worklog-correction.start")
            if escape { app.typeKey(.escape, modifierFlags: []) }
            else { app.buttons["worklog-correction.cancel"].click() }
            waitUntil("Correction cancels") { !self.element("worklog-correction.start").exists }
            XCTAssertEqual(try fixture.worklogs(task), [log])
        }
    }

    func testReversedAndOverlappingTimesDisplayValidationWithoutWrites() throws {
        let task = try fixture.create("Correction validation")
        let minute = floor(Date().timeIntervalSince1970 / 60) * 60
        let start = Date(timeIntervalSince1970: minute - 3600)
        let log = try fixture.completed(task, start: start, end: start.addingTimeInterval(60))
        let following = try fixture.completed(task, start: start.addingTimeInterval(60), end: start.addingTimeInterval(600))
        launch()
        select(task)
        openCorrection(log)
        stepMinute("worklog-correction.start")
        stepMinute("worklog-correction.start")
        XCTAssertTrue(element("worklog-correction.error").waitForExistence(timeout: timeout))
        XCTAssertFalse(app.buttons["worklog-correction.save"].isEnabled)
        app.buttons["worklog-correction.cancel"].click()
        openCorrection(log)
        stepMinute("worklog-correction.end")
        app.buttons["worklog-correction.save"].click()
        XCTAssertTrue(element("worklog-correction.error").waitForExistence(timeout: timeout))
        XCTAssertEqual(try fixture.worklogs(task).sorted { $0.id < $1.id },
                       [log, following].sorted { $0.id < $1.id })
        app.buttons["worklog-correction.cancel"].click()
    }

    func testExternalCorrectionRequiresReviewBeforeSavingRetainedDraft() throws {
        let task = try fixture.create("Concurrent correction")
        let start = Date().addingTimeInterval(-3600)
        let end = Date().addingTimeInterval(-1800)
        let log = try fixture.completed(task, start: start, end: end)
        launch()
        select(task)
        openCorrection(log)
        stepMinute("worklog-correction.start")
        let external = try fixture.correct(log, start: start.addingTimeInterval(-60), end: end)
        app.buttons["worklog-correction.save"].click()
        XCTAssertTrue(app.buttons["worklog-correction.review"].waitForExistence(timeout: timeout))
        XCTAssertFalse(app.buttons["worklog-correction.save"].isEnabled)
        XCTAssertEqual(try fixture.worklogs(task), [external])
        app.buttons["worklog-correction.review"].click()
        stepMinute("worklog-correction.start")
        app.buttons["worklog-correction.save"].click()
        waitUntil("Reviewed correction closes") { !self.element("worklog-correction.start").exists }
        XCTAssertEqual(try fixture.worklogs(task).first?.id, log.id)
    }

    func testFutureRunningStartShowsValidationAndPreservesTheTimer() throws {
        let task = try fixture.create("Future correction guard")
        let start = Date(timeIntervalSince1970: floor(Date().timeIntervalSince1970 / 60) * 60)
        let log = try fixture.start(task, at: start)
        launch()
        select(task)
        openCorrection(log)
        for _ in 0..<5 { stepMinute("worklog-correction.start") }
        XCTAssertTrue(element("worklog-correction.error").waitForExistence(timeout: timeout))
        XCTAssertFalse(app.buttons["worklog-correction.save"].isEnabled)
        XCTAssertEqual(try fixture.activeWorklog(), log)
        app.buttons["worklog-correction.cancel"].click()
    }

    private func parsed(_ timestamp: String) throws -> Date {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return try XCTUnwrap(formatter.date(from: timestamp) ?? ISO8601DateFormatter().date(from: timestamp))
    }

    private func assertMinutePrecision(_ timestamp: String, file: StaticString = #filePath, line: UInt = #line) {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        let parsed = formatter.date(from: timestamp) ?? ISO8601DateFormatter().date(from: timestamp)
        guard let date = parsed else { XCTFail("Invalid saved timestamp \(timestamp)", file: file, line: line); return }
        XCTAssertEqual(date.timeIntervalSince1970.truncatingRemainder(dividingBy: 60), 0,
                       accuracy: 0.001, file: file, line: line)
    }
}

final class ServerWorklogCorrectionUITests: WorklogCorrectionUITests {
    override var fixtureSource: TrackerFixture.Source { .server }
}
