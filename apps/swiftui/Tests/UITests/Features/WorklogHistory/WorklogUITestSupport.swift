import XCTest

@MainActor
extension TrackerUITestCase {
    /// Opens the worklog's context menu once `available` shows as enabled in it. The menu reads the dialogs' state when
    /// it opens, so a menu that opened too early closes and opens again.
    func openWorklogMenu(_ log: FixtureWorklog, until available: String, file: StaticString = #filePath,
                         line: UInt = #line) {
        let row = element("worklog-history.row.\(log.id)")
        XCTAssertTrue(row.waitForExistence(timeout: timeout), file: file, line: line)
        let item = app.menuItems[available]
        let deadline = Date().addingTimeInterval(timeout)
        repeat {
            row.rightClick()
            if item.waitForExistence(timeout: 2), item.isEnabled { return }
            app.typeKey(.escape, modifierFlags: [])
        } while Date() < deadline
        XCTFail("\(available) did not become available in the worklog menu.", file: file, line: line)
    }

    func openWorklogAction(_ log: FixtureWorklog, title: String, sheetID: String) {
        openWorklogMenu(log, until: title)
        app.menuItems[title].click()
        XCTAssertTrue(element(sheetID).waitForExistence(timeout: timeout))
    }

    func openCorrection(_ log: FixtureWorklog) {
        openWorklogAction(log, title: "Edit times...", sheetID: "worklog-correction.start")
    }

    func openMove(_ log: FixtureWorklog) {
        openWorklogAction(log, title: "Move to task...", sheetID: "worklog-move.search")
    }

    /// Waits until the history row reads the stored duration of a completed worklog.
    /// A new duration proves that the history reloaded after another client changed the times.
    /// A worklog that crosses midnight shows its part on its start day in that row.
    func waitForHistoryDuration(of log: FixtureWorklog, file: StaticString = #filePath, line: UInt = #line) {
        let fractional = ISO8601DateFormatter()
        fractional.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        // The server omits the fraction for whole seconds.
        let date = { (value: String) in fractional.date(from: value) ?? ISO8601DateFormatter().date(from: value) }
        guard let start = date(log.start), let end = log.end.flatMap(date) else {
            XCTFail("Worklog \(log.id) has no completed timestamps.", file: file, line: line)
            return
        }
        let midnight = Calendar.current.dateInterval(of: .day, for: start)?.end ?? end
        let duration = spokenDuration(Int(max(0, min(end, midnight).timeIntervalSince(start))))
        let summary = element("worklog-history.summary.\(log.id)")
        waitUntil("History reads the stored duration \(duration)", file: file, line: line) {
            summary.exists && summary.label.hasSuffix(", \(duration)")
        }
    }

    /// A duration as VoiceOver reads it in the window, such as "1 hour, 4 minutes". Seconds count only under a minute.
    func spokenDuration(_ seconds: Int) -> String {
        guard seconds >= 60 else { return "\(seconds) second\(seconds == 1 ? "" : "s")" }
        let hours = seconds / 3600
        let minutes = seconds / 60 % 60
        let parts = [
            hours > 0 ? "\(hours) hour\(hours == 1 ? "" : "s")" : nil,
            minutes > 0 ? "\(minutes) minute\(minutes == 1 ? "" : "s")" : nil,
        ]
        return parts.compactMap { $0 }.joined(separator: ", ")
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
