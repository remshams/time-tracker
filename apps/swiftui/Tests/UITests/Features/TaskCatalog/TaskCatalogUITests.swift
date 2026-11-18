import XCTest

class TaskCatalogUITests: TrackerUITestCase {
    func testEmptyStorageAndEmptyTabsShowTheirEmptyStates() throws {
        launch()
        XCTAssertTrue(element("task-sidebar.empty").waitForExistence(timeout: timeout))
        let create = app.buttons["Create your first task"]
        XCTAssertTrue(create.waitForExistence(timeout: timeout))
        XCTAssertTrue(create.isEnabled)
        XCTAssertFalse(app.staticTexts["Select a task"].exists)
        XCTAssertFalse(app.staticTexts["Nothing running"].exists, "Without tasks the toolbar shows no pill.")
        showTab("Archived")
        XCTAssertTrue(element("task-sidebar.empty").exists)
        XCTAssertTrue(try fixture.tasks().isEmpty)
    }

    func testEmptyStateButtonCreatesTheFirstTask() throws {
        launch()
        let create = app.buttons["Create your first task"]
        XCTAssertTrue(create.waitForExistence(timeout: timeout))
        create.click()
        XCTAssertTrue(element("task-name.input").waitForExistence(timeout: timeout))
        app.textFields["task-name.input"].typeText("First task")
        app.buttons["task-name.submit"].click()
        waitUntil("Creation sheet closes") { !self.element("task-name.input").exists }
        let created = try XCTUnwrap(fixture.tasks().first)
        XCTAssertEqual(created.name, "First task")
        waitUntil("The created task's header replaces the invitation") {
            self.element("task-details.task.\(created.id)").exists && !create.exists
        }
        XCTAssertTrue(app.buttons["Start tracking"].waitForExistence(timeout: timeout), "The pill comes back.")
    }

    func testOnlyArchivedTasksShowTheRegularEmptyTab() throws {
        let archived = try fixture.create("Only archived")
        try fixture.archive(archived)
        launch()
        XCTAssertTrue(element("task-sidebar.empty").waitForExistence(timeout: timeout))
        XCTAssertTrue(app.staticTexts["Select a task"].waitForExistence(timeout: timeout))
        XCTAssertFalse(app.buttons["Create your first task"].exists)
        XCTAssertTrue(app.staticTexts["Nothing running"].exists)
    }

    func testTabsRememberIndependentSelectionsAndDuplicateNamesUseTheirIDs() throws {
        let first = try fixture.create("Planning")
        let second = try fixture.create("Planning")
        let archived = try fixture.create("Archived release")
        try fixture.archive(archived)
        launch()
        select(second)
        XCTAssertTrue(taskRow(first).exists)
        XCTAssertTrue(taskRow(second).exists)
        showTab("Archived")
        select(archived)
        XCTAssertFalse(taskRow(first).exists)
        showTab("Active")
        XCTAssertEqual(taskDetailsNameText, second.name)
        XCTAssertTrue(element("task-details.task.\(second.id)").exists)
        showTab("Archived")
        XCTAssertEqual(taskDetailsNameText, archived.name)
    }

    func testActivityReorderingKeepsTheSelectedTaskAndItsHistory() throws {
        let selected = try fixture.create("Selected review")
        let other = try fixture.create("Recent development")
        let log = try fixture.completed(selected, start: Date().addingTimeInterval(-7200), end: Date().addingTimeInterval(-3600))
        launch()
        select(selected)
        XCTAssertTrue(element("worklog-history.row.\(log.id)").waitForExistence(timeout: timeout))
        _ = try fixture.start(other)
        waitUntil("External activity refreshes the catalog") {
            self.taskRow(other).descendants(matching: .any)
                .matching(identifier: "tracking.active-task").firstMatch.exists
        }
        XCTAssertEqual(taskDetailsNameText, selected.name)
        XCTAssertTrue(element("worklog-history.row.\(log.id)").exists)
    }
}

final class ServerTaskCatalogUITests: TaskCatalogUITests {
    override var fixtureSource: TrackerFixture.Source { .server }
}
