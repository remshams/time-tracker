import XCTest

@MainActor
extension TrackerUITestCase {
    func openRename(_ task: FixtureTask, contextMenu: Bool = false) {
        if contextMenu {
            taskRow(task).rightClick()
            app.menuItems["Edit name"].click()
        } else {
            select(task)
            app.buttons["Edit task name"].click()
        }
        XCTAssertTrue(element("task-name.input").waitForExistence(timeout: timeout))
    }
}
