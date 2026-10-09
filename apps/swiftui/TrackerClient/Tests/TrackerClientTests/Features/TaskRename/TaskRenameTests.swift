import Foundation
import XCTest
@testable import TrackerClient

final class TaskRenameTests: XCTestCase {
    @MainActor
    func testOpeningCapturesIdentityAndCancelMakesNoWrite() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        fixture.session.openTaskRename()
        XCTAssertFalse(fixture.session.canOpenTaskRename)
        XCTAssertFalse(fixture.session.taskRename.isPresented)
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        fixture.session.openTaskRename()
        XCTAssertEqual(fixture.session.taskRename.taskID, firstTask.id)
        XCTAssertEqual(fixture.session.taskRename.originalName, firstTask.name)
        XCTAssertEqual(fixture.session.taskRename.name, firstTask.name)
        XCTAssertFalse(fixture.session.taskRename.canSubmit)
        fixture.session.setTaskRenameName("New name")
        XCTAssertTrue(fixture.session.taskRename.canSubmit)
        fixture.session.cancelTaskRename()
        XCTAssertFalse(fixture.session.taskRename.isPresented)
        XCTAssertNil(fixture.session.taskRename.taskID)
        XCTAssertEqual(fixture.client.operations.count, 2)
        fixture.session.setTaskRenameName("Hidden draft")
        fixture.session.submitTaskRename()
        XCTAssertEqual(fixture.client.operations.count, 2)
    }

    @MainActor
    func testRenameKeepsCapturedTargetWhenSelectionChangesAndPreservesTrackingAndHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(snapshot)
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Renamed task")
        fixture.session.select(secondTask.id)
        let selectedHistory = try await fixture.client.next()
        let selectedLog = WorklogItem(
            id: "second-log", taskId: secondTask.id, start: oldWorklog.start, end: oldWorklog.end)
        selectedHistory.succeed(HistoryPage(worklogs: [selectedLog], nextCursor: "older", reset: false))
        try await fixture.settled()
        fixture.session.submitTaskRename()
        fixture.clock.now.addTimeInterval(30)
        let preflight = try await fixture.client.next()
        XCTAssertEqual(preflight.operation, .taskList)
        preflight.succeed(snapshot)
        let rename = try await fixture.client.next()
        XCTAssertEqual(
            rename.operation, .rename(task: firstTask.id, name: "Renamed task", at: "2025-01-01T00:00:00.000Z"))
        fixture.session.submitTaskRename()
        fixture.session.cancelTaskRename()
        fixture.session.setTaskRenameName("Replacement")
        XCTAssertEqual(fixture.session.taskRename.name, "Renamed task")
        let renamed = TaskItem(id: firstTask.id, name: "Renamed task", archived: false, latestStart: nil)
        rename.succeed(TaskListResources(tasks: [renamed, secondTask], active: activeWorklog))
        try await fixture.settled()
        XCTAssertFalse(fixture.session.taskRename.isPresented)
        XCTAssertEqual(fixture.session.selectedTaskID, secondTask.id)
        XCTAssertEqual(fixture.session.active, activeWorklog)
        XCTAssertEqual(fixture.session.worklogs, [selectedLog])
        XCTAssertEqual(fixture.session.nextCursor, "older")
        XCTAssertEqual(fixture.session.tasks.map(\.id), [firstTask.id, secondTask.id])
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .rename = $0 { return true }; return false
            }.count, 1)
    }

    @MainActor
    func testArchivedTaskCanBeRenamedWithoutMovingTabs() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TaskListResources(tasks: [archivedTask], active: nil)
        try await fixture.start(snapshot)
        XCTAssertFalse(fixture.session.canOpenTaskRename)
        fixture.session.changeTab(.archived)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertTrue(fixture.session.canOpenTaskRename)
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Renamed archive")
        fixture.session.submitTaskRename()
        let preflight = try await fixture.client.next()
        preflight.succeed(snapshot)
        let rename = try await fixture.client.next()
        let renamed = TaskItem(id: archivedTask.id, name: "Renamed archive", archived: true, latestStart: nil)
        rename.succeed(TaskListResources(tasks: [renamed], active: nil))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.tab, .archived)
        XCTAssertEqual(fixture.session.selectedTaskID, archivedTask.id)
        XCTAssertTrue(fixture.session.selectedTask?.archived == true)
        XCTAssertEqual(fixture.client.operations.count, 5)
    }

    @MainActor
    func testRenameQueuesBehindPollingAndPollingDoesNotOverwriteDraft() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(snapshot)
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Draft name")
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        XCTAssertTrue(fixture.session.canOpenTaskRename)
        fixture.session.submitTaskRename()
        fixture.clock.now.addTimeInterval(60)
        fixture.session.submitTaskRename()
        XCTAssertEqual(fixture.client.operations.count, 3)
        poll.succeed(snapshot)
        let preflight = try await fixture.client.next()
        XCTAssertEqual(fixture.session.taskRename.name, "Draft name")
        preflight.succeed(snapshot)
        let rename = try await fixture.client.next()
        XCTAssertEqual(
            rename.operation, .rename(task: firstTask.id, name: "Draft name", at: "2025-01-01T00:00:00.000Z"))
        let renamed = TaskItem(id: firstTask.id, name: "Draft name", archived: false, latestStart: nil)
        rename.succeed(TaskListResources(tasks: [renamed], active: nil))
        try await fixture.settled()
    }

    @MainActor
    func testExternalNameChangeRequiresReviewAndMakesNoWrite() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("My change")
        fixture.session.submitTaskRename()
        let changed = TaskItem(id: firstTask.id, name: "Other client change", archived: false, latestStart: nil)
        let preflight = try await fixture.client.next()
        preflight.succeed(TaskListResources(tasks: [changed], active: nil))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.taskRename.name, "My change")
        XCTAssertEqual(
            fixture.session.taskRename.error,
            "The task name changed on another client. Cancel and reopen the editor to review its current name.")
        XCTAssertFalse(fixture.session.taskRename.canSubmit)
        fixture.session.submitTaskRename()
        XCTAssertEqual(fixture.client.operations.count, 3)
        fixture.session.cancelTaskRename()
        fixture.session.openTaskRename()
        XCTAssertEqual(fixture.session.taskRename.name, changed.name)
        XCTAssertTrue(fixture.session.taskRename.canEditName)
    }

    @MainActor
    func testAlreadyAppliedDesiredNameResolvesWithoutAnotherWrite() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Desired name")
        fixture.session.submitTaskRename()
        let desired = TaskItem(id: firstTask.id, name: "Desired name", archived: false, latestStart: nil)
        let preflight = try await fixture.client.next()
        preflight.succeed(TaskListResources(tasks: [desired], active: nil))
        try await fixture.settled()
        XCTAssertFalse(fixture.session.taskRename.isPresented)
        XCTAssertEqual(fixture.session.selectedTask?.name, desired.name)
        XCTAssertEqual(fixture.client.operations.count, 3)
    }

    @MainActor
    func testLostResponseReconcilesCommittedRenameAndClosesSheet() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(snapshot)
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Confirmed name")
        fixture.session.submitTaskRename()
        let preflight = try await fixture.client.next()
        preflight.succeed(snapshot)
        let rename = try await fixture.client.next()
        rename.fail(BridgeFailure(message: "Response lost", kind: "unavailable", uncertain: true))
        let reconciliation = try await fixture.client.next()
        XCTAssertEqual(reconciliation.operation, .taskList)
        let renamed = TaskItem(id: firstTask.id, name: "Confirmed name", archived: false, latestStart: nil)
        reconciliation.succeed(TaskListResources(tasks: [renamed], active: activeWorklog))
        try await fixture.settled()
        XCTAssertFalse(fixture.session.taskRename.isPresented)
        XCTAssertFalse(fixture.session.isStale)
        XCTAssertNil(fixture.session.trackingError)
        XCTAssertEqual(fixture.session.active, activeWorklog)
    }

    @MainActor
    func testUncertainFailureRetainsTargetNameTimeAndBlocksConnectionAndCreation() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(snapshot)
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Retained name")
        fixture.session.submitTaskRename()
        let preflight = try await fixture.client.next()
        preflight.succeed(snapshot)
        let rename = try await fixture.client.next()
        rename.fail(BridgeFailure(message: "Response lost", uncertain: true))
        let reconciliation = try await fixture.client.next()
        reconciliation.succeed(snapshot)
        try await fixture.settled()
        XCTAssertFalse(fixture.session.taskRename.canEditName)
        XCTAssertTrue(fixture.session.taskRename.canSubmit)
        XCTAssertEqual(fixture.session.taskRename.error, "Response lost")
        fixture.session.cancelTaskRename()
        fixture.session.select(secondTask.id)
        let selectedHistory = try await fixture.client.next()
        selectedHistory.succeed(emptyPage)
        try await fixture.settled()
        fixture.session.openTaskCreation()
        XCTAssertFalse(fixture.session.taskCreation.isPresented)
        let observer = TrackerPresentationObserver(session: fixture.session)
        var displayedConnectionMessages: [String?] = []
        observer.onContentChange = { displayedConnectionMessages.append(fixture.session.connectionMessage) }
        fixture.session.onChange = { observer.update(from: fixture.session) }
        let connecting = Task { await fixture.session.connect(serverSettings) }
        let connected = try await fixture.taskValue(connecting)
        XCTAssertFalse(connected)
        XCTAssertEqual(displayedConnectionMessages, ["Finish or retry task renaming before changing connections."])
        fixture.session.openTaskRename()
        XCTAssertEqual(fixture.session.taskRename.taskID, firstTask.id)
        XCTAssertEqual(fixture.session.taskRename.originalName, firstTask.name)
        fixture.session.setTaskRenameName("Replacement")
        fixture.clock.now.addTimeInterval(90)
        fixture.session.submitTaskRename()
        let retryPreflight = try await fixture.client.next()
        retryPreflight.succeed(snapshot)
        let retry = try await fixture.client.next()
        XCTAssertEqual(retry.operation, rename.operation)
        let renamed = TaskItem(id: firstTask.id, name: "Retained name", archived: false, latestStart: nil)
        retry.succeed(TaskListResources(tasks: [renamed, secondTask], active: nil))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.selectedTaskID, secondTask.id)
    }

    @MainActor
    func testNulRejectionPublishesInlineErrorAndDraftEditsDoNotRedrawLists() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        let observer = TrackerPresentationObserver(session: fixture.session)
        var displayedErrors: [String?] = []
        var contentUpdates = 0
        observer.onTaskRenameChange = { displayedErrors.append(fixture.session.taskRename.error) }
        observer.onContentChange = { contentUpdates += 1 }
        fixture.session.onChange = { observer.update(from: fixture.session) }
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Valid\u{0000}suffix")
        fixture.session.submitTaskRename()
        XCTAssertEqual(displayedErrors, [nil, nil, "Task names must not contain control characters."])
        XCTAssertEqual(contentUpdates, 0)
        XCTAssertFalse(fixture.session.taskRename.isSubmitting)
        XCTAssertEqual(fixture.client.operations.count, 2)
        fixture.session.setTaskRenameName("   ")
        XCTAssertFalse(fixture.session.taskRename.canSubmit)
        fixture.session.submitTaskRename()
        XCTAssertEqual(fixture.client.operations.count, 2)
    }

    @MainActor
    func testConnectionSwitchDiscardsEditingDraftAndEditorsAreMutuallyExclusive() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.session.openTaskCreation()
        XCTAssertFalse(fixture.session.canOpenTaskRename)
        fixture.session.openTaskRename()
        XCTAssertFalse(fixture.session.taskRename.isPresented)
        fixture.session.cancelTaskCreation()
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Old server draft")
        XCTAssertFalse(fixture.session.canOpenTaskCreation)
        fixture.session.openTaskCreation()
        XCTAssertFalse(fixture.session.taskCreation.isPresented)
        let connect = Task { await fixture.session.connect(serverSettings) }
        let connection = try await fixture.client.next()
        connection.succeed(emptySnapshot)
        let connected = try await fixture.taskValue(connect)
        XCTAssertTrue(connected)
        XCTAssertFalse(fixture.session.taskRename.isPresented)
        XCTAssertNil(fixture.session.taskRename.taskID)
        XCTAssertFalse(fixture.session.canOpenTaskRename)
    }

    @MainActor
    func testKnownNameRejectionReconcilesBeforeAllowingDraftChanges() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(snapshot)
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Invalid\u{0001}name")
        fixture.session.submitTaskRename()
        let preflight = try await fixture.client.next()
        preflight.succeed(snapshot)
        let rename = try await fixture.client.next()
        rename.fail(BridgeFailure(message: "Invalid name", kind: "validation"))
        let reconciliation = try await fixture.client.next()
        XCTAssertTrue(fixture.session.isBusy)
        XCTAssertFalse(fixture.session.taskRename.canEditName)
        reconciliation.succeed(snapshot)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.taskRename.error, "Invalid name")
        XCTAssertTrue(fixture.session.taskRename.canEditName)
        fixture.session.setTaskRenameName("Corrected name")
        XCTAssertNil(fixture.session.taskRename.error)
        XCTAssertEqual(fixture.session.taskRename.name, "Corrected name")
    }

    @MainActor
    func testConflictDuringWriteRequiresReviewEvenWhenNameStillMatchesOriginal() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(snapshot)
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Desired name")
        fixture.session.submitTaskRename()
        let preflight = try await fixture.client.next()
        preflight.succeed(snapshot)
        let rename = try await fixture.client.next()
        rename.fail(BridgeFailure(message: "Revision conflict", kind: "conflict", requiresRefresh: true))
        let reconciliation = try await fixture.client.next()
        reconciliation.succeed(snapshot)
        try await fixture.settled()
        XCTAssertEqual(
            fixture.session.taskRename.error,
            "The task changed on another client. Cancel and reopen the editor to review its current state.")
        XCTAssertFalse(fixture.session.taskRename.canSubmit)
        XCTAssertFalse(fixture.session.taskRename.canEditName)
        fixture.session.cancelTaskRename()
        fixture.session.openTaskRename()
        XCTAssertEqual(fixture.session.taskRename.name, firstTask.name)
    }

    @MainActor
    func testUncertainWriteReconciliationDetectsAnotherClientsNameInsteadOfRebasing() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(snapshot)
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("My name")
        fixture.session.submitTaskRename()
        let preflight = try await fixture.client.next()
        preflight.succeed(snapshot)
        let rename = try await fixture.client.next()
        rename.fail(BridgeFailure(message: "Response lost", uncertain: true))
        let reconciliation = try await fixture.client.next()
        let other = TaskItem(id: firstTask.id, name: "Other client's name", archived: false, latestStart: nil)
        reconciliation.succeed(TaskListResources(tasks: [other], active: nil))
        try await fixture.settled()
        XCTAssertFalse(fixture.session.taskRename.canSubmit)
        XCTAssertEqual(fixture.session.selectedTask?.name, other.name)
        XCTAssertEqual(fixture.session.taskRename.name, "My name")
        fixture.session.setTaskRenameName("Replacement")
        XCTAssertEqual(fixture.session.taskRename.name, "My name")
    }

    @MainActor
    func testPreflightOutageShowsErrorWithoutWriteAndKeepsRetryIntent() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Draft name")
        fixture.session.submitTaskRename()
        let preflight = try await fixture.client.next()
        preflight.fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.taskRename.error, "Server offline")
        XCTAssertFalse(fixture.session.taskRename.isSubmitting)
        XCTAssertFalse(fixture.session.taskRename.canEditName)
        XCTAssertTrue(fixture.session.isStale)
        XCTAssertEqual(fixture.client.operations.count, 3)
        fixture.session.cancelTaskRename()
        fixture.session.openTaskRename()
        XCTAssertTrue(fixture.session.taskRename.isPresented)
        XCTAssertEqual(fixture.session.taskRename.name, "Draft name")
    }

    @MainActor
    func testMissingTargetRequiresReviewAndNeverRenamesSelectedReplacement() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("New name")
        fixture.session.submitTaskRename()
        let preflight = try await fixture.client.next()
        preflight.succeed(TaskListResources(tasks: [secondTask], active: nil))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.selectedTaskID, secondTask.id)
        XCTAssertEqual(fixture.session.taskRename.taskID, firstTask.id)
        XCTAssertEqual(
            fixture.session.taskRename.error, "The task no longer exists. Cancel the editor and refresh the task list.")
        XCTAssertFalse(fixture.session.taskRename.canSubmit)
        XCTAssertEqual(fixture.client.operations.count, 4)
    }

    @MainActor
    func testExplicitClickedIdentityIsCapturedAndInvalidIdentityIsIgnored() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        fixture.session.openTaskRename(taskID: "missing-task")
        XCTAssertFalse(fixture.session.taskRename.isPresented)
        fixture.session.openTaskRename(taskID: secondTask.id)
        XCTAssertEqual(fixture.session.taskRename.taskID, secondTask.id)
        XCTAssertEqual(fixture.session.taskRename.name, secondTask.name)
        fixture.session.openTaskRename(taskID: firstTask.id)
        XCTAssertEqual(fixture.session.taskRename.taskID, secondTask.id)
    }

    @MainActor
    func testSleepDuringQueuedRequestAndPreflightDefersWriteUntilWake() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(snapshot)
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Queued rename")
        fixture.session.submitTaskRename()
        XCTAssertTrue(fixture.session.isBlockingControls)
        fixture.session.sleep()
        poll.succeed(snapshot)
        try await fixture.settled()
        XCTAssertEqual(fixture.client.operations.count, 3)
        fixture.session.wake()
        let preflight = try await fixture.client.next()
        XCTAssertEqual(preflight.operation, .taskList)
        fixture.session.sleep()
        preflight.succeed(snapshot)
        try await fixture.settled()
        XCTAssertTrue(fixture.session.taskRename.isSubmitting)
        XCTAssertEqual(fixture.client.operations.count, 4)
        fixture.session.wake()
        let wakePreflight = try await fixture.client.next()
        wakePreflight.succeed(snapshot)
        let rename = try await fixture.client.next()
        XCTAssertEqual(
            rename.operation, .rename(task: firstTask.id, name: "Queued rename", at: "2025-01-01T00:00:00.000Z"))
        let renamed = TaskItem(id: firstTask.id, name: "Queued rename", archived: false, latestStart: nil)
        rename.succeed(TaskListResources(tasks: [renamed], active: nil))
        try await fixture.settled()
    }

    @MainActor
    func testShutdownDiscardsPreflightAndInFlightWriteResults() async throws {
        for shutdownBeforeWrite in [true, false] {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            let snapshot = TaskListResources(tasks: [firstTask], active: nil)
            try await fixture.start(snapshot)
            fixture.session.openTaskRename()
            fixture.session.setTaskRenameName("New name")
            fixture.session.submitTaskRename()
            let preflight = try await fixture.client.next()
            let outstanding: FakeClient.Request
            if shutdownBeforeWrite {
                outstanding = preflight
            } else {
                preflight.succeed(snapshot)
                outstanding = try await fixture.client.next()
            }
            fixture.session.shutdown()
            let renamed = TaskItem(id: firstTask.id, name: "New name", archived: false, latestStart: nil)
            outstanding.succeed(TaskListResources(tasks: [renamed], active: nil))
            await Task.yield()
            XCTAssertEqual(fixture.session.tasks, [firstTask])
            XCTAssertFalse(fixture.session.taskRename.isPresented)
            XCTAssertFalse(fixture.session.taskRename.isSubmitting)
            XCTAssertFalse(fixture.session.canOpenTaskRename)
            fixture.session.setTaskRenameName("After shutdown")
            fixture.session.openTaskRename()
            fixture.session.cancelTaskRename()
            fixture.session.submitTaskRename()
            XCTAssertFalse(fixture.session.taskRename.isPresented)
            XCTAssertEqual(fixture.client.operations.count, shutdownBeforeWrite ? 3 : 4)
        }
    }

    @MainActor
    func testShutdownCancelsQueuedRenameAndIgnoresLateFailure() async throws {
        for queueBehindPoll in [true, false] {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            let snapshot = TaskListResources(tasks: [firstTask], active: nil)
            try await fixture.start(snapshot)
            let outstanding: FakeClient.Request
            if queueBehindPoll {
                fixture.session.refresh()
                outstanding = try await fixture.client.next()
                fixture.session.openTaskRename()
                fixture.session.setTaskRenameName("Queued name")
                fixture.session.submitTaskRename()
            } else {
                fixture.session.openTaskRename()
                fixture.session.setTaskRenameName("Submitted name")
                fixture.session.submitTaskRename()
                let preflight = try await fixture.client.next()
                preflight.succeed(snapshot)
                outstanding = try await fixture.client.next()
            }
            fixture.session.shutdown()
            outstanding.fail(BridgeFailure(message: "Late failure", uncertain: true))
            await Task.yield()
            XCTAssertFalse(fixture.session.taskRename.isPresented)
            XCTAssertNil(fixture.session.taskRename.error)
            XCTAssertEqual(fixture.client.operations.count, queueBehindPoll ? 3 : 4)
        }
    }

    @MainActor
    func testRemoteRenameReceiptKeepsNewerTaskMetadataAfterRecovery() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(snapshot)
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Desired name")
        fixture.session.submitTaskRename()
        let preflight = try await fixture.client.next()
        preflight.succeed(snapshot)
        let rename = try await fixture.client.next()
        let newer = TaskItem(id: firstTask.id, name: "Another client renamed later", archived: false, latestStart: nil)
        rename.succeed(
            TaskListResources(tasks: [newer], active: nil, tasksRevision: "epoch:3", trackingRevision: "epoch:3"))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.tasks, [newer])
        XCTAssertFalse(fixture.session.taskRename.isPresented)
        XCTAssertNil(fixture.session.taskRename.error)
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .rename = $0 { return true }; return false
            }.count, 1)
    }

    @MainActor
    func testMalformedRenameResultIsReconciledAndRetainsFrozenIntent() async throws {
        let wrongTargetWithDesiredName = TaskItem(
            id: secondTask.id, name: "Desired name", archived: false, latestStart: nil)
        for responseTask in [secondTask, firstTask, wrongTargetWithDesiredName] {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            let snapshot = TaskListResources(tasks: [firstTask], active: nil)
            try await fixture.start(snapshot)
            fixture.session.openTaskRename()
            fixture.session.setTaskRenameName("Desired name")
            fixture.session.submitTaskRename()
            let preflight = try await fixture.client.next()
            preflight.succeed(snapshot)
            let rename = try await fixture.client.next()
            let observer = TrackerPresentationObserver(session: fixture.session)
            var displayedErrors: [String?] = []
            observer.onTaskRenameChange = { displayedErrors.append(fixture.session.taskRename.error) }
            fixture.session.onChange = { observer.update(from: fixture.session) }
            rename.succeed(TaskListResources(tasks: [responseTask], active: nil))
            let reconciliation = try await fixture.client.next()
            XCTAssertEqual(reconciliation.operation, .taskList)
            reconciliation.succeed(snapshot)
            let finished = Task { @MainActor in
                while fixture.session.isBusy && !Task.isCancelled { await Task.yield() }
            }
            try await fixture.taskValue(finished)
            XCTAssertEqual(fixture.session.tasks, [firstTask])
            XCTAssertEqual(fixture.session.taskRename.error, "The rename response does not contain the updated task.")
            XCTAssertEqual(displayedErrors.last, "The rename response does not contain the updated task.")
            XCTAssertEqual(fixture.session.taskRename.name, "Desired name")
            XCTAssertTrue(fixture.session.taskRename.isPresented)
            XCTAssertFalse(fixture.session.taskRename.canEditName)
        }
    }

    @MainActor
    func testFailedWriteConfirmationRetainsDraftAndDisablesTrackingUntilRecovery() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(snapshot)
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Desired name")
        fixture.session.submitTaskRename()
        let preflight = try await fixture.client.next()
        preflight.succeed(snapshot)
        let rename = try await fixture.client.next()
        rename.fail(BridgeFailure(message: "Name rejected", kind: "validation"))
        let reconciliation = try await fixture.client.next()
        reconciliation.fail(BridgeFailure(message: "Cannot confirm server state", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.taskRename.error, "Name rejected")
        XCTAssertEqual(fixture.session.connectionMessage, "Cannot confirm server state")
        XCTAssertFalse(fixture.session.taskRename.canEditName)
        XCTAssertTrue(fixture.session.taskRename.canSubmit)
        XCTAssertFalse(fixture.session.canStopTracking)
        XCTAssertEqual(fixture.session.active, activeWorklog)
    }

    @MainActor
    func testShutdownDuringWriteReconciliationDiscardsConfirmedResult() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(snapshot)
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Desired name")
        fixture.session.submitTaskRename()
        let preflight = try await fixture.client.next()
        preflight.succeed(snapshot)
        let rename = try await fixture.client.next()
        rename.fail(BridgeFailure(message: "Response lost", uncertain: true))
        let reconciliation = try await fixture.client.next()
        fixture.session.shutdown()
        let renamed = TaskItem(id: firstTask.id, name: "Desired name", archived: false, latestStart: nil)
        reconciliation.succeed(TaskListResources(tasks: [renamed], active: nil))
        await Task.yield()
        XCTAssertEqual(fixture.session.tasks, [firstTask])
        XCTAssertFalse(fixture.session.taskRename.isPresented)
        XCTAssertNil(fixture.session.taskRename.error)
    }

    @MainActor
    func testFailedSourceChangeKeepsDraftForOriginalConnection() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Current server draft")
        let connect = Task { await fixture.session.connect(serverSettings) }
        let connection = try await fixture.client.next()
        connection.fail(BridgeFailure(message: "Connection failed", kind: "unavailable"))
        let connected = try await fixture.taskValue(connect)
        XCTAssertFalse(connected)
        XCTAssertEqual(fixture.session.connectionSettings, .local)
        XCTAssertEqual(fixture.session.taskRename.taskID, firstTask.id)
        XCTAssertEqual(fixture.session.taskRename.name, "Current server draft")
        XCTAssertTrue(fixture.session.taskRename.isPresented)
    }

    @MainActor
    func testUnresolvedCreationBlocksRenameAfterCreationSheetDismissal() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(snapshot)
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("Unconfirmed new task")
        fixture.session.submitTaskCreation()
        let creation = try await fixture.client.next()
        creation.fail(BridgeFailure(message: "Response lost", uncertain: true))
        let reconciliation = try await fixture.client.next()
        reconciliation.succeed(snapshot)
        try await fixture.settled()
        fixture.session.cancelTaskCreation()
        XCTAssertFalse(fixture.session.canOpenTaskRename)
        fixture.session.openTaskRename()
        XCTAssertFalse(fixture.session.taskRename.isPresented)
        fixture.session.openTaskCreation()
        XCTAssertEqual(fixture.session.taskCreation.name, "Unconfirmed new task")
    }

    @MainActor
    func testAutomaticPauseRunsBeforeQueuedRenameAndResumeUsesSameIdentity() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        let snapshot = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(snapshot)
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Renamed while locked")
        fixture.session.submitTaskRename()
        fixture.session.screenLocked(at: fixture.clock.now)
        poll.succeed(snapshot)
        let automationRead = try await fixture.client.next()
        XCTAssertEqual(automationRead.operation, .taskList)
        automationRead.succeed(snapshot)
        let pause = try await fixture.client.next()
        XCTAssertEqual(pause.operation, .pause(worklog: activeWorklog.id, at: "2025-01-01T00:00:00.000Z"))
        let paused = TaskListResources(tasks: [firstTask], active: nil)
        pause.paused(paused)
        let renameRead = try await fixture.client.next()
        XCTAssertEqual(renameRead.operation, .taskList)
        renameRead.succeed(paused)
        let rename = try await fixture.client.next()
        let renamed = TaskItem(id: firstTask.id, name: "Renamed while locked", archived: false, latestStart: nil)
        let renamedPaused = TaskListResources(tasks: [renamed], active: nil)
        rename.succeed(renamedPaused)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        fixture.clock.now.addTimeInterval(30)
        fixture.session.screenUnlocked(at: fixture.clock.now)
        let resumeRead = try await fixture.client.next()
        resumeRead.succeed(renamedPaused)
        let resume = try await fixture.client.next()
        XCTAssertEqual(resume.operation, .resume(task: firstTask.id, at: "2025-01-01T00:00:30.000Z"))
        let resumed = WorklogItem(id: "resumed", taskId: firstTask.id, start: "2025-01-01T00:00:30.000Z", end: nil)
        resume.succeed(TaskListResources(tasks: [renamed], active: resumed))
        let resumedHistory = try await fixture.client.next()
        resumedHistory.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.runningTaskName, renamed.name)
    }

    @MainActor
    func testCreationSheetUpdatesRenameButtonAvailabilityWithoutChangingRenameDraft() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        let originalPresentation = fixture.session.taskRename
        let observer = TrackerPresentationObserver(session: fixture.session)
        var enabledStates: [Bool] = []
        var contentUpdates = 0
        observer.onTaskRenameChange = { enabledStates.append(fixture.session.canOpenTaskRename) }
        observer.onContentChange = { contentUpdates += 1 }
        fixture.session.onChange = { observer.update(from: fixture.session) }
        fixture.session.openTaskCreation()
        XCTAssertEqual(enabledStates, [false])
        XCTAssertEqual(fixture.session.taskRename, originalPresentation)
        fixture.session.cancelTaskCreation()
        XCTAssertEqual(enabledStates, [false, true])
        XCTAssertEqual(fixture.session.taskRename, originalPresentation)
        XCTAssertEqual(contentUpdates, 0)
    }
}
