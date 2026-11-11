import XCTest
@testable import TrackerClient

/// Each store refreshes the values it shows when their inputs change. These tests move the clock without a display
/// tick, so only the change under test can move the timer text or today's totals.
final class StoreRefreshTests: XCTestCase {
    private let running = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)

    @MainActor
    func testAcceptedTaskListShowsTheTimerBeforeTheHistoryArrives() async throws {
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
    func testFirstReportRequestShowsTheLoadingStatus() async throws {
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
    func testFailedReportShowsTheCachedTotals() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(running, rows: [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 0)])
        fixture.scheduler.poll?.fire()
        let report = try await fixture.client.next()
        fixture.advanceClock(by: 4)
        report.fail(BridgeFailure(message: "Report failed"))
        try await fixture.settled()
        XCTAssertEqual(fixture.features.dailyTotals.status, .cached)
        XCTAssertEqual(fixture.features.dailyTotals.dayTotal, 4)
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
        XCTAssertNil(fixture.features.tracking.error, "The error waits for the reload.")
        reload.fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertEqual(fixture.features.tracking.error, "Start rejected")
        XCTAssertFalse(fixture.features.tracking.isSubmitting)
    }

    @MainActor
    func testSendingATrackingRequestHidesTheShownErrorAtOnce() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.features.tracking.startTracking(taskID: firstTask.id)
        try await fixture.client.next().fail(BridgeFailure(message: "Start rejected"))
        try await fixture.client.next().succeed(TaskListResources(tasks: [firstTask], active: nil))
        try await fixture.settled()
        XCTAssertEqual(fixture.features.tracking.error, "Start rejected")

        fixture.features.tracking.startTracking(taskID: firstTask.id)
        XCTAssertNil(fixture.features.tracking.error)
        try await fixture.client.next().succeed(TaskListResources(tasks: [firstTask], active: activeWorklog))
        try await fixture.client.next().succeed(emptyPage)
        try await fixture.settled()
    }

    @MainActor
    func testWakeShowsTheElapsedTimeAndTodaysTotalsBeforeTheRefreshReplies() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        let rows = [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 0)]
        try await fixture.start(running, rows: rows)
        fixture.sync.sleep()
        fixture.advanceClock(by: 4)
        fixture.sync.wake()
        XCTAssertEqual(fixture.features.tracking.timerText, "00:00:34")
        XCTAssertEqual(fixture.features.dailyTotals.dayTotal, 4)
        try await fixture.client.next().succeed(TaskListTotalsRefresh(taskList: running, rows: rows))
        try await fixture.settled()
    }

    @MainActor
    func testRolloverTimerRefreshesTodaysTotals() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        let rows = [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 0)]
        try await fixture.start(running, rows: rows)
        XCTAssertEqual(fixture.features.dailyTotals.dayTotal, 0)
        let rollover = try XCTUnwrap(fixture.scheduler.active.first { !$0.repeating && $0.tolerance == 0 })
        fixture.advanceClock(by: 4)
        rollover.fire()
        XCTAssertEqual(fixture.features.dailyTotals.dayTotal, 4)
        XCTAssertEqual(fixture.features.tracking.timerText, "00:00:30", "Only a display tick moves the timer.")
        try await fixture.client.next().succeed(TaskListTotalsRefresh(taskList: running, rows: rows))
        try await fixture.settled()
    }
}
