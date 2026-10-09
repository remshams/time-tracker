import Foundation
import XCTest
@testable import TrackerClient

final class ObserverTests: XCTestCase {
    @MainActor
    func testFailureObserverCanStopSessionBeforeOperationSchedulesAnotherPoll() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask], active: nil))
        let stopped = XCTestExpectation(description: "Failure observer stops the session during its request")
        var didStop = false
        fixture.session.onChange = {
            guard !didStop, fixture.session.isStale, fixture.session.isBusy else { return }
            didStop = true
            fixture.session.shutdown()
            stopped.fulfill()
        }

        fixture.session.refresh()
        let request = try await fixture.client.next()
        request.fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        let result = await XCTWaiter.fulfillment(of: [stopped], timeout: 2)
        XCTAssertEqual(result, .completed, "The failure observer was not notified while the request was active.")
        XCTAssertTrue(didStop)
        XCTAssertFalse(fixture.session.isBusy)
        XCTAssertFalse(fixture.session.canStartSelectedTask)
        XCTAssertFalse(fixture.session.canStopTracking)
        XCTAssertTrue(fixture.scheduler.active.isEmpty)
        XCTAssertEqual(
            fixture.client.operations,
            [
                .open(.local), .history(task: firstTask.id, cursor: nil), .refresh(.local),
            ])
    }

    @MainActor
    func testRefreshKeepsCommandsEnabledAndCancelsScheduledPoll() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: nil)
        try await fixture.start(snapshot)
        let previousPoll = try XCTUnwrap(fixture.scheduler.poll)
        XCTAssertTrue(fixture.session.canStartSelectedTask)
        var observedBusy: Bool?
        var observedStartEnabled: Bool?
        var observedPollCancelled: Bool?
        fixture.session.onChange = {
            observedBusy = fixture.session.isBusy
            observedStartEnabled = fixture.session.canStartSelectedTask
            observedPollCancelled = previousPoll.cancelled && fixture.scheduler.poll == nil
        }

        fixture.session.refresh()
        XCTAssertEqual(observedBusy, true)
        XCTAssertEqual(observedStartEnabled, true)
        XCTAssertEqual(observedPollCancelled, true)
        let request = try await fixture.client.next()
        request.succeed(snapshot)
        try await fixture.settled()
    }

    @MainActor
    func testVisibleStartupStartsDisplayTimerAndTickNotifiesWithoutFetchingData() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        fixture.session.setWindowVisible(true)
        try await fixture.start(TrackerSnapshot(tasks: [firstTask], active: activeWorklog))
        let display = try XCTUnwrap(fixture.scheduler.display)
        let operationCount = fixture.client.operations.count
        var observedNow: Date?
        var observedTimerText: String?
        fixture.session.onChange = {
            observedNow = fixture.session.now
            observedTimerText = fixture.session.timerDisplayText
        }

        fixture.clock.now.addTimeInterval(4)
        fixture.clock.uptime += 4
        display.fire()
        XCTAssertEqual(observedNow, fixture.clock.now)
        XCTAssertEqual(observedTimerText, "00:00:34")
        XCTAssertEqual(fixture.client.operations.count, operationCount)
    }

    @MainActor
    func testShowingWindowDuringRefreshNotifiesUpdatedTimeAndSchedulesDisplay() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(snapshot)
        fixture.session.refresh()
        let request = try await fixture.client.next()
        let operationCount = fixture.client.operations.count
        fixture.clock.now.addTimeInterval(4)
        var observedNow: Date?
        var observedBusy: Bool?
        var observedDisplayScheduled: Bool?
        fixture.session.onChange = {
            observedNow = fixture.session.now
            observedBusy = fixture.session.isBusy
            observedDisplayScheduled = fixture.scheduler.display != nil
        }

        fixture.session.setWindowVisible(true)
        XCTAssertEqual(observedNow, fixture.clock.now)
        XCTAssertEqual(observedBusy, true)
        XCTAssertEqual(observedDisplayScheduled, true)
        XCTAssertEqual(fixture.client.operations.count, operationCount)
        request.succeed(snapshot)
        let queuedRefresh = try await fixture.client.next()
        queuedRefresh.succeed(snapshot)
        try await fixture.settled()
    }

    @MainActor
    func testSnapshotObserverCanChooseTaskBeforeItsHistoryIsFetched() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        var choseTask = false
        fixture.session.onChange = {
            guard !choseTask, fixture.session.tasks.contains(secondTask) else { return }
            choseTask = true
            fixture.session.select(secondTask.id)
        }
        fixture.session.start()
        let open = try await fixture.client.next()
        open.succeed(TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        let history = try await fixture.client.next()
        XCTAssertTrue(choseTask)
        XCTAssertEqual(fixture.session.selectedTaskID, secondTask.id)
        XCTAssertEqual(history.operation, .history(task: secondTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
    }
}
