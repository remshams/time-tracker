import Foundation
import XCTest
@testable import TrackerClient

final class TaskCreationTests: XCTestCase {
    @MainActor
    func testSubmissionSendsCreationFirstThenReloadsTaskListAndSelectsCreatedTask() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.taskCreation.open()
        fixture.taskCreation.setName(secondTask.name)
        fixture.taskCreation.submit()
        XCTAssertTrue(fixture.taskCreation.isSubmitting)
        XCTAssertFalse(fixture.taskCreation.canSubmit)
        let creation = try await fixture.client.next()
        XCTAssertEqual(creation.operation, .create(name: secondTask.name, at: "2025-01-01T00:00:00.000Z"))
        creation.created(task: secondTask)
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connection.settings))
        XCTAssertFalse(fixture.taskCreation.isPresented, "The sheet must close before the reload.")
        XCTAssertFalse(fixture.taskCreation.isSubmitting)
        reload.succeed(TaskListResources(tasks: [secondTask], active: nil))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: secondTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertFalse(fixture.taskCreation.isPresented)
        XCTAssertEqual(fixture.taskCreation.name, secondTask.name, "The closing sheet keeps its content.")
        XCTAssertFalse(fixture.taskCreation.canSubmit)
        XCTAssertNil(fixture.taskCreation.error)
        XCTAssertEqual(fixture.session.tasks, [secondTask])
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, secondTask.id)
        XCTAssertEqual(
            fixture.client.operations,
            [
                .open(.local), .create(name: secondTask.name, at: "2025-01-01T00:00:00.000Z"),
                .refresh(fixture.session.connection.settings),
                .history(task: secondTask.id, cursor: nil),
            ])
        fixture.taskCreation.open()
        XCTAssertEqual(fixture.taskCreation.name, "")
    }

    @MainActor
    func testNewerArchivedObservationSelectsArchivedTabAfterCreation() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.taskCreation.open()
        fixture.taskCreation.setName(archivedTask.name)
        fixture.taskCreation.submit()
        let command = try await fixture.client.next()
        let created = TaskItem(id: archivedTask.id, name: archivedTask.name, archived: false, latestStart: nil)
        command.created(task: created)
        let refresh = try await fixture.client.next()
        XCTAssertEqual(refresh.operation, .refresh(fixture.session.connection.settings))
        refresh.succeed(TaskListResources(tasks: [archivedTask], active: nil))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: archivedTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertFalse(fixture.taskCreation.isPresented)
        XCTAssertEqual(fixture.session.taskCatalog.tab, .archived)
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, archivedTask.id)
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
        fixture.taskCreation.open()
        fixture.taskCreation.setName(secondTask.name)
        fixture.taskCreation.submit()
        let command = try await fixture.client.next()
        command.created(task: secondTask)
        let refresh = try await fixture.client.next()
        XCTAssertEqual(refresh.operation, .refresh(fixture.session.connection.settings))
        try await fixture.update("the failed reload") {
            refresh.fail(BridgeFailure(message: "Server unavailable", kind: "unavailable", requiresRefresh: true))
        } until: {
            fixture.session.connection.stale
        }
        XCTAssertFalse(fixture.taskCreation.isPresented)
        XCTAssertNil(fixture.taskCreation.error)
        XCTAssertTrue(fixture.session.tasks.isEmpty)
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .create = $0 { return true }; return false
            }.count, 1)
    }

    @MainActor
    func testFailedReloadStillChangesToTheCreatedTasksTabAndSelectsThatTabsTask() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, archivedTask], active: nil))
        fixture.sync.changeTab(.archived)
        let archivedHistory = try await fixture.client.next()
        archivedHistory.succeed(emptyPage)
        try await fixture.settled()
        fixture.taskCreation.open()
        fixture.taskCreation.setName(secondTask.name)
        fixture.taskCreation.submit()
        let command = try await fixture.client.next()
        command.created(task: secondTask)
        let refresh = try await fixture.client.next()
        XCTAssertEqual(refresh.operation, .refresh(fixture.session.connection.settings))
        refresh.fail(BridgeFailure(message: "Server unavailable", kind: "unavailable", requiresRefresh: true))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertTrue(fixture.session.connection.stale)
        XCTAssertEqual(fixture.session.tasks, [firstTask, archivedTask])
        XCTAssertEqual(fixture.session.taskCatalog.tab, .active)
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, firstTask.id)
    }

    @MainActor
    func testDraftCancellationMakesNoWriteAndEmptyNamesCannotSubmit() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        XCTAssertFalse(fixture.taskCreation.canOpen)
        fixture.taskCreation.open()
        XCTAssertFalse(fixture.taskCreation.isPresented)
        try await fixture.start()
        XCTAssertTrue(fixture.taskCreation.canOpen)
        fixture.taskCreation.open()
        XCTAssertFalse(fixture.taskCreation.canSubmit)
        fixture.taskCreation.setName(" \n\t ")
        fixture.taskCreation.submit()
        XCTAssertFalse(fixture.taskCreation.isSubmitting)
        fixture.taskCreation.setName("New task")
        XCTAssertTrue(fixture.taskCreation.canSubmit)
        fixture.taskCreation.open()
        XCTAssertEqual(fixture.taskCreation.name, "New task", "Opening an open dialog must keep the draft.")
        fixture.taskCreation.cancel()
        XCTAssertFalse(fixture.taskCreation.isPresented)
        XCTAssertEqual(fixture.taskCreation.name, "New task", "The closing sheet keeps its content.")
        XCTAssertFalse(fixture.taskCreation.canSubmit)
        fixture.taskCreation.submit()
        XCTAssertEqual(fixture.client.operations.count, 1)
        fixture.taskCreation.open()
        XCTAssertEqual(fixture.taskCreation.name, "")
    }

    @MainActor
    func testSuccessSelectsCreatedTaskFromArchivedWithoutChangingTracking() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let tasks = [firstTask, archivedTask]
        try await fixture.start(TaskListResources(tasks: tasks, active: activeWorklog))
        fixture.sync.changeTab(.archived)
        let archivedHistory = try await fixture.client.next()
        archivedHistory.succeed(emptyPage)
        try await fixture.settled()
        fixture.taskCreation.open()
        fixture.taskCreation.setName("New task")
        fixture.taskCreation.submit()
        let request = try await fixture.client.next()
        XCTAssertEqual(request.operation, .create(name: "New task", at: "2025-01-01T00:00:00.000Z"))
        fixture.taskCreation.submit()
        fixture.taskCreation.cancel()
        fixture.taskCreation.setName("Changed task")
        XCTAssertTrue(fixture.taskCreation.isPresented)
        XCTAssertEqual(fixture.taskCreation.name, "New task")
        let created = TaskItem(id: "created-id", name: "New task", archived: false, latestStart: nil)
        request.created(
            taskID: created.id, resources: TaskListResources(tasks: tasks + [created], active: activeWorklog))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: created.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertFalse(fixture.taskCreation.isPresented)
        XCTAssertEqual(fixture.session.taskCatalog.tab, .active)
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, created.id)
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
        fixture.sync.refresh()
        let poll = try await fixture.client.next()
        XCTAssertTrue(fixture.taskCreation.canOpen)
        fixture.taskCreation.open()
        fixture.taskCreation.setName("  Task draft  ")
        fixture.taskCreation.submit()
        fixture.clock.now.addTimeInterval(60)
        fixture.taskCreation.setName("Replacement")
        fixture.taskCreation.submit()
        let creation = try await fixture.client.next()
        XCTAssertEqual(creation.operation, .create(name: "  Task draft  ", at: "2025-01-01T00:00:00.000Z"))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 2)
        let created = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.update("the created task") {
            creation.created(taskID: firstTask.id, resources: created)
        } until: {
            fixture.session.taskCatalog.selectedTaskID == firstTask.id
        }
        XCTAssertFalse(fixture.taskCreation.isPresented)
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
        fixture.taskCreation.open()
        fixture.taskCreation.setName("Task\u{0001}")
        fixture.taskCreation.submit()
        let creation = try await fixture.client.next()
        creation.fail(BridgeFailure(message: "Invalid task name", kind: "validation"))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connection.settings))
        XCTAssertFalse(fixture.taskCreation.isSubmitting)
        XCTAssertEqual(fixture.taskCreation.error, "Invalid task name")
        let reloaded = TaskListResources(tasks: [archivedTask], active: nil)
        try await fixture.update("the reloaded task list") {
            reload.succeed(reloaded)
        }
        XCTAssertEqual(fixture.session.tasks, [archivedTask])
        XCTAssertTrue(fixture.taskCreation.isPresented)
        XCTAssertEqual(fixture.taskCreation.name, "Task\u{0001}")
        XCTAssertEqual(fixture.taskCreation.error, "Invalid task name")
        XCTAssertFalse(fixture.taskCreation.isSubmitting)
        XCTAssertTrue(fixture.taskCreation.canSubmit)
        XCTAssertFalse(fixture.session.connection.stale)
        fixture.clock.now.addTimeInterval(30)
        fixture.taskCreation.setName("Valid task")
        XCTAssertNil(fixture.taskCreation.error)
        fixture.taskCreation.submit()
        let retry = try await fixture.client.next()
        XCTAssertEqual(retry.operation, .create(name: "Valid task", at: "2025-01-01T00:00:30.000Z"))
        retry.fail(BridgeFailure(message: "Rejected", kind: "validation"))
        let retryReload = try await fixture.client.next()
        XCTAssertEqual(retryReload.operation, .refresh(fixture.session.connection.settings))
        try await fixture.update("the second reload") {
            retryReload.succeed(reloaded)
        }
        XCTAssertEqual(fixture.taskCreation.error, "Rejected")
        XCTAssertEqual(fixture.taskCreation.name, "Valid task")
    }

    @MainActor
    func testFailedReloadAfterFailedCreationKeepsCreationErrorAndMarksConnectionStale() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.taskCreation.open()
        fixture.taskCreation.setName("New task")
        fixture.taskCreation.submit()
        let creation = try await fixture.client.next()
        creation.fail(BridgeFailure(message: "Response lost", kind: "unavailable", uncertain: true))
        let reload = try await fixture.client.next()
        try await fixture.update("the failed reload") {
            reload.fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        } until: {
            fixture.session.connection.stale
        }
        XCTAssertEqual(
            fixture.taskCreation.error,
            "Could not confirm the change. Check the reloaded data before trying again. Response lost")
        XCTAssertEqual(fixture.taskCreation.name, "New task")
        XCTAssertFalse(fixture.taskCreation.isSubmitting)
        XCTAssertTrue(fixture.taskCreation.canSubmit)
        XCTAssertEqual(fixture.client.operations.count, 3)
    }

    @MainActor
    func testCreationRunsWhileARenameRequestIsInFlight() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.taskRename.open(taskID: firstTask.id)
        fixture.taskRename.setName("Renamed task")
        fixture.taskRename.submit()
        let rename = try await fixture.client.next()
        XCTAssertEqual(
            rename.operation,
            .rename(
                task: firstTask.id, expectedName: firstTask.name, name: "Renamed task",
                at: "2025-01-01T00:00:00.000Z"))
        XCTAssertTrue(fixture.taskRename.isSubmitting)
        XCTAssertTrue(fixture.taskCreation.canOpen)
        fixture.taskCreation.open()
        fixture.taskCreation.setName(secondTask.name)
        XCTAssertTrue(fixture.taskCreation.canSubmit)
        fixture.taskCreation.submit()
        let creation = try await fixture.client.next()
        XCTAssertEqual(creation.operation, .create(name: secondTask.name, at: "2025-01-01T00:00:00.000Z"))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 2)
        creation.created(task: secondTask)
        let creationReload = try await fixture.client.next()
        XCTAssertEqual(creationReload.operation, .refresh(fixture.session.connection.settings))
        XCTAssertFalse(fixture.taskCreation.isPresented)
        XCTAssertTrue(fixture.taskRename.isSubmitting)
        let renamed = TaskItem(id: firstTask.id, name: "Renamed task", archived: false, latestStart: nil)
        try await fixture.update("the rename result") {
            rename.succeed(TaskListResources(tasks: [renamed, secondTask], active: nil))
        } until: {
            !fixture.taskRename.isPresented
        }
        creationReload.succeed(TaskListResources(tasks: [renamed, secondTask], active: nil))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: secondTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.tasks, [renamed, secondTask])
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, secondTask.id)
        XCTAssertNil(fixture.taskCreation.error)
        XCTAssertNil(fixture.taskRename.error)
    }

    @MainActor
    func testTrackingStartAndOtherEditorsStayAvailableWhileCreationRuns() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.taskCreation.open()
        fixture.taskCreation.setName("New task")
        fixture.taskCreation.submit()
        let creation = try await fixture.client.next()
        XCTAssertTrue(fixture.taskCreation.isSubmitting)
        XCTAssertTrue(fixture.taskRename.canOpen)
        XCTAssertTrue(fixture.worklogCorrection.canOpen)
        XCTAssertTrue(fixture.worklogMove.canOpen)
        XCTAssertTrue(fixture.worklogDeletion.canOpen)
        XCTAssertTrue(fixture.trackingAvailability.canStart(taskID: secondTask.id))
        fixture.trackingActions.startTracking(taskID: secondTask.id)
        let start = try await fixture.client.next()
        XCTAssertEqual(start.operation, .start(task: secondTask.id, expected: nil, at: "2025-01-01T00:00:00.000Z"))
        XCTAssertTrue(fixture.taskCreation.isSubmitting)
        let worklog = WorklogItem(id: "worklog-new", taskId: secondTask.id, start: "2025-01-01T00:00:00.000Z", end: nil)
        let tracked = TaskListResources(tasks: [firstTask, secondTask], active: worklog)
        try await fixture.update("the started timer") { start.succeed(tracked) }
        XCTAssertEqual(fixture.session.active, worklog)
        XCTAssertTrue(fixture.taskCreation.isSubmitting)
        let created = TaskItem(id: "created-id", name: "New task", archived: false, latestStart: nil)
        creation.created(
            taskID: created.id, resources: TaskListResources(tasks: [firstTask, secondTask, created], active: worklog))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: created.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertFalse(fixture.taskCreation.isPresented)
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, created.id)
        XCTAssertEqual(fixture.session.active, worklog)
    }

    @MainActor
    func testDraftChangesOnlyNotifyCreationObserver() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        let creation = ObservationProbe { ViewReads.taskCreation(fixture.session) }
        let content = ObservationProbe { ViewReads.content(fixture.session) }
        fixture.taskCreation.open()
        XCTAssertTrue(creation.consume())
        fixture.taskCreation.setName("Draft")
        XCTAssertTrue(creation.consume())
        fixture.taskCreation.setName("Draft")
        XCTAssertFalse(creation.consume(), "Repeating the name must not redraw the sheet.")
        fixture.taskCreation.cancel()
        XCTAssertTrue(creation.consume())
        XCTAssertFalse(content.consume())
    }

    @MainActor
    func testDraftEditsNotifyOnlyTheirSheet() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        fixture.taskCreation.open()
        let lists = ObservationProbe { ViewReads.lists(fixture.session) }
        let controls = ObservationProbe { ViewReads.controls(fixture.session) }
        let rename = ObservationProbe { ViewReads.taskRename(fixture.session) }
        let creation = ObservationProbe { ViewReads.taskCreation(fixture.session) }

        for name in ["N", "Ne", "New"] {
            fixture.taskCreation.setName(name)
            XCTAssertTrue(creation.consume())
            XCTAssertEqual(fixture.taskCreation.name, name)
        }
        XCTAssertFalse(lists.consume())
        XCTAssertFalse(controls.consume())
        XCTAssertFalse(rename.consume())
    }

    @MainActor
    func testSuccessRefreshesDailyReportThenLoadsCreatedTaskHistory() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(
            resources, rows: [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 60_000_000)])
        fixture.taskCreation.open()
        fixture.taskCreation.setName(secondTask.name)
        fixture.taskCreation.submit()
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
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, secondTask.id)
        XCTAssertEqual(fixture.session.active, activeWorklog)
    }

    @MainActor
    func testClosedDraftCannotBeChangedAndCreationInFlightDoesNotBlockConnectionChange() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.taskCreation.setName("Hidden name")
        XCTAssertEqual(fixture.taskCreation.name, "")
        fixture.taskCreation.submit()
        XCTAssertEqual(fixture.client.operations.count, 1)
        fixture.taskCreation.open()
        fixture.taskCreation.setName("Pending task")
        fixture.taskCreation.submit()
        let creation = try await fixture.client.next()
        let connecting = Task { await fixture.sync.connect(serverSettings) }
        let connection = try await fixture.client.next()
        XCTAssertEqual(connection.operation, .connect(serverSettings))
        connection.succeed(emptyTaskListResources)
        let connected = try await fixture.taskValue(connecting)
        XCTAssertTrue(connected)
        XCTAssertEqual(fixture.session.connection.settings, serverSettings)
        XCTAssertTrue(fixture.taskCreation.isSubmitting)
        creation.fail(BridgeFailure(message: "Rejected", kind: "validation"))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connection.settings))
        try await fixture.update("the reloaded task list") {
            reload.succeed(emptyTaskListResources)
        }
        XCTAssertEqual(fixture.taskCreation.error, "Rejected")
        XCTAssertEqual(fixture.taskCreation.name, "Pending task")
    }

    @MainActor
    func testEmbeddedNullIsRejectedBeforeCStringBoundary() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.taskCreation.open()
        fixture.taskCreation.setName("Valid\u{0000}suffix")
        let creation = ObservationProbe { ViewReads.taskCreation(fixture.session) }
        fixture.taskCreation.submit()
        XCTAssertEqual(fixture.taskCreation.error, "Task names must not contain control characters.")
        XCTAssertEqual(fixture.taskCreation.name, "Valid\u{0000}suffix")
        XCTAssertFalse(fixture.taskCreation.isSubmitting)
        XCTAssertEqual(fixture.client.operations.count, 1)
        XCTAssertTrue(creation.consume())
        fixture.taskCreation.cancel()
        XCTAssertEqual(fixture.taskCreation.error, "Task names must not contain control characters.")
        fixture.taskCreation.open()
        XCTAssertNil(fixture.taskCreation.error, "Opening starts a new draft.")
        XCTAssertEqual(fixture.taskCreation.name, "")
    }

    @MainActor
    func testEditingDraftSurvivesSuccessfulConnectionChangeAndUsesNewDestination() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.taskCreation.open()
        fixture.taskCreation.setName("Draft task")
        let connect = Task { await fixture.sync.connect(serverSettings) }
        let connection = try await fixture.client.next()
        connection.succeed(emptyTaskListResources)
        let connected = try await fixture.taskValue(connect)
        XCTAssertTrue(connected)
        XCTAssertEqual(fixture.session.connection.settings, serverSettings)
        XCTAssertTrue(fixture.taskCreation.isPresented)
        XCTAssertEqual(fixture.taskCreation.name, "Draft task")
        fixture.taskCreation.submit()
        let creation = try await fixture.client.next()
        XCTAssertEqual(creation.operation, .create(name: "Draft task", at: "2025-01-01T00:00:00.000Z"))
        creation.fail(BridgeFailure(message: "Invalid", kind: "validation"))
        let reload = try await fixture.client.next()
        try await fixture.update("the reloaded task list") {
            reload.succeed(emptyTaskListResources)
        }
    }

    @MainActor
    func testResubmissionAfterFailureClosesSheetAndReusesSelectedTaskHistory() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.taskCreation.open()
        fixture.taskCreation.setName(firstTask.name)
        fixture.taskCreation.submit()
        let creation = try await fixture.client.next()
        creation.fail(BridgeFailure(message: "Response lost", uncertain: true))
        let confirmed = TaskListResources(tasks: [firstTask], active: nil)
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connection.settings))
        reload.succeed(confirmed)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, firstTask.id)
        XCTAssertEqual(
            fixture.taskCreation.error,
            "Could not confirm the change. Check the reloaded data before trying again. Response lost")
        fixture.clock.now.addTimeInterval(45)
        fixture.taskCreation.submit()
        let resubmission = try await fixture.client.next()
        XCTAssertEqual(resubmission.operation, .create(name: firstTask.name, at: "2025-01-01T00:00:45.000Z"))

        resubmission.created(taskID: firstTask.id, resources: confirmed)
        try await fixture.waitUntil("the sheet to close") { !fixture.taskCreation.isPresented }
        XCTAssertFalse(fixture.taskCreation.isSubmitting)
        XCTAssertFalse(fixture.client.hasPendingRequests)
        XCTAssertTrue(fixture.canStartSelectedTask)
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, firstTask.id)
        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [])
        XCTAssertEqual(
            fixture.client.operations.count, 6,
            "Creating an already selected task must reuse its loaded history.")
    }
}
