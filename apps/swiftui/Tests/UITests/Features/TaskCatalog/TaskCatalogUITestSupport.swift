import XCTest

@MainActor
extension TrackerUITestCase {
    var taskDetailsName: XCUIElement {
        let heading = element("task-details.name")
        let nativeText = heading.staticTexts.firstMatch
        return nativeText.exists ? nativeText : heading
    }

    var taskDetailsNameText: String {
        let text = taskDetailsName
        return text.value as? String ?? text.label
    }

    /// Waits until the sidebar shows `tasks` from top to bottom.
    func waitForSidebarOrder(_ tasks: [FixtureTask], file: StaticString = #filePath, line: UInt = #line) {
        for task in tasks { XCTAssertTrue(taskRow(task).waitForExistence(timeout: timeout), file: file, line: line) }
        waitUntil("The sidebar lists \(tasks.map(\.name))", file: file, line: line) {
            let tops = tasks.map { self.taskRow($0).frame.minY }
            return Set(tops).count == tops.count && tops == tops.sorted()
        }
    }

    var taskSidebar: XCUIElement { element("task-sidebar.list") }

    /// Creates `count` tasks without worklogs, named so their order in the sidebar follows the creation order.
    func createBacklog(count: Int) throws -> [FixtureTask] {
        try (1...count).map { try fixture.create(String(format: "Backlog %02d", $0)) }
    }

    /// Whether the sidebar shows the whole row of `task`.
    func isVisibleInSidebar(_ task: FixtureTask) -> Bool {
        let row = taskRow(task)
        return row.exists && row.isHittable && taskSidebar.frame.contains(row.frame)
    }

    func waitUntilVisibleInSidebar(_ task: FixtureTask, file: StaticString = #filePath, line: UInt = #line) {
        waitUntil("The sidebar shows \(task.name)", file: file, line: line) { self.isVisibleInSidebar(task) }
    }

    /// Scrolls the sidebar to its end and waits until it shows `last`, the task at the bottom.
    func scrollSidebarToBottom(showing last: FixtureTask, file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertTrue(taskSidebar.waitForExistence(timeout: timeout), file: file, line: line)
        taskSidebar.scroll(byDeltaX: 0, deltaY: -10_000)
        waitUntilVisibleInSidebar(last, file: file, line: line)
    }
}
