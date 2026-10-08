import AppKit
import XCTest

@MainActor
extension TrackerUITestCase {
    var statusButton: XCUIElement { element("menu.status") }

    var trackerWindow: XCUIElement {
        app.windows.containing(.any, identifier: "tracker.window.content").firstMatch
    }

    func openSettings() {
        app.activate()
        app.typeKey(",", modifierFlags: .command)
        XCTAssertTrue(element("menu.show-daily-total").waitForExistence(timeout: timeout))
    }

    func showTrackerViaShortcut() {
        app.activate()
        app.typeKey("0", modifierFlags: .command)
        XCTAssertTrue(element("tracker.window.content").waitForExistence(timeout: timeout))
    }

    func openStatusMenu() {
        app.typeKey("t", modifierFlags: [.control, .option])
        XCTAssertTrue(app.menuItems["Open Time Tracker"].waitForExistence(timeout: timeout))
    }

    func dismissStatusMenu() {
        app.typeKey(.escape, modifierFlags: [])
        waitUntil("The native menu closes") { !self.app.menuItems["Open Time Tracker"].exists }
    }

    func menuTask(_ task: FixtureTask) -> XCUIElement { element("menu.task.\(task.id)") }

    @discardableResult
    func completedToday(_ task: FixtureTask, duration: TimeInterval = 61) throws -> FixtureWorklog {
        let now = Date()
        let startOfDay = Calendar.current.startOfDay(for: now)
        let available = now.timeIntervalSince(startOfDay)
        let end = now.addingTimeInterval(-min(60, available / 4))
        return try fixture.completed(task, start: end.addingTimeInterval(-min(duration, available / 2)), end: end)
    }

    func exactTodayDuration(_ task: FixtureTask) throws -> String {
        let now = Date()
        let day = try XCTUnwrap(Calendar.current.dateInterval(of: .day, for: now))
        let report = try fixture.reports(start: day.start, end: day.end, at: now)
        let seconds = (report.rows.first { $0.task.id == task.id }?.durationMicroseconds ?? 0) / 1_000_000
        if seconds >= 3600 { return "\(seconds / 3600)h \(seconds / 60 % 60)m \(seconds % 60)s" }
        if seconds >= 60 { return "\(seconds / 60)m \(seconds % 60)s" }
        return "\(seconds)s"
    }

    func copyFromMenu(_ action: String, task: FixtureTask) {
        let row = menuTask(task)
        XCTAssertTrue(row.waitForExistence(timeout: timeout))
        row.hover()
        let item = app.menuItems[action].firstMatch
        XCTAssertTrue(item.waitForExistence(timeout: timeout))
        XCTAssertTrue(item.isEnabled)
        item.click()
    }

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

    func withRestoredClipboard(_ body: () throws -> Void) rethrows {
        let pasteboard = NSPasteboard.general
        let saved = (pasteboard.pasteboardItems ?? []).map { item in
            item.types.compactMap { type in item.data(forType: type).map { (type, $0) } }
        }
        let restore: @MainActor () -> Void = {
            let pasteboard = NSPasteboard.general
            pasteboard.clearContents()
            let items = saved.map { values -> NSPasteboardItem in
                let item = NSPasteboardItem()
                for (type, data) in values { item.setData(data, forType: type) }
                return item
            }
            pasteboard.writeObjects(items)
        }
        addTeardownBlock { await restore() }
        defer { restore() }
        try body()
    }
}
