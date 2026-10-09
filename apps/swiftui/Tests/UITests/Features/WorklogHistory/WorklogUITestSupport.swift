import XCTest

@MainActor
extension TrackerUITestCase {
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
        // The en_US picker exposes one element; its minute segment precedes AM/PM and the stepper.
        picker.coordinate(withNormalizedOffset: CGVector(dx: 0.70, dy: 0.5)).click()
        picker.typeKey(direction, modifierFlags: [])
        // Move past AM/PM to commit the edited segment without submitting the sheet.
        app.typeKey(.tab, modifierFlags: [])
        app.typeKey(.tab, modifierFlags: [])
    }
}
