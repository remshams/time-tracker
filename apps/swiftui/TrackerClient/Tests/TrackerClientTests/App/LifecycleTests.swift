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
        fixture.session.setWindowVisible(true)
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
        fixture.session.refresh()
        let incompatible = try await fixture.client.next()
        incompatible.fail(BridgeFailure(message: "Unsupported version", kind: "protocol"))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.connectionStatusText, "Incompatible server")
        XCTAssertNil(fixture.scheduler.poll)
        fixture.session.refresh()
        let explicit = try await fixture.client.next()
        explicit.succeed(emptyTaskListResources)
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
        let resources = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(resources)
        XCTAssertNil(fixture.scheduler.display)
        XCTAssertEqual(fixture.session.timerDisplayText, "00:00:30")
        fixture.session.menuOpened()
        let refresh = try await fixture.client.next()
        refresh.succeed(resources)
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
        let resources = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(resources)
        fixture.session.setWindowVisible(true)
        let refresh = try await fixture.client.next()
        refresh.succeed(resources)
        try await fixture.settled()
        let oldPoll = fixture.scheduler.poll
        let oldDisplay = fixture.scheduler.display
        let previousNow = fixture.session.now
        fixture.session.setWindowVisible(false)
        fixture.clock.now.addTimeInterval(100)
        let count = fixture.client.operations.count
        oldPoll?.deliverQueuedAction()
        oldDisplay?.deliverQueuedAction()
        XCTAssertFalse(fixture.client.hasPendingRequests)
        XCTAssertEqual(fixture.client.operations.count, count)
        XCTAssertEqual(fixture.session.now, previousNow)
        XCTAssertEqual(fixture.scheduler.poll?.delay, 60)
        XCTAssertNil(fixture.scheduler.display)
    }

    @MainActor
    func testReplacedTimersCannotPublishAfterSessionResumes() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        fixture.session.setWindowVisible(true)
        let resources = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(resources)
        let replacedTimers = fixture.scheduler.active
        XCTAssertTrue(replacedTimers.contains { $0.repeating })
        XCTAssertTrue(replacedTimers.contains { !$0.repeating && $0.tolerance == 0 })

        fixture.session.sleep()
        fixture.clock.now.addTimeInterval(30)
        fixture.clock.uptime += 30
        fixture.session.wake()
        let resumed = try await fixture.client.next()
        resumed.succeed(TaskListTotalsRefresh(taskList: resources, rows: []))
        try await fixture.settled()
        let currentDisplay = try XCTUnwrap(fixture.scheduler.display)
        let publishedNow = fixture.session.now
        let operationCount = fixture.client.operations.count
        let scheduledCount = fixture.scheduler.scheduled.count
        var changes = 0
        fixture.session.onChange = { changes += 1 }
        fixture.clock.now.addTimeInterval(10)
        fixture.clock.uptime += 10

        replacedTimers.forEach { $0.deliverQueuedAction() }
        XCTAssertEqual(changes, 0, "Callbacks from replaced timers must not publish into the resumed session.")
        XCTAssertEqual(fixture.session.now, publishedNow)
        XCTAssertFalse(fixture.client.hasPendingRequests)
        XCTAssertEqual(fixture.client.operations.count, operationCount)
        XCTAssertEqual(fixture.scheduler.scheduled.count, scheduledCount)

        currentDisplay.fire()
        XCTAssertEqual(changes, 1)
        XCTAssertEqual(fixture.session.now, fixture.clock.now)
        XCTAssertEqual(fixture.client.operations.count, operationCount)
    }

    @MainActor
    func testVisibleIdleSessionDoesNotScheduleDisplayTimer() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.setWindowVisible(true)
        let refresh = try await fixture.client.next()
        refresh.succeed(emptyTaskListResources)
        try await fixture.settled()
        XCTAssertNil(fixture.scheduler.display)
        XCTAssertEqual(fixture.session.timerDisplayText, "Idle")
    }

    @MainActor
    func testElapsedUsesMonotonicClockUntilWakeResetsWallTimeAnchor() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(resources)
        fixture.session.setWindowVisible(true)
        let refresh = try await fixture.client.next()
        refresh.succeed(resources)
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
        wake.succeed(resources)
        try await fixture.settled()
        XCTAssertNotNil(fixture.scheduler.display)
        XCTAssertEqual(fixture.scheduler.poll?.delay, 5)
    }

    func testPollingPolicyClampsFailureCountAndNeverPollsHiddenWindowFasterThanMinute() {
        XCTAssertEqual(TrackerPollingPolicy.interval(visible: true, failures: -10), 5)
        XCTAssertEqual(TrackerPollingPolicy.interval(visible: true, failures: 3), 20)
        XCTAssertEqual(TrackerPollingPolicy.interval(visible: true, failures: 100), 60)
        XCTAssertEqual(TrackerPollingPolicy.interval(visible: false, failures: 0), 60)
        XCTAssertEqual(TrackerPollingPolicy.interval(visible: false, failures: 100), 60)
    }
}
