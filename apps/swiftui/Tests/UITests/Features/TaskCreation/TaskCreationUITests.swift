import XCTest

class TaskCreationUITests: TrackerUITestCase {
    func testToolbarCreationTrimsNameAndPersistsAcrossRelaunch() throws {
        launch()
        openCreation()
        app.textFields["task-name.input"].typeText(" New implementation ")
        XCTAssertEqual(app.textFields["task-name.input"].value as? String, " New implementation ")
        app.buttons["task-name.submit"].click()
        waitUntil("Creation sheet closes") { !self.element("task-name.input").exists }
        let created = try XCTUnwrap(fixture.tasks().first)
        XCTAssertEqual(created.name, "New implementation")
        select(created)
        relaunch()
        select(created)
        XCTAssertEqual(taskDetailsNameText, created.name)
    }

    func testCommandNFocusesNameAndReturnCreatesDistinctDuplicateTask() throws {
        let existing = try fixture.create("Planning")
        launch()
        app.typeKey("n", modifierFlags: .command)
        XCTAssertTrue(element("task-name.input").waitForExistence(timeout: timeout))
        app.typeText(existing.name)
        app.typeKey(.return, modifierFlags: [])
        waitUntil("Return submits creation") { !self.element("task-name.input").exists }
        let tasks = try fixture.tasks()
        XCTAssertEqual(tasks.count, 2)
        XCTAssertEqual(Set(tasks.map(\.id)).count, 2)
        XCTAssertTrue(tasks.allSatisfy { $0.name == existing.name })
    }

    func testFileMenuAndContextMenuPresentOneSheetAndCancelWithoutWriting() throws {
        let task = try fixture.create("Context target")
        launch()
        app.menuBars.menuBarItems["File"].click()
        app.menuItems["New Task"].click()
        XCTAssertTrue(element("task-name.input").waitForExistence(timeout: timeout))
        XCTAssertEqual(app.sheets.count, 1)
        app.buttons["task-name.cancel"].click()
        waitUntil("First sheet closes") { !self.element("task-name.input").exists }
        taskRow(task).rightClick()
        app.menuItems["New task"].click()
        XCTAssertTrue(element("task-name.input").waitForExistence(timeout: timeout))
        app.typeText("Discarded draft")
        app.typeKey(.escape, modifierFlags: [])
        waitUntil("Escape closes creation") { !self.element("task-name.input").exists }
        XCTAssertEqual(try fixture.tasks().map(\.id), [task.id])
    }

    func testBlankNameCannotSubmitAndCancelPreservesRunningWorklog() throws {
        let task = try fixture.create("Running task")
        let log = try fixture.start(task)
        launch()
        openCreation()
        XCTAssertFalse(app.buttons["task-name.submit"].isEnabled)
        app.textFields["task-name.input"].typeText(" ")
        XCTAssertEqual(app.textFields["task-name.input"].value as? String, " ")
        XCTAssertFalse(app.buttons["task-name.submit"].isEnabled)
        replaceText(app.textFields["task-name.input"], "Valid draft")
        XCTAssertTrue(app.buttons["task-name.submit"].isEnabled)
        app.buttons["task-name.cancel"].click()
        XCTAssertEqual(try fixture.activeWorklog(), log)
        XCTAssertEqual(try fixture.tasks().count, 1)
    }

    func testSuccessfulCreationPreservesExistingTimerAndHistory() throws {
        let running = try fixture.create("Existing running task")
        let timer = try fixture.start(running)
        launch()
        openCreation()
        app.textFields["task-name.input"].typeText("Created while tracking")
        app.typeKey(.return, modifierFlags: [])
        waitUntil("New task is created") { !self.element("task-name.input").exists }
        XCTAssertEqual(try fixture.activeWorklog(), timer)
        XCTAssertEqual(try fixture.worklogs(running), [timer])
        XCTAssertEqual(try fixture.tasks().count, 2)
    }

    func testInvalidLongNameDisplaysErrorAndValidNameCanRecover() throws {
        launch()
        openCreation()
        app.textFields["task-name.input"].typeText(String(repeating: "x", count: 257))
        app.buttons["task-name.submit"].click()
        XCTAssertTrue(element("task-name.error").waitForExistence(timeout: timeout))
        XCTAssertTrue(try fixture.tasks().isEmpty)
        replaceText(app.textFields["task-name.input"], "Recovered valid name")
        app.buttons["task-name.submit"].click()
        waitUntil("Valid input recovers creation") { !self.element("task-name.input").exists }
        XCTAssertEqual(try fixture.tasks().first?.name, "Recovered valid name")
    }
}

final class ServerTaskCreationUITests: TaskCreationUITests {
    override var fixtureSource: TrackerFixture.Source { .server }
}

