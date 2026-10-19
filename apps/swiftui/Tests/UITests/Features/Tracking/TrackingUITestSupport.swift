import AppKit
import XCTest

@MainActor
extension TrackerUITestCase {
    func assertRunning(_ task: FixtureTask, file: StaticString = #filePath, line: UInt = #line) {
        waitUntil("The backend tracks \(task.name)", file: file, line: line) {
            (try? self.fixture.activeWorklog())?.taskID == task.id
        }
    }

    func assertStopped(file: StaticString = #filePath, line: UInt = #line) {
        waitUntil("The backend timer stops", file: file, line: line) {
            do { return try self.fixture.activeWorklog() == nil } catch { return false }
        }
    }

    /// Checks the rejected tracking command's alert, then closes it.
    func dismissTrackingFailure(containing text: String, file: StaticString = #filePath, line: UInt = #line) {
        let title = app.staticTexts["Could not change tracking"]
        waitUntil("The tracking failure alert appears", file: file, line: line) { title.exists }
        let message = app.staticTexts.matching(
            NSPredicate(format: "value CONTAINS[c] %@ OR label CONTAINS[c] %@", text, text)).firstMatch
        XCTAssertTrue(message.exists, "The alert shows the reported failure.", file: file, line: line)
        // Query OK inside the alert. An app-wide button query also matches the Touch Bar copy, which cannot be
        // clicked, and Return reaches the alert only when its window is key.
        let buttons = [app.sheets, app.dialogs].map { $0.buttons["OK"].firstMatch }
        waitUntil("The tracking failure alert offers OK", file: file, line: line) { buttons.contains { $0.exists } }
        // The first click on an inactive window only activates it, so allow one more click.
        let closed = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"), object: title)
        for _ in 0..<2 where title.exists {
            buttons.first { $0.exists }?.click()
            _ = XCTWaiter.wait(for: [closed], timeout: 2)
        }
        waitUntil("The tracking failure alert closes", file: file, line: line) { !title.exists }
    }
}
