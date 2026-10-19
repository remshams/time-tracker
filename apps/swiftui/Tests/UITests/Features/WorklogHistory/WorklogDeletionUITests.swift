import XCTest

class WorklogDeletionUITests: TrackerUITestCase {
    private func openDeletion(_ log: FixtureWorklog) {
        openWorklogAction(log, title: "Delete...", sheetID: "worklog-deletion.summary")
    }

    func testConfirmedDeletionRemovesOnlyTheSelectedWorklogPermanently() throws {
        let task = try fixture.create("Deletion task")
        let kept = try fixture.completed(task, start: Date().addingTimeInterval(-7200), end: Date().addingTimeInterval(-5400))
        let log = try fixture.completed(task, start: Date().addingTimeInterval(-3600.375), end: Date().addingTimeInterval(-1800.625))
        launch()
        select(task)
        openDeletion(log)
        app.buttons["worklog-deletion.confirm"].click()
        waitUntil("Deletion sheet closes") { !self.element("worklog-deletion.summary").exists }
        waitUntil("Deleted row disappears") { !self.element("worklog-history.row.\(log.id)").exists }
        XCTAssertTrue(element("worklog-history.row.\(kept.id)").exists)
        XCTAssertEqual(try fixture.worklogs(task), [kept])
        relaunch()
        select(task)
        XCTAssertTrue(element("worklog-history.row.\(kept.id)").waitForExistence(timeout: timeout))
        XCTAssertFalse(element("worklog-history.row.\(log.id)").exists)
    }

    func testCancelKeepsTheWorklog() throws {
        let task = try fixture.create("Cancelled deletion")
        let log = try fixture.completed(task, start: Date().addingTimeInterval(-3600), end: Date().addingTimeInterval(-1800))
        launch()
        select(task)
        openDeletion(log)
        app.typeKey(.escape, modifierFlags: [])
        waitUntil("Escape cancels deletion") { !self.element("worklog-deletion.summary").exists }
        XCTAssertTrue(element("worklog-history.row.\(log.id)").exists)
        XCTAssertEqual(try fixture.worklogs(task), [log])
    }

    func testRunningWorklogCannotBeDeleted() throws {
        let task = try fixture.create("Running deletion")
        let log = try fixture.start(task, at: Date().addingTimeInterval(-120))
        launch()
        select(task)
        let actions = element("worklog-history.actions.\(log.id)")
        XCTAssertTrue(actions.waitForExistence(timeout: timeout))
        waitUntil("Worklog actions become available") { actions.isEnabled }
        actions.click()
        XCTAssertTrue(app.menuItems["Delete..."].waitForExistence(timeout: timeout))
        XCTAssertFalse(app.menuItems["Delete..."].isEnabled)
        app.typeKey(.escape, modifierFlags: [])
        XCTAssertEqual(try fixture.activeWorklog(), log)
    }

    func testExternallyCorrectedWorklogShowsConflictOnConfirmAndReopenedDeletionSucceeds() throws {
        let task = try fixture.create("Conflicting deletion")
        let start = Date().addingTimeInterval(-3600)
        let end = start.addingTimeInterval(600)
        let log = try fixture.completed(task, start: start, end: end)
        launch()
        select(task)
        openDeletion(log)
        let external = try fixture.correct(log, start: start.addingTimeInterval(60), end: end)
        app.buttons["worklog-deletion.confirm"].click()
        waitForText("worklog-deletion.error", containing: "changed")
        XCTAssertTrue(app.buttons["worklog-deletion.confirm"].isEnabled)
        XCTAssertFalse(app.buttons["worklog-deletion.review"].exists)
        XCTAssertEqual(try fixture.worklogs(task), [external])
        app.buttons["worklog-deletion.cancel"].click()
        waitUntil("Deletion closes") { !self.element("worklog-deletion.summary").exists }
        waitForHistoryDuration(of: external)
        openDeletion(external)
        app.buttons["worklog-deletion.confirm"].click()
        waitUntil("Reopened deletion closes") { !self.element("worklog-deletion.summary").exists }
        XCTAssertTrue(try fixture.worklogs(task).isEmpty)
    }
}

final class ServerWorklogDeletionUITests: WorklogDeletionUITests {
    override var fixtureSource: TrackerFixture.Source { .server }
}
