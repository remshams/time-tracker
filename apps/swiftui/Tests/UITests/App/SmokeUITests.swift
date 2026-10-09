import XCTest

@MainActor
final class SmokeUITests: TrackerUITestCase {
    func testEmptyDatabaseLaunchShowsBothEmptyTaskTabs() {
        launch()
        XCTAssertTrue(element("task-sidebar.empty").waitForExistence(timeout: timeout))
        showTab("Archived")
        XCTAssertTrue(element("task-sidebar.empty").exists)
        XCTAssertEqual(historyRows.count, 0)
    }

    func testSeededTaskLoadsItsStoredWorklogThroughTheProductionBridge() throws {
        let task = try fixture.create("Smoke investigation")
        let end = Date().addingTimeInterval(-60)
        let worklog = try fixture.completed(task, start: end.addingTimeInterval(-600), end: end)
        launch()
        select(task)
        XCTAssertTrue(element("worklog-history.row.\(worklog.id)").waitForExistence(timeout: timeout))
        XCTAssertEqual(try fixture.worklogs(task), [worklog])
    }

    func testFixtureCleanupRemovesItsDatabaseDirectoryAndPreferences() throws {
        let isolated = try TrackerFixture()
        let path = isolated.directory.path
        let suite = isolated.defaultsSuite
        _ = try isolated.create("Cleanup verification")
        UserDefaults(suiteName: suite)?.set("isolated", forKey: "cleanup-marker")
        try isolated.cleanup()
        XCTAssertFalse(FileManager.default.fileExists(atPath: path))
        XCTAssertNil(UserDefaults(suiteName: suite)?.string(forKey: "cleanup-marker"))
        try isolated.cleanup()
    }
}
