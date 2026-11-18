import Foundation
import XCTest
@testable import TrackerClient

@MainActor
private func lastTracked(_ latestStart: String?) -> LastTracked {
    var calendar = Calendar(identifier: .gregorian)
    calendar.timeZone = TimeZone(identifier: "UTC")!
    let today = parseTimestamp("2026-11-16T09:00:00.000Z")!
    return LastTracked(latestStart: latestStart, today: today, calendar: calendar, locale: Locale(identifier: "en_GB"))
}

@MainActor
private func total(_ seconds: Int?) -> TaskDailyTotalPresentation {
    TaskDailyTotalPresentation(seconds: seconds, explanation: "")
}

final class TaskHeaderSubtitleTests: XCTestCase {
    @MainActor
    private func text(_ total: TaskDailyTotalPresentation?, running: Bool = false, _ latestStart: String?) -> String {
        TaskHeaderSubtitle(total: total, isRunning: running, lastTracked: lastTracked(latestStart)).text
    }

    @MainActor
    func testTimeTodayShowsOnlyTodaysTotal() async {
        let subtitle = TaskHeaderSubtitle(
            total: total(2712), isRunning: false, lastTracked: lastTracked("2026-11-16T08:00:00.000Z"))
        XCTAssertEqual(subtitle, .today(DurationText(seconds: 2712)))
        XCTAssertEqual(subtitle.text, "0:45:12 today")
        XCTAssertEqual(text(total(1), "2026-11-16T08:00:00.000Z"), "0:00:01 today")
    }

    @MainActor
    func testRunningTaskShowsItsTotalFromTheFirstSecond() async {
        XCTAssertEqual(text(total(0), running: true, "2026-11-16T08:59:59.000Z"), "0:00:00 today")
        XCTAssertEqual(text(total(2712), running: true, "2026-11-15T23:10:00.000Z"), "0:45:12 today")
    }

    @MainActor
    func testNoTimeTodaySaysWhenTheTaskLastRan() async {
        XCTAssertEqual(text(total(0), "2026-11-15T13:12:00.000Z"), "Last tracked yesterday")
        XCTAssertEqual(text(total(0), "2026-11-13T13:12:00.000Z"), "Last tracked 3 days ago")
        XCTAssertEqual(text(total(0), "2026-11-04T13:12:00.000Z"), "Last tracked on 4 Nov 2026")
        XCTAssertEqual(text(total(0), nil), "Not tracked yet")
    }

    @MainActor
    func testUnavailableTotalsLeaveOutTheDuration() async {
        XCTAssertEqual(text(total(nil), "2026-11-16T08:00:00.000Z"), "Last tracked today at 8:00")
        XCTAssertEqual(text(nil, "2026-11-16T08:00:00.000Z"), "Last tracked today at 8:00")
        XCTAssertEqual(text(total(nil), running: true, "2026-11-16T08:00:00.000Z"), "Last tracked today at 8:00")
        XCTAssertEqual(text(nil, running: true, nil), "Not tracked yet")
    }
}
