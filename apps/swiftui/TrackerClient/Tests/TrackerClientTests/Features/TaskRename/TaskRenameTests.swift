import Foundation
import XCTest
@testable import TrackerClient

final class TaskRenameTests: XCTestCase {
    @MainActor
    func testOpeningCapturesIdentityAndCancelMakesNoWrite() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        fixture.taskRename.open(taskID: firstTask.id)
        XCTAssertFalse(fixture.taskRename.canOpen)
        XCTAssertFalse(fixture.taskRename.isPresented)
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        fixture.taskRename.open(taskID: firstTask.id)
        XCTAssertEqual(fixture.taskRename.taskID, firstTask.id)
        XCTAssertEqual(fixture.taskRename.originalName, firstTask.name)
        XCTAssertEqual(fixture.taskRename.name, firstTask.name)
        XCTAssertFalse(fixture.taskRename.canSubmit)
        fixture.taskRename.setName("New name")
        XCTAssertTrue(fixture.taskRename.canSubmit)
        fixture.taskRename.cancel()
        XCTAssertFalse(fixture.taskRename.isPresented)
        XCTAssertEqual(fixture.client.operations.count, 2)
    }

    @MainActor
    func testClosedDialogKeepsItsContentUntilTheNextOpenAndSendsNothing() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        fixture.taskRename.open(taskID: firstTask.id)
        fixture.taskRename.setName("Valid\u{0000}suffix")
        fixture.taskRename.submit()
        XCTAssertEqual(fixture.taskRename.error, "Task names must not contain control characters.")
        fixture.taskRename.cancel()
        XCTAssertFalse(fixture.taskRename.isPresented)
        XCTAssertEqual(fixture.taskRename.taskID, firstTask.id)
        XCTAssertEqual(fixture.taskRename.originalName, firstTask.name)
        XCTAssertEqual(fixture.taskRename.name, "Valid\u{0000}suffix")
        XCTAssertEqual(fixture.taskRename.error, "Task names must not contain control characters.")
        XCTAssertFalse(fixture.taskRename.canSubmit)
        fixture.taskRename.setName("Hidden draft")
        fixture.taskRename.submit()
        XCTAssertEqual(fixture.taskRename.name, "Valid\u{0000}suffix")
        XCTAssertEqual(fixture.client.operations.count, 2)
        fixture.taskRename.open(taskID: secondTask.id)
        XCTAssertTrue(fixture.taskRename.isPresented)
        XCTAssertEqual(fixture.taskRename.taskID, secondTask.id)
        XCTAssertEqual(fixture.taskRename.originalName, secondTask.name)
        XCTAssertEqual(fixture.taskRename.name, secondTask.name)
        XCTAssertNil(fixture.taskRename.error)
    }

    @MainActor
    func testUnconfirmedConnectionKeepsTheDialogClosedEvenWithASelectedTask() async throws {
        let catalog = TaskCatalogStore()
        _ = catalog.apply([firstTask], previousActive: nil, active: nil)
        let rename = TaskRenameStore(
            client: FakeClient(), clock: FakeClock(), connection: ConnectionStore(settings: .local), catalog: catalog,
            reloadAfterWrite: { _ in })
        XCTAssertEqual(catalog.selectedTask, firstTask)
        XCTAssertFalse(rename.canOpen)
        rename.open(taskID: firstTask.id)
        XCTAssertFalse(rename.isPresented)
    }

    @MainActor
    func testSubmissionSendsRenameFirstThenReloadsTaskListAndClosesSheet() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.taskRename.open(taskID: firstTask.id)
        fixture.taskRename.setName("Renamed task")
        fixture.taskRename.submit()
        XCTAssertTrue(fixture.taskRename.isSubmitting)
        XCTAssertFalse(fixture.taskRename.canSubmit)
        let rename = try await fixture.client.next()
        XCTAssertEqual(
            rename.operation,
            .rename(
                task: firstTask.id, expectedName: firstTask.name, name: "Renamed task",
                at: "2025-01-01T00:00:00.000Z"))
        fixture.taskRename.submit()
        fixture.taskRename.cancel()
        fixture.taskRename.setName("Replacement")
        XCTAssertTrue(fixture.taskRename.isPresented)
        XCTAssertEqual(fixture.taskRename.name, "Renamed task")
        let renamed = TaskItem(id: firstTask.id, name: "Renamed task", archived: false, latestStart: nil)
        rename.complete(.success(.task(TaskCommandResult(task: renamed))))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.features.connection.settings))
        XCTAssertFalse(fixture.taskRename.isPresented)
        XCTAssertFalse(fixture.taskRename.isSubmitting)
        try await fixture.update("the reloaded task list") {
            reload.succeed(TaskListResources(tasks: [renamed], active: nil))
        }
        XCTAssertEqual(fixture.features.taskCatalog.tasks, [renamed])
        XCTAssertNil(fixture.taskRename.error)
        XCTAssertEqual(
            fixture.client.operations,
            [
                .open(.local), .history(task: firstTask.id, cursor: nil),
                .rename(
                    task: firstTask.id, expectedName: firstTask.name, name: "Renamed task",
                    at: "2025-01-01T00:00:00.000Z"),
                .refresh(fixture.features.connection.settings),
            ])
    }

    @MainActor
    func testRenameKeepsCapturedTargetWhenSelectionChangesAndPreservesTrackingAndHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(resources)
        fixture.taskRename.open(taskID: firstTask.id)
        fixture.taskRename.setName("Renamed task")
        fixture.features.taskCatalog.select(secondTask.id)
        let selectedHistory = try await fixture.client.next()
        let selectedLog = WorklogItem(
            id: "second-log", taskId: secondTask.id, start: oldWorklog.start, end: oldWorklog.end)
        selectedHistory.succeed(HistoryPage(worklogs: [selectedLog], nextCursor: "older", reset: false))
        try await fixture.settled()
        fixture.taskRename.submit()
        fixture.clock.now.addTimeInterval(30)
        let rename = try await fixture.client.next()
        XCTAssertEqual(
            rename.operation,
            .rename(
                task: firstTask.id, expectedName: firstTask.name, name: "Renamed task", at: "2025-01-01T00:00:00.000Z"))
        let renamed = TaskItem(id: firstTask.id, name: "Renamed task", archived: false, latestStart: nil)
        try await fixture.update("the renamed task") {
            rename.succeed(TaskListResources(tasks: [renamed, secondTask], active: activeWorklog))
        }
        XCTAssertFalse(fixture.taskRename.isPresented)
        XCTAssertEqual(fixture.features.taskCatalog.selectedTaskID, secondTask.id)
        XCTAssertEqual(fixture.features.tracking.active, activeWorklog)
        XCTAssertEqual(fixture.features.worklogHistory.worklogs, [selectedLog])
        XCTAssertEqual(fixture.features.worklogHistory.nextCursor, "older")
        XCTAssertEqual(fixture.features.taskCatalog.tasks, [renamed, secondTask])
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .rename = $0 { return true }; return false
            }.count, 1)
    }

    @MainActor
    func testArchivedTaskCanBeRenamedWithoutMovingTabs() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [archivedTask], active: nil)
        try await fixture.start(resources)
        XCTAssertFalse(fixture.taskRename.canOpen)
        fixture.taskRename.open(taskID: archivedTask.id)
        XCTAssertFalse(fixture.taskRename.isPresented)
        fixture.features.taskCatalog.changeTab(.archived)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertTrue(fixture.taskRename.canOpen)
        fixture.taskRename.open(taskID: archivedTask.id)
        fixture.taskRename.setName("Renamed archive")
        fixture.taskRename.submit()
        let rename = try await fixture.client.next()
        let renamed = TaskItem(id: archivedTask.id, name: "Renamed archive", archived: true, latestStart: nil)
        try await fixture.update("the renamed task") {
            rename.succeed(TaskListResources(tasks: [renamed], active: nil))
        }
        XCTAssertFalse(fixture.taskRename.isPresented)
        XCTAssertEqual(fixture.features.taskCatalog.tab, .archived)
        XCTAssertEqual(fixture.features.taskCatalog.selectedTaskID, archivedTask.id)
        XCTAssertEqual(fixture.features.taskCatalog.selectedTask, renamed)
        XCTAssertEqual(fixture.client.operations.count, 4)
    }

    @MainActor
    func testRenameIsSentWhileAPollRunsAndPollingDoesNotOverwriteDraft() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources)
        fixture.taskRename.open(taskID: firstTask.id)
        fixture.taskRename.setName("Draft name")
        fixture.sync.refresh()
        let poll = try await fixture.client.next()
        XCTAssertTrue(fixture.taskRename.canOpen)
        fixture.taskRename.submit()
        fixture.clock.now.addTimeInterval(60)
        fixture.taskRename.submit()
        let rename = try await fixture.client.next()
        XCTAssertEqual(
            rename.operation,
            .rename(
                task: firstTask.id, expectedName: firstTask.name, name: "Draft name", at: "2025-01-01T00:00:00.000Z"))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 2)
        let backgroundName = TaskItem(id: firstTask.id, name: "Background name", archived: false, latestStart: nil)
        try await fixture.update("the background task name") {
            poll.succeed(TaskListResources(tasks: [backgroundName], active: nil))
        }
        XCTAssertEqual(fixture.features.taskCatalog.tasks, [backgroundName])
        XCTAssertEqual(fixture.taskRename.originalName, firstTask.name)
        XCTAssertEqual(fixture.taskRename.name, "Draft name")
        XCTAssertTrue(fixture.taskRename.isSubmitting)
        let renamed = TaskItem(id: firstTask.id, name: "Draft name", archived: false, latestStart: nil)
        try await fixture.update("the renamed task") {
            rename.succeed(TaskListResources(tasks: [renamed], active: nil))
        }
        XCTAssertFalse(fixture.taskRename.isPresented)
        XCTAssertEqual(fixture.features.taskCatalog.tasks, [renamed])
    }

    @MainActor
    func testRejectionShowsServerMessageKeepsDraftReloadsTaskListAndResubmitUsesNewTimestamp() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.taskRename.open(taskID: firstTask.id)
        fixture.taskRename.setName("Invalid\u{0001}name")
        fixture.taskRename.submit()
        let rename = try await fixture.client.next()
        rename.fail(BridgeFailure(message: "Invalid name", kind: "validation"))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.features.connection.settings))
        XCTAssertFalse(fixture.taskRename.isSubmitting)
        XCTAssertEqual(fixture.taskRename.error, "Invalid name")
        let changed = TaskItem(id: firstTask.id, name: "Changed elsewhere", archived: false, latestStart: nil)
        try await fixture.update("the reloaded task list") {
            reload.succeed(TaskListResources(tasks: [changed], active: nil))
        }
        XCTAssertEqual(fixture.features.taskCatalog.tasks, [changed])
        XCTAssertTrue(fixture.taskRename.isPresented)
        XCTAssertEqual(fixture.taskRename.taskID, firstTask.id)
        XCTAssertEqual(fixture.taskRename.originalName, firstTask.name)
        XCTAssertEqual(fixture.taskRename.name, "Invalid\u{0001}name")
        XCTAssertEqual(fixture.taskRename.error, "Invalid name")
        XCTAssertFalse(fixture.taskRename.isSubmitting)
        XCTAssertTrue(fixture.taskRename.canSubmit)
        XCTAssertFalse(fixture.features.connection.stale)
        fixture.taskRename.setName("Corrected name")
        XCTAssertNil(fixture.taskRename.error)
        XCTAssertEqual(fixture.taskRename.name, "Corrected name")
        fixture.clock.now.addTimeInterval(90)
        fixture.taskRename.submit()
        let retry = try await fixture.client.next()
        XCTAssertEqual(
            retry.operation,
            .rename(
                task: firstTask.id, expectedName: firstTask.name, name: "Corrected name",
                at: "2025-01-01T00:01:30.000Z"))
        retry.fail(BridgeFailure(message: "The task name changed on another client.", kind: "conflict"))
        let retryReload = try await fixture.client.next()
        try await fixture.update("the second reload") {
            retryReload.succeed(TaskListResources(tasks: [changed], active: nil))
        }
        XCTAssertEqual(fixture.taskRename.error, "The task name changed on another client.")
        XCTAssertEqual(fixture.taskRename.name, "Corrected name")
        XCTAssertTrue(fixture.taskRename.canSubmit)
    }

    @MainActor
    func testFailedReloadAfterRejectedRenameKeepsDraftAndDisablesTrackingUntilRecovery() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(resources)
        fixture.taskRename.open(taskID: firstTask.id)
        fixture.taskRename.setName("Desired name")
        fixture.taskRename.submit()
        let rename = try await fixture.client.next()
        rename.fail(BridgeFailure(message: "Name rejected", kind: "validation"))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.features.connection.settings))
        try await fixture.update("the failed reload") {
            reload.fail(BridgeFailure(message: "Cannot confirm server state", kind: "unavailable"))
        } until: {
            fixture.features.connection.stale
        }
        XCTAssertEqual(fixture.taskRename.error, "Name rejected")
        XCTAssertEqual(fixture.features.connection.message, "Cannot confirm server state")
        XCTAssertEqual(fixture.taskRename.name, "Desired name")
        XCTAssertFalse(fixture.taskRename.isSubmitting)
        XCTAssertTrue(fixture.taskRename.canSubmit)
        XCTAssertFalse(fixture.trackingAvailability.canStop)
        XCTAssertEqual(fixture.features.tracking.active, activeWorklog)
    }

    @MainActor
    func testTrackingAndOtherEditorsStayAvailableWhileRenameRuns() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(resources)
        fixture.taskRename.open(taskID: firstTask.id)
        fixture.taskRename.setName("Renamed task")
        fixture.taskRename.submit()
        let rename = try await fixture.client.next()
        XCTAssertTrue(fixture.taskRename.isSubmitting)
        XCTAssertTrue(fixture.taskCreation.canOpen)
        XCTAssertTrue(fixture.worklogCorrection.canOpen)
        XCTAssertTrue(fixture.worklogMove.canOpen)
        XCTAssertTrue(fixture.worklogDeletion.canOpen)
        XCTAssertTrue(fixture.trackingAvailability.canStop)
        XCTAssertTrue(fixture.trackingAvailability.canStart(taskID: secondTask.id))
        fixture.trackingActions.stopTracking(worklogID: activeWorklog.id)
        let stop = try await fixture.client.next()
        XCTAssertEqual(stop.operation, .stop(worklog: activeWorklog.id, at: "2025-01-01T00:00:00.000Z"))
        stop.succeed(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        let stoppedHistory = try await fixture.client.next()
        XCTAssertEqual(stoppedHistory.operation, .history(task: firstTask.id, cursor: nil))
        try await fixture.update("the stopped timer history") { stoppedHistory.succeed(emptyPage) }
        XCTAssertNil(fixture.features.tracking.active)
        XCTAssertTrue(fixture.taskRename.isSubmitting)
        let renamed = TaskItem(id: firstTask.id, name: "Renamed task", archived: false, latestStart: nil)
        try await fixture.update("the renamed task") {
            rename.succeed(TaskListResources(tasks: [renamed, secondTask], active: nil))
        }
        XCTAssertFalse(fixture.taskRename.isPresented)
        XCTAssertEqual(fixture.features.taskCatalog.tasks, [renamed, secondTask])
        XCTAssertNil(fixture.features.tracking.active)
    }

    @MainActor
    func testNulRejectionShowsInlineErrorAndDraftEditsDoNotRedrawLists() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        let content = ObservationProbe { ViewReads.content(fixture.features) }
        fixture.taskRename.open(taskID: firstTask.id)
        fixture.taskRename.setName("Valid\u{0000}suffix")
        fixture.taskRename.submit()
        XCTAssertEqual(fixture.taskRename.error, "Task names must not contain control characters.")
        XCTAssertFalse(content.consume())
        XCTAssertFalse(fixture.taskRename.isSubmitting)
        XCTAssertEqual(fixture.client.operations.count, 2)
        fixture.taskRename.setName("   ")
        XCTAssertFalse(fixture.taskRename.canSubmit)
        fixture.taskRename.submit()
        XCTAssertEqual(fixture.client.operations.count, 2)
    }

    @MainActor
    func testEditorsCanBeOpenTogetherAndRenameDraftSurvivesConnectionSwitch() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.taskCreation.open()
        XCTAssertTrue(fixture.taskRename.canOpen)
        fixture.taskRename.open(taskID: firstTask.id)
        fixture.taskRename.setName("Old server draft")
        XCTAssertTrue(fixture.taskRename.isPresented)
        XCTAssertTrue(fixture.taskCreation.isPresented)
        XCTAssertTrue(fixture.taskCreation.canOpen)
        let connect = Task { await fixture.sync.connect(serverSettings) }
        let connection = try await fixture.client.next()
        connection.succeed(emptyTaskListResources)
        let connected = try await fixture.taskValue(connect)
        XCTAssertTrue(connected)
        XCTAssertTrue(fixture.taskRename.isPresented)
        XCTAssertEqual(fixture.taskRename.taskID, firstTask.id)
        XCTAssertEqual(fixture.taskRename.name, "Old server draft")
        XCTAssertFalse(fixture.taskRename.canOpen)
    }

    @MainActor
    func testExplicitClickedIdentityIsCapturedAndInvalidIdentityIsIgnored() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        fixture.taskRename.open(taskID: "missing-task")
        XCTAssertFalse(fixture.taskRename.isPresented)
        fixture.taskRename.open(taskID: secondTask.id)
        XCTAssertEqual(fixture.taskRename.taskID, secondTask.id)
        XCTAssertEqual(fixture.taskRename.name, secondTask.name)
        fixture.taskRename.open(taskID: firstTask.id)
        XCTAssertEqual(fixture.taskRename.taskID, secondTask.id)
    }

    @MainActor
    func testRenameReloadKeepsNewerTaskMetadataFromTheServer() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources)
        fixture.taskRename.open(taskID: firstTask.id)
        fixture.taskRename.setName("Desired name")
        fixture.taskRename.submit()
        let rename = try await fixture.client.next()
        let newer = TaskItem(id: firstTask.id, name: "Another client renamed later", archived: false, latestStart: nil)
        try await fixture.update("the reloaded task list") {
            rename.succeed(TaskListResources(tasks: [newer], active: nil), applyingTaskCommand: true)
        }
        XCTAssertEqual(fixture.features.taskCatalog.tasks, [newer])
        XCTAssertFalse(fixture.taskRename.isPresented)
        XCTAssertNil(fixture.taskRename.error)
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .rename = $0 { return true }; return false
            }.count, 1)
    }

    @MainActor
    func testFailedSourceChangeKeepsDraftForOriginalConnection() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.taskRename.open(taskID: firstTask.id)
        fixture.taskRename.setName("Current server draft")
        let connect = Task { await fixture.sync.connect(serverSettings) }
        let connection = try await fixture.client.next()
        connection.fail(BridgeFailure(message: "Connection failed", kind: "unavailable"))
        let connected = try await fixture.taskValue(connect)
        XCTAssertFalse(connected)
        XCTAssertEqual(fixture.features.connection.settings, .local)
        XCTAssertEqual(fixture.taskRename.taskID, firstTask.id)
        XCTAssertEqual(fixture.taskRename.name, "Current server draft")
        XCTAssertTrue(fixture.taskRename.isPresented)
    }

    @MainActor
    func testAutomaticPauseRunsWhileRenameIsInFlightAndResumeUsesSameIdentity() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(resources)
        fixture.sync.refresh()
        let poll = try await fixture.client.next()
        fixture.taskRename.open(taskID: firstTask.id)
        fixture.taskRename.setName("Renamed while locked")
        fixture.taskRename.submit()
        let rename = try await fixture.client.next()
        XCTAssertEqual(
            rename.operation,
            .rename(
                task: firstTask.id, expectedName: firstTask.name, name: "Renamed while locked",
                at: "2025-01-01T00:00:00.000Z"))
        fixture.sync.setScreenLocked(true, at: fixture.clock.now)
        // Wait for the poll to apply, or its reply can land after the pause and restore the running worklog.
        try await fixture.update("the poll") { poll.succeed(resources) }
        let pause = try await fixture.client.next()
        XCTAssertEqual(pause.operation, .pause(worklog: activeWorklog.id, at: "2025-01-01T00:00:00.000Z"))
        let paused = TaskListResources(tasks: [firstTask], active: nil)
        pause.paused(paused)
        let pausedHistory = try await fixture.client.next()
        XCTAssertEqual(pausedHistory.operation, .history(task: firstTask.id, cursor: nil))
        try await fixture.update("the paused history") { pausedHistory.succeed(emptyPage) }
        XCTAssertNil(fixture.features.tracking.active)
        let renamed = TaskItem(id: firstTask.id, name: "Renamed while locked", archived: false, latestStart: nil)
        let renamedPaused = TaskListResources(tasks: [renamed], active: nil)
        try await fixture.update("the renamed task") {
            rename.succeed(renamedPaused)
        }
        XCTAssertFalse(fixture.taskRename.isPresented)
        fixture.clock.now.addTimeInterval(30)
        fixture.sync.setScreenLocked(false, at: fixture.clock.now)
        let resume = try await fixture.client.next()
        XCTAssertEqual(resume.operation, .resume(task: firstTask.id, at: "2025-01-01T00:00:30.000Z"))
        let resumed = WorklogItem(id: "resumed", taskId: firstTask.id, start: "2025-01-01T00:00:30.000Z", end: nil)
        resume.succeed(TaskListResources(tasks: [renamed], active: resumed))
        let resumedHistory = try await fixture.client.next()
        resumedHistory.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.features.menuSources.read().runningTaskName, renamed.name)
    }

    @MainActor
    func testCreationSheetDoesNotChangeRenameAvailabilityOrDraft() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        let rename = ObservationProbe { ViewReads.taskRename(fixture.features) }
        let content = ObservationProbe { ViewReads.content(fixture.features) }
        fixture.taskCreation.open()
        fixture.taskCreation.cancel()
        XCTAssertFalse(rename.consume())
        XCTAssertTrue(fixture.taskRename.canOpen)
        XCTAssertFalse(fixture.taskRename.isPresented)
        XCTAssertFalse(content.consume())
    }
}
