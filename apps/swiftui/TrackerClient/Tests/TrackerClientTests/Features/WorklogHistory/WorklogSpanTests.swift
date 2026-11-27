import Foundation
import XCTest
@testable import TrackerClient

@MainActor
private func span(_ start: String, _ end: String?, locale: String = "en_GB") -> WorklogSpan {
    var calendar = Calendar(identifier: .gregorian)
    calendar.timeZone = TimeZone(identifier: "UTC")!
    return WorklogSpan(
        start: parseTimestamp(start)!, end: end.map { parseTimestamp($0)! },
        now: parseTimestamp("2026-11-18T12:00:00.000Z")!, calendar: calendar,
        locale: Locale(identifier: locale))
}

final class WorklogSpanTests: XCTestCase {
    @MainActor
    func testFinishedWorklogShowsItsDayRangeAndHours() async {
        let finished = span("2026-11-18T09:15:00.000Z", "2026-11-18T10:45:00.000Z")
        XCTAssertEqual(finished.day, "Wednesday 18 November")
        XCTAssertEqual(finished.range, "9:15 – 10:45")
        XCTAssertEqual(finished.startHour, 9.25)
        XCTAssertEqual(finished.endHour, 10.75)
    }

    @MainActor
    func testRunningWorklogReachesNow() async {
        let running = span("2026-11-18T09:30:00.000Z", nil)
        XCTAssertEqual(running.range, "9:30 – now")
        XCTAssertEqual(running.startHour, 9.5)
        XCTAssertEqual(running.endHour, 12)
    }

    @MainActor
    func testWorklogAcrossMidnightNamesTheEndDayAndStopsAtMidnight() async {
        let overnight = span("2026-11-17T23:00:00.000Z", "2026-11-18T01:30:00.000Z")
        XCTAssertEqual(overnight.day, "Tuesday 17 November")
        XCTAssertEqual(overnight.range, "23:00 – Wed 18 Nov, 1:30")
        XCTAssertEqual(overnight.startHour, 23)
        XCTAssertEqual(overnight.endHour, 24)
    }

    @MainActor
    func testEndBeforeStartIsAMomentAtTheStart() async {
        let reversed = span("2026-11-18T10:00:00.000Z", "2026-11-18T09:00:00.000Z")
        XCTAssertEqual(reversed.range, "10:00 – 9:00")
        XCTAssertEqual(reversed.startHour, 10)
        XCTAssertEqual(reversed.endHour, 10)
        let earlierDay = span("2026-11-18T10:00:00.000Z", "2026-11-17T09:00:00.000Z")
        XCTAssertEqual(earlierDay.range, "10:00 – Tue 17 Nov, 9:00", "An end on another day names that day.")
        XCTAssertEqual(earlierDay.endHour, 10)
    }

    @MainActor
    func testDayNamesTheYearOnlyOutsideTheCurrentOne() async {
        XCTAssertEqual(
            span("2025-12-31T09:00:00.000Z", "2025-12-31T10:00:00.000Z").day, "Wednesday, 31 December 2025")
    }

    @MainActor
    func testTimesFollowTheLocalesClock() async {
        XCTAssertEqual(
            span("2026-11-18T13:05:00.000Z", "2026-11-18T14:10:00.000Z", locale: "en_US").range,
            "1:05\u{202F}PM – 2:10\u{202F}PM")
    }

    @MainActor
    func testAxisCoversEverySpan() async {
        let opened = span("2026-11-18T09:00:00.000Z", "2026-11-18T10:00:00.000Z")
        let edited = span("2026-11-18T16:30:00.000Z", "2026-11-18T18:15:00.000Z")
        let axis = WorklogHourAxis(covering: [opened, edited])
        XCTAssertEqual([axis.startHour, axis.endHour], [9, 19])
        let single = WorklogHourAxis(covering: [opened])
        XCTAssertEqual([single.startHour, single.endHour], [5, 13], "A short span widens to the minimum.")
        let none = WorklogHourAxis(covering: [])
        XCTAssertEqual([none.startHour, none.endHour], [9, 17])
    }
}
