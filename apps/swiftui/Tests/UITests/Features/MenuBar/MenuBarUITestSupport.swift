import AppKit
import XCTest

enum MenuCopyAction: String {
    case copyName, copyExact, copyRounded

    var stepsAfterTracking: Int {
        switch self {
        case .copyName: return 1
        case .copyExact: return 2
        case .copyRounded: return 3
        }
    }
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

    /// Reads Start tracking from top to bottom. It requires that no task has time logged today.
    func startTrackingOrder(_ tasks: [FixtureTask]) -> [String] {
        openStatusMenu()
        app.typeKey(.downArrow, modifierFlags: [])
        app.typeKey(.rightArrow, modifierFlags: [])
        for task in tasks { XCTAssertTrue(menuTask(task).waitForExistence(timeout: timeout)) }
        let order = tasks.sorted { menuTask($0).frame.minY < menuTask($1).frame.minY }.map(\.name)
        app.typeKey(.leftArrow, modifierFlags: [])
        dismissStatusMenu()
        return order
    }

    func chooseMenuTaskOrder(_ title: String) {
        openSettings()
        let picker = element("menu.task-order")
        picker.click()
        let item = app.menuItems[title].firstMatch
        XCTAssertTrue(item.waitForExistence(timeout: timeout))
        item.click()
        waitUntil("Settings selects \(title)") { picker.value as? String == title }
    }

    func copyMenuItem(_ action: MenuCopyAction, task: FixtureTask) -> XCUIElement {
        menuTask(task).menuItems["menu.copy.\(task.id).\(action.rawValue)"]
    }

    func copyFromMenu(_ action: MenuCopyAction, task: FixtureTask) {
        let row = menuTask(task)
        XCTAssertTrue(row.waitForExistence(timeout: timeout))
        row.hover()
        activateMenuCopy(action, task: task)
    }

    func activateMenuCopy(_ action: MenuCopyAction, task: FixtureTask) {
        let item = copyMenuItem(action, task: task)
        XCTAssertTrue(item.waitForExistence(timeout: timeout))
        XCTAssertTrue(item.isEnabled)
        let trackingEnabled = ["Start tracking", "Stop tracking"].contains { title in
            let tracking = menuTask(task).menuItems[title]
            return tracking.exists && tracking.isEnabled
        }
        let clipboardBefore = NSPasteboard.general.changeCount
        app.typeKey(.rightArrow, modifierFlags: [])
        for _ in 0..<(action.stepsAfterTracking - (trackingEnabled ? 0 : 1)) {
            app.typeKey(.downArrow, modifierFlags: [])
        }
        app.typeKey(.return, modifierFlags: [])
        waitUntil("The native copy action closes the menu") {
            !self.app.menuItems["Open Time Tracker"].exists
        }
        XCTAssertGreaterThan(NSPasteboard.general.changeCount, clipboardBefore,
                             "The \(action.rawValue) command must write to the clipboard")
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
