import Foundation
import XCTest
@testable import TrackerClient

private let locale = Locale(identifier: "en_GB")

private func calendar(_ zone: String = "UTC") -> Calendar {
    var calendar = Calendar(identifier: .gregorian)
    calendar.timeZone = TimeZone(identifier: zone)!
    return calendar
}

@MainActor
private func date(_ value: String) -> Date { parseTimestamp(value)! }

private func worklog(_ id: String, _ start: String, _ end: String?) -> WorklogItem {
    WorklogItem(id: id, taskId: "task", start: start, end: end)
}

final class WorklogDaysTests: XCTestCase {
    @MainActor
    private func days(
        _ worklogs: [WorklogItem], runningEnd: String? = nil, hasOlder: Bool = false,
        today: String = "2026-11-18T12:00:00.000Z", todaySeconds: Int? = nil, zone: String = "UTC"
    ) -> WorklogDays {
        WorklogDays(
            worklogs: worklogs, runningEnd: runningEnd.map(date), hasOlder: hasOlder, today: date(today),
            todaySeconds: todaySeconds, calendar: calendar(zone), locale: locale)
    }

    @MainActor
    func testGroupsByLocalDayNewestFirstWithNewestRowsFirst() async {
        let result = days([
            worklog("late", "2026-11-18T10:00:00.000Z", "2026-11-18T11:00:00.000Z"),
            worklog("early", "2026-11-18T08:00:00.000Z", "2026-11-18T08:30:00.000Z"),
            worklog("before", "2026-11-17T22:30:00.000Z", "2026-11-17T22:45:00.000Z"),
        ])
        XCTAssertEqual(result.days.map(\.title), ["Today", "Yesterday"])
        XCTAssertEqual(result.days.map { $0.rows.map(\.id) }, [["late", "early"], ["before"]])
        XCTAssertEqual(result.days.map(\.totalSeconds), [5400, 900], "A day's total adds its rows.")
        XCTAssertEqual(result.days[0].id, date("2026-11-18T00:00:00.000Z"))
        XCTAssertEqual(
            days([worklog("late", "2026-11-17T23:30:00.000Z", "2026-11-17T23:45:00.000Z")], zone: "Europe/Berlin")
                .days.map(\.title),
            ["Today"], "Days follow the calendar's time zone: 23:30 UTC on the 17th is 00:30 on the 18th in Berlin.")
    }

    @MainActor
    func testRowsWithTheSameStartKeepTheHistoryOrder() async {
        let result = days([
            worklog("first", "2026-11-18T09:00:00.000Z", "2026-11-18T09:10:00.000Z"),
            worklog("second", "2026-11-18T09:00:00.000Z", "2026-11-18T09:20:00.000Z"),
        ])
        XCTAssertEqual(result.days[0].rows.map(\.id), ["first", "second"])
    }

    @MainActor
    func testOlderDaysShowTheWeekdayAndDateAndTheYearOnlyWhenItDiffers() async {
        let result = days([
            worklog("this-year", "2026-11-16T09:00:00.000Z", "2026-11-16T10:00:00.000Z"),
            worklog("last-year", "2025-12-31T09:00:00.000Z", "2025-12-31T10:00:00.000Z"),
            worklog("future", "2026-11-20T09:00:00.000Z", "2026-11-20T10:00:00.000Z"),
        ])
        XCTAssertEqual(result.days.map(\.title), ["Fri 20 Nov", "Mon 16 Nov", "Wed, 31 Dec 2025"])
    }

    @MainActor
    func testTodaysTotalComesFromTheDailyTotalsAndFallsBackToItsRows() async {
        let logs = [
            worklog("today", "2026-11-18T09:00:00.000Z", "2026-11-18T10:00:00.000Z"),
            worklog("yesterday", "2026-11-17T09:00:00.000Z", "2026-11-17T09:30:00.000Z"),
        ]
        XCTAssertEqual(days(logs, todaySeconds: 3725).days.map(\.totalSeconds), [3725, 1800])
        XCTAssertEqual(days(logs).days.map(\.totalSeconds), [3600, 1800])
    }

    @MainActor
    func testOldestDayHidesItsPartialTotalWhileOlderWorklogsAreUnloaded() async {
        let logs = [
            worklog("today", "2026-11-18T09:00:00.000Z", "2026-11-18T10:00:00.000Z"),
            worklog("yesterday", "2026-11-17T09:00:00.000Z", "2026-11-17T09:30:00.000Z"),
        ]
        XCTAssertEqual(days(logs, hasOlder: true).days.map(\.totalSeconds), [3600, nil])
        XCTAssertEqual(days(logs, hasOlder: false).days.map(\.totalSeconds), [3600, 1800])
        let onlyToday = [logs[0]]
        XCTAssertEqual(
            days(onlyToday, hasOlder: true, todaySeconds: 4000).days.map(\.totalSeconds), [4000],
            "The daily totals cover all of today, even when today is the oldest loaded day.")
        XCTAssertEqual(days(onlyToday, hasOlder: true).days.map(\.totalSeconds), [nil])
        XCTAssertEqual(days(onlyToday, hasOlder: true).days[0].total, nil)
        XCTAssertEqual(days(onlyToday).days[0].total, DurationText(seconds: 3600))
    }

