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
}
