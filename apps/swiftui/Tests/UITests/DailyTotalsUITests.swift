import XCTest

@MainActor
class DailyTotalsUITests: TrackerUITestCase {
    func testCompletedArchivedAndZeroTotalsMatchCLIReport() throws {
        let first = try fixture.create("Completed total")
        let archived = try fixture.create("Archived total")
        let unused = try fixture.create("Zero total")
        let now = Date()
        try completedToday(first, duration: 120)
        try completedToday(archived)
        try fixture.archive(archived)
        let day = try XCTUnwrap(Calendar.current.dateInterval(of: .day, for: now))
        let report = try fixture.reports(start: day.start, end: day.end, at: now)
        launch()
        for task in [first, unused] { assertSidebarTotal(task, report: report) }
        showTab("Archived")
        assertSidebarTotal(archived, report: report)
        waitUntil("The status item total matches the CLI report") {
            self.statusButton.value as? String == self.minuteDuration(report.totalMicroseconds)
        }
        openStatusMenu()
        for task in [first, archived] {
            let expected = clockDuration(report.rows.first { $0.task.id == task.id }?.durationMicroseconds ?? 0)
            XCTAssertTrue(menuTask(task).label.contains(expected))
        }
        dismissStatusMenu()
    }

    func testOvernightWorklogCountsOnlyTimeInCurrentLocalDay() throws {
        let task = try fixture.create("Overnight total")
        let now = Date()
        let day = try XCTUnwrap(Calendar.current.dateInterval(of: .day, for: now))
        let end = min(now.addingTimeInterval(-1), day.start.addingTimeInterval(600))
        _ = try fixture.completed(task, start: day.start.addingTimeInterval(-3600), end: end)
        let report = try fixture.reports(start: day.start, end: day.end, at: now)
        XCTAssertEqual(Double(report.totalMicroseconds) / 1_000_000,
                       max(0, end.timeIntervalSince(day.start)), accuracy: 0.001)
        launch()
        assertSidebarTotal(task, report: report)
        waitUntil("The status item shows only today's overnight portion") {
            self.statusButton.value as? String == self.minuteDuration(report.totalMicroseconds)
        }
    }

    func testRunningTotalTracksCLIWithinElapsedWallTimeBound() throws {
        let task = try fixture.create("Live total")
        let before = Date()
        _ = try fixture.start(task, at: before.addingTimeInterval(-120))
        launch()
        select(task)
        let day = try XCTUnwrap(Calendar.current.dateInterval(of: .day, for: before))
        let report = try fixture.reports(start: day.start, end: day.end, at: before)
        let lower = Double(report.totalMicroseconds) / 1_000_000
        let total = element("task-sidebar.total.\(task.id)")
        XCTAssertTrue(total.waitForExistence(timeout: timeout))
        let text = try XCTUnwrap(total.value as? String)
        let actual = try XCTUnwrap(parseDuration(text.replacingOccurrences(of: "Today's total: ", with: "")))
        XCTAssertGreaterThanOrEqual(actual, floor(lower))
        XCTAssertLessThanOrEqual(actual, lower + Date().timeIntervalSince(before) + 2)
        let worklog = try XCTUnwrap(fixture.activeWorklog())
        XCTAssertTrue(app.buttons["Stop tracking"].exists)
        XCTAssertEqual(try fixture.activeWorklog(), worklog)
    }

    private func assertSidebarTotal(_ task: FixtureTask, report: FixtureReport) {
        let seconds = report.rows.first { $0.task.id == task.id }?.durationMicroseconds ?? 0
        let label = "Today's total: \(clockDuration(seconds))"
        waitUntil("The sidebar total for \(task.name) matches the CLI") {
            self.taskRow(task).staticTexts[label].exists
        }
    }

    private func clockDuration(_ microseconds: Int64) -> String {
        let seconds = microseconds / 1_000_000
        return String(format: "%02d:%02d:%02d", seconds / 3600, seconds / 60 % 60, seconds % 60)
    }

    private func minuteDuration(_ microseconds: Int64) -> String {
        let minutes = microseconds / 60_000_000
        return String(format: "%02d:%02d", minutes / 60, minutes % 60)
    }

    private func parseDuration(_ text: String) -> TimeInterval? {
        let parts = text.split(separator: ":").compactMap { Double($0) }
        guard parts.count == 3 else { return nil }
        return parts[0] * 3600 + parts[1] * 60 + parts[2]
    }
}

@MainActor
final class ServerDailyTotalsUITests: DailyTotalsUITests {
    override var fixtureSource: TrackerFixture.Source { .server }
}
