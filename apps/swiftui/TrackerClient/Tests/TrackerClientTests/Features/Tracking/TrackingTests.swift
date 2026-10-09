import Foundation
import XCTest
@testable import TrackerClient

final class TrackingTests: XCTestCase {
    @MainActor
    func testQueuedStartRechecksTheActiveWorklogBeforeWriting() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let tasks = [firstTask, secondTask]
        let initial = TrackerSnapshot(tasks: tasks, active: activeWorklog)
        try await fixture.start(initial)
        fixture.session.select(secondTask.id)
        let poll = try await fixture.client.next()
        fixture.session.startTracking(taskID: secondTask.id)
        poll.succeed(emptyPage)
        let preflight = try await fixture.client.next()
        XCTAssertEqual(preflight.operation, .snapshot)
        let replacement = WorklogItem(
            id: "replacement-worklog", taskId: firstTask.id, start: activeWorklog.start, end: nil)
        preflight.succeed(TrackerSnapshot(tasks: tasks, active: replacement))
        try await fixture.settled()

        XCTAssertEqual(fixture.session.active, replacement)
        XCTAssertEqual(
            fixture.session.trackingError,
            "Tracking changed while Start was waiting. Review the current timer and try again.")
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .start = $0 { return true }; return false
            })
    }

    @MainActor
    func testTaskEligibilityAndAtomicSwitchDoNotDependOnSelection() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        XCTAssertFalse(fixture.session.canStartTracking(taskID: secondTask.id))
        let tasks = [firstTask, secondTask, archivedTask]
        try await fixture.start(TrackerSnapshot(tasks: tasks, active: activeWorklog))
        XCTAssertEqual(fixture.session.selectedTaskID, firstTask.id)
        XCTAssertFalse(fixture.session.canStartSelectedTask)
        XCTAssertTrue(fixture.session.canStartTracking(taskID: secondTask.id))
        let operationCount = fixture.client.operations.count
        for taskID in [firstTask.id, archivedTask.id, "missing-task"] {
            XCTAssertFalse(fixture.session.canStartTracking(taskID: taskID))
            fixture.session.startTracking(taskID: taskID)
        }
        XCTAssertEqual(fixture.client.operations.count, operationCount)

        fixture.session.startTracking(taskID: secondTask.id)
        XCTAssertFalse(fixture.session.canStartTracking(taskID: secondTask.id))
        let switched = WorklogItem(
            id: "switched-worklog", taskId: secondTask.id,
            start: "2025-01-01T00:00:00.000Z", end: nil)
        let start = try await fixture.client.next()
        XCTAssertEqual(
            start.operation,
            .start(
                task: secondTask.id, expected: activeWorklog.id,
                at: "2025-01-01T00:00:00.000Z"))
        start.succeed(TrackerSnapshot(tasks: tasks, active: switched))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(
            Array(fixture.client.operations.dropFirst(operationCount)), [start.operation, history.operation],
            "Switching tasks must delegate one atomic start command without a separate stop.")
        XCTAssertEqual(fixture.session.selectedTaskID, firstTask.id)
        XCTAssertTrue(fixture.session.canStartSelectedTask)
        XCTAssertTrue(fixture.session.canStartTracking(taskID: firstTask.id))
        XCTAssertFalse(fixture.session.canStartTracking(taskID: secondTask.id))
    }

    @MainActor
    func testTaskEligibilityAllowsRefreshButRejectsFailedStateAndShutdown() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        XCTAssertTrue(fixture.session.canStartTracking(taskID: secondTask.id))
        fixture.session.refresh()
        let refresh = try await fixture.client.next()
        XCTAssertTrue(fixture.session.canStartTracking(taskID: secondTask.id))
        let operationCount = fixture.client.operations.count
        fixture.session.startTracking(taskID: secondTask.id)
        XCTAssertEqual(fixture.client.operations.count, operationCount)
        refresh.fail(BridgeFailure(message: "Tracker offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.isStale)
        XCTAssertFalse(fixture.session.canStartTracking(taskID: secondTask.id))
        fixture.session.startTracking(taskID: secondTask.id)
        XCTAssertEqual(fixture.client.operations.count, operationCount)
        fixture.session.shutdown()
        XCTAssertFalse(fixture.session.canStartTracking(taskID: secondTask.id))
        fixture.session.startTracking(taskID: secondTask.id)
        XCTAssertEqual(fixture.client.operations.count, operationCount)
    }

    @MainActor
    func testStartButtonAllowsSwitchingTaskButRejectsRunningAndArchivedSelection() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask, secondTask, archivedTask], active: activeWorklog))
        XCTAssertEqual(fixture.session.runningTaskName, firstTask.name)
        XCTAssertEqual(fixture.session.selectedTaskID, firstTask.id)
        XCTAssertFalse(fixture.session.canStartSelectedTask)

        fixture.session.select(secondTask.id)
        let selected = try await fixture.client.next()
        selected.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertTrue(fixture.session.canStartSelectedTask)

        fixture.session.changeTab(.archived)
        let archived = try await fixture.client.next()
        archived.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.selectedTaskID, archivedTask.id)
        XCTAssertFalse(fixture.session.canStartSelectedTask)
    }

    @MainActor
    func testStartCapturesExpectedActiveIDAndClickTimeAndIgnoresDoubleClick() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask, secondTask, archivedTask], active: activeWorklog))
        fixture.session.startTracking(taskID: secondTask.id)
        fixture.clock.now.addTimeInterval(120)
        fixture.session.startTracking(taskID: secondTask.id)
        let request = try await fixture.client.next()
        XCTAssertEqual(
            request.operation,
            .start(
                task: secondTask.id, expected: activeWorklog.id,
                at: "2025-01-01T00:00:00.000Z"))
        XCTAssertTrue(fixture.session.isBusy)
        XCTAssertFalse(fixture.session.canStopTracking)
        request.succeed(TrackerSnapshot(tasks: [firstTask, secondTask, archivedTask], active: activeWorklog))
        try await fixture.settled()
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .start = $0 { return true }; return false
            }.count, 1)
    }

    @MainActor
    func testStartFromIdleCapturesNilExpectedIDAndRejectsArchivedTask() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask, archivedTask], active: nil))
        XCTAssertTrue(fixture.session.canStartSelectedTask)
        fixture.session.startTracking(taskID: archivedTask.id)
        XCTAssertEqual(fixture.client.operations.count, 2)
        fixture.session.startTracking(taskID: firstTask.id)
        let request = try await fixture.client.next()
        XCTAssertEqual(
            request.operation,
            .start(
                task: firstTask.id, expected: nil,
                at: "2025-01-01T00:00:00.000Z"))
        request.succeed(TrackerSnapshot(tasks: [firstTask, archivedTask], active: nil))
        try await fixture.settled()
    }

    @MainActor
    func testStopRejectsOldWorklogIDAndUsesClickTime() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask], active: activeWorklog))
        fixture.session.stopTracking(worklogID: oldWorklog.id)
        XCTAssertEqual(fixture.client.operations.count, 2)
        fixture.session.stopTracking(worklogID: activeWorklog.id)
        fixture.clock.now.addTimeInterval(30)
        let request = try await fixture.client.next()
        XCTAssertEqual(request.operation, .stop(worklog: activeWorklog.id, at: "2025-01-01T00:00:00.000Z"))
        request.succeed(TrackerSnapshot(tasks: [firstTask], active: nil))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertFalse(fixture.session.canStopTracking)
    }

    @MainActor
    func testBusyRequestsSerializeAndCoalesceRefreshBeforeHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        fixture.session.refresh()
        let refresh = try await fixture.client.next()
        fixture.session.refresh()
        fixture.session.refresh()
        fixture.session.select(secondTask.id)
        XCTAssertEqual(fixture.client.operations.count, 3)
        refresh.succeed(TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        let queuedRefresh = try await fixture.client.next()
        XCTAssertEqual(queuedRefresh.operation, .refresh(.local))
        queuedRefresh.succeed(TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: secondTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.client.operations.count, 5)
    }

    @MainActor
    func testFailedWriteConfirmsSnapshotBeforeEnablingAnotherCommand() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog))
        fixture.session.startTracking(taskID: secondTask.id)
        let write = try await fixture.client.next()
        write.fail(BridgeFailure(message: "Response lost", kind: "unavailable", uncertain: true))
        let confirmation = try await fixture.client.next()
        XCTAssertEqual(confirmation.operation, .snapshot)
        XCTAssertTrue(fixture.session.isBusy)
        XCTAssertTrue(fixture.session.isStale)
        fixture.session.stopTracking(worklogID: activeWorklog.id)
        XCTAssertEqual(fixture.client.operations.count, 4)
        confirmation.succeed(TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog))
        try await fixture.settled()
        XCTAssertFalse(fixture.session.isStale)
        XCTAssertTrue(fixture.session.canStopTracking)
        XCTAssertEqual(fixture.session.trackingError, "Response lost")
        fixture.session.dismissTrackingError()
        XCTAssertNil(fixture.session.trackingError)
    }

    @MainActor
    func testFailedReconciliationRetainsSnapshotAndDisablesWrites() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog))
        fixture.session.startTracking(taskID: secondTask.id)
        let write = try await fixture.client.next()
        write.fail(BridgeFailure(message: "Write response lost", uncertain: true))
        let confirmation = try await fixture.client.next()
        confirmation.fail(BridgeFailure(message: "Confirmation offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.active, activeWorklog)
        XCTAssertTrue(fixture.session.isStale)
        XCTAssertFalse(fixture.session.canStartSelectedTask)
        XCTAssertFalse(fixture.session.canStopTracking)
        XCTAssertEqual(fixture.session.trackingError, "Write response lost")
        XCTAssertEqual(fixture.session.connectionMessage, "Confirmation offline")
    }
}
