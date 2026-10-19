import Foundation
import XCTest
@testable import TrackerClient

final class WorklogDeletionTests: XCTestCase {
    private let precise = WorklogItem(
        id: "precise", taskId: firstTask.id,
        start: "2024-12-30T09:00:12.123456Z", end: "2024-12-30T10:00:45.654321Z")
    private let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)

    private var changed: WorklogItem {
        WorklogItem(id: precise.id, taskId: precise.taskId, start: precise.start, end: "2024-12-30T10:30:00.000000Z")
    }

    @MainActor
    private func open(_ fixture: Fixture, rows: [WorklogItem]? = nil) async throws {
        try await fixture.start(resources)
        fixture.session.retryHistory()
        let page = try await fixture.client.next()
        page.succeed(HistoryPage(worklogs: rows ?? [precise, oldWorklog], nextCursor: "older", reset: false))
        try await fixture.settled()
        fixture.session.openWorklogDeletion(worklogID: precise.id)
    }

    @MainActor
    private func command(_ fixture: Fixture, expected: WorklogItem? = nil) async throws -> FakeClient.Request {
        let command = try await fixture.client.next()
        XCTAssertEqual(command.operation, .delete(expected: expected ?? precise))
        return command
    }

    /// Answers the task-list reload that follows a failed deletion.
    @MainActor
    private func reloadTaskList(_ fixture: Fixture, _ reloaded: TaskListResources? = nil) async throws {
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connectionSettings))
        reload.succeed(reloaded ?? resources)
    }

    @MainActor
    private func finishHistory(
        _ fixture: Fixture, task: String = firstTask.id, rows: [WorklogItem] = [oldWorklog]
    ) async throws {
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: task, cursor: nil))
        try await fixture.update("the reloaded history") {
            history.succeed(HistoryPage(worklogs: rows, nextCursor: nil, reset: false))
        } until: {
            !fixture.session.isHistoryLoading
        }
    }

    @MainActor
    private func deletions(_ fixture: Fixture) -> Int {
        fixture.client.operations.filter {
            if case .delete = $0 { return true }
            return false
        }.count
    }

    // MARK: - Opening and gates

    @MainActor
    func testOpeningShowsTheCompletedWorklogAndIgnoresRunningOrUnknownEntries() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let running = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(running)
        fixture.session.retryHistory()
        let page = try await fixture.client.next()
        page.succeed(HistoryPage(worklogs: [activeWorklog, oldWorklog], nextCursor: nil, reset: false))
        try await fixture.settled()

        fixture.session.openWorklogDeletion(worklogID: activeWorklog.id)
        XCTAssertFalse(fixture.session.worklogDeletion.isPresented)
        fixture.session.openWorklogDeletion(worklogID: "missing")
        XCTAssertFalse(fixture.session.worklogDeletion.isPresented)

        fixture.session.openWorklogDeletion(worklogID: oldWorklog.id)
        let state = fixture.session.worklogDeletion
        XCTAssertTrue(state.isPresented)
        XCTAssertEqual(state.original, oldWorklog)
        XCTAssertEqual(state.taskName, firstTask.name)
        XCTAssertTrue(state.canSubmit)
        XCTAssertFalse(state.isSubmitting)
        XCTAssertNil(state.error)
    }

    @MainActor
    func testOpenDeletionDoesNotBlockOtherEditorsOrConnectionChange() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        XCTAssertTrue(fixture.session.canOpenWorklogCorrection)
        XCTAssertTrue(fixture.session.canOpenWorklogMove)
        XCTAssertTrue(fixture.session.canOpenTaskCreation)
        XCTAssertTrue(fixture.session.canOpenTaskRename)
        XCTAssertTrue(fixture.session.canArchiveTask(taskID: secondTask.id))

        let connecting = Task { await fixture.session.connect(serverSettings) }
        let connection = try await fixture.client.next()
        XCTAssertEqual(connection.operation, .connect(serverSettings))
        connection.succeed(emptyTaskListResources)
        let connected = try await fixture.taskValue(connecting)

        XCTAssertTrue(connected)
        XCTAssertEqual(fixture.session.connectionSettings, serverSettings)
        XCTAssertNil(fixture.session.connectionMessage)
        XCTAssertTrue(fixture.session.worklogDeletion.isPresented)
        XCTAssertEqual(fixture.session.worklogDeletion.original, precise)
        fixture.session.cancelWorklogDeletion()
        XCTAssertFalse(fixture.session.worklogDeletion.isPresented)
        XCTAssertNil(fixture.session.worklogDeletion.original)
    }

    @MainActor
    func testDeletionOpensWhileCorrectionAndArchiveDialogsAreOpen() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.session.cancelWorklogDeletion()

        fixture.session.openWorklogCorrection(worklogID: precise.id)
        fixture.session.openTaskArchive(taskID: secondTask.id)
        XCTAssertTrue(fixture.session.canOpenWorklogDeletion)
        fixture.session.openWorklogDeletion(worklogID: oldWorklog.id)

        XCTAssertTrue(fixture.session.worklogCorrection.isPresented)
        XCTAssertTrue(fixture.session.taskArchiving.isPresented)
        XCTAssertTrue(fixture.session.worklogDeletion.isPresented)
        XCTAssertEqual(fixture.session.worklogDeletion.original, oldWorklog)
    }

    @MainActor
    func testEditorGatesStayClosedUntilTheConnectionIsConfirmed() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        fixture.session.start()
        let opening = try await fixture.client.next()
        XCTAssertFalse(fixture.session.canOpenWorklogDeletion)
        XCTAssertFalse(fixture.session.canOpenWorklogCorrection)
        XCTAssertFalse(fixture.session.canOpenWorklogMove)
        opening.succeed(resources)
        let history = try await fixture.client.next()
        history.succeed(HistoryPage(worklogs: [precise], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.canOpenWorklogDeletion)
        XCTAssertTrue(fixture.session.canOpenWorklogCorrection)
        XCTAssertTrue(fixture.session.canOpenWorklogMove)
    }

    // MARK: - Successful deletion

    @MainActor
    func testSuccessfulDeletionClosesTheDialogBeforeReloadingAndTheRowLeavesAfterTheHistoryReload() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.session.submitWorklogDeletion()
        XCTAssertTrue(fixture.session.worklogDeletion.isSubmitting)
        XCTAssertFalse(fixture.session.worklogDeletion.canSubmit)
        let command = try await command(fixture)
        let harness = PresentationHarness(session: fixture.session)
        let dismissals = harness.recorder(PresentationReads.worklogDeletion) {
            !harness.presentation.worklogDeletion.isPresented
        }

        command.deleted(worklog: precise)

        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connectionSettings))
        XCTAssertEqual(dismissals.values.last, true)
        XCTAssertNil(fixture.session.worklogDeletion.original)
        XCTAssertEqual(fixture.session.worklogs, [precise, oldWorklog])
        let renamed = TaskItem(id: secondTask.id, name: "Renamed task", archived: false, latestStart: nil)
        reload.succeed(TaskListResources(tasks: [firstTask, renamed], active: nil))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        XCTAssertEqual(fixture.session.worklogs, [precise, oldWorklog])
        XCTAssertTrue(fixture.session.isHistoryLoading)
        history.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.tasks, [firstTask, renamed])
        XCTAssertEqual(fixture.session.worklogs, [oldWorklog])
        XCTAssertNil(fixture.session.nextCursor)
        XCTAssertEqual(deletions(fixture), 1)
        XCTAssertEqual(
            Array(fixture.client.operations.suffix(3)),
            [
                .delete(expected: precise), .refresh(fixture.session.connectionSettings),
                .history(task: firstTask.id, cursor: nil),
            ])
    }

    @MainActor
    func testDeletionOfAnotherTasksWorklogReloadsTheSelectedHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.session.select(secondTask.id)
        let selected = try await fixture.client.next()
        XCTAssertEqual(selected.operation, .history(task: secondTask.id, cursor: nil))
        selected.succeed(emptyPage)
        try await fixture.settled()
        fixture.session.submitWorklogDeletion()
        let command = try await command(fixture)
        command.deleted(worklog: precise, resources: resources)
        try await finishHistory(fixture, task: secondTask.id, rows: [])
        XCTAssertFalse(fixture.session.worklogDeletion.isPresented)
        XCTAssertEqual(fixture.session.selectedTaskID, secondTask.id)
    }

    @MainActor
    func testDeletionIsSentWhileAPollRunsAndTrackingStaysUsable() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let running = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(running)
        fixture.session.retryHistory()
        let page = try await fixture.client.next()
        page.succeed(HistoryPage(worklogs: [activeWorklog, precise], nextCursor: nil, reset: false))
        try await fixture.settled()
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        fixture.session.openWorklogDeletion(worklogID: precise.id)
        let harness = PresentationHarness(session: fixture.session)

        fixture.session.submitWorklogDeletion()

        XCTAssertTrue(harness.presentation.worklogDeletionSheetContent.isSubmitting)
        let command = try await command(fixture)
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 2)
        fixture.session.submitWorklogDeletion()
        fixture.session.cancelWorklogDeletion()
        XCTAssertTrue(fixture.session.worklogDeletion.isPresented)
        XCTAssertEqual(deletions(fixture), 1)
        XCTAssertTrue(fixture.session.canStopTracking)
        XCTAssertTrue(fixture.session.canOpenWorklogCorrection)
        XCTAssertTrue(fixture.session.canOpenWorklogMove)
        fixture.session.onChange = nil
        try await fixture.update("the poll result") { poll.succeed(running) }

        fixture.session.stopTracking(worklogID: activeWorklog.id)
        let stop = try await fixture.client.next()
        XCTAssertEqual(stop.operation, .stop(worklog: activeWorklog.id, at: "2025-01-01T00:00:00.000Z"))
        let stopped = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        stop.succeed(stopped)
        try await finishHistory(fixture, rows: [precise])
        XCTAssertNil(fixture.session.active)
        XCTAssertTrue(fixture.session.worklogDeletion.isSubmitting)

        command.deleted(worklog: precise, resources: stopped)
        try await finishHistory(fixture, rows: [])
        XCTAssertFalse(fixture.session.worklogDeletion.isPresented)
        XCTAssertNil(fixture.session.active)
    }

    @MainActor
    func testHistoryReloadDoesNotChangeTheOpenDialog() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.session.retryHistory()
        let updated = try await fixture.client.next()
        updated.succeed(HistoryPage(worklogs: [changed, oldWorklog], nextCursor: nil, reset: false))
        try await fixture.settled()

        XCTAssertEqual(fixture.session.worklogs, [changed, oldWorklog])
        XCTAssertEqual(fixture.session.worklogDeletion.original, precise)
        XCTAssertNil(fixture.session.worklogDeletion.error)
        XCTAssertTrue(fixture.session.worklogDeletion.canSubmit)
    }

    @MainActor
    func testOpeningWhileTheDialogIsPresentedKeepsTheFirstWorklog() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.session.openWorklogDeletion(worklogID: oldWorklog.id)
        XCTAssertEqual(fixture.session.worklogDeletion.original, precise)
        XCTAssertEqual(fixture.session.worklogDeletion.taskName, firstTask.name)
        XCTAssertTrue(fixture.session.worklogDeletion.canSubmit)
    }

    // MARK: - Failures

    @MainActor
    func testFailureShowsServerMessageKeepsTheDialogReloadsAndResubmitSendsANewDeletion() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.session.submitWorklogDeletion()
        let first = try await command(fixture)
        first.fail(BridgeFailure(message: "Deletion failed: Storage error."))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connectionSettings))
        XCTAssertFalse(fixture.session.worklogDeletion.isSubmitting)
        XCTAssertEqual(fixture.session.worklogDeletion.error, "Deletion failed: Storage error.")
        reload.succeed(resources)
        try await finishHistory(fixture, rows: [precise, oldWorklog])

        let state = fixture.session.worklogDeletion
        XCTAssertTrue(state.isPresented)
        XCTAssertEqual(state.original, precise)
        XCTAssertEqual(state.error, "Deletion failed: Storage error.")
        XCTAssertTrue(state.canSubmit)
        XCTAssertFalse(fixture.session.isStale)

        fixture.session.submitWorklogDeletion()
        XCTAssertNil(fixture.session.worklogDeletion.error)
        let retry = try await command(fixture)
        retry.deleted(worklog: precise, resources: resources)
        try await finishHistory(fixture)
        XCTAssertFalse(fixture.session.worklogDeletion.isPresented)
        XCTAssertEqual(fixture.session.worklogs, [oldWorklog])
        XCTAssertEqual(deletions(fixture), 2)
    }

    @MainActor
    func testFailuresShowTheirMessageAndKeepTheDialogUntilCancelled() async throws {
        let failures: [Error] = [
            TestTimeout(description: "Bridge crashed"),
            BridgeFailure(message: "Server unavailable", kind: "unavailable"),
            BridgeFailure(message: "Malformed response", kind: "protocol"),
            BridgeFailure(message: "Worklog changed in another client", kind: "worklog_changed", requiresRefresh: true),
        ]
        for failure in failures {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            try await open(fixture)
            fixture.session.submitWorklogDeletion()
            let command = try await command(fixture)
            command.fail(failure)
            try await reloadTaskList(fixture)
            try await finishHistory(fixture, rows: [precise, oldWorklog])

            XCTAssertEqual(fixture.session.worklogDeletion.error, failure.localizedDescription, "\(failure)")
            XCTAssertEqual(fixture.session.worklogDeletion.original, precise, "\(failure)")
            XCTAssertTrue(fixture.session.worklogDeletion.canSubmit, "\(failure)")
            fixture.session.cancelWorklogDeletion()
            XCTAssertNil(fixture.session.worklogDeletion.original, "\(failure)")
            XCTAssertNil(fixture.session.worklogDeletion.error, "\(failure)")
        }
    }

    @MainActor
    func testUncertainFailureAsksTheUserToCheckTheReloadedData() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.session.submitWorklogDeletion()
        let command = try await command(fixture)
        command.fail(BridgeFailure(message: "Response lost", uncertain: true))
        try await reloadTaskList(fixture)
        try await finishHistory(fixture)

        XCTAssertEqual(fixture.session.worklogs, [oldWorklog])
        XCTAssertTrue(fixture.session.worklogDeletion.isPresented)
        XCTAssertEqual(fixture.session.worklogDeletion.original, precise)
        XCTAssertEqual(
            fixture.session.worklogDeletion.error,
            "Could not confirm the change. Check the reloaded data before trying again. Response lost")
        XCTAssertFalse(fixture.session.isStale)
        XCTAssertEqual(deletions(fixture), 1)
    }

    @MainActor
    func testFailedReloadsAfterRejectedDeletionAreReportedAndKeepTheDialog() async throws {
        for failingStep in ["taskList", "history"] {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            try await open(fixture)
            fixture.session.submitWorklogDeletion()
            let command = try await command(fixture)
            command.fail(BridgeFailure(message: "Deletion failed: Storage error."))
            let taskList = try await fixture.client.next()
            XCTAssertEqual(taskList.operation, .refresh(fixture.session.connectionSettings), failingStep)
            if failingStep == "taskList" {
                taskList.fail(BridgeFailure(message: "Reload offline", kind: "unavailable"))
            } else {
                taskList.succeed(resources)
            }
            let history = try await fixture.client.next()
            XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil), failingStep)
            history.fail(BridgeFailure(message: "Reload offline", kind: "unavailable"))
            try await fixture.settled()

            XCTAssertEqual(fixture.session.error, "Reload offline", failingStep)
            XCTAssertTrue(fixture.session.isStale, failingStep)
            XCTAssertEqual(fixture.session.worklogDeletion.error, "Deletion failed: Storage error.", failingStep)
            XCTAssertEqual(fixture.session.worklogDeletion.original, precise, failingStep)
            XCTAssertTrue(fixture.session.worklogDeletion.canSubmit, failingStep)
        }
    }

    // MARK: - Sleep

    @MainActor
    func testDeletionAndHistoryReloadAreSentWhileSleeping() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.session.sleep()
        fixture.session.submitWorklogDeletion()
        let command = try await command(fixture)
        try await fixture.update("the deletion result") {
            command.deleted(worklog: precise, resources: resources)
        } until: {
            !fixture.session.worklogDeletion.isPresented
                && fixture.client.operations.last == .refresh(fixture.session.connectionSettings)
        }
        XCTAssertEqual(fixture.session.worklogs, [precise, oldWorklog])

        try await finishHistory(fixture)
        XCTAssertEqual(fixture.session.worklogs, [oldWorklog])
        XCTAssertTrue(fixture.scheduler.active.isEmpty)
        fixture.session.wake()
        let refresh = try await fixture.client.next()
        XCTAssertEqual(refresh.operation, .refresh(.local))
        refresh.succeed(resources)
        try await fixture.settled()
        XCTAssertEqual(deletions(fixture), 1)
    }

    // MARK: - Presentation

    @MainActor
    func testObserverFreezesDismissedContentAndOnlyPublishesDeletionChanges() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        let harness = PresentationHarness(session: fixture.session)
        let changes = harness.counter(PresentationReads.worklogDeletion)
        let visible = fixture.session.worklogDeletion
        XCTAssertEqual(harness.presentation.worklogDeletionSheetContent, visible)

        fixture.session.cancelWorklogDeletion()
        XCTAssertEqual(changes.count, 1)
        XCTAssertEqual(harness.presentation.worklogDeletionSheetContent, visible)

        fixture.session.retryHistory()
        let page = try await fixture.client.next()
        page.succeed(HistoryPage(worklogs: [precise, oldWorklog], nextCursor: nil, reset: false))
        try await fixture.settled()
        harness.attach()
        XCTAssertEqual(changes.count, 1)

        fixture.session.openTaskCreation()
        fixture.session.cancelTaskCreation()
        XCTAssertEqual(changes.count, 1)

        fixture.session.openWorklogDeletion(worklogID: oldWorklog.id)
        XCTAssertEqual(changes.count, 2)
        XCTAssertEqual(harness.presentation.worklogDeletionSheetContent.original, oldWorklog)
    }

    // MARK: - State

    @MainActor
    func testStateStartsClosedAndKeepsTheFirstWorklogWhileOpen() {
        let state = WorklogDeletionState()
        XCTAssertFalse(state.presentation.canSubmit)
        XCTAssertNil(state.submit())

        state.open(precise, taskName: firstTask.name)
        state.open(oldWorklog, taskName: secondTask.name)

        XCTAssertEqual(state.presentation.original, precise)
        XCTAssertEqual(state.presentation.taskName, firstTask.name)
        XCTAssertEqual(state.submit(), precise)
        XCTAssertFalse(state.presentation.canSubmit)
        XCTAssertNil(state.submit())
    }

    @MainActor
    func testFailureKeepsTheDialogAndCancelAllowsAnotherWorklogToOpen() {
        let state = WorklogDeletionState()
        state.open(precise, taskName: firstTask.name)
        XCTAssertEqual(state.submit(), precise)
        state.fail(BridgeFailure(message: "Rejected"))
        XCTAssertTrue(state.presentation.isPresented)
        XCTAssertFalse(state.presentation.isSubmitting)
        XCTAssertEqual(state.presentation.error, "Rejected")
        XCTAssertTrue(state.presentation.canSubmit)

        state.cancel()
        state.open(oldWorklog, taskName: secondTask.name)

        XCTAssertEqual(state.presentation.original, oldWorklog)
        XCTAssertEqual(state.presentation.taskName, secondTask.name)
        XCTAssertNil(state.presentation.error)
        XCTAssertEqual(state.submit(), oldWorklog)
    }

    @MainActor
    func testCancelWhileSubmittingKeepsTheDialogOpen() {
        let state = WorklogDeletionState()
        state.open(precise, taskName: firstTask.name)
        XCTAssertEqual(state.submit(), precise)
        state.cancel()
        XCTAssertTrue(state.presentation.isPresented)
        XCTAssertTrue(state.presentation.isSubmitting)
        state.reset()
        XCTAssertFalse(state.presentation.isPresented)
        XCTAssertFalse(state.presentation.isSubmitting)
        XCTAssertNil(state.presentation.original)
    }
}
