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
    func testSubmissionSendsRenameFirstThenReloadsTaskListAndClosesSheet() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Renamed task")
        fixture.session.submitTaskRename()
        XCTAssertTrue(fixture.session.taskRename.isSubmitting)
        XCTAssertFalse(fixture.session.taskRename.canSubmit)
        let rename = try await fixture.client.next()
        XCTAssertEqual(
            rename.operation,
            .rename(
                task: firstTask.id, expectedName: firstTask.name, name: "Renamed task",
                at: "2025-01-01T00:00:00.000Z"))
        fixture.session.submitTaskRename()
        fixture.session.cancelTaskRename()
        fixture.session.setTaskRenameName("Replacement")
        XCTAssertTrue(fixture.session.taskRename.isPresented)
        XCTAssertEqual(fixture.session.taskRename.name, "Renamed task")
        let renamed = TaskItem(id: firstTask.id, name: "Renamed task", archived: false, latestStart: nil)
        rename.complete(.success(.task(TaskCommandResult(task: renamed))))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connectionSettings))
        XCTAssertFalse(fixture.session.taskRename.isPresented)
        XCTAssertFalse(fixture.session.taskRename.isSubmitting)
        try await fixture.update("the reloaded task list") {
            reload.succeed(TaskListResources(tasks: [renamed], active: nil))
        }
        XCTAssertEqual(fixture.session.tasks, [renamed])
        XCTAssertNil(fixture.session.taskRename.taskID)
        XCTAssertNil(fixture.session.taskRename.error)
        XCTAssertEqual(
            fixture.client.operations,
            [
                .open(.local), .history(task: firstTask.id, cursor: nil),
                .rename(
                    task: firstTask.id, expectedName: firstTask.name, name: "Renamed task",
                    at: "2025-01-01T00:00:00.000Z"),
                .refresh(fixture.session.connectionSettings),
            ])
    }

    @MainActor
    func testRenameKeepsCapturedTargetWhenSelectionChangesAndPreservesTrackingAndHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(resources)
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
        let rename = try await fixture.client.next()
        XCTAssertEqual(
            rename.operation,
            .rename(
                task: firstTask.id, expectedName: firstTask.name, name: "Renamed task", at: "2025-01-01T00:00:00.000Z"))
        let renamed = TaskItem(id: firstTask.id, name: "Renamed task", archived: false, latestStart: nil)
        try await fixture.update("the renamed task") {
            rename.succeed(TaskListResources(tasks: [renamed, secondTask], active: activeWorklog))
        }
        XCTAssertFalse(fixture.session.taskRename.isPresented)
        XCTAssertEqual(fixture.session.selectedTaskID, secondTask.id)
        XCTAssertEqual(fixture.session.active, activeWorklog)
        XCTAssertEqual(fixture.session.worklogs, [selectedLog])
        XCTAssertEqual(fixture.session.nextCursor, "older")
        XCTAssertEqual(fixture.session.tasks, [renamed, secondTask])
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
        XCTAssertFalse(fixture.session.canOpenTaskRename)
        fixture.session.changeTab(.archived)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertTrue(fixture.session.canOpenTaskRename)
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Renamed archive")
        fixture.session.submitTaskRename()
        let rename = try await fixture.client.next()
        let renamed = TaskItem(id: archivedTask.id, name: "Renamed archive", archived: true, latestStart: nil)
        try await fixture.update("the renamed task") {
            rename.succeed(TaskListResources(tasks: [renamed], active: nil))
        }
        XCTAssertFalse(fixture.session.taskRename.isPresented)
        XCTAssertEqual(fixture.session.tab, .archived)
        XCTAssertEqual(fixture.session.selectedTaskID, archivedTask.id)
        XCTAssertEqual(fixture.session.selectedTask, renamed)
        XCTAssertEqual(fixture.client.operations.count, 4)
    }

    @MainActor
    func testRenameIsSentWhileAPollRunsAndPollingDoesNotOverwriteDraft() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources)
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Draft name")
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        XCTAssertTrue(fixture.session.canOpenTaskRename)
        fixture.session.submitTaskRename()
        fixture.clock.now.addTimeInterval(60)
        fixture.session.submitTaskRename()
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
        XCTAssertEqual(fixture.session.tasks, [backgroundName])
        XCTAssertEqual(fixture.session.taskRename.originalName, firstTask.name)
        XCTAssertEqual(fixture.session.taskRename.name, "Draft name")
        XCTAssertTrue(fixture.session.taskRename.isSubmitting)
        let renamed = TaskItem(id: firstTask.id, name: "Draft name", archived: false, latestStart: nil)
        try await fixture.update("the renamed task") {
            rename.succeed(TaskListResources(tasks: [renamed], active: nil))
        }
        XCTAssertFalse(fixture.session.taskRename.isPresented)
        XCTAssertEqual(fixture.session.tasks, [renamed])
    }

    @MainActor
    func testRejectionShowsServerMessageKeepsDraftReloadsTaskListAndResubmitUsesNewTimestamp() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Invalid\u{0001}name")
        fixture.session.submitTaskRename()
        let rename = try await fixture.client.next()
        rename.fail(BridgeFailure(message: "Invalid name", kind: "validation"))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connectionSettings))
        XCTAssertFalse(fixture.session.taskRename.isSubmitting)
        XCTAssertEqual(fixture.session.taskRename.error, "Invalid name")
        let changed = TaskItem(id: firstTask.id, name: "Changed elsewhere", archived: false, latestStart: nil)
        try await fixture.update("the reloaded task list") {
            reload.succeed(TaskListResources(tasks: [changed], active: nil))
        }
        XCTAssertEqual(fixture.session.tasks, [changed])
        XCTAssertTrue(fixture.session.taskRename.isPresented)
        XCTAssertEqual(fixture.session.taskRename.taskID, firstTask.id)
        XCTAssertEqual(fixture.session.taskRename.originalName, firstTask.name)
        XCTAssertEqual(fixture.session.taskRename.name, "Invalid\u{0001}name")
        XCTAssertEqual(fixture.session.taskRename.error, "Invalid name")
        XCTAssertFalse(fixture.session.taskRename.isSubmitting)
        XCTAssertTrue(fixture.session.taskRename.canSubmit)
        XCTAssertFalse(fixture.session.isStale)
        fixture.session.setTaskRenameName("Corrected name")
        XCTAssertNil(fixture.session.taskRename.error)
        XCTAssertEqual(fixture.session.taskRename.name, "Corrected name")
        fixture.clock.now.addTimeInterval(90)
        fixture.session.submitTaskRename()
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
        XCTAssertEqual(fixture.session.taskRename.error, "The task name changed on another client.")
        XCTAssertEqual(fixture.session.taskRename.name, "Corrected name")
        XCTAssertTrue(fixture.session.taskRename.canSubmit)
    }

    @MainActor
    func testFailedReloadAfterRejectedRenameKeepsDraftAndDisablesTrackingUntilRecovery() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(resources)
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Desired name")
        fixture.session.submitTaskRename()
        let rename = try await fixture.client.next()
        rename.fail(BridgeFailure(message: "Name rejected", kind: "validation"))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connectionSettings))
        try await fixture.update("the failed reload") {
            reload.fail(BridgeFailure(message: "Cannot confirm server state", kind: "unavailable"))
        } until: {
            fixture.session.isStale
        }
        XCTAssertEqual(fixture.session.taskRename.error, "Name rejected")
        XCTAssertEqual(fixture.session.connectionMessage, "Cannot confirm server state")
        XCTAssertEqual(fixture.session.taskRename.name, "Desired name")
        XCTAssertFalse(fixture.session.taskRename.isSubmitting)
        XCTAssertTrue(fixture.session.taskRename.canSubmit)
        XCTAssertFalse(fixture.session.canStopTracking)
        XCTAssertEqual(fixture.session.active, activeWorklog)
    }

    @MainActor
    func testTrackingAndOtherEditorsStayAvailableWhileRenameRuns() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(resources)
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Renamed task")
        fixture.session.submitTaskRename()
        let rename = try await fixture.client.next()
        XCTAssertTrue(fixture.session.taskRename.isSubmitting)
        XCTAssertTrue(fixture.session.canOpenTaskCreation)
        XCTAssertTrue(fixture.session.canOpenWorklogCorrection)
        XCTAssertTrue(fixture.session.canOpenWorklogMove)
        XCTAssertTrue(fixture.session.canOpenWorklogDeletion)
        XCTAssertTrue(fixture.session.canStopTracking)
        XCTAssertTrue(fixture.session.canStartTracking(taskID: secondTask.id))
        fixture.session.stopTracking(worklogID: activeWorklog.id)
        let stop = try await fixture.client.next()
        XCTAssertEqual(stop.operation, .stop(worklog: activeWorklog.id, at: "2025-01-01T00:00:00.000Z"))
        stop.succeed(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        let stoppedHistory = try await fixture.client.next()
        XCTAssertEqual(stoppedHistory.operation, .history(task: firstTask.id, cursor: nil))
        try await fixture.update("the stopped timer history") { stoppedHistory.succeed(emptyPage) }
        XCTAssertNil(fixture.session.active)
        XCTAssertTrue(fixture.session.taskRename.isSubmitting)
        let renamed = TaskItem(id: firstTask.id, name: "Renamed task", archived: false, latestStart: nil)
        try await fixture.update("the renamed task") {
            rename.succeed(TaskListResources(tasks: [renamed, secondTask], active: nil))
        }
        XCTAssertFalse(fixture.session.taskRename.isPresented)
        XCTAssertEqual(fixture.session.tasks, [renamed, secondTask])
        XCTAssertNil(fixture.session.active)
    }

    @MainActor
    func testNulRejectionPublishesInlineErrorAndDraftEditsDoNotRedrawLists() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        let harness = PresentationHarness(session: fixture.session)
        let displayedErrors = harness.recorder(PresentationReads.taskRename) { fixture.session.taskRename.error }
        let contentUpdates = harness.counter(PresentationReads.content)
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Valid\u{0000}suffix")
        fixture.session.submitTaskRename()
        XCTAssertEqual(displayedErrors.values, [nil, nil, "Task names must not contain control characters."])
        XCTAssertEqual(contentUpdates.count, 0)
        XCTAssertFalse(fixture.session.taskRename.isSubmitting)
        XCTAssertEqual(fixture.client.operations.count, 2)
        fixture.session.setTaskRenameName("   ")
        XCTAssertFalse(fixture.session.taskRename.canSubmit)
        fixture.session.submitTaskRename()
        XCTAssertEqual(fixture.client.operations.count, 2)
    }

    @MainActor
    func testEditorsCanBeOpenTogetherAndRenameDraftSurvivesConnectionSwitch() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.session.openTaskCreation()
        XCTAssertTrue(fixture.session.canOpenTaskRename)
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Old server draft")
        XCTAssertTrue(fixture.session.taskRename.isPresented)
        XCTAssertTrue(fixture.session.taskCreation.isPresented)
        XCTAssertTrue(fixture.session.canOpenTaskCreation)
        let connect = Task { await fixture.session.connect(serverSettings) }
        let connection = try await fixture.client.next()
        connection.succeed(emptyTaskListResources)
        let connected = try await fixture.taskValue(connect)
        XCTAssertTrue(connected)
        XCTAssertTrue(fixture.session.taskRename.isPresented)
        XCTAssertEqual(fixture.session.taskRename.taskID, firstTask.id)
        XCTAssertEqual(fixture.session.taskRename.name, "Old server draft")
        XCTAssertFalse(fixture.session.canOpenTaskRename)
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
    func testRenameReloadKeepsNewerTaskMetadataFromTheServer() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources)
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Desired name")
        fixture.session.submitTaskRename()
        let rename = try await fixture.client.next()
        let newer = TaskItem(id: firstTask.id, name: "Another client renamed later", archived: false, latestStart: nil)
        try await fixture.update("the reloaded task list") {
            rename.succeed(TaskListResources(tasks: [newer], active: nil), applyingTaskCommand: true)
        }
        XCTAssertEqual(fixture.session.tasks, [newer])
        XCTAssertFalse(fixture.session.taskRename.isPresented)
        XCTAssertNil(fixture.session.taskRename.error)
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
    func testAutomaticPauseRunsWhileRenameIsInFlightAndResumeUsesSameIdentity() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(resources)
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Renamed while locked")
        fixture.session.submitTaskRename()
        let rename = try await fixture.client.next()
        XCTAssertEqual(
            rename.operation,
            .rename(
                task: firstTask.id, expectedName: firstTask.name, name: "Renamed while locked",
                at: "2025-01-01T00:00:00.000Z"))
        fixture.session.screenLocked(at: fixture.clock.now)
        poll.succeed(resources)
        let pause = try await fixture.client.next()
        XCTAssertEqual(pause.operation, .pause(worklog: activeWorklog.id, at: "2025-01-01T00:00:00.000Z"))
        let paused = TaskListResources(tasks: [firstTask], active: nil)
        pause.paused(paused)
        let pausedHistory = try await fixture.client.next()
        XCTAssertEqual(pausedHistory.operation, .history(task: firstTask.id, cursor: nil))
        try await fixture.update("the paused history") { pausedHistory.succeed(emptyPage) }
        XCTAssertNil(fixture.session.active)
        let renamed = TaskItem(id: firstTask.id, name: "Renamed while locked", archived: false, latestStart: nil)
        let renamedPaused = TaskListResources(tasks: [renamed], active: nil)
        try await fixture.update("the renamed task") {
            rename.succeed(renamedPaused)
        }
        XCTAssertFalse(fixture.session.taskRename.isPresented)
        fixture.clock.now.addTimeInterval(30)
        fixture.session.screenUnlocked(at: fixture.clock.now)
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
    func testCreationSheetDoesNotChangeRenameAvailabilityOrDraft() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        let originalPresentation = fixture.session.taskRename
        let harness = PresentationHarness(session: fixture.session)
        let enabledStates = harness.recorder(PresentationReads.taskRename) { fixture.session.canOpenTaskRename }
        let contentUpdates = harness.counter(PresentationReads.content)
        fixture.session.openTaskCreation()
        fixture.session.cancelTaskCreation()
        XCTAssertEqual(enabledStates.values, [])
        XCTAssertTrue(fixture.session.canOpenTaskRename)
        XCTAssertEqual(fixture.session.taskRename, originalPresentation)
        XCTAssertEqual(contentUpdates.count, 0)
    }
}
