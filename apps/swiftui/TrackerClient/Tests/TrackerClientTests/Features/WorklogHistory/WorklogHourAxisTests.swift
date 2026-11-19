import Foundation
import XCTest
@testable import TrackerClient

@MainActor
private func axis(_ ranges: [(String, String)]) -> WorklogHourAxis {
    var calendar = Calendar(identifier: .gregorian)
    calendar.timeZone = TimeZone(identifier: "UTC")!
    let worklogs = ranges.enumerated().map { index, range in
        WorklogItem(
            id: "w\(index)", taskId: "task", start: "2026-11-18T\(range.0):00.000Z",
            end: "2026-11-18T\(range.1):00.000Z")
    }
    return WorklogDays(
        worklogs: worklogs, runningEnd: nil, hasOlder: false, today: parseTimestamp("2026-11-18T12:00:00.000Z")!,
        todaySeconds: nil, calendar: calendar, locale: Locale(identifier: "en_GB")
    ).axis
}

final class WorklogHourAxisTests: XCTestCase {
    @MainActor
    func testEmptyHistoryShowsAWorkingDay() async {
        let empty = axis([])
        XCTAssertEqual([empty.startHour, empty.endHour], [9, 17])
    }

    @MainActor
    func testAxisFitsTheRowsRoundedOutToWholeHours() async {
        let fitted = axis([("07:45", "12:00"), ("13:10", "19:20")])
        XCTAssertEqual([fitted.startHour, fitted.endHour], [7, 20])
    }

    @MainActor
    func testShortDaysWidenToEightHoursEarlierFirst() async {
        let short = axis([("10:15", "11:30")])
        XCTAssertEqual([short.startHour, short.endHour], [7, 15], "Each round widens the start, then the end.")
        let early = axis([("00:30", "01:00")])
        XCTAssertEqual([early.startHour, early.endHour], [0, 8], "The start stops at midnight; the end keeps going.")
        let late = axis([("22:00", "23:30")])
        XCTAssertEqual([late.startHour, late.endHour], [16, 24], "The end stops at midnight; the start keeps going.")
        let moment = axis([("10:00", "10:00")])
        XCTAssertEqual([moment.startHour, moment.endHour], [6, 14])
    }

    @MainActor
    func testWideDaysKeepTheirRange() async {
        let wide = axis([("06:00", "14:00")])
        XCTAssertEqual([wide.startHour, wide.endHour], [6, 14])
        let full = axis([("00:00", "23:59")])
        XCTAssertEqual([full.startHour, full.endHour], [0, 24])
    }

    @MainActor
    func testPositionIsTheShareOfTheAxisClampedToItsEnds() async {
        let fitted = axis([("08:00", "16:00")])
        XCTAssertEqual(fitted.position(8), 0)
        XCTAssertEqual(fitted.position(12), 0.5)
        XCTAssertEqual(fitted.position(16), 1)
        XCTAssertEqual(fitted.position(6), 0)
        XCTAssertEqual(fitted.position(20), 1)
    }

    @MainActor
    func testGridStepGrowsWithTheSpan() async {
        XCTAssertEqual(axis([("08:00", "18:00")]).gridHours, [10, 12, 14, 16], "Ten hours use every second hour.")
        XCTAssertEqual(axis([("08:00", "19:00")]).gridHours, [9, 12, 15, 18], "Eleven hours use every third hour.")
        XCTAssertEqual(
            axis([("08:00", "23:00")]).gridHours, [9, 12, 15, 18, 21], "Fifteen hours use every third hour.")
        XCTAssertEqual(axis([("07:00", "23:00")]).gridHours, [8, 12, 16, 20], "Longer days use every fourth hour.")
    }

    @MainActor
    func testBothEndsAreLabelledAndInnerLabelsKeepAStepAway() async {
        XCTAssertEqual(axis([("09:00", "17:00")]).labeledHours, [9, 12, 14, 17])
        XCTAssertEqual(axis([("08:00", "16:00")]).labeledHours, [8, 10, 12, 14, 16])
        XCTAssertEqual(axis([("08:00", "19:00")]).labeledHours, [8, 12, 15, 19])
    }

    @MainActor
    func testHourLabelsFollowTheLocalesClock() async {
        let twentyFour = Locale(identifier: "en_GB")
        let twelve = Locale(identifier: "en_US")
        XCTAssertEqual([9, 17, 24].map { WorklogHourAxis.label($0, locale: twentyFour) }, ["9", "17", "24"])
        XCTAssertEqual(
            [0, 9, 12, 17, 24].map { WorklogHourAxis.label($0, locale: twelve) },
            ["12am", "9am", "12pm", "5pm", "12am"])
    }
}
