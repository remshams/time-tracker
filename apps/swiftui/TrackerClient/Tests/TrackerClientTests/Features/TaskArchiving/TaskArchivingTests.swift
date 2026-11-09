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

        XCTAssertFalse(fixture.taskArchiving.canArchive(taskID: firstTask.id))
        XCTAssertTrue(fixture.taskArchiving.canArchive(taskID: secondTask.id))
        XCTAssertFalse(fixture.taskArchiving.canArchive(taskID: archivedTask.id))
        XCTAssertTrue(fixture.taskArchiving.canUnarchive(taskID: archivedTask.id))
    }

    @MainActor
    func testUnarchiveAvailabilityRequiresAnArchivedTaskWithTheRequestedIdentity() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, archivedTask], active: nil))

        XCTAssertTrue(fixture.taskArchiving.canUnarchive(taskID: archivedTask.id))
        XCTAssertFalse(fixture.taskArchiving.canUnarchive(taskID: firstTask.id))
        XCTAssertFalse(fixture.taskArchiving.canUnarchive(taskID: "missing-task"))
    }

    @MainActor
    func testArchiveCapturesClickedTaskAndCancelDoesNotWrite() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        XCTAssertFalse(fixture.taskArchiving.canArchive(taskID: firstTask.id))
        fixture.taskArchiving.openArchive(taskID: firstTask.id)
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: nil))
        fixture.taskArchiving.openArchive(taskID: secondTask.id)
        XCTAssertEqual(fixture.taskArchiving.taskID, secondTask.id)
        XCTAssertEqual(fixture.taskArchiving.taskName, secondTask.name)
        XCTAssertEqual(fixture.taskArchiving.action, .archive)
        XCTAssertTrue(fixture.taskArchiving.canSubmit)
        fixture.taskArchiving.cancel()
        fixture.taskArchiving.submit()
        XCTAssertFalse(fixture.taskArchiving.isPresented)
        XCTAssertFalse(fixture.taskArchiving.isSubmitting)
        XCTAssertEqual(fixture.client.operations.count, 2)
    }

    @MainActor
    func testArchiveSendsWriteFirstThenReloadsAndPreservesSelectionHistoryAndTimer() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(resources)
        XCTAssertFalse(fixture.taskArchiving.canArchive(taskID: firstTask.id))
        XCTAssertFalse(fixture.taskArchiving.canArchive(taskID: "missing"))
        XCTAssertTrue(fixture.taskArchiving.canArchive(taskID: secondTask.id))
        fixture.taskArchiving.openArchive(taskID: secondTask.id)
        fixture.taskArchiving.submit()
        let command = try await fixture.client.next()
        XCTAssertEqual(
            command.operation,
            .archive(task: secondTask.id, expectedName: secondTask.name, at: "2025-01-01T00:00:00.000Z"))
        fixture.taskArchiving.submit()
        fixture.taskArchiving.cancel()
        XCTAssertTrue(fixture.taskArchiving.isPresented)
        try await fixture.update("the archived task") {
            command.succeed(TaskListResources(tasks: [firstTask, archived(secondTask)], active: activeWorklog))
        }
        XCTAssertEqual(fixture.session.tasks, [firstTask, archived(secondTask)])
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, firstTask.id)
        XCTAssertEqual(fixture.session.active, activeWorklog)
        XCTAssertEqual(fixture.session.taskCatalog.tab, .active)
        XCTAssertFalse(fixture.taskArchiving.isPresented)
        XCTAssertFalse(fixture.taskArchiving.isSubmitting)
        XCTAssertEqual(
            fixture.client.operations,
            [
                .open(.local), .history(task: firstTask.id, cursor: nil),
                .archive(task: secondTask.id, expectedName: secondTask.name, at: "2025-01-01T00:00:00.000Z"),
                .refresh(fixture.session.connection.settings),
            ])
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 1)
    }

    @MainActor
    func testSuccessfulArchiveClosesTheDialogBeforeTheReloadFinishes() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        fixture.taskArchiving.openArchive(taskID: secondTask.id)
        fixture.taskArchiving.submit()
        let command = try await fixture.client.next()
        command.complete(.success(.task(TaskCommandResult(task: archived(secondTask)))))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connection.settings))
        XCTAssertFalse(fixture.taskArchiving.isPresented, "The dialog must close before the reload.")
        XCTAssertFalse(fixture.taskArchiving.isSubmitting)
        reload.succeed(TaskListResources(tasks: [firstTask, archived(secondTask)], active: nil))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.taskCatalog.visibleTasks, [firstTask])
    }

    @MainActor
    func testSelectedArchivedTaskFallsBackAndReloadsHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.taskArchiving.openArchive(taskID: firstTask.id)
        fixture.taskArchiving.submit()
        let command = try await fixture.client.next()
        command.succeed(TaskListResources(tasks: [archived(firstTask), secondTask], active: nil))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: secondTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, secondTask.id)
        XCTAssertEqual(fixture.session.taskCatalog.tab, .active)
        XCTAssertFalse(fixture.taskArchiving.isPresented)
    }

    @MainActor
    func testUnarchiveRunsDirectlyAndRemembersRestoredTaskWithoutSwitchingTabs() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let other = archived(secondTask)
        let resources = TaskListResources(tasks: [firstTask, archivedTask, other], active: activeWorklog)
        try await fixture.start(resources)
        fixture.sync.changeTab(.archived)
        let originalHistory = try await fixture.client.next()
        originalHistory.succeed(emptyPage)
        try await fixture.settled()
        fixture.taskArchiving.unarchive(taskID: other.id)
        XCTAssertFalse(fixture.taskArchiving.isPresented)
        XCTAssertTrue(fixture.taskArchiving.isSubmitting)
        let command = try await fixture.client.next()
        XCTAssertEqual(command.operation, .unarchive(task: other.id, at: "2025-01-01T00:00:00.000Z"))
        XCTAssertFalse(fixture.taskArchiving.canUnarchive(taskID: archivedTask.id))
        fixture.taskArchiving.unarchive(taskID: archivedTask.id)
        try await fixture.update("the restored task") {
            command.succeed(TaskListResources(tasks: [firstTask, archivedTask, secondTask], active: activeWorklog))
        } until: {
            !fixture.taskArchiving.isSubmitting
        }
        XCTAssertFalse(fixture.taskArchiving.isPresented)
        XCTAssertNil(fixture.taskArchiving.error)
        XCTAssertEqual(fixture.session.taskCatalog.tab, .archived)
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, archivedTask.id)
        XCTAssertEqual(fixture.session.active, activeWorklog)
        fixture.sync.changeTab(.active)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: secondTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, secondTask.id)
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .unarchive = $0 { return true }; return false
            }.count, 1)
    }

    @MainActor
    func testOpeningAnotherTaskDuringTheReloadStillRemembersTheRestoredTask() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let other = archived(secondTask)
        try await fixture.start(TaskListResources(tasks: [firstTask, archivedTask, other], active: nil))
        fixture.sync.changeTab(.archived)
        try await fixture.client.next().succeed(emptyPage)
        try await fixture.settled()
        fixture.taskArchiving.unarchive(taskID: other.id)
        let command = try await fixture.client.next()
        command.complete(.success(.task(TaskCommandResult(task: secondTask))))
        let reload = try await fixture.client.next()
        fixture.taskArchiving.openArchive(taskID: firstTask.id)
        XCTAssertTrue(fixture.taskArchiving.isPresented)
        XCTAssertEqual(fixture.taskArchiving.action, .archive)
        try await fixture.update("the restored task") {
            reload.succeed(TaskListResources(tasks: [firstTask, archivedTask, secondTask], active: nil))
        }
        XCTAssertEqual(fixture.taskArchiving.taskID, firstTask.id)
        fixture.sync.changeTab(.active)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: secondTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
    }

    @MainActor
    func testSubmitIsDisabledWhileSubmittingAndOtherFeaturesStayUsable() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: nil)
        try await fixture.start(resources)
        fixture.taskArchiving.openArchive(taskID: secondTask.id)
        fixture.taskArchiving.submit()
        let command = try await fixture.client.next()
        XCTAssertTrue(fixture.taskArchiving.isPresented)
        XCTAssertTrue(fixture.taskArchiving.isSubmitting)
        XCTAssertFalse(fixture.taskArchiving.canSubmit)
        XCTAssertFalse(fixture.taskArchiving.canUnarchive(taskID: archivedTask.id))
        fixture.taskArchiving.openArchive(taskID: firstTask.id)
        XCTAssertEqual(fixture.taskArchiving.taskID, secondTask.id)
        XCTAssertTrue(fixture.taskCreation.canOpen)
        XCTAssertTrue(fixture.taskRename.canOpen)
        XCTAssertTrue(fixture.worklogCorrection.canOpen)
        XCTAssertTrue(fixture.worklogMove.canOpen)
        XCTAssertTrue(fixture.worklogDeletion.canOpen)
        XCTAssertTrue(fixture.bulkTaskArchiving.canOpen)
        XCTAssertTrue(fixture.trackingAvailability.canStart(taskID: firstTask.id))
        fixture.taskCreation.open()
        XCTAssertTrue(fixture.taskCreation.isPresented)
        fixture.trackingActions.startTracking(taskID: firstTask.id)
        let start = try await fixture.client.next()
        XCTAssertEqual(start.operation, .start(task: firstTask.id, expected: nil, at: "2025-01-01T00:00:00.000Z"))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 2)
        start.succeed(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: activeWorklog))
        let startedHistory = try await fixture.client.next()
        XCTAssertEqual(startedHistory.operation, .history(task: firstTask.id, cursor: nil))
        try await fixture.update("the tracking history") { startedHistory.succeed(emptyPage) }
        XCTAssertEqual(fixture.session.active, activeWorklog)
        XCTAssertTrue(fixture.taskArchiving.isSubmitting)
        try await fixture.update("the archived task") {
            command.succeed(
                TaskListResources(tasks: [firstTask, archived(secondTask), archivedTask], active: activeWorklog))
        } until: {
            !fixture.taskArchiving.isSubmitting
        }
        XCTAssertFalse(fixture.taskArchiving.isPresented)
        XCTAssertTrue(fixture.taskCreation.isPresented)
        XCTAssertEqual(fixture.session.active, activeWorklog)
        XCTAssertTrue(fixture.taskArchiving.canUnarchive(taskID: archivedTask.id))
    }

    @MainActor
    func testFailureShowsServerMessageReloadsTaskListAndResubmitUsesNewTimestamp() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.taskArchiving.openArchive(taskID: secondTask.id)
        fixture.taskArchiving.submit()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "The task changed on another client.", kind: "conflict"))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connection.settings))
        XCTAssertFalse(fixture.taskArchiving.isSubmitting)
        XCTAssertEqual(fixture.taskArchiving.error, "The task changed on another client.")
        let renamed = archived(secondTask, false, name: "Renamed elsewhere")
        try await fixture.update("the reloaded task list") {
            reload.succeed(TaskListResources(tasks: [firstTask, renamed], active: nil))
        }
        XCTAssertEqual(fixture.session.tasks, [firstTask, renamed])
        XCTAssertTrue(fixture.taskArchiving.isPresented)
        XCTAssertEqual(fixture.taskArchiving.taskID, secondTask.id)
        XCTAssertEqual(fixture.taskArchiving.taskName, secondTask.name)
        XCTAssertEqual(fixture.taskArchiving.action, .archive)
        XCTAssertEqual(fixture.taskArchiving.error, "The task changed on another client.")
        XCTAssertTrue(fixture.taskArchiving.canSubmit)
        XCTAssertFalse(fixture.session.connection.stale)
        fixture.clock.now.addTimeInterval(90)
        fixture.taskArchiving.submit()
        XCTAssertNil(fixture.taskArchiving.error)
        let retry = try await fixture.client.next()
        XCTAssertEqual(
            retry.operation,
            .archive(task: secondTask.id, expectedName: secondTask.name, at: "2025-01-01T00:01:30.000Z"))
        retry.fail(BridgeFailure(message: "Storage rejected write", kind: "validation"))
        let retryReload = try await fixture.client.next()
        try await fixture.update("the second reload") {
            retryReload.succeed(TaskListResources(tasks: [firstTask, renamed], active: nil))
        }
        XCTAssertEqual(fixture.taskArchiving.error, "Storage rejected write")
        XCTAssertTrue(fixture.taskArchiving.canSubmit)
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
        fixture.taskArchiving.openArchive(taskID: secondTask.id)
        fixture.taskArchiving.submit()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Response lost", uncertain: true))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connection.settings))
        try await fixture.update("the reloaded task list") {
            reload.succeed(TaskListResources(tasks: [firstTask, archived(secondTask)], active: nil))
        }
        XCTAssertEqual(fixture.session.tasks, [firstTask, archived(secondTask)])
        XCTAssertTrue(fixture.taskArchiving.isPresented)
        XCTAssertEqual(
            fixture.taskArchiving.error,
            "Could not confirm the change. Check the reloaded data before trying again. Response lost")
        XCTAssertFalse(fixture.session.connection.stale)
        XCTAssertEqual(fixture.client.operations.count, 4)
    }

    @MainActor
    func testFailedReloadAfterRejectedArchiveKeepsErrorAndMarksConnectionStale() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.taskArchiving.openArchive(taskID: secondTask.id)
        fixture.taskArchiving.submit()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Archive rejected", kind: "validation"))
        let reload = try await fixture.client.next()
        try await fixture.update("the failed reload") {
            reload.fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        } until: {
            fixture.session.connection.stale
        }
        XCTAssertEqual(fixture.session.connection.message, "Server offline")
        XCTAssertTrue(fixture.taskArchiving.isPresented)
        XCTAssertEqual(fixture.taskArchiving.error, "Archive rejected")
        XCTAssertTrue(fixture.taskArchiving.canSubmit)
        XCTAssertEqual(fixture.session.tasks, resources.catalog.value)
    }

    @MainActor
    func testArchiveReloadKeepsNewerTaskStateFromTheServer() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.taskArchiving.openArchive(taskID: secondTask.id)
        fixture.taskArchiving.submit()
        let command = try await fixture.client.next()
        try await fixture.update("the reloaded task list") {
            command.succeed(
                TaskListResources(tasks: [firstTask, secondTask], active: nil), applyingTaskCommand: true)
        } until: {
            !fixture.taskArchiving.isSubmitting
        }
        XCTAssertFalse(fixture.taskArchiving.isPresented)
        XCTAssertNil(fixture.taskArchiving.error)
        XCTAssertFalse(fixture.session.tasks.first { $0.id == secondTask.id }!.archived)
    }

    @MainActor
    func testUnarchiveFailurePresentsDialogWithErrorAndResubmitSendsUnarchive() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, archivedTask], active: nil)
        try await fixture.start(resources)
        fixture.taskArchiving.unarchive(taskID: archivedTask.id)
        XCTAssertFalse(fixture.taskArchiving.isPresented)
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Rejected", kind: "validation"))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connection.settings))
        XCTAssertTrue(fixture.taskArchiving.isPresented)
        try await fixture.update("the reloaded task list") {
            reload.succeed(resources)
        }
        XCTAssertTrue(fixture.taskArchiving.isPresented)
        XCTAssertEqual(fixture.taskArchiving.taskID, archivedTask.id)
        XCTAssertEqual(fixture.taskArchiving.taskName, archivedTask.name)
        XCTAssertEqual(fixture.taskArchiving.action, .unarchive)
        XCTAssertEqual(fixture.taskArchiving.error, "Rejected")
        XCTAssertTrue(fixture.taskArchiving.canSubmit)
        XCTAssertFalse(fixture.taskArchiving.canUnarchive(taskID: archivedTask.id))
        fixture.clock.now.addTimeInterval(60)
        fixture.taskArchiving.submit()
        let retry = try await fixture.client.next()
        XCTAssertEqual(retry.operation, .unarchive(task: archivedTask.id, at: "2025-01-01T00:01:00.000Z"))
        try await fixture.update("the restored task") {
            retry.succeed(TaskListResources(tasks: [firstTask, archived(archivedTask, false)], active: nil))
        } until: {
            !fixture.taskArchiving.isSubmitting
        }
        XCTAssertFalse(fixture.taskArchiving.isPresented)
        XCTAssertNil(fixture.taskArchiving.error)
        XCTAssertEqual(fixture.session.tasks, [firstTask, archived(archivedTask, false)])
    }

    @MainActor
    func testUnarchiveFailurePresentsDialogWhileWindowIsHiddenAndCancelClosesIt() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, archivedTask], active: nil)
        try await fixture.start(resources)
        fixture.sync.setWindowVisible(false)
        fixture.taskArchiving.unarchive(taskID: archivedTask.id)
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Offline", kind: "unavailable"))
        let reload = try await fixture.client.next()
        try await fixture.update("the reloaded task list") {
            reload.succeed(resources)
        }
        XCTAssertTrue(fixture.taskArchiving.isPresented)
        XCTAssertEqual(fixture.taskArchiving.error, "Offline")
        fixture.taskArchiving.cancel()
        XCTAssertFalse(fixture.taskArchiving.isPresented)
        XCTAssertTrue(fixture.taskArchiving.canUnarchive(taskID: archivedTask.id))
    }

    @MainActor
    func testArchiveIsSentWhileAPollRunsAndPollingDoesNotChangeCapturedTask() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.sync.refresh()
        let poll = try await fixture.client.next()
        XCTAssertTrue(fixture.taskArchiving.canArchive(taskID: secondTask.id))
        fixture.taskArchiving.openArchive(taskID: secondTask.id)
        fixture.taskArchiving.submit()
        let command = try await fixture.client.next()
        XCTAssertEqual(
            command.operation,
            .archive(task: secondTask.id, expectedName: secondTask.name, at: "2025-01-01T00:00:00.000Z"))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 2)
        let backgroundName = archived(secondTask, false, name: "Background name")
        try await fixture.update("the background task name") {
            poll.succeed(TaskListResources(tasks: [firstTask, backgroundName], active: nil))
        }
        XCTAssertEqual(fixture.taskArchiving.taskName, secondTask.name)
        XCTAssertTrue(fixture.taskArchiving.isSubmitting)
        try await fixture.update("the archived task") {
            command.succeed(TaskListResources(tasks: [firstTask, archived(backgroundName)], active: nil))
        } until: {
            !fixture.taskArchiving.isSubmitting
        }
        XCTAssertFalse(fixture.taskArchiving.isPresented)
        XCTAssertEqual(fixture.session.tasks, [firstTask, archived(backgroundName)])
    }

    @MainActor
    func testOtherEditorsStayAvailableAndClosingKeepsTheContent() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: nil))
        fixture.worklogCorrection.open(worklogID: "missing")
        let archive = ObservationProbe { ViewReads.taskArchiving(fixture.session) }
        let content = ObservationProbe { ViewReads.content(fixture.session) }
        fixture.taskArchiving.openArchive(taskID: secondTask.id)
        XCTAssertTrue(archive.consume())
        XCTAssertTrue(fixture.worklogCorrection.canOpen)
        XCTAssertTrue(fixture.worklogMove.canOpen)
        XCTAssertTrue(fixture.taskCreation.canOpen)
        XCTAssertTrue(fixture.bulkTaskArchiving.canOpen)
        XCTAssertFalse(fixture.taskArchiving.canArchive(taskID: firstTask.id))
        fixture.taskArchiving.cancel()
        XCTAssertTrue(archive.consume())
        XCTAssertFalse(fixture.taskArchiving.isPresented)
        XCTAssertEqual(fixture.taskArchiving.taskName, secondTask.name)
        XCTAssertFalse(content.consume())
    }

    @MainActor
    func testObserverNotifiesArchiveAvailabilityWhenTimerChangesWithoutEditingState() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: nil)
        try await fixture.start(resources)
        let availability = ObservationProbe {
            _ = fixture.taskArchiving.canArchive(taskID: firstTask.id)
            _ = fixture.taskArchiving.canArchive(taskID: secondTask.id)
            _ = fixture.taskArchiving.canUnarchive(taskID: archivedTask.id)
        }
        fixture.sync.refresh()
        let refresh = try await fixture.client.next()
        refresh.succeed(TaskListResources(tasks: resources.catalog.value, active: activeWorklog))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertFalse(fixture.taskArchiving.isPresented)
        XCTAssertEqual(fixture.taskArchiving.taskName, "")
        XCTAssertTrue(availability.consume())
        XCTAssertFalse(fixture.taskArchiving.canArchive(taskID: firstTask.id))
        XCTAssertTrue(fixture.taskArchiving.canArchive(taskID: secondTask.id))
        XCTAssertTrue(fixture.taskArchiving.canUnarchive(taskID: archivedTask.id))
        fixture.taskCreation.open()
        XCTAssertTrue(fixture.taskCreation.isPresented)
        XCTAssertFalse(availability.consume())
        XCTAssertTrue(fixture.taskArchiving.canArchive(taskID: secondTask.id))
    }

    @MainActor
    func testClosedDialogKeepsItsContentUntilTheNextOpenAndSendsNothing() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        XCTAssertFalse(fixture.taskArchiving.canSubmit)
        fixture.taskArchiving.submit()
        XCTAssertFalse(fixture.taskArchiving.isSubmitting)
        fixture.taskArchiving.openArchive(taskID: secondTask.id)
        fixture.taskArchiving.submit()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Rejected", kind: "validation"))
        let reload = try await fixture.client.next()
        try await fixture.update("the reloaded task list") { reload.succeed(resources) }
        fixture.taskArchiving.cancel()
        XCTAssertFalse(fixture.taskArchiving.isPresented)
        XCTAssertEqual(fixture.taskArchiving.taskID, secondTask.id)
        XCTAssertEqual(fixture.taskArchiving.taskName, secondTask.name)
        XCTAssertEqual(fixture.taskArchiving.action, .archive)
        XCTAssertEqual(fixture.taskArchiving.error, "Rejected")
        XCTAssertFalse(fixture.taskArchiving.canSubmit)
        let operations = fixture.client.operations.count
        fixture.taskArchiving.submit()
        XCTAssertFalse(fixture.taskArchiving.isSubmitting)
        XCTAssertEqual(fixture.client.operations.count, operations)
        fixture.taskArchiving.openArchive(taskID: firstTask.id)
        XCTAssertTrue(fixture.taskArchiving.isPresented)
        XCTAssertEqual(fixture.taskArchiving.taskID, firstTask.id)
        XCTAssertEqual(fixture.taskArchiving.taskName, firstTask.name)
        XCTAssertNil(fixture.taskArchiving.error)
    }

    @MainActor
    func testUnconfirmedConnectionBlocksBothActionsEvenWithLoadedTasks() async throws {
        let catalog = TaskCatalogStore()
        _ = catalog.apply([firstTask, archivedTask], previousActive: nil, active: nil)
        let client = FakeClient()
        let archiving = TaskArchivingStore(
            client: client, clock: FakeClock(), connection: ConnectionStore(settings: .local), catalog: catalog,
            tracking: TrackingStore(), reloadAfterWrite: { _ in })
        XCTAssertFalse(archiving.canArchive(taskID: firstTask.id))
        XCTAssertFalse(archiving.canUnarchive(taskID: archivedTask.id))
        archiving.openArchive(taskID: firstTask.id)
        XCTAssertFalse(archiving.isPresented)
        archiving.unarchive(taskID: archivedTask.id)
        XCTAssertFalse(archiving.isSubmitting)
        XCTAssertEqual(client.operations, [])
    }
}
