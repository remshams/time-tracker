import XCTest
@testable import TrackerClient

final class TrackingStoreTests: XCTestCase {
    @MainActor
    func testCorrectingWorklogStartReanchorsElapsedWithoutChangingItsID() async {
        let store = TrackingStore()
        let clock = FakeClock()
        store.acceptTaskList(active: activeWorklog, clock: clock)
        XCTAssertEqual(store.elapsed(clock: clock), 30)
        clock.now = clock.now.addingTimeInterval(100)
        clock.uptime += 5
        XCTAssertEqual(store.elapsed(clock: clock), 35)
        let corrected = WorklogItem(
            id: activeWorklog.id, taskId: activeWorklog.taskId,
            start: "2024-12-31T23:59:40.000Z", end: nil)

        store.acceptTaskList(active: corrected, clock: clock)

        XCTAssertEqual(store.active, corrected)
        XCTAssertEqual(store.elapsed(clock: clock), 120)
    }

    @MainActor
    func testReplacementWorklogReanchorsElapsedEvenWhenStartIsUnchanged() async {
        let store = TrackingStore()
        let clock = FakeClock()
        store.acceptTaskList(active: activeWorklog, clock: clock)
        clock.now = clock.now.addingTimeInterval(100)
        clock.uptime += 5
        XCTAssertEqual(store.elapsed(clock: clock), 35)
        let replacement = WorklogItem(
            id: "replacement-worklog", taskId: activeWorklog.taskId,
            start: activeWorklog.start, end: nil)

        store.acceptTaskList(active: replacement, clock: clock)

        XCTAssertEqual(store.active, replacement)
        XCTAssertEqual(store.elapsed(clock: clock), 130)
    }

    @MainActor
    func testUnchangedWorklogKeepsMonotonicElapsedAfterWallClockAdjustment() async {
        let store = TrackingStore()
        let clock = FakeClock()
        store.acceptTaskList(active: activeWorklog, clock: clock)
        clock.now = clock.now.addingTimeInterval(100)
        clock.uptime += 5

        store.acceptTaskList(active: activeWorklog, clock: clock)

        XCTAssertEqual(store.elapsed(clock: clock), 35)
        clock.uptime += 2
        XCTAssertEqual(store.elapsed(clock: clock), 37)
    }

    @MainActor
    func testStoppingClearsElapsedAndRestartingEstablishesANewAnchor() async {
        let store = TrackingStore()
        let clock = FakeClock()
        XCTAssertNil(store.elapsed(clock: clock))
        store.acceptTaskList(active: activeWorklog, clock: clock)
        XCTAssertEqual(store.elapsed(clock: clock), 30)

        store.acceptTaskList(active: nil, clock: clock)

        XCTAssertNil(store.active)
        XCTAssertNil(store.elapsed(clock: clock))
        clock.now = clock.now.addingTimeInterval(100)
        clock.uptime += 5
        store.acceptTaskList(active: activeWorklog, clock: clock)
        XCTAssertEqual(store.elapsed(clock: clock), 130)
    }

    @MainActor
    func testFutureStartAndBackwardUptimeNeverProduceNegativeElapsed() async {
        let store = TrackingStore()
        let clock = FakeClock()
        let future = WorklogItem(
            id: "future-worklog", taskId: firstTask.id,
            start: "2025-01-01T00:00:30.000Z", end: nil)
        store.acceptTaskList(active: future, clock: clock)
        XCTAssertEqual(store.elapsed(clock: clock), 0)
        clock.uptime -= 5
        XCTAssertEqual(store.elapsed(clock: clock), 0)

        store.acceptTaskList(active: activeWorklog, clock: clock)
        XCTAssertEqual(store.elapsed(clock: clock), 30)
        clock.uptime -= 5
        XCTAssertEqual(store.elapsed(clock: clock), 30)
    }
}