    @MainActor
    func testWorklogAcrossMidnightIsSplitIntoOneRowPerDay() async {
        let result = days([worklog("night", "2026-11-17T23:10:00.000Z", "2026-11-18T00:40:00.000Z")])
        XCTAssertEqual(result.days.map(\.title), ["Today", "Yesterday"])
        let today = result.days[0].rows[0]
        let yesterday = result.days[1].rows[0]
        XCTAssertEqual(yesterday.id, "night")
        XCTAssertEqual(today.id, "night.2026-11-18")
        XCTAssertEqual(today.worklog, yesterday.worklog)
        XCTAssertEqual([today.range, yesterday.range], ["23:10 – 0:40", "23:10 – 0:40"])
        XCTAssertEqual([today.seconds, yesterday.seconds], [2400, 3000])
        XCTAssertEqual([today.startHour, today.endHour], [0, 40.0 / 60])
        XCTAssertEqual([yesterday.startHour, yesterday.endHour], [23 + 10.0 / 60, 24])
        XCTAssertEqual(result.days.map(\.totalSeconds), [2400, 3000])
    }

    @MainActor
    func testWorklogOverSeveralDaysGetsARowOnEachDay() async {
        let result = days(
            [worklog("weekend", "2026-11-16T22:00:00.000Z", "2026-11-18T02:00:00.000Z")])
        XCTAssertEqual(
            result.days.map { $0.rows.map(\.id) },
            [
                ["weekend.2026-11-18"], ["weekend.2026-11-17"], ["weekend"],
            ])
        XCTAssertEqual(result.days.map(\.totalSeconds), [7200, 86_400, 7200])
    }

    @MainActor
    func testWorklogEndingAtMidnightStaysOnItsDay() async {
        let result = days([worklog("evening", "2026-11-17T22:00:00.000Z", "2026-11-18T00:00:00.000Z")])
        XCTAssertEqual(result.days.map(\.title), ["Yesterday"])
        XCTAssertEqual(result.days[0].rows[0].endHour, 24)
    }

    @MainActor
    func testMomentAtMidnightStaysOnItsDay() async {
        let result = days([worklog("moment", "2026-11-18T00:00:00.000Z", "2026-11-18T00:00:00.000Z")])
        XCTAssertEqual(result.days.map(\.title), ["Today"])
        XCTAssertEqual(result.days[0].rows.map(\.seconds), [0])
    }

    @MainActor
    func testRunningWorklogEndsNowAndOnlyItsLastPartRuns() async {
        let result = days(
            [worklog("running", "2026-11-17T23:10:00.000Z", nil)], runningEnd: "2026-11-18T00:05:30.000Z")
        let today = result.days[0].rows[0]
        let yesterday = result.days[1].rows[0]
        XCTAssertEqual([today.range, yesterday.range], ["23:10 – now", "23:10 – now"])
        XCTAssertEqual([today.isRunning, yesterday.isRunning], [true, false])
        XCTAssertEqual([today.seconds, yesterday.seconds], [330, 3000])
        XCTAssertEqual(today.duration, DurationText(seconds: 330))
        XCTAssertEqual(today.endHour, 5.5 / 60, accuracy: 1e-9)
        XCTAssertEqual(today.accessibilityLabel, "23:10 to now, 5 minutes, running")
        XCTAssertEqual(yesterday.accessibilityLabel, "23:10 to now, 50 minutes")
    }

    @MainActor
    func testRunningWorklogWithUnknownElapsedTimeHasNoDuration() async {
        let result = days([worklog("running", "2026-11-18T09:00:00.000Z", nil)])
        let row = result.days[0].rows[0]
        XCTAssertNil(row.seconds)
        XCTAssertNil(row.duration)
        XCTAssertTrue(row.isRunning)
        XCTAssertEqual([row.startHour, row.endHour], [9, 9])
        XCTAssertEqual(row.accessibilityLabel, "9:00 to now, running")
        XCTAssertEqual(result.days[0].totalSeconds, 0)
        XCTAssertEqual(
            days([worklog("running", "2026-11-18T09:00:00.000Z", nil)], runningEnd: "2026-11-18T08:00:00.000Z")
                .days[0].rows[0].seconds,
            0, "A running end before the start is the start.")
    }

    @MainActor
    func testUnreadableWorklogsAreSkippedOrShownAsAMoment() async {
        let result = days([
            worklog("no-start", "yesterday", "2026-11-18T10:00:00.000Z"),
            worklog("no-end", "2026-11-18T09:00:00.000Z", "later"),
            worklog("backwards", "2026-11-18T08:00:00.000Z", "2026-11-18T07:00:00.000Z"),
        ])
        XCTAssertEqual(result.days[0].rows.map(\.id), ["no-end", "backwards"])
        XCTAssertEqual(result.days[0].rows.map(\.seconds), [0, 0])
        XCTAssertEqual(result.days[0].rows.map(\.isRunning), [false, false])
        XCTAssertEqual(result.days[0].rows.map(\.range), ["9:00 – 9:00", "8:00 – 8:00"])
    }

