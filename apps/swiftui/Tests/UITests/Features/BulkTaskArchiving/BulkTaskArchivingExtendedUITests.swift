import XCTest

class BulkTaskArchivingExtendedUITests: TrackerUITestCase {
    func testChangedCandidatesRejectConfirmationAndRequireFreshPreview() throws {
        let old = try fixture.create("Reviewed inactive task", at: Date().addingTimeInterval(-40 * 86400))
        launch()
        openBulkPreview()
        XCTAssertTrue(element("bulk-archive.candidate.\(old.id)").waitForExistence(timeout: timeout))
        let added = try fixture.create("New inactive candidate", at: Date().addingTimeInterval(-45 * 86400))
        app.buttons["bulk-archive.confirm"].click()
        XCTAssertTrue(element("bulk-archive.error").waitForExistence(timeout: timeout))
        XCTAssertTrue(try fixture.tasks().allSatisfy { !$0.archived })
        XCTAssertFalse(app.buttons["bulk-archive.confirm"].isEnabled)
        app.buttons["bulk-archive.refresh"].click()
        XCTAssertTrue(element("bulk-archive.candidate.\(added.id)").waitForExistence(timeout: timeout))
        waitUntil("Fresh preview permits confirmation") { self.app.buttons["bulk-archive.confirm"].isEnabled }
        app.buttons["bulk-archive.confirm"].click()
        XCTAssertTrue(element("bulk-archive.result").waitForExistence(timeout: timeout))
        XCTAssertEqual(Set(try fixture.tasks().filter(\.archived).map(\.id)), Set([old.id, added.id]))
    }

    func testPreviewFromArchivedTabIncludesInactiveActiveTasksAndPreservesTimer() throws {
        let old = try fixture.create("Inactive active task", at: Date().addingTimeInterval(-40 * 86400))
        let archived = try fixture.create("Already archived task", at: Date().addingTimeInterval(-45 * 86400))
        _ = try fixture.archive(archived)
        let running = try fixture.create("Protected active task", at: Date().addingTimeInterval(-40 * 86400))
        let timer = try fixture.start(running)
        launch()
        showTab("Archived")
        select(archived)
        openBulkPreview()
        XCTAssertTrue(element("bulk-archive.candidate.\(old.id)").waitForExistence(timeout: timeout))
        XCTAssertFalse(element("bulk-archive.candidate.\(archived.id)").exists)
        XCTAssertFalse(element("bulk-archive.candidate.\(running.id)").exists)
        app.buttons["bulk-archive.confirm"].click()
        XCTAssertTrue(element("bulk-archive.result").waitForExistence(timeout: timeout))
        app.buttons["bulk-archive.cancel"].click()
        XCTAssertTrue(taskRow(old).waitForExistence(timeout: timeout))
        XCTAssertEqual(try fixture.activeWorklog(), timer)
        relaunch()
        showTab("Archived")
        XCTAssertTrue(taskRow(old).exists)
        XCTAssertEqual(try fixture.activeWorklog(), timer)
    }

    func testBulkArchiveSheetPreventsAnotherCreationSheet() throws {
        _ = try fixture.create("Old task", at: Date().addingTimeInterval(-40 * 86400))
        launch()
        openBulkPreview()
        app.typeKey("n", modifierFlags: .command)
        XCTAssertEqual(app.sheets.count, 1)
        XCTAssertFalse(element("task-name.input").exists)
        app.typeKey(.escape, modifierFlags: [])
        waitUntil("Escape cancels bulk archive") { !self.element("bulk-archive.days").exists }
        XCTAssertTrue(try fixture.tasks().allSatisfy { !$0.archived })
    }

    func openBulkPreview() {
        let open = app.buttons["bulk-archive.open"]
        waitUntil("Bulk archive becomes available") { open.exists && open.isEnabled }
        open.click()
        XCTAssertTrue(element("bulk-archive.days").waitForExistence(timeout: timeout))
    }
}

final class ServerBulkTaskArchivingUITests: BulkTaskArchivingExtendedUITests {
    override var fixtureSource: TrackerFixture.Source { .server }

    func testLostBulkArchiveResponseRefreshesAuthoritativeStateWithoutResubmission() throws {
        let old = try fixture.create("Committed bulk archive", at: Date().addingTimeInterval(-40 * 86400))
        let running = try fixture.create("Protected running task")
        let timer = try fixture.start(running)
        let proxy = try fixture.enableProxy()
        launch()
        openBulkPreview()
        XCTAssertTrue(element("bulk-archive.candidate.\(old.id)").waitForExistence(timeout: timeout))
        try proxy.dropWriteResponses(method: "POST", path: "/v1/tasks/archive-inactive")
        app.buttons["bulk-archive.confirm"].click()
        XCTAssertTrue(element("bulk-archive.error").waitForExistence(timeout: timeout))
        XCTAssertFalse(app.buttons["bulk-archive.confirm"].isEnabled)
        XCTAssertTrue(try XCTUnwrap(fixture.tasks().first { $0.id == old.id }).archived)
        XCTAssertEqual(try fixture.activeWorklog(), timer)
        try assertAcceptedWriteIntent(proxy, method: "POST", path: "/v1/tasks/archive-inactive")
        app.buttons["bulk-archive.refresh"].click()
        XCTAssertTrue(element("bulk-archive.empty").waitForExistence(timeout: timeout))
        try assertAcceptedWriteIntent(proxy, method: "POST", path: "/v1/tasks/archive-inactive")
    }
}
