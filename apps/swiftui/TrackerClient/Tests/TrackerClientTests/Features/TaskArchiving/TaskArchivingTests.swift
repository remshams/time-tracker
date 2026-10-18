import Foundation
import XCTest
@testable import TrackerClient

final class TaskArchivingTests: XCTestCase {
    @MainActor
    func testArchiveConfirmationRequiresTheTargetTaskAndDesiredStateTogether() {
        let state = TaskArchivingState()
        let intent = TaskArchivingState.Intent(
            taskID: firstTask.id, taskName: firstTask.name, originalArchived: false,
            action: .archive, occurredAt: "2025-01-01T00:00:00.000Z")
        let unrelatedArchived = TaskItem(
            id: secondTask.id, name: secondTask.name, archived: true, latestStart: nil)
        XCTAssertFalse(
            state.responseMatches(
                TaskListResources(tasks: [firstTask, unrelatedArchived], active: nil), intent: intent))
        let archivedTarget = TaskItem(
            id: firstTask.id, name: firstTask.name, archived: true, latestStart: nil)
        XCTAssertTrue(
            state.responseMatches(
                TaskListResources(tasks: [archivedTarget, secondTask], active: nil), intent: intent))
    }

    private func archived(_ task: TaskItem, _ value: Bool = true, name: String? = nil) -> TaskItem {
        TaskItem(id: task.id, name: name ?? task.name, archived: value, latestStart: task.latestStart)
    }

