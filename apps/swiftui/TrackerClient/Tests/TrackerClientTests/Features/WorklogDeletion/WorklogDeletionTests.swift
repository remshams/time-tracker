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
    private func preflight(
        _ fixture: Fixture, rows: [WorklogItem]? = nil, tasks: TaskListResources? = nil
    ) async throws {
        let taskList = try await fixture.client.next()
        XCTAssertEqual(taskList.operation, .taskList)
        taskList.succeed(tasks ?? resources)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        history.succeed(HistoryPage(worklogs: rows ?? [precise], nextCursor: nil, reset: false))
    }

    @MainActor
    private func command(_ fixture: Fixture, expected: WorklogItem? = nil) async throws -> FakeClient.Request {
        let command = try await fixture.client.next()
        XCTAssertEqual(command.operation, .delete(expected: expected ?? precise))
        return command
    }

    @MainActor
    private func finishHistory(_ fixture: Fixture, rows: [WorklogItem] = [oldWorklog]) async throws {
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        history.succeed(HistoryPage(worklogs: rows, nextCursor: nil, reset: false))
        try await fixture.settled()
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
    func testOpenDeletionBlocksOtherEditorsAndConnectionChangesUntilCancelled() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        XCTAssertFalse(fixture.session.canOpenTaskCreation)
        XCTAssertFalse(fixture.session.canOpenTaskRename)
        XCTAssertFalse(fixture.session.canOpenWorklogCorrection)
        XCTAssertFalse(fixture.session.canOpenWorklogMove)
        XCTAssertFalse(fixture.session.canArchiveTask(taskID: secondTask.id))
        var publishedMessage: String?
        fixture.session.onChange = { publishedMessage = fixture.session.connectionMessage }
        let connecting = Task { await fixture.session.connect(serverSettings) }
        let connected = try await fixture.taskValue(connecting)
        XCTAssertFalse(connected)
        XCTAssertEqual(publishedMessage, "Finish or retry worklog deletion before changing connections.")
        fixture.session.onChange = nil

        fixture.session.cancelWorklogDeletion()

        XCTAssertFalse(fixture.session.worklogDeletion.isPresented)
        XCTAssertNil(fixture.session.worklogDeletion.original)
        XCTAssertTrue(fixture.session.canOpenTaskCreation)
        XCTAssertTrue(fixture.session.canOpenTaskRename)
        XCTAssertTrue(fixture.session.canOpenWorklogCorrection)
        XCTAssertTrue(fixture.session.canOpenWorklogMove)
        XCTAssertTrue(fixture.session.canArchiveTask(taskID: secondTask.id))
    }

    @MainActor
    func testOtherEditorsBlockDeletion() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.session.cancelWorklogDeletion()
        XCTAssertTrue(fixture.session.canOpenWorklogDeletion)

        fixture.session.openTaskCreation()
        XCTAssertFalse(fixture.session.canOpenWorklogDeletion)
        XCTAssertFalse(fixture.session.canOpenWorklogCorrection)
        XCTAssertFalse(fixture.session.canArchiveTask(taskID: secondTask.id))
        fixture.session.openWorklogDeletion(worklogID: precise.id)
        XCTAssertFalse(fixture.session.worklogDeletion.isPresented)
        fixture.session.cancelTaskCreation()

        fixture.session.openTaskRename()
        XCTAssertFalse(fixture.session.canOpenWorklogDeletion)
        fixture.session.cancelTaskRename()

        fixture.session.openWorklogCorrection(worklogID: precise.id)
        XCTAssertFalse(fixture.session.canOpenWorklogDeletion)
        XCTAssertFalse(fixture.session.canOpenTaskCreation)
        fixture.session.cancelWorklogCorrection()

        fixture.session.openTaskArchive(taskID: secondTask.id)
        XCTAssertFalse(fixture.session.canOpenWorklogDeletion)
        fixture.session.cancelTaskArchiving()
        XCTAssertTrue(fixture.session.canOpenWorklogDeletion)
    }

    @MainActor
    func testEditorGatesCloseBeforeTheConnectionIsConfirmedAndAfterShutdown() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        fixture.session.start()
        let opening = try await fixture.client.next()
        XCTAssertFalse(fixture.session.canOpenWorklogDeletion)
        opening.succeed(resources)
        let history = try await fixture.client.next()
        history.succeed(HistoryPage(worklogs: [precise], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.canOpenWorklogDeletion)

        fixture.session.shutdown()

        XCTAssertFalse(fixture.session.canOpenWorklogDeletion)
        XCTAssertFalse(fixture.session.canOpenTaskCreation)
        XCTAssertFalse(fixture.session.canOpenTaskRename)
        XCTAssertFalse(fixture.session.canOpenWorklogCorrection)
        XCTAssertFalse(fixture.session.canOpenWorklogMove)
        XCTAssertFalse(fixture.session.canArchiveTask(taskID: secondTask.id))
        fixture.session.openWorklogDeletion(worklogID: precise.id)
        XCTAssertFalse(fixture.session.worklogDeletion.isPresented)
    }

    @MainActor
    func testShutdownClearsAnOpenDeletion() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.session.shutdown()
        XCTAssertFalse(fixture.session.worklogDeletion.isPresented)
        XCTAssertNil(fixture.session.worklogDeletion.original)
        fixture.session.cancelWorklogDeletion()
        fixture.session.reviewLatestWorklogDeletion()
        fixture.session.submitWorklogDeletion()
        XCTAssertEqual(deletions(fixture), 0)
    }

    // MARK: - Successful deletion

    @MainActor
    func testConfirmedDeletionRemovesTheRowAndDismissesBeforeTheHeldRefresh() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.session.submitWorklogDeletion()
        XCTAssertTrue(fixture.session.worklogDeletion.isSubmitting)
        XCTAssertFalse(fixture.session.worklogDeletion.canSubmit)
        let renamed = TaskItem(id: secondTask.id, name: "Renamed task", archived: false, latestStart: nil)
        try await preflight(fixture, tasks: TaskListResources(tasks: [firstTask, renamed], active: nil))
        let command = try await command(fixture)
        XCTAssertTrue(fixture.session.tasks.contains(renamed))
        let harness = PresentationHarness(session: fixture.session)
        let publishedRows = harness.recorder(PresentationReads.content) { harness.presentation.worklogs }
        let dismissals = harness.recorder(PresentationReads.worklogDeletion) {
            !harness.presentation.worklogDeletion.isPresented
        }

        command.deleted(worklog: precise)

        let heldRefresh = try await fixture.client.next()
        XCTAssertEqual(heldRefresh.operation, .taskList)
        XCTAssertEqual(fixture.session.worklogs, [oldWorklog])
        XCTAssertEqual(publishedRows.values.last, [oldWorklog])
        XCTAssertEqual(dismissals.values.last, true)
        XCTAssertNil(fixture.session.nextCursor)
        XCTAssertTrue(fixture.session.isHistoryLoading)
        XCTAssertNil(fixture.session.worklogDeletion.original)
        heldRefresh.succeed(resources)
        try await finishHistory(fixture)
        XCTAssertEqual(fixture.session.worklogs, [oldWorklog])
        XCTAssertEqual(deletions(fixture), 1)
    }

    @MainActor
    func testDeletionOfAnotherTasksWorklogKeepsTheSelectedHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.session.submitWorklogDeletion()
        let taskList = try await fixture.client.next()
        taskList.succeed(resources)
        fixture.session.select(secondTask.id)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        history.succeed(HistoryPage(worklogs: [precise], nextCursor: nil, reset: false))
        let command = try await command(fixture)
        command.deleted(worklog: precise, resources: resources)
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .history(task: secondTask.id, cursor: nil))
        reload.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertFalse(fixture.session.worklogDeletion.isPresented)
    }

    @MainActor
    func testDeletionQueuedDuringRefreshBlocksTrackingAndPublishesSubmission() async throws {
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
        XCTAssertFalse(fixture.session.isBlockingControls)
        fixture.session.openWorklogDeletion(worklogID: precise.id)
        let harness = PresentationHarness(session: fixture.session)

        fixture.session.submitWorklogDeletion()

        XCTAssertTrue(harness.presentation.worklogDeletionSheetContent.isSubmitting)
        XCTAssertTrue(fixture.session.isBlockingControls)
        XCTAssertFalse(fixture.session.canStopTracking)
        XCTAssertFalse(fixture.session.canStartTracking(taskID: secondTask.id))
        fixture.session.onChange = nil
        poll.succeed(running)
        try await preflight(fixture, tasks: running)
        let command = try await command(fixture)
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 1)
        command.deleted(worklog: precise, resources: running)
        try await finishHistory(fixture, rows: [activeWorklog])
        XCTAssertEqual(fixture.session.active, activeWorklog)
    }

    // MARK: - Review

    @MainActor
    func testChangedWorklogRequiresReviewThenDeletesTheLatestEntry() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.session.submitWorklogDeletion()
        try await preflight(fixture, rows: [changed])
        try await fixture.settled()

        var state = fixture.session.worklogDeletion
        XCTAssertTrue(state.requiresReview)
        XCTAssertEqual(state.latest, changed)
        XCTAssertEqual(state.error, "This worklog changed. Review it before deleting.")
        XCTAssertFalse(state.canSubmit)
        XCTAssertFalse(state.isSubmitting)
        XCTAssertEqual(deletions(fixture), 0)
        fixture.session.submitWorklogDeletion()
        XCTAssertFalse(fixture.session.worklogDeletion.isSubmitting)

        let harness = PresentationHarness(session: fixture.session)
        fixture.session.reviewLatestWorklogDeletion()
        fixture.session.onChange = nil

        state = fixture.session.worklogDeletion
        XCTAssertEqual(harness.presentation.worklogDeletionSheetContent, state)
        XCTAssertFalse(state.requiresReview)
        XCTAssertEqual(state.original, changed)
        XCTAssertEqual(state.taskName, firstTask.name)
        XCTAssertNil(state.latest)
        XCTAssertNil(state.error)
        fixture.session.submitWorklogDeletion()
        try await preflight(fixture, rows: [changed])
        let command = try await command(fixture, expected: changed)
        command.deleted(worklog: changed, resources: resources)
        try await finishHistory(fixture)
        XCTAssertFalse(fixture.session.worklogDeletion.isPresented)
    }

    @MainActor
    func testMovedWorklogReviewUsesTheDestinationTaskName() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        let moved = WorklogItem(id: precise.id, taskId: secondTask.id, start: precise.start, end: precise.end)
        fixture.session.retryHistory()
        let page = try await fixture.client.next()
        page.succeed(HistoryPage(worklogs: [moved], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.worklogDeletion.requiresReview)

        fixture.session.reviewLatestWorklogDeletion()

        XCTAssertEqual(fixture.session.worklogDeletion.original, moved)
        XCTAssertEqual(fixture.session.worklogDeletion.taskName, secondTask.name)
    }

    @MainActor
    func testMissingWorklogRequiresReviewWithoutDeleting() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.session.submitWorklogDeletion()
        try await preflight(fixture, rows: [])
        try await fixture.settled()

        let state = fixture.session.worklogDeletion
        XCTAssertTrue(state.requiresReview)
        XCTAssertNil(state.latest)
        XCTAssertEqual(state.error, "This worklog was moved or deleted. Cancel and refresh history.")
        fixture.session.reviewLatestWorklogDeletion()
        XCTAssertTrue(fixture.session.worklogDeletion.requiresReview)
        XCTAssertEqual(deletions(fixture), 0)
    }

    @MainActor
    func testHistoryRefreshRequiresReviewOnlyWhenTheOpenWorklogChanged() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.session.retryHistory()
        let same = try await fixture.client.next()
        same.succeed(HistoryPage(worklogs: [precise, oldWorklog], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertFalse(fixture.session.worklogDeletion.requiresReview)

        fixture.session.retryHistory()
        let updated = try await fixture.client.next()
        updated.succeed(HistoryPage(worklogs: [changed, oldWorklog], nextCursor: nil, reset: false))
        try await fixture.settled()

        XCTAssertTrue(fixture.session.worklogDeletion.requiresReview)
        XCTAssertEqual(fixture.session.worklogDeletion.latest, changed)
    }

    @MainActor
    func testPreflightReportsHistoryThatCannotBeVerifiedWithinTheLimit() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.session.submitWorklogDeletion()
        let taskList = try await fixture.client.next()
        taskList.succeed(resources)
        let first = try await fixture.client.next()
        first.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "page-two", reset: false))
        let second = try await fixture.client.next()
        XCTAssertEqual(second.operation, .history(task: firstTask.id, cursor: "page-two"))
        second.succeed(HistoryPage(worklogs: [], nextCursor: "page-three", reset: false))
        try await fixture.settled()

        XCTAssertEqual(
            fixture.session.worklogDeletion.error,
            "Could not verify this worklog within the history limit. Cancel and reopen the dialog before retrying.")
        XCTAssertEqual(deletions(fixture), 0)
    }

    // MARK: - Failures

    @MainActor
    func testUncertainFailureKeepsTheIntentAndRetryRecognizesTheCommittedDeletion() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.session.submitWorklogDeletion()
        try await preflight(fixture)
        let first = try await command(fixture)
        first.fail(BridgeFailure(message: "Response lost", uncertain: true))
        try await preflight(fixture)
        try await fixture.settled()

        var state = fixture.session.worklogDeletion
        XCTAssertEqual(state.error, "Response lost")
        XCTAssertTrue(state.canSubmit)
        XCTAssertFalse(state.isSubmitting)
        fixture.session.cancelWorklogDeletion()
        state = fixture.session.worklogDeletion
        XCTAssertFalse(state.isPresented)
        XCTAssertEqual(state.original, precise)
        XCTAssertFalse(fixture.session.canOpenTaskCreation)

        fixture.session.openWorklogDeletion(worklogID: oldWorklog.id)
        XCTAssertTrue(fixture.session.worklogDeletion.isPresented)
        XCTAssertEqual(fixture.session.worklogDeletion.original, precise)
        fixture.session.submitWorklogDeletion()
        try await preflight(fixture, rows: [])
        try await finishHistory(fixture)

        XCTAssertFalse(fixture.session.worklogDeletion.isPresented)
        XCTAssertNil(fixture.session.worklogDeletion.original)
        XCTAssertEqual(deletions(fixture), 1)
    }

    @MainActor
    func testLostResponseIsResolvedDuringReconciliation() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.session.submitWorklogDeletion()
        try await preflight(fixture)
        let command = try await command(fixture)
        command.fail(BridgeFailure(message: "Server unavailable", kind: "unavailable"))
        try await preflight(fixture, rows: [])
        try await finishHistory(fixture)

        XCTAssertFalse(fixture.session.worklogDeletion.isPresented)
        XCTAssertNil(fixture.session.worklogDeletion.error)
        XCTAssertEqual(fixture.session.worklogs, [oldWorklog])
    }

    @MainActor
    func testUncertainFailuresRetainTheIntent() async throws {
        let failures: [Error] = [
            TestTimeout(description: "Bridge crashed"),
            BridgeFailure(message: "Response lost", uncertain: true),
            BridgeFailure(message: "Server unavailable", kind: "unavailable"),
            BridgeFailure(message: "Malformed response", kind: "protocol"),
        ]
        for failure in failures {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            try await open(fixture)
            fixture.session.submitWorklogDeletion()
            try await preflight(fixture)
            let command = try await command(fixture)
            command.fail(failure)
            try await preflight(fixture)
            try await fixture.settled()

            XCTAssertEqual(fixture.session.worklogDeletion.error, failure.localizedDescription)
            fixture.session.cancelWorklogDeletion()
            XCTAssertEqual(fixture.session.worklogDeletion.original, precise, "\(failure)")
        }
    }

    @MainActor
    func testRejectedDeletionClearsTheIntentAndAFreshAttemptDoesNotAssumeACommit() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.session.submitWorklogDeletion()
        try await preflight(fixture)
        let command = try await command(fixture)
        command.fail(BridgeFailure(message: "Deletion failed: Storage error."))
        try await preflight(fixture)
        try await fixture.settled()

        XCTAssertEqual(fixture.session.worklogDeletion.error, "Deletion failed: Storage error.")
        XCTAssertTrue(fixture.session.worklogDeletion.canSubmit)
        fixture.session.submitWorklogDeletion()
        try await preflight(fixture, rows: [])
        try await fixture.settled()

        XCTAssertTrue(fixture.session.worklogDeletion.requiresReview)
        fixture.session.cancelWorklogDeletion()
        XCTAssertNil(fixture.session.worklogDeletion.original)
    }

    @MainActor
    func testWorklogChangedFailureRequiresReview() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.session.submitWorklogDeletion()
        try await preflight(fixture)
        let command = try await command(fixture)
        command.fail(
            BridgeFailure(message: "Worklog changed in another client", kind: "worklog_changed", requiresRefresh: true))
        try await preflight(fixture)
        try await fixture.settled()

        let state = fixture.session.worklogDeletion
        XCTAssertTrue(state.requiresReview)
        XCTAssertEqual(state.latest, precise)
        XCTAssertEqual(state.error, "This worklog changed. Review it before deleting.")
    }

    @MainActor
    func testMalformedResponseReconcilesBeforeAllowingRetry() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.session.submitWorklogDeletion()
        try await preflight(fixture)
        let command = try await command(fixture)
        command.deleted(worklog: oldWorklog)
        try await preflight(fixture)
        try await fixture.settled()

        XCTAssertEqual(
            fixture.session.worklogDeletion.error, "The deletion response does not contain the deleted worklog.")
        XCTAssertTrue(fixture.session.worklogDeletion.canSubmit)
        fixture.session.cancelWorklogDeletion()
        XCTAssertEqual(fixture.session.worklogDeletion.original, precise)
    }

    @MainActor
    func testReconciliationFailuresAreReportedAndKeepTheIntent() async throws {
        for failingStep in ["taskList", "history"] {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            try await open(fixture)
            fixture.session.submitWorklogDeletion()
            try await preflight(fixture)
            let command = try await command(fixture)
            command.fail(BridgeFailure(message: "Deletion failed: Storage error."))
            let taskList = try await fixture.client.next()
            if failingStep == "taskList" {
                taskList.fail(BridgeFailure(message: "Reconcile offline", kind: "unavailable"))
            } else {
                taskList.succeed(resources)
                let history = try await fixture.client.next()
                history.fail(BridgeFailure(message: "Reconcile offline", kind: "unavailable"))
            }
            try await fixture.settled()

            XCTAssertEqual(fixture.session.error, "Reconcile offline", failingStep)
            XCTAssertTrue(fixture.session.isStale, failingStep)
            XCTAssertEqual(fixture.session.worklogDeletion.error, "Deletion failed: Storage error.")
            fixture.session.cancelWorklogDeletion()
            XCTAssertEqual(fixture.session.worklogDeletion.original, precise, failingStep)
        }
    }

    @MainActor
    func testPreflightFailuresOnlyMarkTheConnectionStaleWhenRequested() async throws {
        let cases: [(BridgeFailure, stale: Bool, retained: Bool)] = [
            (BridgeFailure(message: "Preflight offline", kind: "unavailable"), true, true),
            (BridgeFailure(message: "Refresh required", requiresRefresh: true), true, false),
            (BridgeFailure(message: "Preflight rejected"), false, false),
        ]
        for (failure, stale, retained) in cases {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            try await open(fixture)
            fixture.session.submitWorklogDeletion()
            let taskList = try await fixture.client.next()
            taskList.fail(failure)
            try await fixture.settled()

            XCTAssertEqual(fixture.session.isStale, stale, failure.message)
            XCTAssertEqual(fixture.session.worklogDeletion.error, failure.message)
            XCTAssertEqual(deletions(fixture), 0)
            fixture.session.cancelWorklogDeletion()
            XCTAssertEqual(fixture.session.worklogDeletion.original != nil, retained, failure.message)
        }
    }

    // MARK: - Sleep

    @MainActor
    func testSleepDefersEachPreflightStepUntilWake() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await open(fixture)
        fixture.session.submitWorklogDeletion()
        let taskList = try await fixture.client.next()
        fixture.session.sleep()
        taskList.succeed(resources)
        try await fixture.settled()
        XCTAssertTrue(fixture.session.worklogDeletion.isSubmitting)
        XCTAssertTrue(fixture.session.isBlockingControls)

        fixture.session.wake()
        let retriedTaskList = try await fixture.client.next()
        XCTAssertEqual(retriedTaskList.operation, .taskList)
        retriedTaskList.succeed(resources)
        let history = try await fixture.client.next()
        fixture.session.sleep()
        history.succeed(HistoryPage(worklogs: [precise], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertEqual(deletions(fixture), 0)

        fixture.session.wake()
        try await preflight(fixture)
        let command = try await command(fixture)
        command.deleted(worklog: precise, resources: resources)
        try await finishHistory(fixture)
        XCTAssertFalse(fixture.session.worklogDeletion.isPresented)
    }

    @MainActor
    func testSleepDuringReconciliationKeepsTheIntentForRetry() async throws {
        for sleepsDuringHistory in [false, true] {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            try await open(fixture)
            fixture.session.submitWorklogDeletion()
            try await preflight(fixture)
            let command = try await command(fixture)
            command.fail(BridgeFailure(message: "Deletion failed: Storage error."))
            let taskList = try await fixture.client.next()
            if sleepsDuringHistory {
                taskList.succeed(resources)
                let history = try await fixture.client.next()
                fixture.session.sleep()
                history.succeed(HistoryPage(worklogs: [precise], nextCursor: nil, reset: false))
            } else {
                fixture.session.sleep()
                taskList.succeed(resources)
            }
            try await fixture.settled()

            let state = fixture.session.worklogDeletion
            XCTAssertFalse(state.isSubmitting, "\(sleepsDuringHistory)")
            XCTAssertEqual(state.error, "Deletion failed: Storage error.")
            fixture.session.cancelWorklogDeletion()
            XCTAssertEqual(fixture.session.worklogDeletion.original, precise)
        }
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
        XCTAssertEqual(changes.count, 2)
        fixture.session.cancelTaskCreation()
        XCTAssertEqual(changes.count, 3)

        fixture.session.openWorklogDeletion(worklogID: oldWorklog.id)
        XCTAssertEqual(changes.count, 4)
        XCTAssertEqual(harness.presentation.worklogDeletionSheetContent.original, oldWorklog)
    }

    // MARK: - State

    @MainActor
    func testStateStartsClosedAndKeepsTheFirstWorklogWhileOpen() {
        let state = WorklogDeletionState()
        XCTAssertFalse(state.presentation.canSubmit)
        XCTAssertFalse(state.blocksConnectionChange)
        XCTAssertFalse(state.submit())
        XCTAssertNil(state.takePendingIntent())

        state.open(precise, taskName: firstTask.name, historyPageLimit: 3)
        state.open(oldWorklog, taskName: secondTask.name, historyPageLimit: 3)

        XCTAssertEqual(state.presentation.original, precise)
        XCTAssertEqual(state.presentation.taskName, firstTask.name)
        XCTAssertTrue(state.blocksConnectionChange)
        XCTAssertTrue(state.submit())
        XCTAssertFalse(state.presentation.canSubmit)
        XCTAssertFalse(state.submit())
        state.cancel()
        XCTAssertTrue(state.presentation.isPresented)
        let intent = state.takePendingIntent()
        XCTAssertEqual(intent?.expected, precise)
        XCTAssertEqual(intent?.historyPageLimit, 3)
        XCTAssertNil(state.takePendingIntent())
    }

    @MainActor
    func testRetainedIntentSurvivesDismissalAndReopensTheOriginal() {
        let state = WorklogDeletionState()
        state.open(precise, taskName: firstTask.name, historyPageLimit: 2)
        XCTAssertTrue(state.submit())
        _ = state.takePendingIntent()
        state.commandStarted()
        state.fail(BridgeFailure(message: "Response lost"), retainIntent: true)
        XCTAssertTrue(state.mayHaveCommitted)
        state.cancel()
        XCTAssertFalse(state.presentation.isPresented)
        XCTAssertTrue(state.blocksConnectionChange)

        state.open(oldWorklog, taskName: secondTask.name, historyPageLimit: 2)

        XCTAssertTrue(state.presentation.isPresented)
        XCTAssertEqual(state.presentation.original, precise)
        XCTAssertEqual(state.presentation.error, "Response lost")
        XCTAssertTrue(state.submit())
        XCTAssertEqual(state.takePendingIntent()?.expected, precise)
    }

    @MainActor
    func testFailuresAndReviewsForgetAPossibleCommit() {
        let state = WorklogDeletionState()
        state.open(precise, taskName: firstTask.name, historyPageLimit: 2)
        XCTAssertTrue(state.submit())
        state.commandStarted()
        state.fail(BridgeFailure(message: "Rejected"), retainIntent: false)
        XCTAssertFalse(state.mayHaveCommitted)
        XCTAssertNil(state.intent)

        XCTAssertTrue(state.submit())
        state.commandStarted()
        state.requireReview(latest: changed, message: "Changed")
        XCTAssertFalse(state.mayHaveCommitted)
        XCTAssertNil(state.intent)
        XCTAssertFalse(state.pending)

        state.reviewLatest(taskName: nil)
        XCTAssertTrue(state.submit())
        state.commandStarted()
        state.reset()
        XCTAssertFalse(state.mayHaveCommitted)
        XCTAssertFalse(state.blocksConnectionChange)
    }

    @MainActor
    func testReviewRejectsARunningLatestEntry() {
        let state = WorklogDeletionState()
        state.open(precise, taskName: firstTask.name, historyPageLimit: 2)
        let running = WorklogItem(id: precise.id, taskId: precise.taskId, start: precise.start, end: nil)
        state.requireReview(latest: running, message: "Changed")

        state.reviewLatest(taskName: secondTask.name)

        XCTAssertTrue(state.presentation.requiresReview)
        XCTAssertEqual(state.presentation.original, precise)
        XCTAssertEqual(state.presentation.taskName, firstTask.name)
    }

    @MainActor
    func testObservationIgnoresOtherEntriesAndSubmittedIntents() {
        let state = WorklogDeletionState()
        state.observe(changed)
        XCTAssertFalse(state.presentation.requiresReview)
        state.open(precise, taskName: firstTask.name, historyPageLimit: 2)
        state.observe(precise)
        state.observe(oldWorklog)
        XCTAssertFalse(state.presentation.requiresReview)
        XCTAssertTrue(state.submit())
        state.observe(changed)
        XCTAssertFalse(state.presentation.requiresReview)
        state.fail(BridgeFailure(message: "Response lost"), retainIntent: true)
        state.observe(changed)
        XCTAssertFalse(state.presentation.requiresReview)
        state.fail(BridgeFailure(message: "Rejected"), retainIntent: false)

        state.observe(changed)

        XCTAssertTrue(state.presentation.requiresReview)
        XCTAssertEqual(state.presentation.latest, changed)
    }

    @MainActor
    func testIntentMatchesOnlyTheSameTaskAndEquivalentTimestamps() {
        let intent = WorklogDeletionState.Intent(expected: precise, historyPageLimit: 2)
        let equivalent = WorklogItem(
            id: precise.id, taskId: precise.taskId,
            start: "2024-12-30T10:00:12.123456+01:00", end: "2024-12-30T10:00:45.654321Z")
        XCTAssertTrue(intent.matches(equivalent))
        XCTAssertFalse(intent.matches(changed))
        XCTAssertFalse(
            intent.matches(WorklogItem(id: precise.id, taskId: secondTask.id, start: precise.start, end: precise.end)))
        XCTAssertFalse(
            intent.matches(WorklogItem(id: "other", taskId: precise.taskId, start: precise.start, end: precise.end)))
        XCTAssertFalse(
            intent.matches(
                WorklogItem(id: precise.id, taskId: precise.taskId, start: oldWorklog.start, end: precise.end)))
    }

    @MainActor
    func testCancelWhileSubmittingKeepsTheDialogOpen() {
        let state = WorklogDeletionState()
        state.open(precise, taskName: firstTask.name, historyPageLimit: 2)
        XCTAssertTrue(state.submit())
        state.cancel()
        XCTAssertTrue(state.presentation.isPresented)
        XCTAssertTrue(state.presentation.isSubmitting)
        state.deferUntilWake()
        XCTAssertNotNil(state.takePendingIntent())
    }
}
