import AppKit
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

    func assertStatusTooltip(_ expected: String, file: StaticString = #filePath, line: UInt = #line) {
        trackerWindow.coordinate(withNormalizedOffset: CGVector(dx: 0.7, dy: 0.6)).hover()
        waitUntil("The previous tooltip closes", file: file, line: line) {
            !self.app.descendants(matching: .helpTag).firstMatch.exists
        }
        statusButton.hover()
        let tooltip = app.descendants(matching: .helpTag).firstMatch
        waitUntil("Hover shows the full tracking status", file: file, line: line) {
            guard tooltip.exists else { return false }
            return tooltip.label.components(separatedBy: "\n").first == expected
                || (tooltip.value as? String)?.components(separatedBy: "\n").first == expected
                || tooltip.descendants(matching: .any)
                    .matching(NSPredicate(
                        format: "label == %@ OR value == %@ OR label BEGINSWITH %@ OR value BEGINSWITH %@",
                        expected, expected, expected + "\n", expected + "\n")).firstMatch.exists
        }
        trackerWindow.coordinate(withNormalizedOffset: CGVector(dx: 0.7, dy: 0.6)).hover()
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
