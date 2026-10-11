import AppKit
import CoreGraphics
import XCTest

@MainActor
extension TrackerUITestCase {
    var statusButton: XCUIElement { element("menu.status") }

    func chooseMenuBarDisplay(_ label: String) {
        element("menu.display").click()
        let item = app.menuItems[label].firstMatch
        XCTAssertTrue(item.waitForExistence(timeout: timeout))
        item.click()
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

    func copyFromMenu(_ action: String, task: FixtureTask) {
        let row = menuTask(task)
        XCTAssertTrue(row.waitForExistence(timeout: timeout))
        row.hover()
        let item = row.menuItems[action].firstMatch
        XCTAssertTrue(item.waitForExistence(timeout: timeout))
        XCTAssertTrue(item.isEnabled)
        clickTaskSubmenuItem(item, task: task)
    }

    func clickTaskSubmenuItem(_ item: XCUIElement, task: FixtureTask) {
        waitUntil("The task submenu action is reachable") { item.exists && item.isHittable }
        let row = menuTask(task)
        let actionFrame = item.frame
        let rowFrame = row.frame
        // Enter the submenu at the parent row's height before moving to the action.
        // A diagonal move can cross another parent row when the submenu opens to the left.
        screenCoordinate(at: CGPoint(x: actionFrame.midX, y: rowFrame.midY)).hover()
        screenCoordinate(at: CGPoint(x: actionFrame.midX, y: actionFrame.midY)).click()
    }

    private func screenCoordinate(at point: CGPoint) -> XCUICoordinate {
        let window = trackerWindow
        let frame = window.frame
        return window.coordinate(withNormalizedOffset: .zero)
            .withOffset(CGVector(dx: point.x - frame.minX, dy: point.y - frame.minY))
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
