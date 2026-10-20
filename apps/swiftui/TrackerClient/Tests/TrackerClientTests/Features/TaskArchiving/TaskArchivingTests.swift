import Foundation
import XCTest
@testable import TrackerClient

final class TaskArchivingTests: XCTestCase {
    private func archived(_ task: TaskItem, _ value: Bool = true, name: String? = nil) -> TaskItem {
        TaskItem(id: task.id, name: name ?? task.name, archived: value, latestStart: task.latestStart)
    }

    @MainActor
    func testAvailabilityDistinguishesRunningInactiveAndArchivedTasks() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: activeWorklog))

        XCTAssertFalse(fixture.session.canArchiveTask(taskID: firstTask.id))
        XCTAssertTrue(fixture.session.canArchiveTask(taskID: secondTask.id))
        XCTAssertTrue(fixture.session.canUnarchiveTask(taskID: archivedTask.id))
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
        XCTAssertFalse(fixture.session.taskArchiving.isPresented)
        XCTAssertNil(fixture.session.taskArchiving.taskID)
        XCTAssertEqual(fixture.client.operations.count, 2)
    }

    @MainActor
    func testArchiveSendsWriteFirstThenReloadsAndPreservesSelectionHistoryAndTimer() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(resources)
        XCTAssertFalse(fixture.session.canArchiveTask(taskID: firstTask.id))
        XCTAssertFalse(fixture.session.canArchiveTask(taskID: "missing"))
        XCTAssertTrue(fixture.session.canArchiveTask(taskID: secondTask.id))
        fixture.session.openTaskArchive(taskID: secondTask.id)
        fixture.session.submitTaskArchiving()
        let command = try await fixture.client.next()
        XCTAssertEqual(
            command.operation,
            .archive(task: secondTask.id, expectedName: secondTask.name, at: "2025-01-01T00:00:00.000Z"))
        fixture.session.submitTaskArchiving()
        fixture.session.cancelTaskArchiving()
        XCTAssertTrue(fixture.session.taskArchiving.isPresented)
        try await fixture.update("the archived task") {
            command.succeed(TaskListResources(tasks: [firstTask, archived(secondTask)], active: activeWorklog))
        }
        XCTAssertEqual(fixture.session.tasks, [firstTask, archived(secondTask)])
        XCTAssertEqual(fixture.session.selectedTaskID, firstTask.id)
        XCTAssertEqual(fixture.session.active, activeWorklog)
        XCTAssertEqual(fixture.session.tab, .active)
        XCTAssertFalse(fixture.session.taskArchiving.isPresented)
        XCTAssertFalse(fixture.session.taskArchiving.isSubmitting)
        XCTAssertNil(fixture.session.taskArchiving.taskID)
        XCTAssertEqual(
            fixture.client.operations,
            [
                .open(.local), .history(task: firstTask.id, cursor: nil),
                .archive(task: secondTask.id, expectedName: secondTask.name, at: "2025-01-01T00:00:00.000Z"),
                .refresh(fixture.session.connectionSettings),
            ])
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 1)
    }

    @MainActor
    func testSuccessfulArchiveClosesTheDialogBeforeTheReloadFinishes() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        fixture.session.openTaskArchive(taskID: secondTask.id)
        fixture.session.submitTaskArchiving()
        let command = try await fixture.client.next()
        let harness = PresentationHarness(session: fixture.session)
        command.complete(.success(.task(TaskCommandResult(task: archived(secondTask), receipt: nil))))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connectionSettings))
        XCTAssertFalse(harness.presentation.taskArchiving.isPresented, "The dialog must close before the reload.")
        XCTAssertFalse(harness.presentation.taskArchiving.isSubmitting)
        reload.succeed(TaskListResources(tasks: [firstTask, archived(secondTask)], active: nil))
        try await harness.settled(fixture)
        XCTAssertEqual(harness.presentation.visibleTasks, [firstTask])
    }

    @MainActor
    func testSelectedArchivedTaskFallsBackAndReloadsHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.session.openTaskArchive(taskID: firstTask.id)
        fixture.session.submitTaskArchiving()
        let command = try await fixture.client.next()
        command.succeed(TaskListResources(tasks: [archived(firstTask), secondTask], active: nil))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: secondTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.selectedTaskID, secondTask.id)
        XCTAssertEqual(fixture.session.tab, .active)
        XCTAssertFalse(fixture.session.taskArchiving.isPresented)
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
        let command = try await fixture.client.next()
        XCTAssertEqual(command.operation, .unarchive(task: other.id, at: "2025-01-01T00:00:00.000Z"))
        XCTAssertFalse(fixture.session.canUnarchiveTask(taskID: archivedTask.id))
        fixture.session.unarchiveTask(taskID: archivedTask.id)
        try await fixture.update("the restored task") {
            command.succeed(TaskListResources(tasks: [firstTask, archivedTask, secondTask], active: activeWorklog))
        } until: {
            !fixture.session.taskArchiving.isSubmitting
        }
        XCTAssertFalse(fixture.session.taskArchiving.isPresented)
        XCTAssertNil(fixture.session.taskArchiving.error)
        XCTAssertEqual(fixture.session.tab, .archived)
        XCTAssertEqual(fixture.session.selectedTaskID, archivedTask.id)
        XCTAssertEqual(fixture.session.active, activeWorklog)
        fixture.session.changeTab(.active)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: secondTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.selectedTaskID, secondTask.id)
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .unarchive = $0 { return true }; return false
            }.count, 1)
    }

    @MainActor
    func testSubmitIsDisabledWhileSubmittingAndOtherFeaturesStayUsable() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: nil)
        try await fixture.start(resources)
        fixture.session.openTaskArchive(taskID: secondTask.id)
        fixture.session.submitTaskArchiving()
        let command = try await fixture.client.next()
        XCTAssertTrue(fixture.session.taskArchiving.isPresented)
        XCTAssertTrue(fixture.session.taskArchiving.isSubmitting)
        XCTAssertFalse(fixture.session.taskArchiving.canSubmit)
        XCTAssertFalse(fixture.session.canUnarchiveTask(taskID: archivedTask.id))
        XCTAssertTrue(fixture.session.canOpenTaskCreation)
        XCTAssertTrue(fixture.session.canOpenTaskRename)
        XCTAssertTrue(fixture.session.canOpenWorklogCorrection)
        XCTAssertTrue(fixture.session.canOpenWorklogMove)
        XCTAssertTrue(fixture.session.canOpenWorklogDeletion)
        XCTAssertTrue(fixture.session.canOpenBulkTaskArchiving)
        XCTAssertTrue(fixture.session.canStartTracking(taskID: firstTask.id))
        fixture.session.openTaskCreation()
        XCTAssertTrue(fixture.session.taskCreation.isPresented)
        fixture.session.startTracking(taskID: firstTask.id)
        let start = try await fixture.client.next()
        XCTAssertEqual(start.operation, .start(task: firstTask.id, expected: nil, at: "2025-01-01T00:00:00.000Z"))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 2)
        start.succeed(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: activeWorklog))
        let startedHistory = try await fixture.client.next()
        XCTAssertEqual(startedHistory.operation, .history(task: firstTask.id, cursor: nil))
        try await fixture.update("the tracking history") { startedHistory.succeed(emptyPage) }
        XCTAssertEqual(fixture.session.active, activeWorklog)
        XCTAssertTrue(fixture.session.taskArchiving.isSubmitting)
        try await fixture.update("the archived task") {
            command.succeed(
                TaskListResources(tasks: [firstTask, archived(secondTask), archivedTask], active: activeWorklog))
        } until: {
            !fixture.session.taskArchiving.isSubmitting
        }
        XCTAssertFalse(fixture.session.taskArchiving.isPresented)
        XCTAssertTrue(fixture.session.taskCreation.isPresented)
        XCTAssertEqual(fixture.session.active, activeWorklog)
        XCTAssertTrue(fixture.session.canUnarchiveTask(taskID: archivedTask.id))
    }

    @MainActor
    func testFailureShowsServerMessageReloadsTaskListAndResubmitUsesNewTimestamp() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.session.openTaskArchive(taskID: secondTask.id)
        fixture.session.submitTaskArchiving()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "The task changed on another client.", kind: "conflict"))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connectionSettings))
        XCTAssertFalse(fixture.session.taskArchiving.isSubmitting)
        XCTAssertEqual(fixture.session.taskArchiving.error, "The task changed on another client.")
        let renamed = archived(secondTask, false, name: "Renamed elsewhere")
        try await fixture.update("the reloaded task list") {
            reload.succeed(TaskListResources(tasks: [firstTask, renamed], active: nil))
        }
        XCTAssertEqual(fixture.session.tasks, [firstTask, renamed])
        XCTAssertTrue(fixture.session.taskArchiving.isPresented)
        XCTAssertEqual(fixture.session.taskArchiving.taskID, secondTask.id)
        XCTAssertEqual(fixture.session.taskArchiving.taskName, secondTask.name)
        XCTAssertEqual(fixture.session.taskArchiving.action, .archive)
        XCTAssertEqual(fixture.session.taskArchiving.error, "The task changed on another client.")
        XCTAssertTrue(fixture.session.taskArchiving.canSubmit)
        XCTAssertFalse(fixture.session.isStale)
        fixture.clock.now.addTimeInterval(90)
        fixture.session.submitTaskArchiving()
        XCTAssertNil(fixture.session.taskArchiving.error)
        let retry = try await fixture.client.next()
        XCTAssertEqual(
            retry.operation,
            .archive(task: secondTask.id, expectedName: secondTask.name, at: "2025-01-01T00:01:30.000Z"))
        retry.fail(BridgeFailure(message: "Storage rejected write", kind: "validation"))
        let retryReload = try await fixture.client.next()
        try await fixture.update("the second reload") {
            retryReload.succeed(TaskListResources(tasks: [firstTask, renamed], active: nil))
        }
        XCTAssertEqual(fixture.session.taskArchiving.error, "Storage rejected write")
        XCTAssertTrue(fixture.session.taskArchiving.canSubmit)
        fixture.session.cancelTaskArchiving()
        XCTAssertFalse(fixture.session.taskArchiving.isPresented)
        XCTAssertNil(fixture.session.taskArchiving.error)
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .archive = $0 { return true }; return false
            }.count, 2)
    }

    @MainActor
    func testUncertainFailureAsksTheUserToCheckTheReloadedData() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.session.openTaskArchive(taskID: secondTask.id)
        fixture.session.submitTaskArchiving()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Response lost", uncertain: true))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connectionSettings))
        try await fixture.update("the reloaded task list") {
            reload.succeed(TaskListResources(tasks: [firstTask, archived(secondTask)], active: nil))
        }
        XCTAssertEqual(fixture.session.tasks, [firstTask, archived(secondTask)])
        XCTAssertTrue(fixture.session.taskArchiving.isPresented)
        XCTAssertEqual(
            fixture.session.taskArchiving.error,
            "Could not confirm the change. Check the reloaded data before trying again. Response lost")
        XCTAssertFalse(fixture.session.isStale)
        XCTAssertEqual(fixture.client.operations.count, 4)
    }

    @MainActor
    func testFailedReloadAfterRejectedArchiveKeepsErrorAndMarksConnectionStale() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.session.openTaskArchive(taskID: secondTask.id)
        fixture.session.submitTaskArchiving()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Archive rejected", kind: "validation"))
        let reload = try await fixture.client.next()
        try await fixture.update("the failed reload") {
            reload.fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        } until: {
            fixture.session.isStale
        }
        XCTAssertEqual(fixture.session.connectionMessage, "Server offline")
        XCTAssertTrue(fixture.session.taskArchiving.isPresented)
        XCTAssertEqual(fixture.session.taskArchiving.error, "Archive rejected")
        XCTAssertTrue(fixture.session.taskArchiving.canSubmit)
        XCTAssertEqual(fixture.session.tasks, resources.catalog.value)
    }

    @MainActor
    func testArchiveReloadKeepsNewerTaskStateFromTheServer() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.session.openTaskArchive(taskID: secondTask.id)
        fixture.session.submitTaskArchiving()
        let command = try await fixture.client.next()
        try await fixture.update("the reloaded task list") {
            command.succeed(
                TaskListResources(
                    tasks: [firstTask, secondTask], active: nil,
                    tasksRevision: "epoch:3", trackingRevision: "epoch:3"))
        } until: {
            !fixture.session.taskArchiving.isSubmitting
        }
        XCTAssertFalse(fixture.session.taskArchiving.isPresented)
        XCTAssertNil(fixture.session.taskArchiving.error)
        XCTAssertFalse(fixture.session.tasks.first { $0.id == secondTask.id }!.archived)
    }

    @MainActor
    func testUnarchiveFailurePresentsDialogWithErrorAndResubmitSendsUnarchive() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, archivedTask], active: nil)
        try await fixture.start(resources)
        fixture.session.unarchiveTask(taskID: archivedTask.id)
        XCTAssertFalse(fixture.session.taskArchiving.isPresented)
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Rejected", kind: "validation"))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connectionSettings))
        XCTAssertTrue(fixture.session.taskArchiving.isPresented)
        try await fixture.update("the reloaded task list") {
            reload.succeed(resources)
        }
        XCTAssertTrue(fixture.session.taskArchiving.isPresented)
        XCTAssertEqual(fixture.session.taskArchiving.taskID, archivedTask.id)
        XCTAssertEqual(fixture.session.taskArchiving.taskName, archivedTask.name)
        XCTAssertEqual(fixture.session.taskArchiving.action, .unarchive)
        XCTAssertEqual(fixture.session.taskArchiving.error, "Rejected")
        XCTAssertTrue(fixture.session.taskArchiving.canSubmit)
        XCTAssertFalse(fixture.session.canUnarchiveTask(taskID: archivedTask.id))
        fixture.clock.now.addTimeInterval(60)
        fixture.session.submitTaskArchiving()
        let retry = try await fixture.client.next()
        XCTAssertEqual(retry.operation, .unarchive(task: archivedTask.id, at: "2025-01-01T00:01:00.000Z"))
        try await fixture.update("the restored task") {
            retry.succeed(TaskListResources(tasks: [firstTask, archived(archivedTask, false)], active: nil))
        } until: {
            !fixture.session.taskArchiving.isSubmitting
        }
        XCTAssertFalse(fixture.session.taskArchiving.isPresented)
        XCTAssertNil(fixture.session.taskArchiving.error)
        XCTAssertEqual(fixture.session.tasks, [firstTask, archived(archivedTask, false)])
    }

    @MainActor
    func testUnarchiveFailurePresentsDialogWhileWindowIsHiddenAndCancelClosesIt() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, archivedTask], active: nil)
        try await fixture.start(resources)
        fixture.session.setWindowVisible(false)
        fixture.session.unarchiveTask(taskID: archivedTask.id)
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Offline", kind: "unavailable"))
        let reload = try await fixture.client.next()
        try await fixture.update("the reloaded task list") {
            reload.succeed(resources)
        }
        XCTAssertTrue(fixture.session.taskArchiving.isPresented)
        XCTAssertEqual(fixture.session.taskArchiving.error, "Offline")
        fixture.session.cancelTaskArchiving()
        XCTAssertFalse(fixture.session.taskArchiving.isPresented)
        XCTAssertNil(fixture.session.taskArchiving.taskID)
        XCTAssertTrue(fixture.session.canUnarchiveTask(taskID: archivedTask.id))
    }

    @MainActor
    func testArchiveIsSentWhileAPollRunsAndPollingDoesNotChangeCapturedTask() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        XCTAssertTrue(fixture.session.canArchiveTask(taskID: secondTask.id))
        fixture.session.openTaskArchive(taskID: secondTask.id)
        fixture.session.submitTaskArchiving()
        let command = try await fixture.client.next()
        XCTAssertEqual(
            command.operation,
            .archive(task: secondTask.id, expectedName: secondTask.name, at: "2025-01-01T00:00:00.000Z"))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 2)
        let backgroundName = archived(secondTask, false, name: "Background name")
        try await fixture.update("the background task name") {
            poll.succeed(TaskListResources(tasks: [firstTask, backgroundName], active: nil))
        }
        XCTAssertEqual(fixture.session.taskArchiving.taskName, secondTask.name)
        XCTAssertTrue(fixture.session.taskArchiving.isSubmitting)
        try await fixture.update("the archived task") {
            command.succeed(TaskListResources(tasks: [firstTask, archived(backgroundName)], active: nil))
        } until: {
            !fixture.session.taskArchiving.isSubmitting
        }
        XCTAssertFalse(fixture.session.taskArchiving.isPresented)
        XCTAssertEqual(fixture.session.tasks, [firstTask, archived(backgroundName)])
    }

    @MainActor
    func testOtherEditorsStayAvailableAndObserverFreezesClosingContent() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: nil))
        fixture.session.openWorklogCorrection(worklogID: "missing")
        let harness = PresentationHarness(session: fixture.session)
        let archiveUpdates = harness.counter(PresentationReads.taskArchiving)
        let contentUpdates = harness.counter(PresentationReads.content)
        fixture.session.openTaskArchive(taskID: secondTask.id)
        XCTAssertTrue(fixture.session.canOpenWorklogCorrection)
        XCTAssertTrue(fixture.session.canOpenWorklogMove)
        XCTAssertTrue(fixture.session.canOpenTaskCreation)
        XCTAssertTrue(fixture.session.canOpenBulkTaskArchiving)
        XCTAssertFalse(fixture.session.canArchiveTask(taskID: firstTask.id))
        let shown = harness.presentation.taskArchivingSheetContent
        fixture.session.cancelTaskArchiving()
        XCTAssertEqual(harness.presentation.taskArchivingSheetContent, shown)
        XCTAssertFalse(fixture.session.taskArchiving.isPresented)
        XCTAssertEqual(archiveUpdates.count, 2)
        XCTAssertEqual(contentUpdates.count, 0)
    }

    @MainActor
    func testOpenConfirmationDoesNotBlockConnectionChange() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        fixture.session.openTaskArchive(taskID: secondTask.id)
        let connecting = Task { await fixture.session.connect(serverSettings) }
        let connection = try await fixture.client.next()
        XCTAssertEqual(connection.operation, .connect(serverSettings))
        connection.succeed(emptyTaskListResources)
        let connected = try await fixture.taskValue(connecting)
        XCTAssertTrue(connected)
        XCTAssertEqual(fixture.session.connectionSettings, serverSettings)
        XCTAssertNil(fixture.session.connectionMessage)
        XCTAssertTrue(fixture.session.taskArchiving.isPresented)
        XCTAssertEqual(fixture.session.taskArchiving.taskID, secondTask.id)
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
    func testStateCapturesRequestAndKeepsDialogUntilFailureOrReset() {
        let state = TaskArchivingState()
        XCTAssertNil(state.submit())
        state.open(firstTask, action: .archive)
        XCTAssertTrue(state.isOpen)
        XCTAssertTrue(state.presentation.canSubmit)
        let request = state.submit()
        XCTAssertEqual(request?.taskID, firstTask.id)
        XCTAssertEqual(request?.taskName, firstTask.name)
        XCTAssertEqual(request?.action, .archive)
        XCTAssertNil(state.submit())
        XCTAssertFalse(state.presentation.canSubmit)
        state.cancel()
        XCTAssertTrue(state.isSubmitting)
        state.open(secondTask, action: .archive)
        XCTAssertEqual(state.original, firstTask)
        state.fail(BridgeFailure(message: "Rejected", kind: "validation"))
        XCTAssertFalse(state.isSubmitting)
        XCTAssertTrue(state.isPresented)
        XCTAssertEqual(state.error, "Rejected")
        XCTAssertNotNil(state.submit())
        XCTAssertNil(state.error)
        state.reset()
        XCTAssertFalse(state.isOpen)
        XCTAssertNil(state.original)

        state.open(archivedTask, action: .unarchive)
        XCTAssertFalse(state.isPresented)
        XCTAssertFalse(state.presentation.canSubmit)
        let restore = state.submit()
        XCTAssertEqual(restore?.taskID, archivedTask.id)
        XCTAssertEqual(restore?.action, .unarchive)
        XCTAssertTrue(state.isOpen)
        state.fail(BridgeFailure(message: "Offline", kind: "unavailable"))
        XCTAssertTrue(state.isPresented)
        XCTAssertEqual(state.presentation.action, .unarchive)
        XCTAssertEqual(state.presentation.error, "Offline")
        XCTAssertTrue(state.presentation.canSubmit)
        state.cancel()
        XCTAssertFalse(state.isOpen)
        XCTAssertNil(state.error)
    }
}
