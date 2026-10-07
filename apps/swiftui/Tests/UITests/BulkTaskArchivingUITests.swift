import XCTest

final class BulkTaskArchivingUITests: XCTestCase {
    private var app: XCUIApplication!
    private var fixture: ArchiveFixture!
    private var seed: ArchiveSeed!
    private let timeout: TimeInterval = 15

    override func setUpWithError() throws {
        continueAfterFailure = false
        fixture = try ArchiveFixture()
        seed = try fixture.seed()
        app = XCUIApplication()
        app.launchArguments = ["-tt-ui-testing", "-ApplePersistenceIgnoreState", "YES",
                               "-AppleLanguages", "(en)", "-AppleLocale", "en_US"]
        app.launchEnvironment["TT_UI_TEST_DATABASE_PATH"] = fixture.database.path
        app.launchEnvironment["TT_UI_TEST_DEFAULTS_SUITE"] = fixture.defaultsSuite
        launch()
    }

    override func tearDownWithError() throws {
        if let app, testRun?.hasSucceeded == false {
            if app.windows.firstMatch.exists {
                let screenshot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
                screenshot.name = "Bulk archive failure"
                screenshot.lifetime = .keepAlways
                add(screenshot)
            }
            let hierarchy = XCTAttachment(string: app.windows.firstMatch.exists
                                          ? app.windows.firstMatch.debugDescription
                                          : "No tracker window exists.")
            hierarchy.name = "Tracker window accessibility hierarchy"
            hierarchy.lifetime = .keepAlways
            add(hierarchy)
        }
        app?.terminate()
        try fixture?.cleanup()
        app = nil
        fixture = nil
        seed = nil
    }

    func testDefaultPreviewExcludesRecentAndRunningTasksAndCancelPreservesStorage() throws {
        openDialog()
        XCTAssertEqual(days.value as? String, "14")
        assertCandidates([seed.old, seed.medium], count: 2)
        XCTAssertTrue(confirm.isEnabled)
        cancelDialog()
        try assertArchived([])
        XCTAssertEqual(try fixture.activeWorklog(), seed.worklog)
    }

    func testChangingPeriodAndRefreshingShowsUpdatedCandidates() throws {
        openDialog()
        assertCandidates([seed.old, seed.medium], count: 2)
        replaceDays("30")
        assertCandidates([seed.old], count: 1)
        let added = try fixture.create("Additional inactive task", at: Date().addingTimeInterval(-50 * 86_400))
        app.buttons["bulk-archive.refresh"].click()
        assertCandidates([seed.old, added], count: 2)
        cancelDialog()
    }

    func testInvalidPeriodsDisableConfirmationAndValidInputRecovers() throws {
        openDialog()
        assertCandidates([seed.old, seed.medium], count: 2)
        for invalid in ["0", "-1", "abc", "1.5", "4294967296"] {
            replaceDays(invalid)
            let error = element("bulk-archive.error")
            XCTAssertTrue(error.waitForExistence(timeout: timeout))
            XCTAssertFalse(confirm.isEnabled, "Archive must be disabled for \(invalid).")
        }
        replaceDays("14")
        assertCandidates([seed.old, seed.medium], count: 2)
        XCTAssertTrue(confirm.isEnabled)
        cancelDialog()
        try assertArchived([])
    }

    func testEmptyPreviewCannotBeConfirmed() throws {
        openDialog()
        replaceDays("45")
        XCTAssertTrue(element("bulk-archive.empty").waitForExistence(timeout: timeout))
        XCTAssertEqual(element("bulk-archive.count").value as? String, "0 tasks to archive")
        XCTAssertFalse(confirm.isEnabled)
        cancelDialog()
        try assertArchived([])
    }

    func testConfirmArchivesPreviewAndPreservesRunningTimerAcrossRelaunch() throws {
        openDialog()
        assertCandidates([seed.old, seed.medium], count: 2)
        confirm.click()
        assertResult("Archived 2 tasks.")
        cancelDialog()
        try assertArchived([seed.old, seed.medium])
        try assertRunningTimer()
        assertSidebar([seed.recent, seed.running], excluded: [seed.old, seed.medium])
        showTab("Archived")
        assertSidebar([seed.old, seed.medium], excluded: [seed.recent, seed.running])

        app.terminate()
        launch()
        showTab("Archived")
        assertSidebar([seed.old, seed.medium], excluded: [seed.recent, seed.running])
        showTab("Active")
        assertSidebar([seed.recent, seed.running], excluded: [seed.old, seed.medium])
        try assertRunningTimer()
        try assertArchived([seed.old, seed.medium])
    }

