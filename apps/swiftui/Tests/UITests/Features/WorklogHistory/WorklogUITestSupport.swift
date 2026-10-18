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

    /// Waits until the history row shows the stored duration of a completed worklog.
    /// A new duration proves that the history reloaded after another client changed the times.
    func waitForHistoryDuration(of log: FixtureWorklog, file: StaticString = #filePath, line: UInt = #line) {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        guard let start = formatter.date(from: log.start), let end = log.end.flatMap(formatter.date(from:)) else {
            XCTFail("Worklog \(log.id) has no completed timestamps.", file: file, line: line)
            return
        }
        let total = Int(max(0, end.timeIntervalSince(start)))
        let duration = String(format: "%02d:%02d:%02d", total / 3600, total / 60 % 60, total % 60)
        let row = element("worklog-history.row.\(log.id)")
        waitUntil("History shows the stored duration \(duration)", file: file, line: line) {
            row.staticTexts[duration].exists
        }
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
