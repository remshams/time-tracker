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
        fixture.sync.retryHistory()
        let page = try await fixture.client.next()
        page.succeed(HistoryPage(worklogs: rows ?? [precise, oldWorklog], nextCursor: "older", reset: false))
        try await fixture.settled()
        fixture.worklogDeletion.open(worklogID: precise.id)
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
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connection.settings))
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
            !fixture.session.worklogHistory.isLoading
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
        fixture.sync.retryHistory()
        let page = try await fixture.client.next()
        page.succeed(HistoryPage(worklogs: [activeWorklog, oldWorklog], nextCursor: nil, reset: false))
        try await fixture.settled()

        fixture.worklogDeletion.open(worklogID: activeWorklog.id)
        XCTAssertFalse(fixture.worklogDeletion.isPresented)
        fixture.worklogDeletion.open(worklogID: "missing")
        XCTAssertFalse(fixture.worklogDeletion.isPresented)

        fixture.worklogDeletion.open(worklogID: oldWorklog.id)
        XCTAssertTrue(fixture.worklogDeletion.isPresented)
        XCTAssertEqual(fixture.worklogDeletion.original, oldWorklog)
        XCTAssertEqual(fixture.worklogDeletion.taskName, firstTask.name)
        XCTAssertTrue(fixture.worklogDeletion.canSubmit)
        XCTAssertFalse(fixture.worklogDeletion.isSubmitting)
        XCTAssertNil(fixture.worklogDeletion.error)
    }

    @MainActor
    func testDeletionOpensWhileCorrectionAndArchiveDialogsAreOpen() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.worklogDeletion.cancel()

        fixture.worklogCorrection.open(worklogID: precise.id)
        fixture.taskArchiving.openArchive(taskID: secondTask.id)
        XCTAssertTrue(fixture.worklogDeletion.canOpen)
        fixture.worklogDeletion.open(worklogID: oldWorklog.id)

        XCTAssertTrue(fixture.worklogCorrection.isPresented)
        XCTAssertTrue(fixture.taskArchiving.isPresented)
        XCTAssertTrue(fixture.worklogDeletion.isPresented)
        XCTAssertEqual(fixture.worklogDeletion.original, oldWorklog)
    }

    @MainActor
    func testEditorGatesStayClosedUntilTheConnectionIsConfirmed() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        fixture.sync.start()
        let opening = try await fixture.client.next()
        XCTAssertFalse(fixture.worklogDeletion.canOpen)
        XCTAssertFalse(fixture.worklogCorrection.canOpen)
        XCTAssertFalse(fixture.worklogMove.canOpen)
        opening.succeed(resources)
        let history = try await fixture.client.next()
        history.succeed(HistoryPage(worklogs: [precise], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertTrue(fixture.worklogDeletion.canOpen)
        XCTAssertTrue(fixture.worklogCorrection.canOpen)
        XCTAssertTrue(fixture.worklogMove.canOpen)
    }

    // MARK: - Successful deletion

    @MainActor
    func testSuccessfulDeletionClosesTheDialogBeforeReloadingAndTheRowLeavesAfterTheHistoryReload() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.worklogDeletion.submit()
        XCTAssertTrue(fixture.worklogDeletion.isSubmitting)
        XCTAssertFalse(fixture.worklogDeletion.canSubmit)
        let command = try await command(fixture)

        command.deleted(worklog: precise)

        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connection.settings))
        XCTAssertFalse(fixture.worklogDeletion.isPresented)
        XCTAssertFalse(fixture.worklogDeletion.isSubmitting)
        XCTAssertEqual(fixture.worklogDeletion.original, precise)
        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [precise, oldWorklog])
        let renamed = TaskItem(id: secondTask.id, name: "Renamed task", archived: false, latestStart: nil)
        reload.succeed(TaskListResources(tasks: [firstTask, renamed], active: nil))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [precise, oldWorklog])
        XCTAssertTrue(fixture.session.worklogHistory.isLoading)
        history.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.tasks, [firstTask, renamed])
        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [oldWorklog])
        XCTAssertNil(fixture.session.worklogHistory.nextCursor)
        XCTAssertEqual(deletions(fixture), 1)
        XCTAssertEqual(
            Array(fixture.client.operations.suffix(3)),
            [
                .delete(expected: precise), .refresh(fixture.session.connection.settings),
                .history(task: firstTask.id, cursor: nil),
            ])
    }

    @MainActor
    func testDeletionOfAnotherTasksWorklogReloadsTheSelectedHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.sync.selectTask(secondTask.id)
        let selected = try await fixture.client.next()
        XCTAssertEqual(selected.operation, .history(task: secondTask.id, cursor: nil))
        selected.succeed(emptyPage)
        try await fixture.settled()
        fixture.worklogDeletion.submit()
        let command = try await command(fixture)
        command.deleted(worklog: precise, resources: resources)
        try await finishHistory(fixture, task: secondTask.id, rows: [])
        XCTAssertFalse(fixture.worklogDeletion.isPresented)
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, secondTask.id)
    }

    @MainActor
    func testDeletionIsSentWhileAPollRunsAndTrackingStaysUsable() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let running = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(running)
        fixture.sync.retryHistory()
        let page = try await fixture.client.next()
        page.succeed(HistoryPage(worklogs: [activeWorklog, precise], nextCursor: nil, reset: false))
        try await fixture.settled()
        fixture.sync.refresh()
        let poll = try await fixture.client.next()
        fixture.worklogDeletion.open(worklogID: precise.id)

        fixture.worklogDeletion.submit()

        XCTAssertTrue(fixture.worklogDeletion.isSubmitting)
        let command = try await command(fixture)
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 2)
        fixture.worklogDeletion.submit()
        fixture.worklogDeletion.cancel()
        XCTAssertTrue(fixture.worklogDeletion.isPresented)
        XCTAssertEqual(deletions(fixture), 1)
        XCTAssertTrue(fixture.trackingAvailability.canStop)
        XCTAssertTrue(fixture.worklogCorrection.canOpen)
        XCTAssertTrue(fixture.worklogMove.canOpen)
        try await fixture.update("the poll result") { poll.succeed(running) }

        fixture.trackingActions.stopTracking(worklogID: activeWorklog.id)
        let stop = try await fixture.client.next()
        XCTAssertEqual(stop.operation, .stop(worklog: activeWorklog.id, at: "2025-01-01T00:00:00.000Z"))
        let stopped = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        stop.succeed(stopped)
        try await finishHistory(fixture, rows: [precise])
        XCTAssertNil(fixture.session.active)
        XCTAssertTrue(fixture.worklogDeletion.isSubmitting)

        command.deleted(worklog: precise, resources: stopped)
        try await finishHistory(fixture, rows: [])
        XCTAssertFalse(fixture.worklogDeletion.isPresented)
        XCTAssertNil(fixture.session.active)
    }

    @MainActor
    func testHistoryReloadDoesNotChangeTheOpenDialog() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.sync.retryHistory()
        let updated = try await fixture.client.next()
        updated.succeed(HistoryPage(worklogs: [changed, oldWorklog], nextCursor: nil, reset: false))
        try await fixture.settled()

        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [changed, oldWorklog])
        XCTAssertEqual(fixture.worklogDeletion.original, precise)
        XCTAssertNil(fixture.worklogDeletion.error)
        XCTAssertTrue(fixture.worklogDeletion.canSubmit)
    }

    @MainActor
    func testOpeningWhileTheDialogIsPresentedKeepsTheFirstWorklog() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.worklogDeletion.open(worklogID: oldWorklog.id)
        XCTAssertEqual(fixture.worklogDeletion.original, precise)
        XCTAssertEqual(fixture.worklogDeletion.taskName, firstTask.name)
        XCTAssertTrue(fixture.worklogDeletion.canSubmit)
    }

    // MARK: - Failures

    @MainActor
    func testFailureShowsServerMessageKeepsTheDialogReloadsAndResubmitSendsANewDeletion() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.worklogDeletion.submit()
        let first = try await command(fixture)
        first.fail(BridgeFailure(message: "Deletion failed: Storage error."))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connection.settings))
        XCTAssertFalse(fixture.worklogDeletion.isSubmitting)
        XCTAssertEqual(fixture.worklogDeletion.error, "Deletion failed: Storage error.")
        reload.succeed(resources)
        try await finishHistory(fixture, rows: [precise, oldWorklog])

        XCTAssertTrue(fixture.worklogDeletion.isPresented)
        XCTAssertEqual(fixture.worklogDeletion.original, precise)
        XCTAssertEqual(fixture.worklogDeletion.error, "Deletion failed: Storage error.")
        XCTAssertTrue(fixture.worklogDeletion.canSubmit)
        XCTAssertFalse(fixture.session.connection.stale)

        fixture.worklogDeletion.submit()
        XCTAssertNil(fixture.worklogDeletion.error)
        let retry = try await command(fixture)
        retry.deleted(worklog: precise, resources: resources)
        try await finishHistory(fixture)
        XCTAssertFalse(fixture.worklogDeletion.isPresented)
        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [oldWorklog])
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
            fixture.worklogDeletion.submit()
            let command = try await command(fixture)
            command.fail(failure)
            try await reloadTaskList(fixture)
            try await finishHistory(fixture, rows: [precise, oldWorklog])

            XCTAssertEqual(fixture.worklogDeletion.error, failure.localizedDescription, "\(failure)")
            XCTAssertEqual(fixture.worklogDeletion.original, precise, "\(failure)")
            XCTAssertTrue(fixture.worklogDeletion.canSubmit, "\(failure)")
            fixture.worklogDeletion.cancel()
            XCTAssertFalse(fixture.worklogDeletion.isPresented, "\(failure)")
            XCTAssertFalse(fixture.worklogDeletion.canSubmit, "\(failure)")
        }
    }

    @MainActor
    func testUncertainFailureAsksTheUserToCheckTheReloadedData() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.worklogDeletion.submit()
        let command = try await command(fixture)
        command.fail(BridgeFailure(message: "Response lost", uncertain: true))
        try await reloadTaskList(fixture)
        try await finishHistory(fixture)

        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [oldWorklog])
        XCTAssertTrue(fixture.worklogDeletion.isPresented)
        XCTAssertEqual(fixture.worklogDeletion.original, precise)
        XCTAssertEqual(
            fixture.worklogDeletion.error,
            "Could not confirm the change. Check the reloaded data before trying again. Response lost")
        XCTAssertFalse(fixture.session.connection.stale)
        XCTAssertEqual(deletions(fixture), 1)
    }

    @MainActor
    func testFailedReloadsAfterRejectedDeletionAreReportedAndKeepTheDialog() async throws {
        for failingStep in ["taskList", "history"] {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            try await open(fixture)
            fixture.worklogDeletion.submit()
            let command = try await command(fixture)
            command.fail(BridgeFailure(message: "Deletion failed: Storage error."))
            let taskList = try await fixture.client.next()
            XCTAssertEqual(taskList.operation, .refresh(fixture.session.connection.settings), failingStep)
            if failingStep == "taskList" {
                taskList.fail(BridgeFailure(message: "Reload offline", kind: "unavailable"))
            } else {
                taskList.succeed(resources)
            }
            let history = try await fixture.client.next()
            XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil), failingStep)
            history.fail(BridgeFailure(message: "Reload offline", kind: "unavailable"))
            try await fixture.settled()

            XCTAssertEqual(fixture.session.worklogHistory.error, "Reload offline", failingStep)
            XCTAssertTrue(fixture.session.connection.stale, failingStep)
            XCTAssertEqual(fixture.worklogDeletion.error, "Deletion failed: Storage error.", failingStep)
            XCTAssertEqual(fixture.worklogDeletion.original, precise, failingStep)
            XCTAssertTrue(fixture.worklogDeletion.canSubmit, failingStep)
        }
    }

    // MARK: - Sleep

    @MainActor
    func testDeletionAndHistoryReloadAreSentWhileSleeping() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.sync.sleep()
        fixture.worklogDeletion.submit()
        let command = try await command(fixture)
        try await fixture.update("the deletion result") {
            command.deleted(worklog: precise, resources: resources)
        } until: {
            !fixture.worklogDeletion.isPresented
                && fixture.client.operations.last == .refresh(fixture.session.connection.settings)
        }
        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [precise, oldWorklog])

        try await finishHistory(fixture)
        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [oldWorklog])
        XCTAssertTrue(fixture.scheduler.active.isEmpty)
        fixture.sync.wake()
        let refresh = try await fixture.client.next()
        XCTAssertEqual(refresh.operation, .refresh(.local))
        refresh.succeed(resources)
        try await fixture.settled()
        XCTAssertEqual(deletions(fixture), 1)
    }

    // MARK: - Closed dialog

    @MainActor
    func testClosedDialogKeepsItsContentUntilTheNextOpenAndSendsNothing() async throws {
        let otherTaskWorklog = WorklogItem(
            id: "other-task", taskId: secondTask.id,
            start: "2024-12-30T11:00:00.000000Z", end: "2024-12-30T12:00:00.000000Z")
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.worklogDeletion.submit()
        let first = try await command(fixture)
        first.fail(BridgeFailure(message: "Rejected"))
        try await reloadTaskList(fixture)
        try await finishHistory(fixture, rows: [precise, otherTaskWorklog])

        fixture.worklogDeletion.cancel()

        XCTAssertFalse(fixture.worklogDeletion.isPresented)
        XCTAssertEqual(fixture.worklogDeletion.original, precise)
        XCTAssertEqual(fixture.worklogDeletion.taskName, firstTask.name)
        XCTAssertEqual(fixture.worklogDeletion.error, "Rejected")
        XCTAssertFalse(fixture.worklogDeletion.canSubmit)
        fixture.worklogDeletion.submit()
        XCTAssertFalse(fixture.worklogDeletion.isSubmitting)

        fixture.worklogDeletion.open(worklogID: otherTaskWorklog.id)

        XCTAssertTrue(fixture.worklogDeletion.isPresented)
        XCTAssertEqual(fixture.worklogDeletion.original, otherTaskWorklog)
        XCTAssertEqual(fixture.worklogDeletion.taskName, secondTask.name)
        XCTAssertNil(fixture.worklogDeletion.error)
        XCTAssertTrue(fixture.worklogDeletion.canSubmit)

        fixture.worklogDeletion.submit()
        let second = try await command(fixture, expected: otherTaskWorklog)
        second.deleted(worklog: otherTaskWorklog, resources: resources)
        try await finishHistory(fixture, rows: [precise])
        XCTAssertFalse(fixture.worklogDeletion.isPresented)
    }

    @MainActor
    func testSubmitBeforeTheFirstOpenSendsNothing() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(resources)
        XCTAssertFalse(fixture.worklogDeletion.canSubmit)

        fixture.worklogDeletion.submit()

        XCTAssertFalse(fixture.worklogDeletion.isSubmitting)
        try await fixture.settled()
        XCTAssertEqual(deletions(fixture), 0)
    }

    @MainActor
    func testWorklogOfATaskOutsideTheCatalogShowsAPlaceholderName() async throws {
        let orphan = WorklogItem(
            id: "orphan", taskId: "missing-task",
            start: "2024-12-30T09:00:00.000000Z", end: "2024-12-30T10:00:00.000000Z")
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(resources)
        fixture.sync.retryHistory()
        let page = try await fixture.client.next()
        page.succeed(HistoryPage(worklogs: [orphan], nextCursor: nil, reset: false))
        try await fixture.settled()

        fixture.worklogDeletion.open(worklogID: orphan.id)

        XCTAssertEqual(fixture.worklogDeletion.original, orphan)
        XCTAssertEqual(fixture.worklogDeletion.taskName, "Unknown task")
    }

    @MainActor
    func testUnconfirmedConnectionKeepsTheDialogClosedEvenWithALoadedWorklog() async throws {
        let client = FakeClient()
        let history = WorklogHistoryStore(client: client)
        history.accept(HistoryPage(worklogs: [oldWorklog], nextCursor: nil, reset: false), cursor: nil)
        let deletion = WorklogDeletionStore(
            client: client, connection: ConnectionStore(settings: .local), catalog: TaskCatalogStore(),
            history: history, reloadAfterWrite: {})
        XCTAssertFalse(deletion.canOpen)

        deletion.open(worklogID: oldWorklog.id)

        XCTAssertFalse(deletion.isPresented)
        XCTAssertNil(deletion.original)
    }

    // MARK: - Redraws

    @MainActor
    func testOnlyDialogChangesRedrawTheSheet() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        let harness = PresentationHarness(session: fixture.session)
        let sheet = ObservationProbe { PresentationReads.worklogDeletion(harness) }

        fixture.worklogDeletion.cancel()
        XCTAssertTrue(sheet.consume())

        fixture.sync.retryHistory()
        let page = try await fixture.client.next()
        page.succeed(HistoryPage(worklogs: [precise, oldWorklog], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertFalse(sheet.consume())

        fixture.taskRename.open(taskID: firstTask.id)
        fixture.taskRename.cancel()
        XCTAssertFalse(sheet.consume())

        fixture.worklogDeletion.open(worklogID: oldWorklog.id)
        XCTAssertTrue(sheet.consume())
        XCTAssertEqual(fixture.worklogDeletion.original, oldWorklog)
    }
}
