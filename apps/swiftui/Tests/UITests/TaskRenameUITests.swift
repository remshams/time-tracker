import XCTest

class TaskRenameUITests: TrackerUITestCase {
    func testRenamingRunningTaskPreservesWorklogsAndUpdatesConfirmedNameAfterRelaunch() throws {
        let task = try fixture.create("Original task")
        let log = try fixture.start(task)
        launch()
        openRename(task)
        replaceText(app.textFields["task-name.input"], " Renamed task ")
        XCTAssertEqual(app.textFields["task-name.input"].value as? String, " Renamed task ")
        app.typeKey(.return, modifierFlags: [])
        waitUntil("Rename sheet closes") { !self.element("task-name.input").exists }
        waitUntil("Confirmed details display the new name") { self.taskDetailsNameText == "Renamed task" }
        XCTAssertEqual(try fixture.activeWorklog(), log)
        XCTAssertEqual(try fixture.worklogs(task), [log])
        XCTAssertEqual(try fixture.tasks().first?.id, task.id)
        relaunch()
        select(try XCTUnwrap(fixture.tasks().first { $0.id == task.id }))
        XCTAssertEqual(taskDetailsNameText, "Renamed task")
    }

    func testContextRenameTargetsUnselectedArchivedTaskAndCancelKeepsItsName() throws {
        let selected = try fixture.create("Selected archive")
        let target = try fixture.create("Unselected archive")
        try fixture.archive(selected)
        try fixture.archive(target)
        launch()
        showTab("Archived")
        select(selected)
        openRename(target, contextMenu: true)
        XCTAssertEqual(app.textFields["task-name.input"].value as? String, target.name)
        replaceText(app.textFields["task-name.input"], "Discarded name")
        app.typeKey(.escape, modifierFlags: [])
        waitUntil("Escape closes rename") { !self.element("task-name.input").exists }
        XCTAssertEqual(try fixture.tasks().first { $0.id == target.id }?.name, target.name)
        openRename(target, contextMenu: true)
        replaceText(app.textFields["task-name.input"], "Renamed archive")
        app.buttons["task-name.submit"].click()
        waitUntil("Archive rename finishes") { !self.element("task-name.input").exists }
        XCTAssertEqual(try fixture.tasks().first { $0.id == target.id }?.name, "Renamed archive")
        XCTAssertTrue(try XCTUnwrap(fixture.tasks().first { $0.id == target.id }).archived)
    }

    func testUnchangedAndBlankNamesCannotSubmit() throws {
        let task = try fixture.create("Existing name")
        launch()
        openRename(task)
        XCTAssertFalse(app.buttons["task-name.submit"].isEnabled)
        replaceText(app.textFields["task-name.input"], " Existing name ")
        XCTAssertEqual(app.textFields["task-name.input"].value as? String, " Existing name ")
        XCTAssertFalse(app.buttons["task-name.submit"].isEnabled)
        replaceText(app.textFields["task-name.input"], " ")
        XCTAssertEqual(app.textFields["task-name.input"].value as? String, " ")
        XCTAssertFalse(app.buttons["task-name.submit"].isEnabled)
        app.buttons["task-name.cancel"].click()
        XCTAssertEqual(try fixture.tasks().first?.name, task.name)
    }

    func testExternalRenameRetainsDraftAndShowsConflict() throws {
        let task = try fixture.create("Original name")
        launch()
        openRename(task)
        replaceText(app.textFields["task-name.input"], "My draft")
        try fixture.rename(task, to: "External name")
        app.buttons["task-name.submit"].click()
        XCTAssertTrue(element("task-name.error").waitForExistence(timeout: timeout))
        XCTAssertEqual(app.textFields["task-name.input"].value as? String, "My draft")
        XCTAssertEqual(try fixture.tasks().first?.name, "External name")
        app.buttons["task-name.cancel"].click()
    }
}

final class ServerTaskRenameUITests: TaskRenameUITests {
    override var fixtureSource: TrackerFixture.Source { .server }
}
