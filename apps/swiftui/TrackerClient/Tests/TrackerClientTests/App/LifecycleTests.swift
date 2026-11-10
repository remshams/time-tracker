import Foundation
import XCTest
@testable import TrackerClient

final class LifecycleTests: XCTestCase {
    @MainActor
    func testPollingUsesVisibleBackoffAndProtocolFailureBlocksUntilExplicitRefresh() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        XCTAssertEqual(fixture.scheduler.poll?.delay, 60)
        fixture.sync.setWindowVisible(true)
        let shown = try await fixture.client.next()
        shown.succeed(emptyTaskListResources)
        try await fixture.settled()
        XCTAssertEqual(fixture.scheduler.poll?.delay, 5)

        for delay in [5.0, 10.0, 20.0, 40.0, 60.0, 60.0] {
            fixture.scheduler.poll?.fire()
            let refresh = try await fixture.client.next()
            XCTAssertEqual(refresh.operation, .refresh(.local))
            refresh.fail(BridgeFailure(message: "Offline", kind: "unavailable"))
            try await fixture.settled()
            XCTAssertEqual(fixture.scheduler.poll?.delay, delay)
        }
        fixture.sync.refresh()
        let incompatible = try await fixture.client.next()
        incompatible.fail(BridgeFailure(message: "Unsupported version", kind: "protocol"))
        try await fixture.settled()
        XCTAssertEqual(fixture.features.connection.statusText, "Incompatible server")
        XCTAssertNil(fixture.scheduler.poll)
        fixture.sync.refresh()
        let explicit = try await fixture.client.next()
        explicit.succeed(emptyTaskListResources)
        try await fixture.settled()
        XCTAssertFalse(fixture.features.connection.stale)
        XCTAssertEqual(fixture.scheduler.poll?.delay, 5)
        fixture.sync.setWindowVisible(false)
        XCTAssertEqual(fixture.scheduler.poll?.delay, 60)
    }

    @MainActor
    func testDisplayTimerNeedsRunningTaskAndVisibilityAndDoesNotFetchData() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(resources)
        XCTAssertNil(fixture.scheduler.display)
        XCTAssertEqual(fixture.features.tracking.timerText, "00:00:30")
        fixture.sync.setMenuVisible(true)
        let refresh = try await fixture.client.next()
        refresh.succeed(resources)
        try await fixture.settled()
        let timer = fixture.scheduler.display
        XCTAssertEqual(timer?.delay, 1)
        XCTAssertEqual(timer?.tolerance, 0.2)
        let operationCount = fixture.client.operations.count
        fixture.advanceClock(by: 9)
        timer?.fire()
        XCTAssertEqual(fixture.features.tracking.timerText, "00:00:39")
        XCTAssertEqual(fixture.client.operations.count, operationCount)
        fixture.sync.setMenuVisible(false)
        XCTAssertTrue(timer?.cancelled == true)
        XCTAssertNil(fixture.scheduler.display)
    }

    @MainActor
    func testMenuBarClockRunsWhileTrackingWithTheSettingOnAndRepublishesEachMinute() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        fixture.features.menuBar.setClockEnabled(true)
        XCTAssertNil(fixture.scheduler.menuBarClock, "The clock waits for the sync to start.")
        let resources = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(resources, rows: [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 0)])
        let clock = try XCTUnwrap(fixture.scheduler.menuBarClock)
        XCTAssertEqual(clock.tolerance, 5)
        XCTAssertNil(fixture.scheduler.display, "The clock runs while no window or menu is open.")
        XCTAssertEqual(fixture.features.dailyTotals.dayTotal, 0)
        let operationCount = fixture.client.operations.count
        fixture.advanceClock(by: 60)
        clock.fire()
        XCTAssertEqual(fixture.features.dailyTotals.dayTotal, 60)
        XCTAssertEqual(fixture.client.operations.count, operationCount)
        fixture.features.menuBar.setClockEnabled(false)
        XCTAssertTrue(clock.cancelled)
        XCTAssertNil(fixture.scheduler.menuBarClock)
        fixture.features.menuBar.setClockEnabled(true)
        XCTAssertNotNil(fixture.scheduler.menuBarClock)
        fixture.features.menuBar.setClockEnabled(true)
        XCTAssertEqual(fixture.scheduler.active.filter { $0.delay == 60 && $0.repeating }.count, 1)
        fixture.sync.refresh()
        let stopped = try await fixture.client.next()
        stopped.succeed(TaskListTotalsRefresh(taskList: TaskListResources(tasks: [firstTask], active: nil), rows: []))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertNil(fixture.scheduler.menuBarClock, "The total stays the same while no timer runs.")
    }

    @MainActor
    func testMenuBarClockStopsWhileAsleepAndResumesOnWake() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: activeWorklog))
        fixture.features.menuBar.setClockEnabled(true)
        let clock = try XCTUnwrap(fixture.scheduler.menuBarClock)
        fixture.sync.sleep()
        XCTAssertTrue(clock.cancelled)
        XCTAssertNil(fixture.scheduler.menuBarClock)
        fixture.sync.wake()
        XCTAssertNotNil(fixture.scheduler.menuBarClock)
        let refresh = try await fixture.client.next()
        refresh.succeed(TaskListResources(tasks: [firstTask], active: activeWorklog))
        try await fixture.settled()
        XCTAssertNotNil(fixture.scheduler.menuBarClock)
    }

    @MainActor
    func testVisibleIdleAppDoesNotScheduleDisplayTimer() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.sync.setWindowVisible(true)
        let refresh = try await fixture.client.next()
        refresh.succeed(emptyTaskListResources)
        try await fixture.settled()
        XCTAssertNil(fixture.scheduler.display)
        XCTAssertEqual(fixture.features.tracking.timerText, "Idle")
    }

    @MainActor
    func testElapsedUsesMonotonicClockUntilWakeResetsWallTimeAnchor() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(resources)
        fixture.sync.setWindowVisible(true)
        let refresh = try await fixture.client.next()
        refresh.succeed(resources)
        try await fixture.settled()
        fixture.clock.uptime += 7
        fixture.clock.now.addTimeInterval(3_600)
        XCTAssertEqual(fixture.features.tracking.elapsed(), 37)
        fixture.sync.sleep()
        XCTAssertTrue(fixture.scheduler.active.isEmpty)
        let count = fixture.client.operations.count
        fixture.sync.refresh()
        XCTAssertEqual(fixture.client.operations.count, count)
        fixture.clock.now.addTimeInterval(300)
        fixture.sync.wake()
        let wake = try await fixture.client.next()
        XCTAssertEqual(wake.operation, .refresh(.local))
        XCTAssertEqual(fixture.features.tracking.elapsed(), 3_930)
        wake.succeed(resources)
        try await fixture.settled()
        XCTAssertNotNil(fixture.scheduler.display)
        XCTAssertEqual(fixture.scheduler.poll?.delay, 5)
    }

    @MainActor
    func testPollingPolicyClampsFailureCountAndNeverPollsHiddenWindowFasterThanMinute() {
        XCTAssertEqual(TrackerPollingPolicy.interval(visible: true, failures: -10), 5)
        XCTAssertEqual(TrackerPollingPolicy.interval(visible: true, failures: 3), 20)
        XCTAssertEqual(TrackerPollingPolicy.interval(visible: true, failures: 100), 60)
        XCTAssertEqual(TrackerPollingPolicy.interval(visible: false, failures: 0), 60)
        XCTAssertEqual(TrackerPollingPolicy.interval(visible: false, failures: 100), 60)
    }
}
