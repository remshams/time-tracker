import XCTest

@MainActor
extension TrackerUITestCase {
    func openCreation() {
        let button = app.buttons["New task"]
        waitUntil("Creation becomes available") { button.exists && button.isEnabled }
        button.click()
        XCTAssertTrue(element("task-name.input").waitForExistence(timeout: timeout))
    }

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

    func openWorklogAction(_ log: FixtureWorklog, title: String, sheetID: String) {
        let actions = element("worklog-history.actions.\(log.id)")
        XCTAssertTrue(actions.waitForExistence(timeout: timeout))
        waitUntil("Worklog actions become available") { actions.isEnabled }
        actions.click()
        app.menuItems[title].click()
        XCTAssertTrue(element(sheetID).waitForExistence(timeout: timeout))
    }

    func openCorrection(_ log: FixtureWorklog) {
        openWorklogAction(log, title: "Edit times...", sheetID: "worklog-correction.start")
    }

    func openMove(_ log: FixtureWorklog) {
        openWorklogAction(log, title: "Move to task...", sheetID: "worklog-move.search")
    }

    func stepMinute(_ identifier: String, direction: XCUIKeyboardKey = .upArrow) {
        let picker = element(identifier)
        picker.click()
        for _ in 0..<8 { picker.typeKey(.leftArrow, modifierFlags: []) }
        for _ in 0..<4 { picker.typeKey(.rightArrow, modifierFlags: []) }
        picker.typeKey(direction, modifierFlags: [])
        app.typeKey(.tab, modifierFlags: [])
    }
}