    @MainActor
    func testRowsSitAtTheirWallClockTimesOnDaylightSavingDays() async {
        // Berlin moved its clocks back from 03:00 to 02:00 on 25 October 2026, so that day had 25 hours.
        let result = days(
            [worklog("after-change", "2026-10-25T09:00:00.000Z", "2026-10-25T10:30:00.000Z")],
            today: "2026-10-25T12:00:00.000Z", zone: "Europe/Berlin")
        let row = result.days[0].rows[0]
        XCTAssertEqual([row.startHour, row.endHour], [10, 11.5], "09:00 UTC is 10:00 in Berlin after the change.")
        XCTAssertEqual(row.seconds, 5400)
    }

    @MainActor
    func testDayAndRowLabelsForVoiceOver() async {
        let result = days(
            [worklog("work", "2026-11-18T09:12:00.000Z", "2026-11-18T10:40:00.000Z")], hasOlder: false)
        XCTAssertEqual(result.days[0].accessibilityLabel, "Today, 1 hour, 28 minutes")
        XCTAssertEqual(result.days[0].rows[0].accessibilityLabel, "9:12 to 10:40, 1 hour, 28 minutes")
        XCTAssertEqual(
            days([worklog("work", "2026-11-18T09:12:00.000Z", "2026-11-18T10:40:00.000Z")], hasOlder: true)
                .days[0].accessibilityLabel,
            "Today", "A hidden total isn't read either.")
    }

    @MainActor
    func testNoWorklogsHaveNoDays() async {
        let result = days([])
        XCTAssertEqual(result.days, [])
        XCTAssertEqual(result.axis, WorklogHourAxis(rows: []))
    }

    @MainActor
    func testStoresProvideTheRunningEndTodayAndTodaysTotal() async throws {
        let fixture = Fixture(reports: true, calendar: calendar())
        defer { fixture.cleanup() }
        fixture.sync.setWindowVisible(true)
        // The clock starts at midnight on 1 January 2025; the running worklog started 30 seconds earlier.
        try await fixture.start(
            TaskListResources(tasks: [firstTask], active: activeWorklog),
            rows: [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 10_000_000)])
        let features = fixture.features
        features.worklogHistory.accept(
            HistoryPage(worklogs: [activeWorklog, oldWorklog], nextCursor: nil, reset: false), cursor: nil)
        fixture.advanceClock(by: 90)
        fixture.scheduler.display?.fire()
        let shown = WorklogDays(
            history: features.worklogHistory, tracking: features.tracking, dailyTotals: features.dailyTotals,
            taskID: firstTask.id)
        // The older day's title follows the user's locale here.
        XCTAssertEqual(shown.days.prefix(2).map(\.title), ["Today", "Yesterday"])
        XCTAssertEqual(shown.days.count, 3)
        XCTAssertEqual(shown.days[0].rows.map(\.seconds), [90])
        XCTAssertEqual(shown.days[0].rows.map(\.isRunning), [true])
        XCTAssertEqual(
            shown.days.map(\.totalSeconds), [100, 30, 3600],
            "Today's total is the report's 10 seconds plus the 90 running seconds since midnight.")
        features.worklogHistory.accept(
            HistoryPage(worklogs: [activeWorklog, oldWorklog], nextCursor: "older", reset: false), cursor: nil)
        let partial = WorklogDays(
            history: features.worklogHistory, tracking: features.tracking, dailyTotals: features.dailyTotals,
            taskID: secondTask.id)
        XCTAssertEqual(
            partial.days.map(\.totalSeconds), [90, 30, nil],
            "Without a daily total for the task, today adds its rows, and the oldest day hides its partial total.")
    }

    @MainActor
    func testAnotherTasksRunningWorklogDoesNotShowAnEmptyDailyTotal() async throws {
        let fixture = Fixture(reports: true, calendar: calendar())
        defer { fixture.cleanup() }
        fixture.sync.setWindowVisible(true)
        // The first task runs. The report has no time for the second task, although its history has a worklog today.
        try await fixture.start(
            TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog),
            rows: [TaskReportTotal(taskId: secondTask.id, durationMicroseconds: 0)])
        let features = fixture.features
        fixture.advanceClock(by: 600)
        fixture.scheduler.display?.fire()
        let today = WorklogItem(
            id: "worklog-today", taskId: secondTask.id, start: "2025-01-01T00:02:00.000Z",
            end: "2025-01-01T00:03:00.000Z")
        features.worklogHistory.accept(HistoryPage(worklogs: [today], nextCursor: nil, reset: false), cursor: nil)
        let shown = WorklogDays(
            history: features.worklogHistory, tracking: features.tracking, dailyTotals: features.dailyTotals,
            taskID: secondTask.id)
        XCTAssertEqual(
            shown.days.map(\.totalSeconds), [60],
            "A task that doesn't run and has no time in the report shows the sum of its rows.")
    }
}
