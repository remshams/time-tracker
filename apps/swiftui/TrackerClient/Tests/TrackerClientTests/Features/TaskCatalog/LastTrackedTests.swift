import Foundation
import XCTest
@testable import TrackerClient

private let locale = Locale(identifier: "en_GB")

private func calendar(_ zone: String) -> Calendar {
    var calendar = Calendar(identifier: .gregorian)
    calendar.timeZone = TimeZone(identifier: zone)!
    return calendar
}

final class LastTrackedTests: XCTestCase {
    @MainActor
    private func text(_ latestStart: String?, today: String, zone: String = "UTC") -> String {
        LastTracked(
            latestStart: latestStart, today: parseTimestamp(today)!, calendar: calendar(zone), locale: locale
        ).text
    }

    @MainActor
    func testTaskWithoutAValidStartWasNeverTracked() async {
        XCTAssertEqual(text(nil, today: "2026-11-16T00:00:00.000Z"), "Never")
        XCTAssertEqual(text("not a timestamp", today: "2026-11-16T00:00:00.000Z"), "Never")
    }

    @MainActor
    func testStartTodayShowsItsLocalTime() async {
        XCTAssertEqual(text("2026-11-16T13:12:45.000Z", today: "2026-11-16T00:00:00.000Z"), "Today 13:12")
        XCTAssertEqual(
            text("2026-11-16T13:12:45.000Z", today: "2026-11-16T00:00:00.000Z", zone: "Europe/Berlin"),
            "Today 14:12", "The time is in the calendar's time zone.")
    }

    @MainActor
    func testEarlierStartsThisWeekCountWholeDays() async {
        let today = "2026-11-16T09:00:00.000Z"
        XCTAssertEqual(text("2026-11-15T23:59:59.000Z", today: today), "Yesterday")
        XCTAssertEqual(text("2026-11-14T00:00:00.000Z", today: today), "2 days ago")
        XCTAssertEqual(text("2026-11-10T12:00:00.000Z", today: today), "6 days ago")
    }

    @MainActor
    func testStartsAWeekAgoOrLaterShowTheirDate() async {
        let today = "2026-11-16T09:00:00.000Z"
        XCTAssertEqual(text("2026-11-09T12:00:00.000Z", today: today), "9 Nov 2026")
        XCTAssertEqual(text("2025-12-31T12:00:00.000Z", today: today), "31 Dec 2025")
        XCTAssertEqual(
            text("2026-11-17T08:00:00.000Z", today: today), "17 Nov 2026",
            "A start after today shows its date.")
    }

    @MainActor
    func testDayNamesTheCaseForTheHeader() async {
        let today = parseTimestamp("2026-11-16T09:00:00.000Z")!
        func day(_ latestStart: String?) -> LastTracked.Day {
            LastTracked(latestStart: latestStart, today: today, calendar: calendar("UTC"), locale: locale).day
        }
        XCTAssertEqual(day("2026-11-16T13:12:00.000Z"), .today(time: "13:12"))
        XCTAssertEqual(day("2026-11-15T13:12:00.000Z"), .yesterday)
        XCTAssertEqual(day("2026-11-13T13:12:00.000Z"), .daysAgo(3))
        XCTAssertEqual(day("2026-11-04T13:12:00.000Z"), .date("4 Nov 2026"))
        XCTAssertEqual(day(nil), .never)
    }

    @MainActor
    func testDaysFollowTheCalendarsTimeZone() async {
        // 23:30 UTC on the 15th is 13:30 on the 16th at UTC+14.
        XCTAssertEqual(
            text("2026-11-15T23:30:00.000Z", today: "2026-11-16T09:00:00.000Z", zone: "Pacific/Kiritimati"),
            "Today 13:30")
        XCTAssertEqual(text("2026-11-15T23:30:00.000Z", today: "2026-11-16T09:00:00.000Z"), "Yesterday")
    }
}
