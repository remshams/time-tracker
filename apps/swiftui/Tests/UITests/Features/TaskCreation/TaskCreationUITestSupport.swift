import XCTest

@MainActor
extension TrackerUITestCase {
    func openCreation() {
        let button = app.buttons["New task"]
        waitUntil("Creation becomes available") { button.exists && button.isEnabled }
        button.click()
        XCTAssertTrue(element("task-name.input").waitForExistence(timeout: timeout))
    }
}