    func testConfirmationUsesChangedPeriod() throws {
        openDialog()
        replaceDays("30")
        assertCandidates([seed.old], count: 1)
        confirm.click()
        assertResult("Archived 1 task.")
        cancelDialog()
        try assertArchived([seed.old])
        assertSidebar([seed.medium, seed.recent, seed.running], excluded: [seed.old])
        try assertRunningTimer()
    }

    private var days: XCUIElement { app.textFields["bulk-archive.days"] }
    private var confirm: XCUIElement { app.buttons["bulk-archive.confirm"] }

    private func element(_ identifier: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: identifier).firstMatch
    }

    private func launch() {
        app.launch()
        app.activate()
        let windowMenu = app.menuBars.menuBarItems["Window"]
        XCTAssertTrue(windowMenu.waitForExistence(timeout: timeout))
        windowMenu.click()
        let showTracker = app.menuItems["Show Time Tracker"]
        XCTAssertTrue(showTracker.waitForExistence(timeout: timeout))
        showTracker.click()
        let open = app.buttons["bulk-archive.open"]
        XCTAssertTrue(open.waitForExistence(timeout: timeout), "The tracker window did not open.")
        waitUntil("Archive action becomes available") { open.isEnabled }
    }

    private func openDialog() {
        app.buttons["bulk-archive.open"].click()
        XCTAssertTrue(days.waitForExistence(timeout: timeout))
    }

    private func replaceDays(_ text: String) {
        days.click()
        days.typeKey("a", modifierFlags: .command)
        days.typeText(text)
    }

    private func assertCandidates(_ expected: [FixtureTask], count: Int, file: StaticString = #filePath, line: UInt = #line) {
        let text = "\(count) \(count == 1 ? "task" : "tasks") to archive"
        waitUntil("Preview displays \(text)", file: file, line: line) {
            let value = self.element("bulk-archive.count")
            return value.exists && value.value as? String == text && expected.allSatisfy {
                self.element("bulk-archive.candidate.\($0.id)").exists
            }
        }
        let expectedIDs = Set(expected.map(\.id))
        for task in [seed.old, seed.medium, seed.recent, seed.running] where !expectedIDs.contains(task.id) {
            XCTAssertFalse(element("bulk-archive.candidate.\(task.id)").exists, "Unexpected candidate \(task.name).", file: file, line: line)
        }
    }

    private func assertResult(_ text: String) {
        waitUntil("Archive reports its actual count") {
            let result = self.element("bulk-archive.result")
            guard result.exists else { return false }
            return result.value as? String == text
                || result.staticTexts.matching(NSPredicate(format: "value == %@", text)).firstMatch.exists
        }
    }

    private func cancelDialog() {
        app.buttons["bulk-archive.cancel"].click()
        waitUntil("Archive dialog closes") { !self.days.exists }
    }

    private func showTab(_ name: String) {
        let tab = element("task-sidebar.tab.\(name)")
        XCTAssertTrue(tab.waitForExistence(timeout: timeout))
        tab.click()
    }

    private func assertSidebar(_ expected: [FixtureTask], excluded: [FixtureTask]) {
        waitUntil("Sidebar displays the selected task state") {
            expected.allSatisfy { self.element("task-sidebar.task.\($0.id)").exists }
                && excluded.allSatisfy { !self.element("task-sidebar.task.\($0.id)").exists }
        }
    }

    private func assertArchived(_ expected: [FixtureTask], file: StaticString = #filePath, line: UInt = #line) throws {
        let tasks = try fixture.tasks()
        XCTAssertEqual(Set(tasks.map(\.id)), Set([seed.old, seed.medium, seed.recent, seed.running].map(\.id)), file: file, line: line)
        XCTAssertEqual(Set(tasks.filter(\.archived).map(\.id)), Set(expected.map(\.id)), file: file, line: line)
    }

    private func assertRunningTimer() throws {
        XCTAssertEqual(try fixture.activeWorklog(), seed.worklog, "Archiving must preserve the running worklog's identity and start.")
        let indicator = element("tracking.active-task")
        XCTAssertTrue(indicator.waitForExistence(timeout: timeout))
        XCTAssertEqual(indicator.label, "Tracking")
        element("task-sidebar.task.\(seed.running.id)").click()
        XCTAssertTrue(app.buttons["Stop tracking"].waitForExistence(timeout: timeout))
    }

    private func waitUntil(_ description: String, file: StaticString = #filePath, line: UInt = #line, condition: @escaping () -> Bool) {
        let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [expectation], timeout: timeout), .completed, description, file: file, line: line)
    }
}
