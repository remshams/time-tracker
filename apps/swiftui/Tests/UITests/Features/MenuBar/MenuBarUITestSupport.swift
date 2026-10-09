import AppKit
import XCTest

@MainActor
extension TrackerUITestCase {
    var statusButton: XCUIElement { element("menu.status") }

    func openStatusMenu() {
        app.typeKey("t", modifierFlags: [.control, .option])
        XCTAssertTrue(app.menuItems["Open Time Tracker"].waitForExistence(timeout: timeout))
    }

    func dismissStatusMenu() {
        app.typeKey(.escape, modifierFlags: [])
        waitUntil("The native menu closes") { !self.app.menuItems["Open Time Tracker"].exists }
    }

    func menuTask(_ task: FixtureTask) -> XCUIElement { element("menu.task.\(task.id)") }

    func copyFromMenu(_ action: String, task: FixtureTask) {
        let row = menuTask(task)
        XCTAssertTrue(row.waitForExistence(timeout: timeout))
        row.hover()
        let item = app.menuItems[action].firstMatch
        XCTAssertTrue(item.waitForExistence(timeout: timeout))
        XCTAssertTrue(item.isEnabled)
        item.click()
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
