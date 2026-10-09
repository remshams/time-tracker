import XCTest

class TaskArchivingUITests: TrackerUITestCase {
    func testArchiveAndImmediateUnarchivePreserveHistoryAndCurrentTab() throws {
        let target = try fixture.create("Archive target")
        let fallback = try fixture.create("Fallback task")
        let log = try fixture.completed(target, start: Date().addingTimeInterval(-3600), end: Date().addingTimeInterval(-1800))
        launch()
        select(target)
        app.buttons["Archive task"].click()
        XCTAssertTrue(app.buttons["task-archive.confirm"].waitForExistence(timeout: timeout))
        app.buttons["task-archive.confirm"].click()
        waitUntil("Archived task leaves Active") { !self.taskRow(target).exists }
        XCTAssertTrue(taskRow(fallback).exists)
        waitUntil("Details select the remaining active task") {
            self.element("task-details.task.\(fallback.id)").exists
                && self.taskDetailsNameText == fallback.name
        }
        XCTAssertEqual(taskDetailsNameText, fallback.name)
        showTab("Archived")
        select(target)
        XCTAssertTrue(element("worklog-history.row.\(log.id)").waitForExistence(timeout: timeout))
        app.buttons["Unarchive task"].click()
        waitUntil("Restored task leaves Archived") { !self.taskRow(target).exists }
        XCTAssertTrue(element("task-sidebar.empty").exists)
        showTab("Active")
        select(target)
        XCTAssertTrue(element("worklog-history.row.\(log.id)").exists)
        XCTAssertEqual(try fixture.worklogs(target), [log])
        XCTAssertFalse(try XCTUnwrap(fixture.tasks().first { $0.id == target.id }).archived)
        relaunch()
        select(target)
        XCTAssertTrue(element("worklog-history.row.\(log.id)").waitForExistence(timeout: timeout))
    }

    func testContextArchiveCancelAndEscapeDoNotWrite() throws {
        let task = try fixture.create("Archive cancellation")
        launch()
        select(task)
        for useEscape in [false, true] {
            taskRow(task).rightClick()
            app.menuItems["Archive task..."].click()
            XCTAssertTrue(app.buttons["task-archive.confirm"].waitForExistence(timeout: timeout))
            if useEscape { app.typeKey(.escape, modifierFlags: []) }
            else { app.buttons["task-archive.cancel"].click() }
            waitUntil("Archive confirmation closes") { !self.app.buttons["task-archive.confirm"].exists }
            XCTAssertFalse(try XCTUnwrap(fixture.tasks().first).archived)
        }
    }

    func testRunningTaskCannotBeArchivedByToolbarOrContextMenu() throws {
        let task = try fixture.create("Running archive guard")
        let log = try fixture.start(task)
        launch()
        select(task)
        XCTAssertFalse(app.buttons["Archive task"].isEnabled)
        taskRow(task).rightClick()
        XCTAssertFalse(app.menuItems["Archive task..."].isEnabled)
        app.typeKey(.escape, modifierFlags: [])
        XCTAssertEqual(try fixture.activeWorklog(), log)
        XCTAssertFalse(try XCTUnwrap(fixture.tasks().first).archived)
    }

    func testExternalRenameRequiresReviewBeforeArchiveAndKeepsLatestName() throws {
        let task = try fixture.create("Archive original")
        launch()
        select(task)
        app.buttons["Archive task"].click()
        XCTAssertTrue(app.buttons["task-archive.confirm"].waitForExistence(timeout: timeout))
        try fixture.rename(task, to: "External archive name")
        app.buttons["task-archive.confirm"].click()
        XCTAssertTrue(app.buttons["task-archive.review"].waitForExistence(timeout: timeout))
        XCTAssertFalse(app.buttons["task-archive.confirm"].isEnabled)
        XCTAssertFalse(try XCTUnwrap(fixture.tasks().first).archived)
        app.buttons["task-archive.review"].click()
        app.buttons["task-archive.confirm"].click()
        waitUntil("Reviewed archive closes") { !self.app.buttons["task-archive.confirm"].exists }
        let stored = try XCTUnwrap(fixture.tasks().first)
        XCTAssertTrue(stored.archived)
        XCTAssertEqual(stored.name, "External archive name")
    }
}

final class ServerTaskArchivingUITests: TaskArchivingUITests {
    override var fixtureSource: TrackerFixture.Source { .server }
}
