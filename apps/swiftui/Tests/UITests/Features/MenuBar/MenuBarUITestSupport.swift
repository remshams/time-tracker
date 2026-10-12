import AppKit
import XCTest

enum MenuCopyAction: String {
    case copyName, copyExact, copyRounded
}

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

    func copyMenuItem(_ action: MenuCopyAction, task: FixtureTask) -> XCUIElement {
        menuTask(task).menuItems["menu.copy.\(task.id).\(action.rawValue)"]
    }

    func copyFromMenu(_ action: MenuCopyAction, task: FixtureTask) {
        let row = menuTask(task)
        XCTAssertTrue(row.waitForExistence(timeout: timeout))
        row.hover()
        let item = copyMenuItem(action, task: task)
        activateMenuCopy(item)
    }

    func activateMenuCopy(_ item: XCUIElement) {
        XCTAssertTrue(item.waitForExistence(timeout: timeout))
        XCTAssertTrue(item.isEnabled)
        item.hover()
        app.typeKey(.return, modifierFlags: [])
        waitUntil("The native copy action closes the menu") {
            !self.app.menuItems["Open Time Tracker"].exists
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
        pasteboard.clearContents()
        pasteboard.setString("Clipboard before copying", forType: .string)
        try body()
    }
}
