import Foundation
import XCTest
@testable import TrackerClient

final class TaskCreationTests: XCTestCase {
    @MainActor
    func testDraftCancellationMakesNoWriteAndEmptyNamesCannotSubmit() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        XCTAssertFalse(fixture.session.canOpenTaskCreation)
        fixture.session.openTaskCreation()
        XCTAssertFalse(fixture.session.taskCreation.isPresented)
        try await fixture.start()
        XCTAssertTrue(fixture.session.canOpenTaskCreation)
        fixture.session.openTaskCreation()
        XCTAssertFalse(fixture.session.taskCreation.canSubmit)
        fixture.session.setTaskCreationName(" \n\t ")
        fixture.session.submitTaskCreation()
        XCTAssertFalse(fixture.session.taskCreation.isSubmitting)
        fixture.session.setTaskCreationName("New task")
        XCTAssertTrue(fixture.session.taskCreation.canSubmit)
        fixture.session.cancelTaskCreation()
        XCTAssertFalse(fixture.session.taskCreation.isPresented)
        XCTAssertEqual(fixture.client.operations.count, 1)
        fixture.session.openTaskCreation()
        XCTAssertEqual(fixture.session.taskCreation.name, "")
    }

    @MainActor
    func testSuccessSelectsCreatedTaskFromArchivedWithoutChangingTracking() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let tasks = [firstTask, archivedTask]
        try await fixture.start(TrackerSnapshot(tasks: tasks, active: activeWorklog))
        fixture.session.changeTab(.archived)
        let archivedHistory = try await fixture.client.next()
        archivedHistory.succeed(emptyPage)
        try await fixture.settled()
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("New task")
        fixture.session.submitTaskCreation()
        let request = try await fixture.client.next()
        XCTAssertEqual(request.operation, .create(name: "New task", at: "2025-01-01T00:00:00.000Z"))
        fixture.session.submitTaskCreation()
        fixture.session.cancelTaskCreation()
        fixture.session.setTaskCreationName("Changed task")
        XCTAssertTrue(fixture.session.taskCreation.isPresented)
        XCTAssertEqual(fixture.session.taskCreation.name, "New task")
        let created = TaskItem(id: "created-id", name: "New task", archived: false, latestStart: nil)
        request.created(taskID: created.id, snapshot: TrackerSnapshot(tasks: tasks + [created], active: activeWorklog))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: created.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertFalse(fixture.session.taskCreation.isPresented)
        XCTAssertEqual(fixture.session.tab, .active)
        XCTAssertEqual(fixture.session.selectedTaskID, created.id)
        XCTAssertEqual(fixture.session.active, activeWorklog)
        XCTAssertEqual(fixture.session.tasks, tasks + [created])
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .create = $0 { return true }; return false
            }.count, 1)
    }

    @MainActor
    func testSubmissionQueuesBehindPollAndCapturesNameAndTime() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        XCTAssertTrue(fixture.session.canOpenTaskCreation)
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("  Task draft  ")
        fixture.session.submitTaskCreation()
        fixture.clock.now.addTimeInterval(60)
        fixture.session.setTaskCreationName("Replacement")
        fixture.session.submitTaskCreation()
        XCTAssertEqual(fixture.client.operations.count, 2)
        XCTAssertTrue(fixture.session.taskCreation.isSubmitting)
        poll.succeed(emptySnapshot)
        let creation = try await fixture.client.next()
        XCTAssertEqual(creation.operation, .create(name: "  Task draft  ", at: "2025-01-01T00:00:00.000Z"))
        creation.created(taskID: firstTask.id, snapshot: TrackerSnapshot(tasks: [firstTask], active: nil))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
    }

    @MainActor
    func testUncertainFailureRetainsFrozenIntentThroughDismissalAndRetry() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("Original task")
        fixture.session.submitTaskCreation()
        let creation = try await fixture.client.next()
        creation.fail(BridgeFailure(message: "Response lost", kind: "unavailable", uncertain: true))
        let reconcile = try await fixture.client.next()
        XCTAssertEqual(reconcile.operation, .snapshot)
        reconcile.succeed(emptySnapshot)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.taskCreation.error, "Response lost")
        XCTAssertFalse(fixture.session.taskCreation.canEditName)
        XCTAssertNil(fixture.session.trackingError)
        fixture.session.setTaskCreationName("Replacement")
        fixture.session.cancelTaskCreation()
        XCTAssertFalse(fixture.session.taskCreation.isPresented)
        fixture.session.openTaskCreation()
        XCTAssertEqual(fixture.session.taskCreation.name, "Original task")
        let connect = Task { await fixture.session.connect(.local) }
        let connected = try await fixture.taskValue(connect)
        XCTAssertFalse(connected)
        fixture.clock.now.addTimeInterval(120)
        fixture.session.submitTaskCreation()
        let retry = try await fixture.client.next()
        XCTAssertEqual(retry.operation, creation.operation)
        retry.created(taskID: firstTask.id, snapshot: TrackerSnapshot(tasks: [firstTask], active: nil))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertFalse(fixture.session.taskCreation.isPresented)
    }

    @MainActor
    func testRejectedNameCanBeEditedAndRetryUsesNewTimestamp() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("Task\u{0001}")
        fixture.session.submitTaskCreation()
        let creation = try await fixture.client.next()
        creation.fail(BridgeFailure(message: "Invalid task name", kind: "validation"))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.taskCreation.name, "Task\u{0001}")
        XCTAssertEqual(fixture.session.taskCreation.error, "Invalid task name")
        XCTAssertTrue(fixture.session.taskCreation.canEditName)
        XCTAssertFalse(fixture.session.isStale)
        fixture.clock.now.addTimeInterval(30)
        fixture.session.setTaskCreationName("Valid task")
        XCTAssertNil(fixture.session.taskCreation.error)
        fixture.session.submitTaskCreation()
        let retry = try await fixture.client.next()
        XCTAssertEqual(retry.operation, .create(name: "Valid task", at: "2025-01-01T00:00:30.000Z"))
        retry.fail(BridgeFailure(message: "Rejected", kind: "validation"))
        try await fixture.settled()
    }

    @MainActor
    func testShutdownDiscardsQueuedAndDelayedCreationResults() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("New task")
        fixture.session.submitTaskCreation()
        let creation = try await fixture.client.next()
        fixture.session.shutdown()
        creation.created(taskID: firstTask.id, snapshot: TrackerSnapshot(tasks: [firstTask], active: nil))
        await Task.yield()
        XCTAssertTrue(fixture.session.tasks.isEmpty)
        XCTAssertFalse(fixture.session.taskCreation.isPresented)
        XCTAssertFalse(fixture.session.canOpenTaskCreation)
        XCTAssertFalse(fixture.session.taskCreation.isSubmitting)
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("Later task")
        fixture.session.submitTaskCreation()
        XCTAssertEqual(fixture.client.operations.count, 2)
    }

    @MainActor
    func testDraftChangesOnlyNotifyCreationObserver() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        let observer = TrackerPresentationObserver(session: fixture.session)
        var creationUpdates = 0
        var contentUpdates = 0
        observer.onTaskCreationChange = { creationUpdates += 1 }
        observer.onContentChange = { contentUpdates += 1 }
        fixture.session.onChange = { observer.update(from: fixture.session) }
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("Draft")
        fixture.session.setTaskCreationName("Draft")
        observer.update(from: fixture.session)
        XCTAssertEqual(creationUpdates, 2)
        XCTAssertEqual(contentUpdates, 0)
        fixture.session.cancelTaskCreation()
        XCTAssertEqual(creationUpdates, 3)
        XCTAssertEqual(contentUpdates, 0)
    }

    @MainActor
    func testCreationWaitsDuringSleepAndResumesOnWake() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("Queued task")
        fixture.session.submitTaskCreation()
        fixture.session.sleep()
        poll.succeed(emptySnapshot)
        try await fixture.settled()
        XCTAssertEqual(fixture.client.operations.count, 2)
        XCTAssertTrue(fixture.session.taskCreation.isSubmitting)
        fixture.session.wake()
        let creation = try await fixture.client.next()
        XCTAssertEqual(creation.operation, .create(name: "Queued task", at: "2025-01-01T00:00:00.000Z"))
        creation.fail(BridgeFailure(message: "Rejected", kind: "validation"))
        try await fixture.settled()
    }

    @MainActor
    func testShutdownPreventsQueuedWriteAfterPollCompletes() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("Queued task")
        fixture.session.submitTaskCreation()
        fixture.session.shutdown()
        poll.succeed(emptySnapshot)
        await Task.yield()
        XCTAssertEqual(fixture.client.operations.count, 2)
        XCTAssertFalse(fixture.session.taskCreation.isSubmitting)
    }

    @MainActor
    func testFailedPollIsReconciledBeforeQueuedWrite() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("Queued task")
        fixture.session.submitTaskCreation()
        poll.fail(BridgeFailure(message: "Poll offline", kind: "unavailable"))
        let reconciliation = try await fixture.client.next()
        XCTAssertEqual(reconciliation.operation, .snapshot)
        XCTAssertTrue(fixture.session.isStale)
        reconciliation.succeed(emptySnapshot)
        let creation = try await fixture.client.next()
        XCTAssertEqual(creation.operation, .create(name: "Queued task", at: "2025-01-01T00:00:00.000Z"))
        creation.fail(BridgeFailure(message: "Invalid", kind: "validation"))
        try await fixture.settled()
        XCTAssertFalse(fixture.session.isStale)
    }

    @MainActor
    func testFailedReconciliationAllowsReopeningAndRetryAfterReadRecovery() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("Original task")
        fixture.session.submitTaskCreation()
        let creation = try await fixture.client.next()
        creation.fail(BridgeFailure(message: "Response lost", uncertain: true))
        let confirmation = try await fixture.client.next()
        confirmation.fail(BridgeFailure(message: "Still offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.isStale)
        XCTAssertEqual(fixture.session.taskCreation.error, "Response lost")
        fixture.session.cancelTaskCreation()
        fixture.session.openTaskCreation()
        XCTAssertTrue(fixture.session.taskCreation.isPresented)
        fixture.session.submitTaskCreation()
        let recovery = try await fixture.client.next()
        XCTAssertEqual(recovery.operation, .snapshot)
        recovery.succeed(emptySnapshot)
        let retry = try await fixture.client.next()
        XCTAssertEqual(retry.operation, creation.operation)
        retry.fail(BridgeFailure(message: "Rejected", kind: "validation"))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.taskCreation.canEditName)
    }

    @MainActor
    func testInvalidCreationResponsesNeverCloseSheetOrSelectUnconfirmedTask() async throws {
        for (id, tasks) in [
            ("missing", [firstTask]),
            ("", [TaskItem(id: "", name: "Invalid identity", archived: false, latestStart: nil)]),
        ] {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            try await fixture.start()
            fixture.session.openTaskCreation()
            fixture.session.setTaskCreationName("New task")
            fixture.session.submitTaskCreation()
            let creation = try await fixture.client.next()
            creation.created(taskID: id, snapshot: TrackerSnapshot(tasks: tasks, active: nil))
            let confirmation = try await fixture.client.next()
            XCTAssertEqual(confirmation.operation, .snapshot)
            confirmation.succeed(emptySnapshot)
            try await fixture.settled()
            XCTAssertTrue(fixture.session.taskCreation.isPresented)
            XCTAssertFalse(fixture.session.taskCreation.canEditName)
            XCTAssertEqual(
                fixture.session.taskCreation.error, "The creation response does not contain the created task.")
            XCTAssertNil(fixture.session.selectedTaskID)
            XCTAssertTrue(fixture.session.tasks.isEmpty)
        }
    }

    @MainActor
    func testSuccessRefreshesDailyReportBeforeCreatedTaskHistory() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(
            snapshot, rows: [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 60_000_000)])
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName(secondTask.name)
        fixture.session.submitTaskCreation()
        let creation = try await fixture.client.next()
        let createdSnapshot = TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog)
        creation.created(taskID: secondTask.id, snapshot: createdSnapshot)
        let report = try await fixture.client.next()
        guard case .report = report.operation else {
            return XCTFail("Creation must refresh daily totals before history.")
        }
        report.succeed(
            TrackerReport(
                snapshot: createdSnapshot,
                rows: [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 60_000_000)]))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: secondTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.dailyDuration(taskID: firstTask.id), 60)
        XCTAssertEqual(fixture.session.dailyDuration(taskID: secondTask.id), 0)
        XCTAssertEqual(fixture.session.selectedTaskID, secondTask.id)
        XCTAssertEqual(fixture.session.active, activeWorklog)
    }

    @MainActor
    func testRecoveryClassificationDistinguishesKnownRejectionFromUnknownOutcome() {
        XCTAssertFalse(TaskCreationState.requiresRecovery(BridgeFailure(message: "Invalid", kind: "validation")))
        for failure in [
            BridgeFailure(message: "Uncertain", uncertain: true),
            BridgeFailure(message: "Refresh", requiresRefresh: true),
            BridgeFailure(message: "Offline", kind: "unavailable"),
            BridgeFailure(message: "Malformed", kind: "protocol"),
        ] {
            XCTAssertTrue(TaskCreationState.requiresRecovery(failure))
        }
        XCTAssertTrue(TaskCreationState.requiresRecovery(CancellationError()))
    }

    @MainActor
    func testClosedDraftCannotBeChangedAndPendingIntentBlocksConnectionWithoutCancellingAutomation() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.setTaskCreationName("Hidden name")
        XCTAssertEqual(fixture.session.taskCreation.name, "")
        fixture.session.submitTaskCreation()
        XCTAssertEqual(fixture.client.operations.count, 1)
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("Pending task")
        fixture.session.submitTaskCreation()
        let creation = try await fixture.client.next()
        let observer = TrackerPresentationObserver(session: fixture.session)
        var displayedConnectionMessages: [String?] = []
        observer.onContentChange = { displayedConnectionMessages.append(fixture.session.connectionMessage) }
        fixture.session.onChange = { observer.update(from: fixture.session) }
        let connect = Task { await fixture.session.connect(serverSettings) }
        let connected = try await fixture.taskValue(connect)
        XCTAssertFalse(connected)
        XCTAssertEqual(fixture.session.connectionSettings, .local)
        XCTAssertEqual(fixture.session.connectionMessage, "Finish or retry task creation before changing connections.")
        XCTAssertEqual(displayedConnectionMessages, ["Finish or retry task creation before changing connections."])
        creation.fail(BridgeFailure(message: "Rejected", kind: "validation"))
        try await fixture.settled()
        fixture.session.cancelTaskCreation()
        let connectTask = Task { await fixture.session.connect(serverSettings) }
        let connection = try await fixture.client.next()
        XCTAssertEqual(connection.operation, .connect(serverSettings))
        connection.succeed(emptySnapshot)
        let didConnect = try await fixture.taskValue(connectTask)
        XCTAssertTrue(didConnect)
    }

    @MainActor
    func testEmbeddedNullIsRejectedBeforeCStringBoundary() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("Valid\u{0000}suffix")
        let observer = TrackerPresentationObserver(session: fixture.session)
        var displayedErrors: [String?] = []
        observer.onTaskCreationChange = { displayedErrors.append(fixture.session.taskCreation.error) }
        fixture.session.onChange = { observer.update(from: fixture.session) }
        fixture.session.submitTaskCreation()
        XCTAssertEqual(fixture.session.taskCreation.error, "Task names must not contain control characters.")
        XCTAssertEqual(fixture.session.taskCreation.name, "Valid\u{0000}suffix")
        XCTAssertTrue(fixture.session.taskCreation.canEditName)
        XCTAssertFalse(fixture.session.taskCreation.isSubmitting)
        XCTAssertEqual(fixture.client.operations.count, 1)
        XCTAssertEqual(displayedErrors, ["Task names must not contain control characters."])
    }

    @MainActor
    func testQueuedCreationAfterFailedReadShowsFailureInsteadOfRemainingSubmitting() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("Queued task")
        fixture.session.submitTaskCreation()
        poll.fail(BridgeFailure(message: "Read offline", kind: "unavailable"))
        let confirmation = try await fixture.client.next()
        XCTAssertEqual(confirmation.operation, .snapshot)
        confirmation.fail(BridgeFailure(message: "Cannot confirm connection", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertFalse(fixture.session.taskCreation.isSubmitting)
        XCTAssertEqual(fixture.session.taskCreation.name, "Queued task")
        XCTAssertEqual(fixture.session.taskCreation.error, "Cannot confirm connection")
        XCTAssertTrue(fixture.session.isStale)
        XCTAssertEqual(fixture.client.operations.count, 3)
    }

    @MainActor
    func testEditingDraftSurvivesSuccessfulConnectionChangeAndUsesNewDestination() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("Draft task")
        let connect = Task { await fixture.session.connect(serverSettings) }
        let connection = try await fixture.client.next()
        XCTAssertFalse(fixture.session.canOpenTaskCreation)
        fixture.session.submitTaskCreation()
        XCTAssertFalse(fixture.session.taskCreation.isSubmitting)
        connection.succeed(emptySnapshot)
        let connected = try await fixture.taskValue(connect)
        XCTAssertTrue(connected)
        XCTAssertEqual(fixture.session.connectionSettings, serverSettings)
        XCTAssertTrue(fixture.session.taskCreation.isPresented)
        XCTAssertEqual(fixture.session.taskCreation.name, "Draft task")
        fixture.session.submitTaskCreation()
        let creation = try await fixture.client.next()
        XCTAssertEqual(creation.operation, .create(name: "Draft task", at: "2025-01-01T00:00:00.000Z"))
        creation.fail(BridgeFailure(message: "Invalid", kind: "validation"))
        try await fixture.settled()
    }

    @MainActor
    func testSleepDuringPreflightReconciliationDefersWriteUntilWake() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("Queued task")
        fixture.session.submitTaskCreation()
        poll.fail(BridgeFailure(message: "Offline", kind: "unavailable"))
        let preflight = try await fixture.client.next()
        fixture.session.sleep()
        preflight.succeed(emptySnapshot)
        try await fixture.settled()
        XCTAssertEqual(fixture.client.operations.count, 3)
        XCTAssertTrue(fixture.session.taskCreation.isSubmitting)
        fixture.session.wake()
        let creation = try await fixture.client.next()
        XCTAssertEqual(creation.operation, .create(name: "Queued task", at: "2025-01-01T00:00:00.000Z"))
        creation.fail(BridgeFailure(message: "Invalid", kind: "validation"))
        try await fixture.settled()
    }

    @MainActor
    func testRecoveredTaskArchivedByAnotherClientIsSelectedWithoutCreatingReplacement() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("Task archived remotely")
        fixture.session.submitTaskCreation()
        let creation = try await fixture.client.next()
        creation.created(taskID: archivedTask.id, snapshot: TrackerSnapshot(tasks: [archivedTask], active: nil))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: archivedTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertFalse(fixture.session.taskCreation.isPresented)
        XCTAssertEqual(fixture.session.tab, .archived)
        XCTAssertEqual(fixture.session.selectedTaskID, archivedTask.id)
        XCTAssertEqual(fixture.client.operations.count, 3)
    }

    @MainActor
    func testRecoveredCreationPublishesClosedSheetAndReusesSelectedTaskHistory() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName(firstTask.name)
        fixture.session.submitTaskCreation()
        let creation = try await fixture.client.next()
        creation.fail(BridgeFailure(message: "Response lost", uncertain: true))
        let confirmed = TrackerSnapshot(tasks: [firstTask], active: nil)
        let reconciliation = try await fixture.client.next()
        reconciliation.succeed(confirmed)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.selectedTaskID, firstTask.id)
        fixture.session.submitTaskCreation()
        let recovery = try await fixture.client.next()

        let observer = TrackerPresentationObserver(session: fixture.session)
        var closedSheetUpdates = 0
        observer.onTaskCreationChange = {
            if !fixture.session.taskCreation.isPresented { closedSheetUpdates += 1 }
        }
        fixture.session.onChange = { observer.update(from: fixture.session) }
        recovery.created(taskID: firstTask.id, snapshot: confirmed)
        let finished = Task { @MainActor in
            while fixture.session.isBusy && !Task.isCancelled { await Task.yield() }
        }
        try await fixture.taskValue(finished)
        XCTAssertEqual(closedSheetUpdates, 1)
        XCTAssertFalse(fixture.session.taskCreation.isPresented)
        XCTAssertFalse(fixture.session.taskCreation.isSubmitting)
        XCTAssertFalse(fixture.session.isBusy)
        XCTAssertTrue(fixture.session.canStartSelectedTask)
        XCTAssertEqual(fixture.session.selectedTaskID, firstTask.id)
        XCTAssertEqual(fixture.session.worklogs, [])
        XCTAssertEqual(
            fixture.client.operations.count, 5,
            "Recovering an already selected task must reuse its loaded history.")
    }
}
