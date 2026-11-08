import Foundation
import XCTest
@testable import TrackerClient

final class TrackingTests: XCTestCase {
    @MainActor
    func testTaskEligibilityAndAtomicSwitchDoNotDependOnSelection() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        XCTAssertFalse(fixture.trackingAvailability.canStart(taskID: secondTask.id))
        let tasks = [firstTask, secondTask, archivedTask]
        try await fixture.start(TaskListResources(tasks: tasks, active: activeWorklog))
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, firstTask.id)
        XCTAssertFalse(fixture.canStartSelectedTask)
        XCTAssertTrue(fixture.trackingAvailability.canStart(taskID: secondTask.id))
        let operationCount = fixture.client.operations.count
        for taskID in [firstTask.id, archivedTask.id, "missing-task"] {
            XCTAssertFalse(fixture.trackingAvailability.canStart(taskID: taskID))
            fixture.trackingActions.startTracking(taskID: taskID)
        }
        XCTAssertEqual(fixture.client.operations.count, operationCount)

        fixture.trackingActions.startTracking(taskID: secondTask.id)
        XCTAssertFalse(fixture.trackingAvailability.canStart(taskID: secondTask.id))
        let switched = WorklogItem(
            id: "switched-worklog", taskId: secondTask.id,
            start: "2025-01-01T00:00:00.000Z", end: nil)
        let start = try await fixture.client.next()
        XCTAssertEqual(
            start.operation,
            .start(
                task: secondTask.id, expected: activeWorklog.id,
                at: "2025-01-01T00:00:00.000Z"))
        start.succeed(TaskListResources(tasks: tasks, active: switched))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(
            Array(fixture.client.operations.dropFirst(operationCount)),
            [start.operation, .refresh(fixture.session.connection.settings), history.operation],
            "Switching tasks must delegate one atomic start command without a separate stop.")
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, firstTask.id)
        XCTAssertTrue(fixture.canStartSelectedTask)
        XCTAssertTrue(fixture.trackingAvailability.canStart(taskID: firstTask.id))
        XCTAssertFalse(fixture.trackingAvailability.canStart(taskID: secondTask.id))
    }

    @MainActor
    func testTaskEligibilityAllowsRefreshButRejectsFailedState() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        XCTAssertTrue(fixture.trackingAvailability.canStart(taskID: secondTask.id))
        fixture.sync.refresh()
        let refresh = try await fixture.client.next()
        XCTAssertTrue(fixture.trackingAvailability.canStart(taskID: secondTask.id))
        let operationCount = fixture.client.operations.count
        refresh.fail(BridgeFailure(message: "Tracker offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.connection.stale)
        XCTAssertFalse(fixture.trackingAvailability.canStart(taskID: secondTask.id))
        fixture.trackingActions.startTracking(taskID: secondTask.id)
        XCTAssertEqual(fixture.client.operations.count, operationCount)
        fixture.trackingActions.startTracking(taskID: secondTask.id)
        XCTAssertEqual(fixture.client.operations.count, operationCount)
    }

    @MainActor
    func testStartButtonAllowsSwitchingTaskButRejectsRunningAndArchivedSelection() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: activeWorklog))
        XCTAssertEqual(TrackerMenuInputs.current(fixture.session).runningTaskName, firstTask.name)
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, firstTask.id)
        XCTAssertFalse(fixture.canStartSelectedTask)

        fixture.sync.selectTask(secondTask.id)
        let selected = try await fixture.client.next()
        selected.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertTrue(fixture.canStartSelectedTask)

        fixture.sync.changeTab(.archived)
        let archived = try await fixture.client.next()
        archived.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, archivedTask.id)
        XCTAssertFalse(fixture.canStartSelectedTask)
    }

    @MainActor
    func testStartCapturesExpectedActiveIDAndClickTimeAndIgnoresDoubleClick() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: activeWorklog))
        fixture.trackingActions.startTracking(taskID: secondTask.id)
        fixture.clock.now.addTimeInterval(120)
        fixture.trackingActions.startTracking(taskID: secondTask.id)
        let request = try await fixture.client.next()
        XCTAssertEqual(
            request.operation,
            .start(
                task: secondTask.id, expected: activeWorklog.id,
                at: "2025-01-01T00:00:00.000Z"))
        XCTAssertTrue(fixture.client.hasPendingRequests)
        XCTAssertFalse(fixture.trackingAvailability.canStop)
        request.succeed(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: activeWorklog))
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
        try await fixture.start(TaskListResources(tasks: [firstTask, archivedTask], active: nil))
        XCTAssertTrue(fixture.canStartSelectedTask)
        fixture.trackingActions.startTracking(taskID: archivedTask.id)
        XCTAssertEqual(fixture.client.operations.count, 2)
        fixture.trackingActions.startTracking(taskID: firstTask.id)
        let request = try await fixture.client.next()
        XCTAssertEqual(
            request.operation,
            .start(
                task: firstTask.id, expected: nil,
                at: "2025-01-01T00:00:00.000Z"))
        request.succeed(TaskListResources(tasks: [firstTask, archivedTask], active: nil))
        try await fixture.settled()
    }

    @MainActor
    func testStopRejectsOldWorklogIDAndUsesClickTime() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: activeWorklog))
        fixture.trackingActions.stopTracking(worklogID: oldWorklog.id)
        XCTAssertEqual(fixture.client.operations.count, 2)
        fixture.trackingActions.stopTracking(worklogID: activeWorklog.id)
        fixture.clock.now.addTimeInterval(30)
        let request = try await fixture.client.next()
        XCTAssertEqual(request.operation, .stop(worklog: activeWorklog.id, at: "2025-01-01T00:00:00.000Z"))
        request.succeed(TaskListResources(tasks: [firstTask], active: nil))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertFalse(fixture.trackingAvailability.canStop)
    }

    @MainActor
    func testUncertainFailureShowsMessageAndReloadsBeforeEnablingTracking() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        fixture.trackingActions.startTracking(taskID: secondTask.id)
        let write = try await fixture.client.next()
        write.fail(BridgeFailure(message: "Response lost", kind: "unavailable", uncertain: true))
        let confirmation = try await fixture.client.next()
        XCTAssertEqual(confirmation.operation, .refresh(fixture.session.connection.settings))
        XCTAssertTrue(fixture.client.hasPendingRequests)
        XCTAssertFalse(fixture.trackingAvailability.canStop)
        XCTAssertFalse(fixture.trackingAvailability.canStart(taskID: secondTask.id))
        fixture.trackingActions.stopTracking(worklogID: activeWorklog.id)
        XCTAssertEqual(fixture.client.operations.count, 4)
        confirmation.succeed(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        try await fixture.settled()
        XCTAssertFalse(fixture.session.connection.stale)
        XCTAssertTrue(fixture.trackingAvailability.canStop)
        XCTAssertEqual(
            fixture.session.tracking.recordedError,
            "Could not confirm the change. Check the reloaded data before trying again. Response lost")
        fixture.trackingActions.dismissTrackingError()
        XCTAssertNil(fixture.session.tracking.recordedError)
    }

    @MainActor
    func testFailedReloadRetainsResourcesAndDisablesWrites() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        fixture.trackingActions.startTracking(taskID: secondTask.id)
        let write = try await fixture.client.next()
        write.fail(BridgeFailure(message: "Write response lost", uncertain: true))
        let confirmation = try await fixture.client.next()
        confirmation.fail(BridgeFailure(message: "Confirmation offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.active, activeWorklog)
        XCTAssertTrue(fixture.session.connection.stale)
        XCTAssertFalse(fixture.canStartSelectedTask)
        XCTAssertFalse(fixture.trackingAvailability.canStop)
        XCTAssertEqual(
            fixture.session.tracking.recordedError,
            "Could not confirm the change. Check the reloaded data before trying again. Write response lost")
        XCTAssertEqual(fixture.session.connection.message, "Confirmation offline")
    }

    @MainActor
    func testRejectedTrackingReloadsKeepsOtherFeaturesUsableAndManualRetryUsesNewTime() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(resources)
        fixture.trackingActions.startTracking(taskID: secondTask.id)
        let rejected = try await fixture.client.next()
        rejected.fail(BridgeFailure(message: "The active timer changed"))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(.local))
        XCTAssertEqual(fixture.session.tracking.recordedError, "The active timer changed")
        XCTAssertFalse(fixture.trackingAvailability.canStart(taskID: secondTask.id))
        XCTAssertFalse(fixture.trackingAvailability.canStop)
        XCTAssertTrue(fixture.taskCreation.canOpen)
        XCTAssertTrue(fixture.taskRename.canOpen)
        reload.succeed(resources)
        try await fixture.settled()
        XCTAssertTrue(fixture.trackingAvailability.canStart(taskID: secondTask.id))
        XCTAssertTrue(fixture.trackingAvailability.canStop)
        XCTAssertEqual(fixture.client.operations.count, 4)
        fixture.clock.now.addTimeInterval(10)
        fixture.trackingActions.startTracking(taskID: secondTask.id)
        XCTAssertNil(fixture.session.tracking.recordedError)
        let retried = try await fixture.client.next()
        XCTAssertEqual(
            retried.operation,
            .start(task: secondTask.id, expected: activeWorklog.id, at: "2025-01-01T00:00:10.000Z"))
        retried.succeed(resources)
        try await fixture.settled()
        XCTAssertTrue(fixture.trackingAvailability.canStop)
        XCTAssertTrue(fixture.trackingAvailability.canStart(taskID: secondTask.id))
    }

    @MainActor
    func testUnchangedPollNotifiesNoTrackingReader() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(resources)
        let probes = TrackingProbes(fixture.session.tracking)

        fixture.scheduler.poll?.fire()
        try await fixture.client.next().succeed(resources)
        try await fixture.settled()
        XCTAssertEqual(probes.changed(), [])
    }

    @MainActor
    func testDisplayTickNotifiesOnlyTimerReadersWhenTheShownSecondChanges() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        fixture.sync.setWindowVisible(true)
        try await fixture.start(TaskListResources(tasks: [firstTask], active: activeWorklog))
        XCTAssertEqual(fixture.session.tracking.timerText, "00:00:30")
        let probes = TrackingProbes(fixture.session.tracking)

        fixture.clock.uptime += 0.4
        fixture.scheduler.display?.fire()
        XCTAssertEqual(probes.changed(), [])
        fixture.clock.uptime += 0.6
        fixture.scheduler.display?.fire()
        XCTAssertEqual(probes.changed(), ["timerText"])
        XCTAssertEqual(fixture.session.tracking.timerText, "00:00:31")
    }

    @MainActor
    func testStartingTrackingNotifiesSubmittingThenActiveAndTimerReaders() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        XCTAssertEqual(fixture.session.tracking.timerText, "Idle")
        let probes = TrackingProbes(fixture.session.tracking)

        fixture.trackingActions.startTracking(taskID: firstTask.id)
        XCTAssertEqual(probes.changed(), ["isSubmitting"])
        try await fixture.client.next().succeed(TaskListResources(tasks: [firstTask], active: activeWorklog))
        // The new worklog runs the selected task, so the session reloads its history.
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(probes.changed(), ["active", "isSubmitting", "timerText"])
        XCTAssertEqual(fixture.session.tracking.active, activeWorklog)
        XCTAssertFalse(fixture.session.tracking.isSubmitting)
        XCTAssertEqual(fixture.session.tracking.timerText, "00:00:30")
    }

    @MainActor
    func testRejectedTrackingShowsTheErrorWithTheReloadedDataAndDismissingClearsIt() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources)
        let probes = TrackingProbes(fixture.session.tracking)

        fixture.trackingActions.startTracking(taskID: firstTask.id)
        XCTAssertEqual(probes.changed(), ["isSubmitting"])
        try await fixture.client.next().fail(BridgeFailure(message: "Start rejected"))
        let reload = try await fixture.client.next()
        XCTAssertEqual(fixture.session.tracking.recordedError, "Start rejected")
        XCTAssertNil(fixture.session.tracking.error)
        XCTAssertEqual(probes.changed(), [])
        reload.succeed(resources)
        try await fixture.settled()
        XCTAssertEqual(probes.changed(), ["error", "isSubmitting"])
        XCTAssertEqual(fixture.session.tracking.error, "Start rejected")
        fixture.trackingActions.dismissTrackingError()
        XCTAssertEqual(probes.changed(), ["error"])
        XCTAssertNil(fixture.session.tracking.error)
    }

    @MainActor
    func testANewTrackingRequestClearsTheShownErrorWhenItStarts() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources)
        fixture.trackingActions.startTracking(taskID: firstTask.id)
        try await fixture.client.next().fail(BridgeFailure(message: "Start rejected"))
        try await fixture.client.next().succeed(resources)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.tracking.error, "Start rejected")

        fixture.trackingActions.startTracking(taskID: firstTask.id)
        XCTAssertNil(fixture.session.tracking.error)
        try await fixture.client.next().fail(BridgeFailure(message: "Start rejected again"))
        try await fixture.client.next().succeed(resources)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.tracking.error, "Start rejected again")
    }
}

/// Reports which tracking properties changed, in declaration order, as views reading each one would see them.
@MainActor
private final class TrackingProbes {
    private let probes: [(String, ObservationProbe)]

    init(_ store: TrackingStore) {
        probes = [
            ("active", ObservationProbe { _ = store.active }),
            ("error", ObservationProbe { _ = store.error }),
            ("isSubmitting", ObservationProbe { _ = store.isSubmitting }),
            ("timerText", ObservationProbe { _ = store.timerText }),
        ]
    }

    func changed() -> [String] { probes.filter { $0.1.consume() }.map(\.0) }
}
