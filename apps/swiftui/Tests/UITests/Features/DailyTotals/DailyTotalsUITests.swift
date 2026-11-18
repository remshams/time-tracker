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
        assertSidebarTotal(first, report: report)
        XCTAssertTrue(taskRow(unused).exists)
        XCTAssertFalse(element("task-sidebar.total.\(unused.id)").exists, "A task without time today shows no total.")
        showTab("Archived")
        assertSidebarTotal(archived, report: report)
        waitUntil("The status item total matches the CLI report") {
            self.statusButton.value as? String == self.minuteDuration(report.totalMicroseconds)
        }
        openStatusMenu()
        for task in [first, archived] {
            let expected = menuDuration(report.rows.first { $0.task.id == task.id }?.durationMicroseconds ?? 0)
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

    func testTodaySummarySplitsTheDayAndItsSegmentsShowTheirTasks() throws {
        let first = try fixture.create("A larger share")
        let archived = try fixture.create("B archived share")
        let second = try fixture.create("C smaller share")
        let unshared = try fixture.create("D no share")
        try completedToday(first, duration: 180)
        try completedToday(archived, duration: 120)
        try completedToday(second, duration: 60)
        try fixture.archive(archived)
        let now = Date()
        let day = try XCTUnwrap(Calendar.current.dateInterval(of: .day, for: now))
        let report = try fixture.reports(start: day.start, end: day.end, at: now)
        launch()
        let summary = element("today-summary.total")
        let spoken = spokenTotal(report.totalMicroseconds)
        let shown = windowDuration(report.totalMicroseconds)
        // VoiceOver reads the spoken total; the text shows the clock form.
        let showsTotal = {
            let texts = [summary.label, summary.value as? String ?? ""]
            return texts.contains { $0.contains(spoken) || $0.contains(shown) }
        }
        waitUntil("The summary shows today's total") { summary.exists && showsTotal() }
        XCTAssertTrue(
            showsTotal(),
            "Expected \(spoken) or \(shown), got label \(summary.label), value \(String(describing: summary.value))")
        let shares = app.descendants(matching: .any)
            .matching(NSPredicate(format: "identifier BEGINSWITH %@", "today-summary.share."))
        waitUntil("Each task with time today has one segment") { shares.count == 3 }
        // Right after midnight `completedToday` shortens the worklogs, so the report decides the expected order:
        // largest first, then by name.
        let seconds = { (task: FixtureTask) -> Int64 in
            (report.rows.first { $0.task.id == task.id }?.durationMicroseconds ?? 0) / 1_000_000
        }
        let expected = [first, archived, second].sorted {
            seconds($0) != seconds($1) ? seconds($0) > seconds($1) : $0.name < $1.name
        }
        XCTAssertEqual(
            (0..<shares.count).map { shares.element(boundBy: $0).identifier },
            expected.map { "today-summary.share.\($0.id)" },
            "The largest share comes first.")
        let firstShare = element("today-summary.share.\(first.id)")
        XCTAssertTrue(firstShare.label.hasPrefix("\(first.name), "))
        XCTAssertTrue(firstShare.label.hasSuffix(" percent"))

        select(unshared)
        firstShare.click()
        waitUntil("A segment selects its task") { self.element("task-details.task.\(first.id)").exists }
        element("today-summary.share.\(archived.id)").click()
        waitUntil("An archived task's segment opens the Archived tab") {
            self.element("task-details.task.\(archived.id)").exists && self.taskRow(archived).exists
        }
    }

    func testRowsShowWhenEachTaskLastRan() throws {
        let today = try fixture.create("Ran today")
        let never = try fixture.create("Never ran")
        let log = try completedToday(today)
        launch()
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        let started = try XCTUnwrap(formatter.date(from: log.start) ?? ISO8601DateFormatter().date(from: log.start))
        let time = started.formatted(date: .omitted, time: .shortened)
        waitUntil("The row of a task that ran today shows its start") {
            self.taskRow(today).staticTexts["Today \(time)"].exists
        }
        XCTAssertTrue(taskRow(never).staticTexts["Never"].exists)
    }

    func testTaskHeaderShowsTodaysTotalOrWhenTheTaskLastRan() throws {
        let today = try fixture.create("Header today")
        let yesterday = try fixture.create("Header yesterday")
        let never = try fixture.create("Header never")
        try completedToday(today)
        let startOfDay = Calendar.current.startOfDay(for: Date())
        _ = try fixture.completed(
            yesterday, start: startOfDay.addingTimeInterval(-7200), end: startOfDay.addingTimeInterval(-3600))
        let now = Date()
        let day = try XCTUnwrap(Calendar.current.dateInterval(of: .day, for: now))
        let report = try fixture.reports(start: day.start, end: day.end, at: now)
        let expected = spokenTotal(report.rows.first { $0.task.id == today.id }?.durationMicroseconds ?? 0)
        launch()
        let subtitle = element("task-details.subtitle")
        let expectations = [
            (today, "\(expected) today"), (yesterday, "Last tracked yesterday"), (never, "Not tracked yet"),
        ]
        for (task, text) in expectations {
            select(task)
            waitUntil("The header of \(task.name) reads \(text)") { subtitle.label == text }
        }
    }

    private func spokenTotal(_ microseconds: Int64) -> String {
        let seconds = microseconds / 1_000_000
        guard seconds >= 60 else { return "\(seconds) second\(seconds == 1 ? "" : "s")" }
        let hours = seconds / 3600
        let minutes = seconds / 60 % 60
        let parts = [
            hours > 0 ? "\(hours) hour\(hours == 1 ? "" : "s")" : nil,
            minutes > 0 ? "\(minutes) minute\(minutes == 1 ? "" : "s")" : nil,
        ]
        return parts.compactMap { $0 }.joined(separator: ", ")
    }

    private func assertSidebarTotal(_ task: FixtureTask, report: FixtureReport) {
        let seconds = report.rows.first { $0.task.id == task.id }?.durationMicroseconds ?? 0
        let label = "Today's total: \(windowDuration(seconds))"
        waitUntil("The sidebar total for \(task.name) matches the CLI") {
            self.taskRow(task).staticTexts[label].exists
        }
    }

    private func menuDuration(_ microseconds: Int64) -> String {
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
