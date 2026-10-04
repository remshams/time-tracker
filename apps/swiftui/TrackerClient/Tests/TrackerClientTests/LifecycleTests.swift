import Foundation
import XCTest
@testable import TrackerClient

final class LifecycleTests: XCTestCase {
    @MainActor
    func testShutdownFromStartupNotificationPreventsOpeningClient() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        fixture.session.onChange = {
            if fixture.session.isBusy {
                fixture.session.onChange = nil
                fixture.session.shutdown()
            }
        }
        let opened = XCTestExpectation(description: "Stopped session must not open the client")
        opened.isInverted = true
        let observer = Task {
            do {
                _ = try await fixture.client.next(timeout: 4)
                opened.fulfill()
            } catch {}
        }

        fixture.session.start()
        let result = await XCTWaiter.fulfillment(of: [opened], timeout: 2)
        XCTAssertEqual(result, .completed, "The client was opened after a startup observer stopped the session.")
        XCTAssertTrue(fixture.client.operations.isEmpty)
        XCTAssertFalse(fixture.session.isBusy)
        XCTAssertTrue(fixture.scheduler.active.isEmpty)
        fixture.cleanup()
        try await fixture.taskValue(observer)
    }

    @MainActor
    func testPollingUsesVisibleBackoffAndProtocolFailureBlocksUntilExplicitRefresh() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        XCTAssertEqual(fixture.scheduler.poll?.delay, 60)
        fixture.session.setWindowVisible(true)
        let shown = try await fixture.client.next()
        shown.succeed(emptySnapshot)
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
        fixture.session.refresh()
        let incompatible = try await fixture.client.next()
        incompatible.fail(BridgeFailure(message: "Unsupported version", kind: "protocol"))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.connectionStatusText, "Incompatible server")
        XCTAssertNil(fixture.scheduler.poll)
        fixture.session.refresh()
        let explicit = try await fixture.client.next()
        explicit.succeed(emptySnapshot)
        try await fixture.settled()
        XCTAssertFalse(fixture.session.isStale)
        XCTAssertEqual(fixture.scheduler.poll?.delay, 5)
        fixture.session.setWindowVisible(false)
        XCTAssertEqual(fixture.scheduler.poll?.delay, 60)
    }

    @MainActor
    func testDisplayTimerNeedsRunningTaskAndVisibilityAndDoesNotFetchData() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(snapshot)
        XCTAssertNil(fixture.scheduler.display)
        XCTAssertEqual(fixture.session.timerDisplayText, "00:00:30")
        fixture.session.menuOpened()
        let refresh = try await fixture.client.next()
        refresh.succeed(snapshot)
        try await fixture.settled()
        let timer = fixture.scheduler.display
        XCTAssertEqual(timer?.delay, 1)
        XCTAssertEqual(timer?.tolerance, 0.2)
        let operationCount = fixture.client.operations.count
        fixture.clock.uptime += 9
        fixture.clock.now.addTimeInterval(9)
        timer?.fire()
        XCTAssertEqual(fixture.session.now, fixture.clock.now)
        XCTAssertEqual(fixture.session.timerDisplayText, "00:00:39")
        XCTAssertEqual(fixture.client.operations.count, operationCount)
        fixture.session.menuClosed()
        XCTAssertTrue(timer?.cancelled == true)
        XCTAssertNil(fixture.scheduler.display)
    }

    @MainActor
    func testCancelledTimerCallbacksAlreadyQueuedCannotRefreshOrUpdateDisplay() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(snapshot)
        fixture.session.setWindowVisible(true)
        let refresh = try await fixture.client.next()
        refresh.succeed(snapshot)
        try await fixture.settled()
        let oldPoll = fixture.scheduler.poll
        let oldDisplay = fixture.scheduler.display
        let previousNow = fixture.session.now
        fixture.session.setWindowVisible(false)
        fixture.clock.now.addTimeInterval(100)
        let count = fixture.client.operations.count
        oldPoll?.deliverQueuedAction()
        oldDisplay?.deliverQueuedAction()
        XCTAssertFalse(fixture.session.isBusy)
        XCTAssertEqual(fixture.client.operations.count, count)
        XCTAssertEqual(fixture.session.now, previousNow)
        XCTAssertEqual(fixture.scheduler.poll?.delay, 60)
        XCTAssertNil(fixture.scheduler.display)
    }

    @MainActor
    func testVisibleIdleSessionDoesNotScheduleDisplayTimer() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.setWindowVisible(true)
        let refresh = try await fixture.client.next()
        refresh.succeed(emptySnapshot)
        try await fixture.settled()
        XCTAssertNil(fixture.scheduler.display)
        XCTAssertEqual(fixture.session.timerDisplayText, "Idle")
    }

    @MainActor
    func testNestedMenusKeepVisibilityUntilLastMenuCloses() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.menuOpened()
        let refresh = try await fixture.client.next()
        refresh.succeed(emptySnapshot)
        try await fixture.settled()
        let operationCount = fixture.client.operations.count
        fixture.session.menuOpened()
        fixture.session.menuClosed()
        XCTAssertEqual(fixture.scheduler.poll?.delay, 5)
        XCTAssertEqual(fixture.client.operations.count, operationCount)
        fixture.session.menuClosed()
        fixture.session.menuClosed()
        XCTAssertEqual(fixture.scheduler.poll?.delay, 60)
    }

    @MainActor
    func testElapsedUsesMonotonicClockUntilWakeResetsWallTimeAnchor() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(snapshot)
        fixture.session.setWindowVisible(true)
        let refresh = try await fixture.client.next()
        refresh.succeed(snapshot)
        try await fixture.settled()
        fixture.clock.uptime += 7
        fixture.clock.now.addTimeInterval(3_600)
        XCTAssertEqual(fixture.session.elapsed, 37)
        fixture.session.sleep()
        XCTAssertTrue(fixture.scheduler.active.isEmpty)
        let count = fixture.client.operations.count
        fixture.session.refresh()
        XCTAssertEqual(fixture.client.operations.count, count)
        fixture.clock.now.addTimeInterval(300)
        fixture.session.wake()
        let wake = try await fixture.client.next()
        XCTAssertEqual(wake.operation, .refresh(.local))
        XCTAssertEqual(fixture.session.elapsed, 3_930)
        wake.succeed(snapshot)
        try await fixture.settled()
        XCTAssertNotNil(fixture.scheduler.display)
        XCTAssertEqual(fixture.scheduler.poll?.delay, 5)
    }

    @MainActor
    func testShutdownCancelsTimersAndIgnoresLateConnectionResult() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask], active: activeWorklog))
        fixture.session.setWindowVisible(true)
        let refresh = try await fixture.client.next()
        refresh.succeed(TrackerSnapshot(tasks: [firstTask], active: activeWorklog))
        try await fixture.settled()
        let tokens = fixture.scheduler.active
        let connecting = Task { await fixture.session.connect(serverSettings) }
        let candidate = try await fixture.client.next()
        fixture.session.shutdown()
        XCTAssertTrue(tokens.allSatisfy { $0.cancelled })
        XCTAssertTrue(fixture.scheduler.active.isEmpty)
        candidate.succeed(TrackerSnapshot(tasks: [secondTask], active: nil))
        let connected = try await fixture.taskValue(connecting)
        XCTAssertFalse(connected)
        XCTAssertEqual(fixture.session.connectionSettings, .local)
        XCTAssertEqual(fixture.session.tasks, [firstTask])
        XCTAssertEqual(fixture.session.active, activeWorklog)
        XCTAssertTrue(fixture.settings.writes.isEmpty)
        let operationCount = fixture.client.operations.count
        fixture.session.start()
        fixture.session.refresh()
        fixture.session.wake()
        fixture.session.menuOpened()
        tokens.forEach { $0.deliverQueuedAction() }
        XCTAssertEqual(fixture.client.operations.count, operationCount)
        XCTAssertTrue(fixture.scheduler.active.isEmpty)
    }

    func testPollingPolicyClampsFailureCountAndNeverPollsHiddenWindowFasterThanMinute() {
        XCTAssertEqual(TrackerPollingPolicy.interval(visible: true, failures: -10), 5)
        XCTAssertEqual(TrackerPollingPolicy.interval(visible: true, failures: 3), 20)
        XCTAssertEqual(TrackerPollingPolicy.interval(visible: true, failures: 100), 60)
        XCTAssertEqual(TrackerPollingPolicy.interval(visible: false, failures: 0), 60)
        XCTAssertEqual(TrackerPollingPolicy.interval(visible: false, failures: 100), 60)
    }
}
