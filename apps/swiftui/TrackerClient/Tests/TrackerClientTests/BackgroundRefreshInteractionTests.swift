import Foundation
import XCTest
@testable import TrackerClient

final class BackgroundRefreshInteractionTests: XCTestCase {
    @MainActor
    func testQueuedStartCapturesTaskExpectedWorklogAndClickTimeWithoutOverlappingRequests() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(snapshot)
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        XCTAssertTrue(fixture.session.isBusy)
        XCTAssertFalse(fixture.session.isBlockingControls)
        XCTAssertTrue(fixture.session.canStartTracking(taskID: secondTask.id))
        XCTAssertTrue(fixture.session.canStopTracking)
        let count = fixture.client.operations.count

        fixture.session.startTracking(taskID: secondTask.id)
        XCTAssertTrue(fixture.session.isBlockingControls)
        XCTAssertFalse(fixture.session.canStopTracking)
        fixture.clock.now.addTimeInterval(120)
        fixture.session.startTracking(taskID: firstTask.id)
        XCTAssertEqual(fixture.client.operations.count, count)
        poll.succeed(snapshot)

        let start = try await fixture.client.next()
        XCTAssertEqual(start.operation, .start(task: secondTask.id, expected: activeWorklog.id,
                                             at: "2025-01-01T00:00:00.000Z"))
        start.succeed(snapshot)
        try await fixture.settled()
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 1)
        XCTAssertEqual(fixture.client.operations.count, count + 1)
        XCTAssertFalse(fixture.session.isBlockingControls)
    }

    @MainActor
    func testQueuedStopCapturesWorklogAndClickTimeAndIgnoresDoubleClick() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(snapshot)
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        let count = fixture.client.operations.count
        XCTAssertTrue(fixture.session.canStopTracking)
        fixture.session.stopTracking(worklogID: activeWorklog.id)
        fixture.clock.now.addTimeInterval(90)
        fixture.session.stopTracking(worklogID: activeWorklog.id)
        XCTAssertEqual(fixture.client.operations.count, count)
        poll.succeed(snapshot)

        let stop = try await fixture.client.next()
        XCTAssertEqual(stop.operation, .stop(worklog: activeWorklog.id, at: "2025-01-01T00:00:00.000Z"))
        stop.succeed(TrackerSnapshot(tasks: [firstTask], active: nil))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 1)
        XCTAssertEqual(fixture.client.operations.count, count + 2)
        XCTAssertNil(fixture.session.active)
    }

    @MainActor
    func testQueuedStartKeepsClickedTaskAndRunsBeforeCoalescedRefreshAndSelectionHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(snapshot)
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        fixture.session.startTracking(taskID: try XCTUnwrap(fixture.session.selectedTaskID))
        fixture.session.select(secondTask.id)
        fixture.session.refresh()
        fixture.session.refresh()
        poll.succeed(snapshot)
        let start = try await fixture.client.next()
        XCTAssertEqual(start.operation, .start(task: firstTask.id, expected: nil,
                                             at: "2025-01-01T00:00:00.000Z"))
        start.succeed(snapshot)
        let coalesced = try await fixture.client.next()
        XCTAssertEqual(coalesced.operation, .refresh(.local))
        coalesced.succeed(snapshot)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: secondTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 1)
        XCTAssertEqual(fixture.session.selectedTaskID, secondTask.id)
    }

    @MainActor
    func testQueuedStartRejectsRemovedOrArchivedTargetAfterPoll() async throws {
        for target in [TaskItem?.none, TaskItem(id: secondTask.id, name: secondTask.name,
                                              archived: true, latestStart: nil)] {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            try await fixture.start(TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
            fixture.session.refresh()
            let poll = try await fixture.client.next()
            fixture.session.startTracking(taskID: secondTask.id)
            let count = fixture.client.operations.count
            poll.succeed(TrackerSnapshot(tasks: [firstTask] + (target.map { [$0] } ?? []), active: nil))
            try await fixture.settled()
            XCTAssertEqual(fixture.client.operations.count, count)
            XCTAssertNotNil(fixture.session.trackingError, "A rejected queued click must explain why it did not run.")
            XCTAssertFalse(fixture.session.isBlockingControls)
        }
    }

    @MainActor
    func testQueuedStartRejectsChangedActiveWorklogAfterPoll() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let tasks = [firstTask, secondTask]
        try await fixture.start(TrackerSnapshot(tasks: tasks, active: activeWorklog))
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        fixture.session.startTracking(taskID: secondTask.id)
        let replacement = WorklogItem(id: "replacement-worklog", taskId: firstTask.id,
                                      start: "2025-01-01T00:00:10.000Z", end: nil)
        poll.succeed(TrackerSnapshot(tasks: tasks, active: replacement))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.active, replacement)
        XCTAssertNotNil(fixture.session.trackingError)
        XCTAssertFalse(fixture.client.operations.contains { if case .start = $0 { return true }; return false })
    }

    @MainActor
    func testQueuedStopNeverStopsReplacementWorklogAfterPoll() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask], active: activeWorklog))
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        fixture.session.stopTracking(worklogID: activeWorklog.id)
        let replacement = WorklogItem(id: "replacement-worklog", taskId: firstTask.id,
                                      start: "2025-01-01T00:00:10.000Z", end: nil)
        poll.succeed(TrackerSnapshot(tasks: [firstTask], active: replacement))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.active, replacement)
        XCTAssertNotNil(fixture.session.trackingError)
        XCTAssertFalse(fixture.client.operations.contains { if case .stop = $0 { return true }; return false })
    }

    @MainActor
    func testFailedPollRejectsQueuedTrackingWithoutWriting() async throws {
        for stop in [false, true] {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            try await fixture.start(TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog))
            fixture.session.refresh()
            let poll = try await fixture.client.next()
            if stop { fixture.session.stopTracking(worklogID: activeWorklog.id) }
            else { fixture.session.startTracking(taskID: secondTask.id) }
            let count = fixture.client.operations.count
            poll.fail(BridgeFailure(message: "Server unavailable", kind: "unavailable"))
            try await fixture.settled()
            XCTAssertEqual(fixture.client.operations.count, count)
            XCTAssertTrue(fixture.session.isStale)
            XCTAssertNotNil(fixture.session.trackingError)
            XCTAssertFalse(fixture.session.canStopTracking)
            XCTAssertFalse(fixture.session.isBlockingControls)
        }
    }

    @MainActor
    func testReportReadKeepsControlsEnabledAndQueuesTracking() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(snapshot)
        fixture.scheduler.poll?.fire()
        let report = try await fixture.client.next()
        XCTAssertFalse(fixture.session.isBlockingControls)
        XCTAssertTrue(fixture.session.canStartTracking(taskID: secondTask.id))
        XCTAssertTrue(fixture.session.canStopTracking)
        let count = fixture.client.operations.count
        fixture.session.startTracking(taskID: secondTask.id)
        XCTAssertEqual(fixture.client.operations.count, count)
        report.succeed(TrackerReport(snapshot: snapshot, rows: []))
        let start = try await fixture.client.next()
        XCTAssertEqual(start.operation, .start(task: secondTask.id, expected: activeWorklog.id,
                                             at: "2025-01-01T00:00:00.000Z"))
        start.succeed(snapshot)
        let totals = try await fixture.client.next()
        totals.succeed(TrackerReport(snapshot: snapshot, rows: []))
        try await fixture.settled()
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 1)
    }

    @MainActor
    func testHistoryReadKeepsControlsEnabledAndConfirmsStateBeforeQueuedWrite() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(snapshot)
        fixture.session.retryHistory()
        let history = try await fixture.client.next()
        XCTAssertFalse(fixture.session.isBlockingControls)
        XCTAssertTrue(fixture.session.canStopTracking)
        let count = fixture.client.operations.count
        fixture.session.stopTracking(worklogID: activeWorklog.id)
        XCTAssertEqual(fixture.client.operations.count, count)
        history.succeed(emptyPage)
        let confirmation = try await fixture.client.next()
        XCTAssertEqual(confirmation.operation, .snapshot)
        confirmation.succeed(snapshot)
        let stop = try await fixture.client.next()
        XCTAssertEqual(stop.operation, .stop(worklog: activeWorklog.id, at: "2025-01-01T00:00:00.000Z"))
        stop.succeed(snapshot)
        try await fixture.settled()
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 1)
    }

    @MainActor
    func testSettingsTestWaitsForPollWithoutDroppingClickOrOverlappingRequests() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        let count = fixture.client.operations.count
        XCTAssertFalse(fixture.session.isBlockingControls)
        let testing = Task { try await fixture.session.testConnection(serverSettings) }
        try await fixture.waitUntil("connection test to queue") { fixture.session.isBlockingControls }
        XCTAssertEqual(fixture.client.operations.count, count)
        poll.succeed(emptySnapshot)
        let test = try await fixture.client.next()
        XCTAssertEqual(test.operation, .test(serverSettings))
        test.tested()
        try await fixture.taskValue(testing)
        try await fixture.settled()
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 1)
        XCTAssertTrue(fixture.settings.writes.isEmpty)
    }

    @MainActor
    func testSettingsConnectWaitsForPollAndSavesOnlySuccessfulCandidate() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        let count = fixture.client.operations.count
        let connecting = Task { await fixture.session.connect(serverSettings) }
        try await fixture.waitUntil("connection change to queue") { fixture.session.isBlockingControls }
        XCTAssertEqual(fixture.client.operations.count, count)
        XCTAssertTrue(fixture.settings.writes.isEmpty)
        poll.succeed(emptySnapshot)
        let connect = try await fixture.client.next()
        XCTAssertEqual(connect.operation, .connect(serverSettings))
        connect.succeed(emptySnapshot)
        let connected = try await fixture.taskValue(connecting)
        XCTAssertTrue(connected)
        try await fixture.settled()
        XCTAssertEqual(fixture.settings.writes, [serverSettings])
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 1)
    }

    @MainActor
    func testCancelledQueuedConnectionTestCompletesWithoutClientCall() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        let count = fixture.client.operations.count
        let testing = Task { try await fixture.session.testConnection(serverSettings) }
        try await fixture.waitUntil("connection test to queue") { fixture.session.isBlockingControls }
        testing.cancel()
        do {
            try await fixture.taskValue(testing)
            XCTFail("Cancelling a queued test must throw cancellation.")
        } catch is CancellationError {}
        XCTAssertFalse(fixture.session.isBlockingControls)
        poll.succeed(emptySnapshot)
        try await fixture.settled()
        XCTAssertEqual(fixture.client.operations.count, count)
        XCTAssertTrue(fixture.settings.writes.isEmpty)
    }

    @MainActor
    func testCancelledQueuedConnectionChangeCompletesWithoutSavingOrClientCall() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        let count = fixture.client.operations.count
        let connecting = Task { await fixture.session.connect(serverSettings) }
        try await fixture.waitUntil("connection change to queue") { fixture.session.isBlockingControls }
        connecting.cancel()
        let connected = try await fixture.taskValue(connecting)
        XCTAssertFalse(connected)
        XCTAssertFalse(fixture.session.isBlockingControls)
        poll.succeed(emptySnapshot)
        try await fixture.settled()
        XCTAssertEqual(fixture.client.operations.count, count)
        XCTAssertTrue(fixture.settings.writes.isEmpty)
        XCTAssertEqual(fixture.session.connectionSettings, .local)
    }

    @MainActor
    func testSleepAndShutdownCompleteQueuedConnectionTestWithoutClientCall() async throws {
        for sleeping in [false, true] {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            try await fixture.start()
            fixture.session.refresh()
            let poll = try await fixture.client.next()
            let count = fixture.client.operations.count
            let testing = Task { try await fixture.session.testConnection(serverSettings) }
            try await fixture.waitUntil("connection test to queue") { fixture.session.isBlockingControls }
            if sleeping { fixture.session.sleep() } else { fixture.session.shutdown() }
            do {
                try await fixture.taskValue(testing)
                XCTFail("Stopping queued work must cancel the connection test.")
            } catch is CancellationError {}
            poll.succeed(emptySnapshot)
            try await fixture.settled()
            XCTAssertEqual(fixture.client.operations.count, count)
            XCTAssertTrue(fixture.settings.writes.isEmpty)
        }
    }

    @MainActor
    func testSleepAndShutdownCompleteQueuedConnectionChangeWithoutSaving() async throws {
        for sleeping in [false, true] {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            try await fixture.start()
            fixture.session.refresh()
            let poll = try await fixture.client.next()
            let count = fixture.client.operations.count
            let connecting = Task { await fixture.session.connect(serverSettings) }
            try await fixture.waitUntil("connection change to queue") { fixture.session.isBlockingControls }
            if sleeping { fixture.session.sleep() } else { fixture.session.shutdown() }
            let connected = try await fixture.taskValue(connecting)
            XCTAssertFalse(connected)
            poll.succeed(emptySnapshot)
            try await fixture.settled()
            XCTAssertEqual(fixture.client.operations.count, count)
            XCTAssertTrue(fixture.settings.writes.isEmpty)
            XCTAssertEqual(fixture.session.connectionSettings, .local)
        }
    }

    @MainActor
    func testSleepAndShutdownDiscardQueuedTrackingBeforePollCompletes() async throws {
        for sleeping in [false, true] {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            let snapshot = TrackerSnapshot(tasks: [firstTask], active: activeWorklog)
            try await fixture.start(snapshot)
            fixture.session.refresh()
            let poll = try await fixture.client.next()
            fixture.session.stopTracking(worklogID: activeWorklog.id)
            let count = fixture.client.operations.count
            if sleeping { fixture.session.sleep() } else { fixture.session.shutdown() }
            poll.succeed(snapshot)
            try await fixture.settled()
            XCTAssertEqual(fixture.client.operations.count, count)
            XCTAssertEqual(fixture.session.active, activeWorklog)
            XCTAssertNotNil(fixture.session.trackingError)
            XCTAssertFalse(fixture.session.canStopTracking)
        }
    }

    @MainActor
    func testScreenLockDuringSettingsHandoffCancelsUnsentRequests() async throws {
        for changingConnection in [false, true] {
            let fixture = Fixture(pauseOnScreenLock: true)
            defer { fixture.cleanup() }
            try await fixture.start()
            fixture.session.refresh()
            let poll = try await fixture.client.next()
            let count = fixture.client.operations.count
            let action = Task { () throws -> Bool in
                if changingConnection { return await fixture.session.connect(serverSettings) }
                try await fixture.session.testConnection(serverSettings)
                return true
            }
            try await fixture.waitUntil("settings action to queue") { fixture.session.isBlockingControls }
            var publications = 0
            fixture.session.onChange = {
                guard fixture.session.isBusy, fixture.session.isBlockingControls else { return }
                publications += 1
                // The snapshot publishes before the reservation transfers to the next operation.
                if publications == 2 {
                    fixture.session.onChange = nil
                    fixture.session.screenLocked(at: fixture.clock.now)
                }
            }
            poll.succeed(emptySnapshot)
            do {
                let succeeded = try await fixture.taskValue(action)
                XCTAssertTrue(changingConnection)
                XCTAssertFalse(succeeded)
            } catch is CancellationError {
                XCTAssertFalse(changingConnection)
            }
            try await fixture.settled()
            XCTAssertEqual(publications, 2)
            XCTAssertEqual(fixture.client.operations.count, count)
            XCTAssertTrue(fixture.settings.writes.isEmpty)
            XCTAssertFalse(fixture.session.isBlockingControls)
        }
    }

    @MainActor
    func testShutdownDuringConnectionNotificationPreventsUnsentRequest() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        let count = fixture.client.operations.count
        fixture.session.onChange = {
            guard fixture.session.isChangingConnection else { return }
            fixture.session.onChange = nil
            fixture.session.shutdown()
        }
        let connecting = Task { await fixture.session.connect(serverSettings) }
        let connected = try await fixture.taskValue(connecting)
        XCTAssertFalse(connected)
        XCTAssertEqual(fixture.client.operations.count, count)
        XCTAssertTrue(fixture.settings.writes.isEmpty)
        XCTAssertEqual(fixture.session.connectionSettings, .local)
    }
}
