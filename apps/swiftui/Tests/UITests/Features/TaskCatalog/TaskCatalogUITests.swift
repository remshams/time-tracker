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

/// The sidebar scrolls to the selected task when something other than a click changes the selection. The behavior
/// lives in the view, so these tests don't repeat for the server source.
final class TaskSelectionScrollUITests: TrackerUITestCase {
    private let backlogSize = 40

    func testCreatedTaskAtTheBottomScrollsIntoView() throws {
        let backlog = try createBacklog(count: backlogSize)
        launch()
        waitUntilVisibleInSidebar(backlog[0])
        openCreation()
        // Without worklogs and with the last name, the new task sorts to the bottom of the list.
        app.textFields["task-name.input"].typeText("Zulu follow-up")
        app.buttons["task-name.submit"].click()
        waitUntil("Creation sheet closes") { !self.element("task-name.input").exists }
        let created = try XCTUnwrap(fixture.tasks().first { $0.name == "Zulu follow-up" })
        waitUntil("The created task is selected") { self.element("task-details.task.\(created.id)").exists }
        waitUntilVisibleInSidebar(created)
        XCTAssertFalse(isVisibleInSidebar(backlog[0]), "The list is taller than the sidebar.")
    }

    // The tracking pill's "Show Task" calls the same catalog method, but a right click on the toolbar opens the
    // toolbar's own menu in UI tests, so this test uses today's summary.
    func testShowingATaskFromTodaysSummaryScrollsToIt() throws {
        let backlog = try createBacklog(count: backlogSize)
        // The running task has the latest start, so "Most recently logged first" puts it at the top.
        let running = backlog[backlogSize - 1]
        _ = try fixture.start(running)
        launch()
        scrollSidebarToBottom(showing: backlog[backlogSize - 2])
        // A row from the middle of the visible part, so no edge cuts it off.
        let visible = backlog[backlogSize - 6]
        let visibleTop = taskRow(visible).frame.minY
        select(visible)
        // The native list shifts by a point on a click, with or without the scroll to the selection.
        XCTAssertEqual(taskRow(visible).frame.minY, visibleTop, accuracy: 1,
                       "Clicking a fully visible row doesn't move the list.")
        XCTAssertFalse(isVisibleInSidebar(running), "The running task starts out of view.")
        let share = element("today-summary.share.\(running.id)")
        XCTAssertTrue(share.waitForExistence(timeout: timeout))
        share.click()
        waitUntil("The share selects the running task") { self.element("task-details.task.\(running.id)").exists }
        waitUntilVisibleInSidebar(running)
    }

    func testReturningToATabScrollsToItsRememberedTask() throws {
        let backlog = try createBacklog(count: backlogSize)
        let archived = try fixture.create("Archived release")
        try fixture.archive(archived)
        launch()
        let bottom = backlog[backlogSize - 1]
        scrollSidebarToBottom(showing: bottom)
        select(bottom)
        showTab("Archived")
        waitUntilVisibleInSidebar(archived)
        showTab("Active")
        waitUntil("The Active tab restores its selection") { self.element("task-details.task.\(bottom.id)").exists }
        waitUntilVisibleInSidebar(bottom)
    }
}
