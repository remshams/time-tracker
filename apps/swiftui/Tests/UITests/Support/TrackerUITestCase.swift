import Foundation
import XCTest

@MainActor
class TrackerUITestCase: XCTestCase {
    var app: XCUIApplication!
    var fixture: TrackerFixture!
    var fixtureSource: TrackerFixture.Source { .local }
    let timeout: TimeInterval = 15
    var additionalLaunchArguments: [String] = []

    override func setUpWithError() throws {
        continueAfterFailure = false
        fixture = try TrackerFixture(source: fixtureSource)
        app = XCUIApplication()
    }

    override func tearDownWithError() throws {
        if testRun?.hasSucceeded == false {
            if let app {
                for window in app.windows.allElementsBoundByIndex where window.exists {
                    let screenshot = XCTAttachment(screenshot: window.screenshot())
                    screenshot.name = "Failed UI test window"
                    screenshot.lifetime = .keepAlways
                    add(screenshot)
                }
                attach(app.debugDescription, named: "Application accessibility hierarchy")
            }
            if let fixture {
                let files = (try? FileManager.default.contentsOfDirectory(at: fixture.directory,
                            includingPropertiesForKeys: nil)) ?? []
                for file in files where ["log", "json"].contains(file.pathExtension) {
                    if let data = try? Data(contentsOf: file) {
                        attach(String(decoding: data, as: UTF8.self), named: file.lastPathComponent)
                    }
                }
            }
        }
        app?.terminate()
        try fixture?.cleanup()
        app = nil
        fixture = nil
    }

    func launch() {
        app.launchArguments = ["-tt-ui-testing", "-ApplePersistenceIgnoreState", "YES",
                               "-AppleLanguages", "(en)", "-AppleLocale", "en_US"] + additionalLaunchArguments
        app.launchEnvironment["TT_UI_TEST_DATABASE_PATH"] = fixture.database.path
        app.launchEnvironment["TT_UI_TEST_DEFAULTS_SUITE"] = fixture.defaultsSuite
        app.launch()
        app.activate()
        showTrackerWindow()
        waitUntil("Initial connection finishes") {
            let status = self.element("connection-summary.status")
            guard status.exists else { return false }
            let text = status.value as? String ?? status.label
            return ["Local database", "Connected", "Unavailable", "Incompatible server"].contains(text)
        }
        if ["Local database", "Connected"].contains(elementText("connection-summary.status")) {
            waitUntil("Initial application controls become available") {
                self.app.buttons["New task"].exists && self.app.buttons["New task"].isEnabled
            }
        }
    }

    func relaunch() { app.terminate(); launch() }

    func showTrackerWindow() {
        app.activate()
        let windowMenu = app.menuBars.menuBarItems["Window"]
        XCTAssertTrue(windowMenu.waitForExistence(timeout: timeout))
        windowMenu.click()
        let showTracker = app.menuItems["Show Time Tracker"]
        XCTAssertTrue(showTracker.waitForExistence(timeout: timeout))
        showTracker.click()
        XCTAssertTrue(app.buttons["bulk-archive.open"].waitForExistence(timeout: timeout),
                      "The tracker window did not open.")
    }

    func element(_ identifier: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: identifier).firstMatch
    }
    func taskRow(_ task: FixtureTask) -> XCUIElement { element("task-sidebar.task.\(task.id)") }
    func select(_ task: FixtureTask) {
        let row = taskRow(task)
        XCTAssertTrue(row.waitForExistence(timeout: timeout))
        row.click()
        waitUntil("Selected task appears") {
            self.element("task-details.task.\(task.id)").exists
        }
    }
    func selectTask(_ task: FixtureTask) { select(task) }
    func showTab(_ name: String) {
        let tab = element("task-sidebar.tab.\(name)")
        XCTAssertTrue(tab.waitForExistence(timeout: timeout))
        tab.click()
    }
    var historyRows: XCUIElementQuery {
        app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "worklog-history.row."))
    }
    func replaceText(_ field: XCUIElement, _ text: String) {
        XCTAssertTrue(field.waitForExistence(timeout: timeout))
        field.click()
        field.typeKey("a", modifierFlags: .command)
        if text.isEmpty { field.typeKey(XCUIKeyboardKey.delete, modifierFlags: []) }
        else { field.typeText(text) }
    }
    func waitUntil(_ description: String, file: StaticString = #filePath, line: UInt = #line,
                   condition: @escaping () -> Bool) {
        let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [expectation], timeout: timeout), .completed,
                       description, file: file, line: line)
    }
    private func attach(_ text: String, named name: String) {
        let attachment = XCTAttachment(string: text)
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
