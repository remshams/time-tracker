import XCTest
@testable import TrackerClient

final class TrackingStateTests: XCTestCase {
    @MainActor
    func testCorrectingWorklogStartReanchorsElapsedWithoutChangingItsID() async {
        let state = TrackingState()
        let clock = FakeClock()
        state.apply(activeWorklog, clock: clock)
        XCTAssertEqual(state.elapsed(clock: clock), 30)
        clock.now = clock.now.addingTimeInterval(100)
        clock.uptime += 5
        XCTAssertEqual(state.elapsed(clock: clock), 35)
        let corrected = WorklogItem(
            id: activeWorklog.id, taskId: activeWorklog.taskId,
            start: "2024-12-31T23:59:40.000Z", end: nil)

        state.apply(corrected, clock: clock)

        XCTAssertEqual(state.active, corrected)
        XCTAssertEqual(state.elapsed(clock: clock), 120)
    }

    @MainActor
    func testReplacementWorklogReanchorsElapsedEvenWhenStartIsUnchanged() async {
        let state = TrackingState()
        let clock = FakeClock()
        state.apply(activeWorklog, clock: clock)
        clock.now = clock.now.addingTimeInterval(100)
        clock.uptime += 5
        XCTAssertEqual(state.elapsed(clock: clock), 35)
        let replacement = WorklogItem(
            id: "replacement-worklog", taskId: activeWorklog.taskId,
            start: activeWorklog.start, end: nil)

        state.apply(replacement, clock: clock)

        XCTAssertEqual(state.active, replacement)
        XCTAssertEqual(state.elapsed(clock: clock), 130)
    }

    @MainActor
    func testUnchangedWorklogKeepsMonotonicElapsedAfterWallClockAdjustment() async {
        let state = TrackingState()
        let clock = FakeClock()
        state.apply(activeWorklog, clock: clock)
        clock.now = clock.now.addingTimeInterval(100)
        clock.uptime += 5

        state.apply(activeWorklog, clock: clock)

        XCTAssertEqual(state.elapsed(clock: clock), 35)
        clock.uptime += 2
        XCTAssertEqual(state.elapsed(clock: clock), 37)
    }

    @MainActor
    func testStoppingClearsElapsedAndRestartingEstablishesANewAnchor() async {
        let state = TrackingState()
        let clock = FakeClock()
        XCTAssertNil(state.elapsed(clock: clock))
        state.apply(activeWorklog, clock: clock)
        XCTAssertEqual(state.elapsed(clock: clock), 30)

        state.apply(nil, clock: clock)

        XCTAssertNil(state.active)
        XCTAssertNil(state.elapsed(clock: clock))
        clock.now = clock.now.addingTimeInterval(100)
        clock.uptime += 5
        state.apply(activeWorklog, clock: clock)
        XCTAssertEqual(state.elapsed(clock: clock), 130)
    }

    @MainActor
    func testFutureStartAndBackwardUptimeNeverProduceNegativeElapsed() async {
        let state = TrackingState()
        let clock = FakeClock()
        let future = WorklogItem(
            id: "future-worklog", taskId: firstTask.id,
            start: "2025-01-01T00:00:30.000Z", end: nil)
        state.apply(future, clock: clock)
        XCTAssertEqual(state.elapsed(clock: clock), 0)
        clock.uptime -= 5
        XCTAssertEqual(state.elapsed(clock: clock), 0)

        state.apply(activeWorklog, clock: clock)
        XCTAssertEqual(state.elapsed(clock: clock), 30)
        clock.uptime -= 5
        XCTAssertEqual(state.elapsed(clock: clock), 30)
    }
}