    @MainActor
    func testAvailabilityDistinguishesRunningInactiveAndArchivedTasks() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: activeWorklog))

        XCTAssertEqual(fixture.session.taskArchivingAvailability[firstTask.id], false)
        XCTAssertEqual(fixture.session.taskArchivingAvailability[secondTask.id], true)
        XCTAssertEqual(fixture.session.taskArchivingAvailability[archivedTask.id], true)
    }

    @MainActor
    func testUnarchiveAvailabilityRequiresAnArchivedTaskWithTheRequestedIdentity() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, archivedTask], active: nil))

        XCTAssertTrue(fixture.session.canUnarchiveTask(taskID: archivedTask.id))
        XCTAssertFalse(fixture.session.canUnarchiveTask(taskID: firstTask.id))
        XCTAssertFalse(fixture.session.canUnarchiveTask(taskID: "missing-task"))
    }

    @MainActor
    func testArchiveCapturesClickedTaskAndCancelDoesNotWrite() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        XCTAssertFalse(fixture.session.canArchiveTask(taskID: firstTask.id))
        fixture.session.openTaskArchive(taskID: firstTask.id)
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: nil))
        fixture.session.openTaskArchive(taskID: secondTask.id)
        XCTAssertEqual(fixture.session.taskArchiving.taskID, secondTask.id)
        XCTAssertEqual(fixture.session.taskArchiving.taskName, secondTask.name)
        XCTAssertEqual(fixture.session.taskArchiving.action, .archive)
        XCTAssertTrue(fixture.session.taskArchiving.canSubmit)
        fixture.session.cancelTaskArchiving()
        fixture.session.submitTaskArchiving()
        fixture.session.reopenTaskArchiving()
        XCTAssertFalse(fixture.session.taskArchiving.isPresented)
        XCTAssertEqual(fixture.client.operations.count, 2)
    }

    @MainActor
    func testArchivePreservesUnrelatedSelectionHistoryAndTimer() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(resources)
        XCTAssertFalse(fixture.session.canArchiveTask(taskID: firstTask.id))
        XCTAssertFalse(fixture.session.canArchiveTask(taskID: "missing"))
        XCTAssertTrue(fixture.session.canArchiveTask(taskID: secondTask.id))
        fixture.session.openTaskArchive(taskID: secondTask.id)
        fixture.session.submitTaskArchiving()
        let preflight = try await fixture.client.next()
        XCTAssertEqual(preflight.operation, .taskList)
        preflight.succeed(resources)
        let command = try await fixture.client.next()
        XCTAssertEqual(
            command.operation,
            .archive(task: secondTask.id, expectedName: secondTask.name, at: "2025-01-01T00:00:00.000Z"))
        fixture.session.submitTaskArchiving()
        fixture.session.cancelTaskArchiving()
        command.succeed(TaskListResources(tasks: [firstTask, archived(secondTask)], active: activeWorklog))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.selectedTaskID, firstTask.id)
        XCTAssertEqual(fixture.session.active, activeWorklog)
        XCTAssertEqual(fixture.session.tab, .active)
        XCTAssertFalse(fixture.session.taskArchiving.isPresented)
        XCTAssertEqual(fixture.client.operations.count, 5)
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 1)
    }

    @MainActor
    func testSelectedArchivedTaskFallsBackAndReloadsHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.session.openTaskArchive(taskID: firstTask.id)
        fixture.session.submitTaskArchiving()
        let preflight = try await fixture.client.next()
        preflight.succeed(resources)
        let command = try await fixture.client.next()
        command.succeed(TaskListResources(tasks: [archived(firstTask), secondTask], active: nil))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: secondTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.selectedTaskID, secondTask.id)
        XCTAssertEqual(fixture.session.tab, .active)
    }

    @MainActor
    func testUnarchiveRunsDirectlyAndRemembersRestoredTaskWithoutSwitchingTabs() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let other = archived(secondTask)
        let resources = TaskListResources(tasks: [firstTask, archivedTask, other], active: activeWorklog)
        try await fixture.start(resources)
        fixture.session.changeTab(.archived)
        let originalHistory = try await fixture.client.next()
        originalHistory.succeed(emptyPage)
        try await fixture.settled()
        fixture.session.unarchiveTask(taskID: other.id)
        XCTAssertFalse(fixture.session.taskArchiving.isPresented)
        XCTAssertTrue(fixture.session.taskArchiving.isSubmitting)
        let preflight = try await fixture.client.next()
        preflight.succeed(resources)
        let command = try await fixture.client.next()
        XCTAssertEqual(command.operation, .unarchive(task: other.id, at: "2025-01-01T00:00:00.000Z"))
        fixture.session.unarchiveTask(taskID: archivedTask.id)
        command.succeed(TaskListResources(tasks: [firstTask, archivedTask, secondTask], active: activeWorklog))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.tab, .archived)
        XCTAssertEqual(fixture.session.selectedTaskID, archivedTask.id)
        XCTAssertEqual(fixture.session.active, activeWorklog)
        fixture.session.changeTab(.active)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: secondTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.selectedTaskID, secondTask.id)
    }

    @MainActor
    func testRenameRaceRequiresExplicitReviewOfCapturedTask() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        fixture.session.openTaskArchive(taskID: secondTask.id)
        fixture.session.submitTaskArchiving()
        let latest = archived(secondTask, false, name: "Renamed task")
        let resources = TaskListResources(tasks: [firstTask, latest], active: nil)
        let preflight = try await fixture.client.next()
        preflight.succeed(resources)
        try await fixture.settled()
        XCTAssertTrue(fixture.session.taskArchiving.requiresReview)
        XCTAssertEqual(fixture.session.taskArchiving.latest, latest)
        XCTAssertEqual(fixture.session.taskArchiving.taskName, secondTask.name)
        fixture.session.submitTaskArchiving()
        XCTAssertEqual(fixture.client.operations.count, 3)
        fixture.session.reviewLatestTaskArchiving()
        XCTAssertEqual(fixture.session.taskArchiving.taskName, latest.name)
        fixture.clock.now.addTimeInterval(30)
        fixture.session.submitTaskArchiving()
        let reviewedRead = try await fixture.client.next()
        reviewedRead.succeed(resources)
        let command = try await fixture.client.next()
        XCTAssertEqual(
            command.operation, .archive(task: secondTask.id, expectedName: latest.name, at: "2025-01-01T00:00:30.000Z"))
        command.succeed(TaskListResources(tasks: [firstTask, archived(latest)], active: nil))
        try await fixture.settled()
    }

    @MainActor
    func testRunningRaceAndMissingTaskRequireReviewWithoutWriting() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.session.openTaskArchive(taskID: firstTask.id)
        fixture.session.submitTaskArchiving()
        let preflight = try await fixture.client.next()
        preflight.succeed(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.taskArchiving.error, "Stop tracking this task before archiving it.")
        fixture.session.reviewLatestTaskArchiving()
        XCTAssertTrue(fixture.session.taskArchiving.requiresReview)
        fixture.session.cancelTaskArchiving()
        fixture.session.openTaskArchive(taskID: secondTask.id)
        fixture.session.submitTaskArchiving()
        let missing = try await fixture.client.next()
        missing.succeed(TaskListResources(tasks: [firstTask], active: activeWorklog))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.taskArchiving.requiresReview)
        XCTAssertNil(fixture.session.taskArchiving.latest)
        fixture.session.reviewLatestTaskArchiving()
        XCTAssertFalse(fixture.session.taskArchiving.canSubmit)
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .archive = $0 { return true }; return false
            }.count, 0)
    }

    @MainActor
    func testLostArchiveResponseReconcilesAndDoesNotWriteTwice() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.session.openTaskArchive(taskID: secondTask.id)
        fixture.session.submitTaskArchiving()
        let preflight = try await fixture.client.next()
        preflight.succeed(resources)
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Response lost", uncertain: true))
        let recovered = try await fixture.client.next()
        recovered.succeed(TaskListResources(tasks: [firstTask, archived(secondTask)], active: nil))
        try await fixture.settled()
        XCTAssertFalse(fixture.session.taskArchiving.isPresented)
        XCTAssertFalse(fixture.session.taskArchiving.hasUnresolvedIntent)
        XCTAssertFalse(fixture.session.isStale)
        XCTAssertEqual(fixture.client.operations.count, 5)
    }

    @MainActor
    func testUnresolvedRetryKeepsTargetActionAndTimestampAndBlocksConnection() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.session.openTaskArchive(taskID: secondTask.id)
        fixture.session.submitTaskArchiving()
        let preflight = try await fixture.client.next()
        preflight.succeed(resources)
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Response lost", uncertain: true))
        let recovered = try await fixture.client.next()
        recovered.fail(BridgeFailure(message: "Offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.taskArchiving.hasUnresolvedIntent)
        fixture.session.cancelTaskArchiving()
        XCTAssertTrue(fixture.session.taskArchiving.hasPendingAction)
        let connecting = Task { await fixture.session.connect(serverSettings) }
        let connected = try await fixture.taskValue(connecting)
        XCTAssertFalse(connected)
        XCTAssertEqual(fixture.session.connectionMessage, "Finish or retry task archiving before changing connections.")
        fixture.session.reopenTaskArchiving()
        fixture.clock.now.addTimeInterval(120)
        fixture.session.submitTaskArchiving()
        let retryRead = try await fixture.client.next()
        retryRead.succeed(resources)
        let retry = try await fixture.client.next()
        XCTAssertEqual(retry.operation, command.operation)
        retry.succeed(TaskListResources(tasks: [firstTask, archived(secondTask)], active: nil))
        try await fixture.settled()
    }

    @MainActor
    func testDesiredStateDuringRetryResolvesWithoutSendingCommand() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.session.openTaskArchive(taskID: secondTask.id)
        fixture.session.submitTaskArchiving()
        let preflight = try await fixture.client.next()
        preflight.fail(BridgeFailure(message: "Offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.taskArchiving.hasUnresolvedIntent)
        fixture.session.submitTaskArchiving()
        let retry = try await fixture.client.next()
        retry.succeed(TaskListResources(tasks: [firstTask, archived(secondTask)], active: nil))
        try await fixture.settled()
        XCTAssertFalse(fixture.session.taskArchiving.isPresented)
        XCTAssertEqual(fixture.client.operations.count, 4)
    }

    @MainActor
    func testRevisionRejectionReconcilesBeforeExplicitRetryAndKeepsCapturedIntent() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.session.openTaskArchive(taskID: secondTask.id)
        fixture.session.submitTaskArchiving()
        let preflight = try await fixture.client.next()
        preflight.succeed(resources)
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "State changed", kind: "general", requiresRefresh: true))
        let recovered = try await fixture.client.next()
        recovered.succeed(resources)
        try await fixture.settled()
        XCTAssertFalse(fixture.session.taskArchiving.requiresReview)
        XCTAssertTrue(fixture.session.taskArchiving.hasUnresolvedIntent)
        fixture.clock.now.addTimeInterval(90)
        fixture.session.submitTaskArchiving()
        let reviewRead = try await fixture.client.next()
        reviewRead.succeed(resources)
        let rejected = try await fixture.client.next()
        XCTAssertEqual(rejected.operation, command.operation)
        rejected.fail(BridgeFailure(message: "Storage rejected write", kind: "validation"))
        let rejectedRead = try await fixture.client.next()
        rejectedRead.succeed(resources)
        try await fixture.settled()
        XCTAssertFalse(fixture.session.taskArchiving.hasUnresolvedIntent)
        XCTAssertTrue(fixture.session.taskArchiving.canSubmit)
        XCTAssertEqual(fixture.session.taskArchiving.error, "Storage rejected write")
    }

    @MainActor
    func testRemoteArchiveReceiptAcceptsLaterRestoreFromRecovery() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.session.openTaskArchive(taskID: secondTask.id)
        fixture.session.submitTaskArchiving()
        let preflight = try await fixture.client.next()
        preflight.succeed(resources)
        let command = try await fixture.client.next()
        command.succeed(
            TaskListResources(
                tasks: [firstTask, secondTask], active: nil,
                tasksRevision: "epoch:3", trackingRevision: "epoch:3"))
        try await fixture.settled()
        XCTAssertFalse(fixture.session.taskArchiving.hasUnresolvedIntent)
        XCTAssertNil(fixture.session.taskArchiving.error)
        XCTAssertFalse(fixture.session.tasks.first { $0.id == secondTask.id }!.archived)
    }

    @MainActor
    func testMalformedWriteResponseRequiresRecoveryBeforeRetry() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.session.openTaskArchive(taskID: secondTask.id)
        fixture.session.submitTaskArchiving()
        let preflight = try await fixture.client.next()
        preflight.succeed(resources)
        let command = try await fixture.client.next()
        command.succeed(resources)
        let recovered = try await fixture.client.next()
        recovered.succeed(resources)
        try await fixture.settled()
        XCTAssertTrue(fixture.session.taskArchiving.hasUnresolvedIntent)
        XCTAssertEqual(fixture.session.taskArchiving.error, "The response does not confirm the task's archive state.")
        XCTAssertFalse(fixture.session.tasks.first { $0.id == secondTask.id }!.archived)
    }

    @MainActor
    func testHiddenUnarchiveFailureDoesNotPresentWindowAndCanReopenKnownError() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, archivedTask], active: nil)
        try await fixture.start(resources)
        fixture.session.unarchiveTask(taskID: archivedTask.id)
        let preflight = try await fixture.client.next()
        preflight.succeed(resources)
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Rejected", kind: "validation"))
        let recovered = try await fixture.client.next()
        recovered.succeed(resources)
        try await fixture.settled()
        XCTAssertFalse(fixture.session.taskArchiving.isPresented)
        XCTAssertTrue(fixture.session.taskArchiving.hasPendingAction)
        XCTAssertFalse(fixture.session.taskArchiving.hasUnresolvedIntent)
        fixture.session.reopenTaskArchiving()
        XCTAssertTrue(fixture.session.taskArchiving.isPresented)
        XCTAssertEqual(fixture.session.taskArchiving.action, .unarchive)
        XCTAssertEqual(fixture.session.taskArchiving.error, "Rejected")
        fixture.session.cancelTaskArchiving()
        XCTAssertFalse(fixture.session.taskArchiving.hasPendingAction)
    }

    @MainActor
    func testVisibleUnarchiveFailurePresentsStatusSheet() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        fixture.session.setWindowVisible(true)
        let resources = TaskListResources(tasks: [firstTask, archivedTask], active: nil)
        try await fixture.start(resources)
        fixture.session.unarchiveTask(taskID: archivedTask.id)
        let preflight = try await fixture.client.next()
        preflight.fail(BridgeFailure(message: "Offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.taskArchiving.isPresented)
        XCTAssertEqual(fixture.session.taskArchiving.taskName, archivedTask.name)
        XCTAssertTrue(fixture.session.taskArchiving.hasUnresolvedIntent)
    }

    @MainActor
    func testQueuedArchiveWaitsForPollAndSleepDefersWriteUntilWake() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        fixture.session.openTaskArchive(taskID: secondTask.id)
        fixture.session.submitTaskArchiving()
        XCTAssertTrue(fixture.session.isBlockingControls)
        let backgroundName = TaskItem(id: secondTask.id, name: "Background name", archived: false, latestStart: nil)
        poll.succeed(TaskListResources(tasks: [firstTask, backgroundName], active: nil))
        let preflight = try await fixture.client.next()
        XCTAssertEqual(fixture.session.taskArchiving.taskName, secondTask.name)
        fixture.session.sleep()
        preflight.succeed(resources)
        try await fixture.settled()
        XCTAssertTrue(fixture.session.taskArchiving.isSubmitting)
        XCTAssertEqual(fixture.client.operations.count, 4)
        fixture.session.wake()
        let awakeRead = try await fixture.client.next()
        XCTAssertEqual(awakeRead.operation, .taskList)
        awakeRead.succeed(resources)
        let command = try await fixture.client.next()
        XCTAssertEqual(
            command.operation,
            .archive(task: secondTask.id, expectedName: secondTask.name, at: "2025-01-01T00:00:00.000Z"))
        command.succeed(TaskListResources(tasks: [firstTask, archived(secondTask)], active: nil))
        try await fixture.settled()
        XCTAssertFalse(fixture.session.taskArchiving.isPresented)
    }

    @MainActor
    func testShutdownIgnoresLateArchivingResponseAndResetsState() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.session.openTaskArchive(taskID: secondTask.id)
        fixture.session.submitTaskArchiving()
        let preflight = try await fixture.client.next()
        preflight.succeed(resources)
        let command = try await fixture.client.next()
        fixture.session.shutdown()
        command.succeed(TaskListResources(tasks: [firstTask, archived(secondTask)], active: nil))
        await Task.yield()
        XCTAssertFalse(fixture.session.taskArchiving.isPresented)
        XCTAssertNil(fixture.session.taskArchiving.taskID)
        XCTAssertEqual(fixture.session.tasks, resources.catalog.value)
        XCTAssertFalse(fixture.session.canArchiveTask(taskID: secondTask.id))
    }

    @MainActor
    func testEditorsAreExclusiveAndObserverFreezesClosingContent() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: nil))
        fixture.session.openWorklogCorrection(worklogID: "missing")
        let harness = PresentationHarness(session: fixture.session)
        let archiveUpdates = harness.counter(PresentationReads.taskArchiving)
        let contentUpdates = harness.counter(PresentationReads.content)
        fixture.session.openTaskArchive(taskID: secondTask.id)
        XCTAssertFalse(fixture.session.canOpenWorklogCorrection)
        XCTAssertFalse(fixture.session.canOpenWorklogMove)
        let shown = harness.presentation.taskArchivingSheetContent
        fixture.session.cancelTaskArchiving()
        XCTAssertEqual(harness.presentation.taskArchivingSheetContent, shown)
        XCTAssertFalse(fixture.session.taskArchiving.isPresented)
        XCTAssertEqual(archiveUpdates.count, 2)
        XCTAssertEqual(contentUpdates.count, 0)
    }

    @MainActor
    func testConnectionChangeRejectsUnsubmittedConfirmation() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        fixture.session.openTaskArchive(taskID: secondTask.id)
        var publishedMessage: String?
        fixture.session.onChange = { publishedMessage = fixture.session.connectionMessage }
        let connecting = Task { await fixture.session.connect(serverSettings) }
        let connected = try await fixture.taskValue(connecting)
        XCTAssertFalse(connected)
        XCTAssertTrue(fixture.session.taskArchiving.isPresented)
        XCTAssertEqual(fixture.session.taskArchiving.taskID, secondTask.id)
        XCTAssertEqual(fixture.client.operations.count, 2)
        XCTAssertEqual(publishedMessage, "Finish or retry task archiving before changing connections.")
    }

    @MainActor
    func testClosingWindowDuringUnarchiveFailureKeepsRetryHidden() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        fixture.session.setWindowVisible(true)
        let resources = TaskListResources(tasks: [firstTask, archivedTask], active: nil)
        try await fixture.start(resources)
        fixture.session.unarchiveTask(taskID: archivedTask.id)
        let preflight = try await fixture.client.next()
        preflight.succeed(resources)
        let command = try await fixture.client.next()
        fixture.session.setWindowVisible(false)
        command.fail(BridgeFailure(message: "Response lost", uncertain: true))
        let recovery = try await fixture.client.next()
        recovery.succeed(resources)
        try await fixture.settled()
        XCTAssertFalse(fixture.session.taskArchiving.isPresented)
        XCTAssertTrue(fixture.session.taskArchiving.hasPendingAction)
        XCTAssertTrue(fixture.session.taskArchiving.hasUnresolvedIntent)
        fixture.session.reopenTaskArchiving()
        fixture.clock.now.addTimeInterval(60)
        fixture.session.submitTaskArchiving()
        let retryRead = try await fixture.client.next()
        retryRead.succeed(resources)
        let retry = try await fixture.client.next()
        XCTAssertEqual(retry.operation, command.operation)
        retry.succeed(TaskListResources(tasks: [firstTask, archived(archivedTask, false)], active: nil))
        try await fixture.settled()
        XCTAssertFalse(fixture.session.taskArchiving.hasPendingAction)
    }

    @MainActor
    func testUnarchiveLostResponseRecognizesRestorationAndKeepsArchivedFallback() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, archivedTask, archived(secondTask)], active: nil)
        try await fixture.start(resources)
        fixture.session.changeTab(.archived)
        let originalHistory = try await fixture.client.next()
        originalHistory.succeed(emptyPage)
        try await fixture.settled()
        fixture.session.unarchiveTask(taskID: archivedTask.id)
        let preflight = try await fixture.client.next()
        preflight.succeed(resources)
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Response lost", uncertain: true))
        let recovered = try await fixture.client.next()
        recovered.succeed(
            TaskListResources(tasks: [firstTask, archived(archivedTask, false), archived(secondTask)], active: nil))
        let fallbackHistory = try await fixture.client.next()
        XCTAssertEqual(fallbackHistory.operation, .history(task: secondTask.id, cursor: nil))
        fallbackHistory.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.tab, .archived)
        XCTAssertEqual(fixture.session.selectedTaskID, secondTask.id)
        XCTAssertFalse(fixture.session.taskArchiving.hasUnresolvedIntent)
        fixture.session.changeTab(.active)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: archivedTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.selectedTaskID, archivedTask.id)
    }

    @MainActor
    func testQueuedConnectionCannotChangeSourceAfterArchiveSubmission() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.session.openTaskArchive(taskID: secondTask.id)
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        let connecting = Task { await fixture.session.connect(serverSettings) }
        await Task.yield()
        fixture.session.submitTaskArchiving()
        poll.succeed(resources)
        let preflight = try await fixture.client.next()
        preflight.fail(BridgeFailure(message: "Offline", kind: "unavailable"))
        let connected = try await fixture.taskValue(connecting)
        XCTAssertFalse(connected)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.connectionSettings, .local)
        XCTAssertTrue(fixture.session.taskArchiving.hasUnresolvedIntent)
    }

    @MainActor
    func testKnownHiddenErrorReopensWhileTaskCreationIsOpen() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, archivedTask], active: nil)
        try await fixture.start(resources)
        fixture.session.unarchiveTask(taskID: archivedTask.id)
        let preflight = try await fixture.client.next()
        preflight.fail(BridgeFailure(message: "Rejected", kind: "validation"))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.taskArchiving.hasPendingAction)
        XCTAssertFalse(fixture.session.taskArchiving.hasUnresolvedIntent)
        fixture.session.openTaskCreation()
        fixture.session.reopenTaskArchiving()
        XCTAssertTrue(fixture.session.taskArchiving.isPresented)
        XCTAssertTrue(fixture.session.taskCreation.isPresented)
    }

    @MainActor
    func testQueuedConnectionPreventsOpeningArchiveForTheOldSource() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: nil)
        try await fixture.start(resources)
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        let connecting = Task { await fixture.session.connect(serverSettings) }
        await Task.yield()
        XCTAssertTrue(fixture.session.isBlockingControls)
        fixture.session.openTaskArchive(taskID: secondTask.id)
        fixture.session.unarchiveTask(taskID: archivedTask.id)
        XCTAssertNil(fixture.session.taskArchiving.taskID)
        poll.succeed(resources)
        let connection = try await fixture.client.next()
        XCTAssertEqual(connection.operation, .connect(serverSettings))
        connection.succeed(emptyTaskListResources)
        let connected = try await fixture.taskValue(connecting)
        XCTAssertTrue(connected)
        try await fixture.settled()
        XCTAssertFalse(fixture.session.taskArchiving.isPresented)
    }

    @MainActor
    func testArchivingRecoveryPolicyIncludesMalformedTransportAndUnknownErrors() {
        XCTAssertFalse(TaskArchivingState.requiresRecovery(BridgeFailure(message: "Rejected", kind: "validation")))
        XCTAssertTrue(TaskArchivingState.requiresRecovery(BridgeFailure(message: "Malformed", kind: "protocol")))
        XCTAssertTrue(TaskArchivingState.requiresRecovery(BridgeFailure(message: "Offline", kind: "unavailable")))
        XCTAssertTrue(TaskArchivingState.requiresRecovery(BridgeFailure(message: "Refresh", requiresRefresh: true)))
        XCTAssertTrue(TaskArchivingState.requiresRecovery(BridgeFailure(message: "Lost", uncertain: true)))
        XCTAssertTrue(TaskArchivingState.requiresRecovery(CancellationError()))
    }

    @MainActor
    func testObserverNotifiesArchiveAvailabilityWhenTimerChangesWithoutEditingState() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: nil)
        try await fixture.start(resources)
        let harness = PresentationHarness(session: fixture.session)
        let original = fixture.session.taskArchiving
        let presentation = harness.presentation
        let availability = harness.recorder(PresentationReads.taskArchiving) {
            (
                presentation.canArchiveTask(taskID: firstTask.id),
                presentation.canArchiveTask(taskID: secondTask.id),
                presentation.canUnarchiveTask(taskID: archivedTask.id)
            )
        }
        fixture.session.refresh()
        let refresh = try await fixture.client.next()
        refresh.succeed(TaskListResources(tasks: resources.catalog.value, active: activeWorklog))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.taskArchiving, original)
        XCTAssertTrue(availability.values.contains { !$0.0 && $0.1 && $0.2 })
        let countAfterStart = availability.count
        harness.attach()
        harness.update()
        XCTAssertEqual(availability.count, countAfterStart)
        fixture.session.openTaskCreation()
        XCTAssertTrue(fixture.session.taskCreation.isPresented)
        XCTAssertEqual(availability.count, countAfterStart)
        XCTAssertTrue(fixture.session.canArchiveTask(taskID: secondTask.id))
    }

    @MainActor
    func testStateResponseRequiresCorrectArchiveFlagAndNoRunningArchivedTask() {
        let state = TaskArchivingState()
        state.open(firstTask, action: .archive)
        XCTAssertTrue(state.submit(at: "captured"))
        let intent = state.takePendingIntent()!
        XCTAssertNil(state.takePendingIntent())
        XCTAssertFalse(state.responseMatches(TaskListResources(tasks: [], active: nil), intent: intent))
        XCTAssertFalse(state.responseMatches(TaskListResources(tasks: [firstTask], active: nil), intent: intent))
        XCTAssertFalse(
            state.responseMatches(
                TaskListResources(tasks: [archived(firstTask)], active: activeWorklog), intent: intent))
        XCTAssertTrue(
            state.responseMatches(TaskListResources(tasks: [archived(firstTask)], active: nil), intent: intent))
        XCTAssertEqual(
            state.preflight(intent, resources: TaskListResources(tasks: [archived(firstTask)], active: activeWorklog)),
            .review)
        state.reset()
        state.open(archivedTask, action: .unarchive)
        state.reopen()
        XCTAssertTrue(state.submit(at: "restored"))
        let restore = state.takePendingIntent()!
        XCTAssertTrue(
            state.responseMatches(
                TaskListResources(tasks: [archived(archivedTask, false)], active: activeWorklog), intent: restore))
    }
    @MainActor
    func testRejectedArchiveRequiresReviewWhenAnotherWriterAlreadyArchivedTheTask() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources)
        fixture.session.openTaskArchive(taskID: firstTask.id)
        fixture.session.submitTaskArchiving()
        let preflight = try await fixture.client.next()
        preflight.succeed(resources)
        let command = try await fixture.client.next()
        command.fail(
            BridgeFailure(message: "Revision conflict", kind: "conflict", uncertain: false, requiresRefresh: true))
        let desired = archived(firstTask, name: "Competing archive name")
        let reconciliation = try await fixture.client.next()
        reconciliation.succeed(TaskListResources(tasks: [desired], active: nil))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.tasks, [desired])
        XCTAssertTrue(fixture.session.taskArchiving.isPresented)
        XCTAssertTrue(fixture.session.taskArchiving.requiresReview)
        XCTAssertFalse(fixture.session.taskArchiving.canSubmit)
        XCTAssertFalse(fixture.session.taskArchiving.hasUnresolvedIntent)
        XCTAssertEqual(fixture.session.taskArchiving.taskID, firstTask.id)
        XCTAssertEqual(fixture.session.taskArchiving.taskName, firstTask.name)
        XCTAssertEqual(fixture.session.taskArchiving.latest, desired)
        XCTAssertEqual(
            fixture.session.taskArchiving.error,
            "The task changed on another client. Review its current state before continuing.")
        fixture.session.submitTaskArchiving()
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .archive = $0 { return true }; return false
            }.count, 1)
    }

    @MainActor
    func testRejectedArchiveRequiresReviewEvenWhenReconciliationFailsOrStillShowsTheReviewedName() async throws {
        for reconciliationFails in [false, true] {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            let resources = TaskListResources(tasks: [firstTask], active: nil)
            try await fixture.start(resources)
            fixture.session.openTaskArchive(taskID: firstTask.id)
            fixture.session.submitTaskArchiving()
            let preflight = try await fixture.client.next()
            preflight.succeed(resources)
            let command = try await fixture.client.next()
            XCTAssertEqual(
                command.operation,
                .archive(task: firstTask.id, expectedName: firstTask.name, at: "2025-01-01T00:00:00.000Z"))
            command.fail(
                BridgeFailure(message: "Task changed in another client", kind: "conflict", requiresRefresh: true))
            let reconciliation = try await fixture.client.next()
            if reconciliationFails {
                reconciliation.fail(BridgeFailure(message: "Refresh failed", kind: "unavailable"))
            } else {
                reconciliation.succeed(resources)
            }
            try await fixture.settled()
            XCTAssertTrue(fixture.session.taskArchiving.requiresReview)
            XCTAssertFalse(fixture.session.taskArchiving.canSubmit)
            XCTAssertFalse(fixture.session.taskArchiving.hasUnresolvedIntent)
            XCTAssertEqual(fixture.session.taskArchiving.latest, reconciliationFails ? nil : firstTask)
            XCTAssertEqual(
                fixture.session.taskArchiving.error,
                "The task changed on another client. Review its current state before continuing.")
            fixture.session.submitTaskArchiving()
            XCTAssertEqual(
                fixture.client.operations.filter {
                    if case .archive = $0 { return true }; return false
                }.count, 1)
        }
    }

}
