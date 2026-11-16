import Foundation
import XCTest
@testable import TrackerClient

private func task(_ id: String, _ name: String, archived: Bool = false) -> TaskItem {
    TaskItem(id: id, name: name, archived: archived, latestStart: nil)
}

final class DailyTotalShareTests: XCTestCase {
    @MainActor
    func testSharesRunLargestFirstThenByNameThenByID() {
        let tasks = [task("c", "Admin"), task("a", "Client"), task("b", "Admin"), task("d", "Review")]
        let shares = DailyTotalShare.shares(
            of: tasks, durations: ["a": 3600, "b": 1800, "c": 1800, "d": 3600.9])
        XCTAssertEqual(shares.map(\.taskID), ["a", "d", "b", "c"])
        XCTAssertEqual(shares.map(\.seconds), [3600, 3600, 1800, 1800])
    }

    @MainActor
    func testTasksWithoutAWholeSecondOrADurationAreLeftOut() {
        let tasks = [task("a", "Client"), task("b", "Admin"), task("c", "Review"), task("d", "Idle")]
        let shares = DailyTotalShare.shares(of: tasks, durations: ["a": 60, "b": 0.9, "c": -5])
        XCTAssertEqual(shares.map(\.taskID), ["a"])
        XCTAssertEqual(shares.first?.fraction, 1)
        XCTAssertEqual(DailyTotalShare.shares(of: tasks, durations: [:]), [])
        XCTAssertEqual(DailyTotalShare.shares(of: [], durations: ["a": 60]), [], "Only catalog tasks get a share.")
    }

    @MainActor
    func testArchivedTasksWithTimeTodayGetAShare() {
        let shares = DailyTotalShare.shares(
            of: [task("a", "Client"), task("b", "Old", archived: true)], durations: ["a": 30, "b": 90])
        XCTAssertEqual(shares.map(\.taskID), ["b", "a"])
    }

    @MainActor
    func testFractionsSplitTheSharedTimeAndRoundToPercent() {
        let shares = DailyTotalShare.shares(
            of: [task("a", "Client"), task("b", "Admin"), task("c", "Review")],
            durations: ["a": 7200, "b": 3600, "c": 1])
        XCTAssertEqual(shares.map(\.fraction), [7200.0 / 10801, 3600.0 / 10801, 1.0 / 10801])
        XCTAssertEqual(shares.map(\.percent), [67, 33, 0])
        XCTAssertEqual(DailyTotalShare(taskID: "a", name: "A", seconds: 1, fraction: 0.125).percent, 13)
        XCTAssertEqual(DailyTotalShare(taskID: "a", name: "A", seconds: 1, fraction: 0.124).percent, 12)
    }

    @MainActor
    func testAccessibilityLabelNamesTheTaskItsTimeAndItsPercent() {
        let share = DailyTotalShare(taskID: "a", name: "Client A", seconds: 3840, fraction: 1.0 / 3)
        XCTAssertEqual(share.id, "a")
        XCTAssertEqual(share.accessibilityLabel, "Client A, 1 hour, 4 minutes, 33 percent")
    }

    @MainActor
    func testTaskTotalOffersItsDurationAndWhetherItHasTime() {
        let unavailable = TaskDailyTotalPresentation(seconds: nil, explanation: "")
        XCTAssertNil(unavailable.duration)
        XCTAssertFalse(unavailable.hasTime)
        let zero = TaskDailyTotalPresentation(seconds: 0, explanation: "")
        XCTAssertEqual(zero.duration, DurationText(seconds: 0))
        XCTAssertFalse(zero.hasTime)
        let timed = TaskDailyTotalPresentation(seconds: 1, explanation: "")
        XCTAssertEqual(timed.duration?.hoursAndMinutes, "0:00")
        XCTAssertEqual(timed.duration?.seconds, ":01")
        XCTAssertTrue(timed.hasTime)
    }
}
