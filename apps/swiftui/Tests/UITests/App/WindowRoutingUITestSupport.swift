import AppKit
import XCTest

@MainActor
extension TrackerUITestCase {
    var trackerWindows: XCUIElementQuery { app.windows.containing(.button, identifier: "bulk-archive.open") }

    var trackerWindow: XCUIElement { trackerWindows.firstMatch }

    func showTrackerViaShortcut() {
        app.activate()
        app.typeKey("0", modifierFlags: .command)
        XCTAssertTrue(trackerWindow.waitForExistence(timeout: timeout))
    }
}
