import XCTest
@testable import TrackerClient

/// `TrackerSync.publish()` recomputes the values derived from the clock and shows recorded errors. These tests move
/// the clock without a timer tick, so only the publish under test can move the timer text. The selection and reload
/// publishes are in `TrackerSyncSelectionAndReloadTests`.
final class TrackerSyncPublishTests: XCTestCase {
    private let running = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)

    @MainActor
    private func timerText(_ fixture: Fixture) -> String { fixture.session.tracking.timerText }

    @MainActor
    func testAcceptedTaskListPublishesBeforeTheHistoryArrives() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        fixture.sync.start()
        try await fixture.client.next().succeed(running)
        let history = try await fixture.client.next()
        XCTAssertEqual(timerText(fixture), "00:00:30")
        history.succeed(emptyPage)
        try await fixture.settled()
    }

    @MainActor
    func testSleepPublishes() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.advanceClock(by: 4)
        fixture.sync.sleep()
        XCTAssertEqual(timerText(fixture), "00:00:34")
    }

    @MainActor
    func testRetryingTheHistoryPublishesBeforeAndAfterThePage() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.advanceClock(by: 4)
        fixture.sync.retryHistory()
        XCTAssertEqual(timerText(fixture), "00:00:34")
        let page = try await fixture.client.next()
        fixture.advanceClock(by: 4)
        page.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(timerText(fixture), "00:00:38")
    }

    @MainActor
    func testLoadingOlderWorklogsPublishes() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.sync.retryHistory()
        try await fixture.client.next().succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "older", reset: false))
        try await fixture.settled()
        fixture.advanceClock(by: 4)
        fixture.sync.loadOlder()
        XCTAssertEqual(timerText(fixture), "00:00:34")
        let older = try await fixture.client.next()
        XCTAssertEqual(older.operation, .history(task: firstTask.id, cursor: "older"))
        older.succeed(emptyPage)
        try await fixture.settled()
    }

    @MainActor
    func testRejectedConnectionChangePublishesWhenItStartsAndEnds() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.advanceClock(by: 4)
        let connecting = Task { @MainActor in await fixture.sync.connect(serverSettings) }
        let request = try await fixture.client.next()
        XCTAssertEqual(timerText(fixture), "00:00:34")
        fixture.advanceClock(by: 4)
        request.fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        let connected = try await fixture.taskValue(connecting)
        XCTAssertFalse(connected)
        XCTAssertEqual(timerText(fixture), "00:00:38")
    }

    @MainActor
    func testChangingThePausePreferencePublishes() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.advanceClock(by: 4)
        fixture.sync.setPauseOnScreenLock(true)
        XCTAssertEqual(timerText(fixture), "00:00:34")
    }

    @MainActor
    func testLockingAndUnlockingTheScreenPublishes() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.advanceClock(by: 4)
        fixture.sync.setScreenLocked(true, at: fixture.clock.now)
        XCTAssertEqual(timerText(fixture), "00:00:34")
        fixture.advanceClock(by: 4)
        fixture.sync.setScreenLocked(false, at: fixture.clock.now)
        XCTAssertEqual(timerText(fixture), "00:00:38")
    }

    @MainActor
    func testFirstReportRequestPublishesTheLoadingStatus() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        fixture.sync.start()
        try await fixture.client.next().succeed(emptyTaskListResources)
        let report = try await fixture.client.next()
        XCTAssertEqual(fixture.session.dailyTotals.status, .loading)
        report.succeed(TaskListTotalsRefresh(taskList: emptyTaskListResources, rows: []))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.dailyTotals.status, .current)
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
        XCTAssertEqual(timerText(fixture), "00:00:34")
        XCTAssertEqual(fixture.session.dailyTotals.status, .cached)
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
        XCTAssertEqual(timerText(fixture), "00:00:34")
    }

    @MainActor
    func testFailedTrackingRequestShowsItsErrorWhenTheReloadAlsoFails() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.trackingActions.startTracking(taskID: firstTask.id)
        try await fixture.client.next().fail(BridgeFailure(message: "Start rejected"))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connection.settings))
        XCTAssertNil(fixture.session.tracking.error, "The error waits for the publish after the reload.")
        reload.fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.tracking.error, "Start rejected")
        XCTAssertFalse(fixture.session.tracking.isSubmitting)
    }

    @MainActor
    func testSendingATrackingRequestPublishesBeforeTheReply() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.advanceClock(by: 4)
        fixture.trackingActions.startTracking(taskID: secondTask.id)
        XCTAssertEqual(timerText(fixture), "00:00:34")
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
        XCTAssertEqual(timerText(fixture), "00:00:34")
        try await fixture.client.next().succeed(TaskListTotalsRefresh(taskList: running, rows: rows))
        try await fixture.settled()
    }
}
