import XCTest
@testable import TrackerClient

final class TrackingStoreTests: XCTestCase {
    @MainActor
    func testCorrectingWorklogStartReanchorsElapsedWithoutChangingItsID() async {
        let clock = FakeClock()
        let store = TrackingStore(clock: clock)
        store.acceptTaskList(active: activeWorklog)
        XCTAssertEqual(store.elapsed(), 30)
        clock.now = clock.now.addingTimeInterval(100)
        clock.uptime += 5
        XCTAssertEqual(store.elapsed(), 35)
        let corrected = WorklogItem(
            id: activeWorklog.id, taskId: activeWorklog.taskId,
            start: "2024-12-31T23:59:40.000Z", end: nil)

        store.acceptTaskList(active: corrected)

        XCTAssertEqual(store.active, corrected)
        XCTAssertEqual(store.elapsed(), 120)
    }

    @MainActor
    func testReplacementWorklogReanchorsElapsedEvenWhenStartIsUnchanged() async {
        let clock = FakeClock()
        let store = TrackingStore(clock: clock)
        store.acceptTaskList(active: activeWorklog)
        clock.now = clock.now.addingTimeInterval(100)
        clock.uptime += 5
        XCTAssertEqual(store.elapsed(), 35)
        let replacement = WorklogItem(
            id: "replacement-worklog", taskId: activeWorklog.taskId,
            start: activeWorklog.start, end: nil)

        store.acceptTaskList(active: replacement)

        XCTAssertEqual(store.active, replacement)
        XCTAssertEqual(store.elapsed(), 130)
    }

    @MainActor
    func testUnchangedWorklogKeepsMonotonicElapsedAfterWallClockAdjustment() async {
        let clock = FakeClock()
        let store = TrackingStore(clock: clock)
        store.acceptTaskList(active: activeWorklog)
        clock.now = clock.now.addingTimeInterval(100)
        clock.uptime += 5

        store.acceptTaskList(active: activeWorklog)

        XCTAssertEqual(store.elapsed(), 35)
        clock.uptime += 2
        XCTAssertEqual(store.elapsed(), 37)
    }

    @MainActor
    func testStoppingClearsElapsedAndRestartingEstablishesANewAnchor() async {
        let clock = FakeClock()
        let store = TrackingStore(clock: clock)
        XCTAssertNil(store.elapsed())
        store.acceptTaskList(active: activeWorklog)
        XCTAssertEqual(store.elapsed(), 30)

        store.acceptTaskList(active: nil)

        XCTAssertNil(store.active)
        XCTAssertNil(store.elapsed())
        clock.now = clock.now.addingTimeInterval(100)
        clock.uptime += 5
        store.acceptTaskList(active: activeWorklog)
        XCTAssertEqual(store.elapsed(), 130)
    }

    @MainActor
    func testFutureStartAndBackwardUptimeNeverProduceNegativeElapsed() async {
        let clock = FakeClock()
        let store = TrackingStore(clock: clock)
        let future = WorklogItem(
            id: "future-worklog", taskId: firstTask.id,
            start: "2025-01-01T00:00:30.000Z", end: nil)
        store.acceptTaskList(active: future)
        XCTAssertEqual(store.elapsed(), 0)
        clock.uptime -= 5
        XCTAssertEqual(store.elapsed(), 0)

        store.acceptTaskList(active: activeWorklog)
        XCTAssertEqual(store.elapsed(), 30)
        clock.uptime -= 5
        XCTAssertEqual(store.elapsed(), 30)
    }
}

@MainActor
private extension TrackingStore {
    /// Builds the store for tests of the elapsed time, which send no tracking request.
    convenience init(clock: FakeClock) {
        self.init(
            client: FakeClient(), clock: clock, connection: ConnectionStore(settings: .local),
            catalog: TaskCatalogStore(), gate: PresentationGate())
    }
}
