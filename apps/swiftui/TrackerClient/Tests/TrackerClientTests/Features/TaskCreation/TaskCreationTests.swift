import Foundation
import XCTest
@testable import TrackerClient

final class TaskCreationTests: XCTestCase {
    @MainActor
    func testSubmissionSendsCreationFirstThenReloadsTaskListAndSelectsCreatedTask() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName(secondTask.name)
        fixture.session.submitTaskCreation()
        XCTAssertTrue(fixture.session.taskCreation.isSubmitting)
        XCTAssertFalse(fixture.session.taskCreation.canSubmit)
        let creation = try await fixture.client.next()
        XCTAssertEqual(creation.operation, .create(name: secondTask.name, at: "2025-01-01T00:00:00.000Z"))
        let harness = PresentationHarness(session: fixture.session)
        creation.created(task: secondTask)
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connectionSettings))
        XCTAssertFalse(fixture.session.taskCreation.isPresented)
        XCTAssertFalse(fixture.session.taskCreation.isSubmitting)
        XCTAssertFalse(harness.presentation.taskCreation.isPresented, "The sheet must close before the reload.")
        reload.succeed(TaskListResources(tasks: [secondTask], active: nil))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: secondTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await harness.settled(fixture)
        XCTAssertFalse(fixture.session.taskCreation.isPresented)
        XCTAssertEqual(fixture.session.taskCreation.name, "")
        XCTAssertNil(fixture.session.taskCreation.error)
        XCTAssertEqual(fixture.session.tasks, [secondTask])
        XCTAssertEqual(fixture.session.selectedTaskID, secondTask.id)
        XCTAssertEqual(
            fixture.client.operations,
            [
                .open(.local), .create(name: secondTask.name, at: "2025-01-01T00:00:00.000Z"),
                .refresh(fixture.session.connectionSettings),
                .history(task: secondTask.id, cursor: nil),
            ])
    }

    @MainActor
    func testNewerArchivedObservationSelectsArchivedTabAfterCreation() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName(archivedTask.name)
        fixture.session.submitTaskCreation()
        let command = try await fixture.client.next()
        let created = TaskItem(id: archivedTask.id, name: archivedTask.name, archived: false, latestStart: nil)
        command.created(task: created)
        let refresh = try await fixture.client.next()
        XCTAssertEqual(refresh.operation, .refresh(fixture.session.connectionSettings))
        refresh.succeed(TaskListResources(tasks: [archivedTask], active: nil))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: archivedTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertFalse(fixture.session.taskCreation.isPresented)
        XCTAssertEqual(fixture.session.tab, .archived)
        XCTAssertEqual(fixture.session.selectedTaskID, archivedTask.id)
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .create = $0 { return true }; return false
            }.count, 1)
    }

    @MainActor
    func testCommittedCreationSurvivesFailedResourceRefresh() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName(secondTask.name)
        fixture.session.submitTaskCreation()
        let command = try await fixture.client.next()
        command.created(task: secondTask)
        let refresh = try await fixture.client.next()
        XCTAssertEqual(refresh.operation, .refresh(fixture.session.connectionSettings))
        try await fixture.update("the failed reload") {
            refresh.fail(BridgeFailure(message: "Server unavailable", kind: "unavailable", requiresRefresh: true))
        } until: {
            fixture.session.isStale
        }
        XCTAssertFalse(fixture.session.taskCreation.isPresented)
        XCTAssertNil(fixture.session.taskCreation.error)
        XCTAssertTrue(fixture.session.tasks.isEmpty)
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .create = $0 { return true }; return false
            }.count, 1)
    }

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
        try await fixture.start(TaskListResources(tasks: tasks, active: activeWorklog))
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
        request.created(
            taskID: created.id, resources: TaskListResources(tasks: tasks + [created], active: activeWorklog))
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
    func testCreationIsSentWhileAPollRunsAndCapturesNameAndTime() async throws {
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
        let creation = try await fixture.client.next()
        XCTAssertEqual(creation.operation, .create(name: "  Task draft  ", at: "2025-01-01T00:00:00.000Z"))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 2)
        let created = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.update("the created task") {
            creation.created(taskID: firstTask.id, resources: created)
        } until: {
            fixture.session.selectedTaskID == firstTask.id
        }
        XCTAssertFalse(fixture.session.taskCreation.isPresented)
        poll.succeed(created)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .create = $0 { return true }; return false
            }.count, 1)
    }

    @MainActor
    func testFailureShowsServerMessageKeepsDraftReloadsTaskListAndResubmitUsesNewTimestamp() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("Task\u{0001}")
        fixture.session.submitTaskCreation()
        let creation = try await fixture.client.next()
        creation.fail(BridgeFailure(message: "Invalid task name", kind: "validation"))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connectionSettings))
        XCTAssertFalse(fixture.session.taskCreation.isSubmitting)
        XCTAssertEqual(fixture.session.taskCreation.error, "Invalid task name")
        let reloaded = TaskListResources(tasks: [archivedTask], active: nil)
        try await fixture.update("the reloaded task list") {
            reload.succeed(reloaded)
        }
        XCTAssertEqual(fixture.session.tasks, [archivedTask])
        XCTAssertTrue(fixture.session.taskCreation.isPresented)
        XCTAssertEqual(fixture.session.taskCreation.name, "Task\u{0001}")
        XCTAssertEqual(fixture.session.taskCreation.error, "Invalid task name")
        XCTAssertFalse(fixture.session.taskCreation.isSubmitting)
        XCTAssertTrue(fixture.session.taskCreation.canSubmit)
        XCTAssertFalse(fixture.session.isStale)
        fixture.clock.now.addTimeInterval(30)
        fixture.session.setTaskCreationName("Valid task")
        XCTAssertNil(fixture.session.taskCreation.error)
        fixture.session.submitTaskCreation()
        let retry = try await fixture.client.next()
        XCTAssertEqual(retry.operation, .create(name: "Valid task", at: "2025-01-01T00:00:30.000Z"))
        retry.fail(BridgeFailure(message: "Rejected", kind: "validation"))
        let retryReload = try await fixture.client.next()
        XCTAssertEqual(retryReload.operation, .refresh(fixture.session.connectionSettings))
        try await fixture.update("the second reload") {
            retryReload.succeed(reloaded)
        }
        XCTAssertEqual(fixture.session.taskCreation.error, "Rejected")
        XCTAssertEqual(fixture.session.taskCreation.name, "Valid task")
    }

    @MainActor
    func testFailedReloadAfterFailedCreationKeepsCreationErrorAndMarksConnectionStale() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("New task")
        fixture.session.submitTaskCreation()
        let creation = try await fixture.client.next()
        creation.fail(BridgeFailure(message: "Response lost", kind: "unavailable", uncertain: true))
        let reload = try await fixture.client.next()
        try await fixture.update("the failed reload") {
            reload.fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        } until: {
            fixture.session.isStale
        }
        XCTAssertEqual(
            fixture.session.taskCreation.error,
            "Could not confirm the change. Check the reloaded data before trying again. Response lost")
        XCTAssertEqual(fixture.session.taskCreation.name, "New task")
        XCTAssertFalse(fixture.session.taskCreation.isSubmitting)
        XCTAssertTrue(fixture.session.taskCreation.canSubmit)
        XCTAssertEqual(fixture.client.operations.count, 3)
    }

    @MainActor
    func testCreationRunsWhileARenameRequestIsInFlight() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Renamed task")
        fixture.session.submitTaskRename()
        let rename = try await fixture.client.next()
        XCTAssertEqual(
            rename.operation,
            .rename(
                task: firstTask.id, expectedName: firstTask.name, name: "Renamed task",
                at: "2025-01-01T00:00:00.000Z"))
        XCTAssertTrue(fixture.session.taskRename.isSubmitting)
        XCTAssertTrue(fixture.session.canOpenTaskCreation)
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName(secondTask.name)
        XCTAssertTrue(fixture.session.taskCreation.canSubmit)
        fixture.session.submitTaskCreation()
        let creation = try await fixture.client.next()
        XCTAssertEqual(creation.operation, .create(name: secondTask.name, at: "2025-01-01T00:00:00.000Z"))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 2)
        creation.created(task: secondTask)
        let creationReload = try await fixture.client.next()
        XCTAssertEqual(creationReload.operation, .refresh(fixture.session.connectionSettings))
        XCTAssertFalse(fixture.session.taskCreation.isPresented)
        XCTAssertTrue(fixture.session.taskRename.isSubmitting)
        let renamed = TaskItem(id: firstTask.id, name: "Renamed task", archived: false, latestStart: nil)
        try await fixture.update("the rename result") {
            rename.succeed(TaskListResources(tasks: [renamed, secondTask], active: nil))
        } until: {
            !fixture.session.taskRename.isPresented
        }
        creationReload.succeed(TaskListResources(tasks: [renamed, secondTask], active: nil))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: secondTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.tasks, [renamed, secondTask])
        XCTAssertEqual(fixture.session.selectedTaskID, secondTask.id)
        XCTAssertNil(fixture.session.taskCreation.error)
        XCTAssertNil(fixture.session.taskRename.error)
    }

    @MainActor
    func testTrackingStartAndOtherEditorsStayAvailableWhileCreationRuns() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("New task")
        fixture.session.submitTaskCreation()
        let creation = try await fixture.client.next()
        XCTAssertTrue(fixture.session.taskCreation.isSubmitting)
        XCTAssertTrue(fixture.session.canOpenTaskRename)
        XCTAssertTrue(fixture.session.canOpenWorklogCorrection)
        XCTAssertTrue(fixture.session.canOpenWorklogMove)
        XCTAssertTrue(fixture.session.canOpenWorklogDeletion)
        XCTAssertTrue(fixture.session.canStartTracking(taskID: secondTask.id))
        fixture.session.startTracking(taskID: secondTask.id)
        let start = try await fixture.client.next()
        XCTAssertEqual(start.operation, .start(task: secondTask.id, expected: nil, at: "2025-01-01T00:00:00.000Z"))
        XCTAssertTrue(fixture.session.taskCreation.isSubmitting)
        let worklog = WorklogItem(id: "worklog-new", taskId: secondTask.id, start: "2025-01-01T00:00:00.000Z", end: nil)
        let tracked = TaskListResources(tasks: [firstTask, secondTask], active: worklog)
        try await fixture.update("the started timer") { start.succeed(tracked) }
        XCTAssertEqual(fixture.session.active, worklog)
        XCTAssertTrue(fixture.session.taskCreation.isSubmitting)
        let created = TaskItem(id: "created-id", name: "New task", archived: false, latestStart: nil)
        creation.created(
            taskID: created.id, resources: TaskListResources(tasks: [firstTask, secondTask, created], active: worklog))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: created.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertFalse(fixture.session.taskCreation.isPresented)
        XCTAssertEqual(fixture.session.selectedTaskID, created.id)
        XCTAssertEqual(fixture.session.active, worklog)
    }

    @MainActor
    func testDraftChangesOnlyNotifyCreationObserver() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        let harness = PresentationHarness(session: fixture.session)
        let creationUpdates = harness.counter(PresentationReads.taskCreation)
        let contentUpdates = harness.counter(PresentationReads.content)
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("Draft")
        fixture.session.setTaskCreationName("Draft")
        harness.update()
        XCTAssertEqual(creationUpdates.count, 2)
        XCTAssertEqual(contentUpdates.count, 0)
        fixture.session.cancelTaskCreation()
        XCTAssertEqual(creationUpdates.count, 3)
        XCTAssertEqual(contentUpdates.count, 0)
    }

    @MainActor
    func testSuccessRefreshesDailyReportThenLoadsCreatedTaskHistory() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(
            resources, rows: [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 60_000_000)])
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName(secondTask.name)
        fixture.session.submitTaskCreation()
        let creation = try await fixture.client.next()
        let createdResources = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
        creation.created(taskID: secondTask.id, resources: createdResources)
        let report = try await fixture.client.next()
        guard case .report = report.operation else {
            return XCTFail("Creation must reload daily totals before loading the created task history.")
        }
        report.succeed(
            TaskListTotalsRefresh(
                taskList: createdResources,
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
    func testClosedDraftCannotBeChangedAndCreationInFlightDoesNotBlockConnectionChange() async throws {
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
        let connecting = Task { await fixture.session.connect(serverSettings) }
        let connection = try await fixture.client.next()
        XCTAssertEqual(connection.operation, .connect(serverSettings))
        connection.succeed(emptyTaskListResources)
        let connected = try await fixture.taskValue(connecting)
        XCTAssertTrue(connected)
        XCTAssertEqual(fixture.session.connectionSettings, serverSettings)
        XCTAssertTrue(fixture.session.taskCreation.isSubmitting)
        creation.fail(BridgeFailure(message: "Rejected", kind: "validation"))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connectionSettings))
        try await fixture.update("the reloaded task list") {
            reload.succeed(emptyTaskListResources)
        }
        XCTAssertEqual(fixture.session.taskCreation.error, "Rejected")
        XCTAssertEqual(fixture.session.taskCreation.name, "Pending task")
    }

    @MainActor
    func testEmbeddedNullIsRejectedBeforeCStringBoundary() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("Valid\u{0000}suffix")
        let harness = PresentationHarness(session: fixture.session)
        let displayedErrors = harness.recorder(PresentationReads.taskCreation) { fixture.session.taskCreation.error }
        fixture.session.submitTaskCreation()
        XCTAssertEqual(fixture.session.taskCreation.error, "Task names must not contain control characters.")
        XCTAssertEqual(fixture.session.taskCreation.name, "Valid\u{0000}suffix")
        XCTAssertFalse(fixture.session.taskCreation.isSubmitting)
        XCTAssertEqual(fixture.client.operations.count, 1)
        XCTAssertEqual(displayedErrors.values, ["Task names must not contain control characters."])
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
        connection.succeed(emptyTaskListResources)
        let connected = try await fixture.taskValue(connect)
        XCTAssertTrue(connected)
        XCTAssertEqual(fixture.session.connectionSettings, serverSettings)
        XCTAssertTrue(fixture.session.taskCreation.isPresented)
        XCTAssertEqual(fixture.session.taskCreation.name, "Draft task")
        fixture.session.submitTaskCreation()
        let creation = try await fixture.client.next()
        XCTAssertEqual(creation.operation, .create(name: "Draft task", at: "2025-01-01T00:00:00.000Z"))
        creation.fail(BridgeFailure(message: "Invalid", kind: "validation"))
        let reload = try await fixture.client.next()
        try await fixture.update("the reloaded task list") {
            reload.succeed(emptyTaskListResources)
        }
    }

    @MainActor
    func testCreatedTaskArchivedByAnotherClientIsSelectedInArchivedTab() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("Task archived remotely")
        fixture.session.submitTaskCreation()
        let creation = try await fixture.client.next()
        creation.created(taskID: archivedTask.id, resources: TaskListResources(tasks: [archivedTask], active: nil))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: archivedTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertFalse(fixture.session.taskCreation.isPresented)
        XCTAssertEqual(fixture.session.tab, .archived)
        XCTAssertEqual(fixture.session.selectedTaskID, archivedTask.id)
        XCTAssertEqual(fixture.client.operations.count, 4)
    }

    @MainActor
    func testResubmissionAfterFailureClosesSheetAndReusesSelectedTaskHistory() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName(firstTask.name)
        fixture.session.submitTaskCreation()
        let creation = try await fixture.client.next()
        creation.fail(BridgeFailure(message: "Response lost", uncertain: true))
        let confirmed = TaskListResources(tasks: [firstTask], active: nil)
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connectionSettings))
        reload.succeed(confirmed)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.selectedTaskID, firstTask.id)
        XCTAssertEqual(
            fixture.session.taskCreation.error,
            "Could not confirm the change. Check the reloaded data before trying again. Response lost")
        fixture.clock.now.addTimeInterval(45)
        fixture.session.submitTaskCreation()
        let resubmission = try await fixture.client.next()
        XCTAssertEqual(resubmission.operation, .create(name: firstTask.name, at: "2025-01-01T00:00:45.000Z"))

        let harness = PresentationHarness(session: fixture.session)
        let creationUpdates = harness.recorder(PresentationReads.taskCreation) {
            fixture.session.taskCreation.isPresented
        }
        resubmission.created(taskID: firstTask.id, resources: confirmed)
        let finished = Task { @MainActor in
            while fixture.session.taskCreation.isPresented && !Task.isCancelled { await Task.yield() }
        }
        try await fixture.taskValue(finished)
        XCTAssertEqual(creationUpdates.values.filter { !$0 }.count, 1)
        XCTAssertFalse(fixture.session.taskCreation.isSubmitting)
        XCTAssertFalse(fixture.client.hasPendingRequests)
        XCTAssertTrue(fixture.session.canStartSelectedTask)
        XCTAssertEqual(fixture.session.selectedTaskID, firstTask.id)
        XCTAssertEqual(fixture.session.worklogs, [])
        XCTAssertEqual(
            fixture.client.operations.count, 6,
            "Creating an already selected task must reuse its loaded history.")
    }
}
