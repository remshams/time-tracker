import XCTest
@testable import TrackerClient

/// `TrackerSync.publish()` recomputes the values derived from the clock and shows recorded errors. These tests move
/// the clock without a timer tick, so only the publish under test can move the timer text.
final class TrackerSyncPublishTests: XCTestCase {
    private let running = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)

    @MainActor
    func testAcceptedTaskListPublishesBeforeTheHistoryArrives() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        fixture.sync.start()
        try await fixture.client.next().succeed(running)
        let history = try await fixture.client.next()
        XCTAssertEqual(fixture.features.tracking.timerText, "00:00:30")
        history.succeed(emptyPage)
        try await fixture.settled()
    }

    @MainActor
    func testChangingThePausePreferencePublishes() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.advanceClock(by: 4)
        fixture.trackingAutomation.setPauseOnScreenLock(true)
        XCTAssertEqual(fixture.features.tracking.timerText, "00:00:34")
    }

    @MainActor
    func testLockingAndUnlockingTheScreenPublishes() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.advanceClock(by: 4)
        fixture.sync.setScreenLocked(true, at: fixture.clock.now)
        XCTAssertEqual(fixture.features.tracking.timerText, "00:00:34")
        fixture.advanceClock(by: 4)
        fixture.sync.setScreenLocked(false, at: fixture.clock.now)
        XCTAssertEqual(fixture.features.tracking.timerText, "00:00:38")
    }

    @MainActor
    func testFirstReportRequestPublishesTheLoadingStatus() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        fixture.sync.start()
        try await fixture.client.next().succeed(emptyTaskListResources)
        let report = try await fixture.client.next()
        XCTAssertEqual(fixture.features.dailyTotals.status, .loading)
        report.succeed(TaskListTotalsRefresh(taskList: emptyTaskListResources, rows: []))
        try await fixture.settled()
        XCTAssertEqual(fixture.features.dailyTotals.status, .current)
    }

    @MainActor
    func testFailedReportPublishes() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(running, rows: [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 0)])
        fixture.scheduler.poll?.fire()
        let report = try await fixture.client.next()
        fixture.advanceClock(by: 4)
        report.fail(BridgeFailure(message: "Report failed"))
        try await fixture.settled()
        XCTAssertEqual(fixture.features.tracking.timerText, "00:00:34")
        XCTAssertEqual(fixture.features.dailyTotals.status, .cached)
    }

    @MainActor
    func testConnectionFailurePublishes() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.sync.refresh()
        let refresh = try await fixture.client.next()
        fixture.advanceClock(by: 4)
        refresh.fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertEqual(fixture.features.tracking.timerText, "00:00:34")
    }

    @MainActor
    func testFailedTrackingRequestShowsItsErrorWhenTheReloadAlsoFails() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.features.tracking.startTracking(taskID: firstTask.id)
        try await fixture.client.next().fail(BridgeFailure(message: "Start rejected"))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.features.connection.settings))
        XCTAssertNil(fixture.features.tracking.error, "The error waits for the publish after the reload.")
        reload.fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertEqual(fixture.features.tracking.error, "Start rejected")
        XCTAssertFalse(fixture.features.tracking.isSubmitting)
    }

    @MainActor
    func testSendingATrackingRequestPublishesBeforeTheReply() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.advanceClock(by: 4)
        fixture.features.tracking.startTracking(taskID: secondTask.id)
        XCTAssertEqual(fixture.features.tracking.timerText, "00:00:34")
        try await fixture.client.next().succeed(running)
        try await fixture.settled()
    }

    @MainActor
    func testRolloverTimerPublishes() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        let rows = [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 0)]
        try await fixture.start(running, rows: rows)
        let rollover = try XCTUnwrap(fixture.scheduler.active.first { !$0.repeating && $0.tolerance == 0 })
        fixture.advanceClock(by: 4)
        rollover.fire()
        XCTAssertEqual(fixture.features.tracking.timerText, "00:00:34")
        try await fixture.client.next().succeed(TaskListTotalsRefresh(taskList: running, rows: rows))
        try await fixture.settled()
    }
}
