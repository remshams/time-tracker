import XCTest

class TaskCatalogUITests: TrackerUITestCase {
    func testEmptyStorageAndEmptyTabsShowTheirEmptyStates() throws {
        launch()
        XCTAssertTrue(element("task-sidebar.empty").waitForExistence(timeout: timeout))
        XCTAssertTrue(app.staticTexts["Select a task"].exists)
        showTab("Archived")
        XCTAssertTrue(element("task-sidebar.empty").exists)
        XCTAssertTrue(try fixture.tasks().isEmpty)
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
        XCTAssertEqual(element("task-details.name").value as? String, second.name)
        XCTAssertTrue(element("task-details.task.\(second.id)").exists)
        showTab("Archived")
        XCTAssertEqual(element("task-details.name").value as? String, archived.name)
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
        XCTAssertEqual(element("task-details.name").value as? String, selected.name)
        XCTAssertTrue(element("worklog-history.row.\(log.id)").exists)
    }
}

final class ServerTaskCatalogUITests: TaskCatalogUITests {
    override var fixtureSource: TrackerFixture.Source { .server }
}
