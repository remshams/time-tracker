import Foundation
import XCTest
@testable import TrackerClient

final class BulkTaskArchivingTests: XCTestCase {
    @MainActor
    private func answerPreview(_ request: FakeClient.Request, tasks: [TaskItem] = [secondTask]) throws
        -> InactiveTaskPreview
    {
        guard case .inactivePreview(let days, let at) = request.operation else {
            throw TestTimeout(description: "Expected an inactive task preview.")
        }
        let preview = InactiveTaskPreview(asOf: at, inactiveDays: days, tasks: tasks)
        request.inactivePreview(preview)
        return preview
    }

    /// Answers a preview request and waits until the session publishes its result.
    @MainActor
    @discardableResult
    private func loadPreview(
        _ fixture: Fixture, _ request: FakeClient.Request, tasks: [TaskItem] = [secondTask]
    ) async throws -> InactiveTaskPreview {
        var preview: InactiveTaskPreview?
        try await fixture.update("the inactive task preview") {
            preview = try? answerPreview(request, tasks: tasks)
        }
        guard let preview else { throw TestTimeout(description: "Expected an inactive task preview.") }
        return preview
    }

    @MainActor
    private func failPreview(_ fixture: Fixture, _ request: FakeClient.Request, _ failure: Error) async throws {
        try await fixture.update("the failed preview") {
            request.fail(failure)
        }
    }

    @MainActor
    @discardableResult
    private func openPreview(_ fixture: Fixture, tasks: [TaskItem] = [secondTask]) async throws
        -> InactiveTaskPreview
    {
        fixture.session.openBulkTaskArchiving()
        return try await loadPreview(fixture, try await fixture.client.next(), tasks: tasks)
    }

    @MainActor
    private func archivedInactive(
        _ fixture: Fixture, _ request: FakeClient.Request, count: Int, resources: TaskListResources
    ) async throws {
        try await fixture.update("the archive result") {
            request.archivedInactive(count: count, resources: resources)
        } until: {
            !fixture.session.bulkTaskArchiving.isSubmitting
        }
    }

    @MainActor
    func testSuccessfulArchiveSendsWriteThenReloadsTaskCatalog() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        let preview = try await openPreview(fixture)
        fixture.session.submitBulkTaskArchiving()
        let command = try await fixture.client.next()
        XCTAssertEqual(command.operation, .archiveInactive(preview))
        let archived = TaskItem(id: secondTask.id, name: secondTask.name, archived: true, latestStart: nil)
        try await archivedInactive(
            fixture, command, count: 1, resources: TaskListResources(tasks: [firstTask, archived], active: nil))

        XCTAssertEqual(fixture.session.tasks, [firstTask, archived])
        XCTAssertEqual(fixture.session.selectedTaskID, firstTask.id)
        XCTAssertEqual(fixture.session.bulkTaskArchiving.archivedCount, 1)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.isPresented)
        XCTAssertEqual(Array(fixture.client.operations.suffix(2)), [.archiveInactive(preview), .taskList])
    }

    @MainActor
    func testPreviewFailuresThatNeedReconciliationMarkTheConnectionStale() async throws {
        let failures = [
            BridgeFailure(message: "Preview unavailable", kind: "unavailable"),
            BridgeFailure(message: "Preview protocol error", kind: "protocol"),
            BridgeFailure(message: "Preview uncertain", uncertain: true),
            BridgeFailure(message: "Preview requires refresh", requiresRefresh: true),
        ]
        for failure in failures {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            try await fixture.start()
            fixture.session.openBulkTaskArchiving()
            let request = try await fixture.client.next()
            try await failPreview(fixture, request, failure)

            XCTAssertTrue(fixture.session.isStale, failure.message)
            XCTAssertEqual(fixture.session.connectionMessage, failure.message)
            XCTAssertEqual(fixture.session.bulkTaskArchiving.error, failure.message)
            XCTAssertFalse(fixture.session.bulkTaskArchiving.isLoading)
        }
    }

    @MainActor
    func testDefaultPreviewWithNoSelectedTaskAndCancel() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        XCTAssertFalse(fixture.session.canOpenBulkTaskArchiving)
        try await fixture.start()
        XCTAssertNil(fixture.session.selectedTaskID)
        fixture.session.openBulkTaskArchiving()
        XCTAssertEqual(fixture.session.bulkTaskArchiving.daysText, "14")
        XCTAssertTrue(fixture.session.bulkTaskArchiving.isLoading)
        XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
        let request = try await fixture.client.next()
        XCTAssertEqual(request.operation, .inactivePreview(days: 14, at: "2025-01-01T00:00:00.000Z"))
        try await loadPreview(fixture, request)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.canSubmit)
        fixture.session.cancelBulkTaskArchiving()
        fixture.session.submitBulkTaskArchiving()
        XCTAssertFalse(fixture.session.bulkTaskArchiving.isPresented)
        XCTAssertEqual(fixture.client.operations.count, 2)
    }

    @MainActor
    func testEachDayChangeSendsAPreviewAndOutdatedResponsesAreIgnored() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openBulkTaskArchiving()
        let first = try await fixture.client.next()
        fixture.session.updateBulkArchiveDays("3")
        let second = try await fixture.client.next()
        fixture.session.updateBulkArchiveDays("30")
        let latest = try await fixture.client.next()
        XCTAssertEqual(first.operation, .inactivePreview(days: 14, at: "2025-01-01T00:00:00.000Z"))
        XCTAssertEqual(second.operation, .inactivePreview(days: 3, at: "2025-01-01T00:00:00.000Z"))
        XCTAssertEqual(latest.operation, .inactivePreview(days: 30, at: "2025-01-01T00:00:00.000Z"))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 3)
        try await loadPreview(fixture, first)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.tasks.isEmpty)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.isLoading)
        XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
        try await loadPreview(fixture, latest, tasks: [firstTask])
        XCTAssertEqual(fixture.session.bulkTaskArchiving.tasks, [firstTask])
        XCTAssertFalse(fixture.session.bulkTaskArchiving.isLoading)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.canSubmit)
        try await loadPreview(fixture, second, tasks: [secondTask])
        XCTAssertEqual(fixture.session.bulkTaskArchiving.tasks, [firstTask])
        XCTAssertEqual(fixture.session.bulkTaskArchiving.daysText, "30")
        XCTAssertTrue(fixture.session.bulkTaskArchiving.canSubmit)
    }

    @MainActor
    func testOutdatedPreviewFailureDoesNotReplaceTheLatestPreview() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openBulkTaskArchiving()
        let old = try await fixture.client.next()
        fixture.session.refreshBulkArchivePreview()
        let latest = try await fixture.client.next()
        old.fail(BridgeFailure(message: "Old preview failed", kind: "unavailable"))
        try await loadPreview(fixture, latest)
        XCTAssertNil(fixture.session.bulkTaskArchiving.error)
        XCTAssertEqual(fixture.session.bulkTaskArchiving.tasks, [secondTask])
        XCTAssertTrue(fixture.session.bulkTaskArchiving.canSubmit)
        XCTAssertFalse(fixture.session.isStale)
    }

    @MainActor
    func testRefreshKeepsCandidatesVisibleButCannotSubmitUntilResponse() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        try await openPreview(fixture)

        fixture.session.refreshBulkArchivePreview()
        let refresh = try await fixture.client.next()
        XCTAssertTrue(fixture.session.bulkTaskArchiving.isLoading)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.hasPreview)
        XCTAssertEqual(
            fixture.session.bulkTaskArchiving.tasks, [secondTask], "Refreshing must keep the displayed candidates.")
        XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
        let requests = fixture.client.operations.count
        fixture.session.submitBulkTaskArchiving()
        XCTAssertEqual(fixture.client.operations.count, requests)

        let updated = try await loadPreview(fixture, refresh, tasks: [firstTask])
        XCTAssertEqual(fixture.session.bulkTaskArchiving.tasks, [firstTask])
        XCTAssertFalse(fixture.session.bulkTaskArchiving.isLoading)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.canSubmit)
        fixture.session.submitBulkTaskArchiving()
        let submission = try await fixture.client.next()
        XCTAssertEqual(submission.operation, .archiveInactive(updated))
        try await archivedInactive(fixture, submission, count: 1, resources: emptyTaskListResources)
    }

    @MainActor
    func testRefreshingAnEmptyPreviewRetainsItsLoadedState() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openBulkTaskArchiving()
        XCTAssertFalse(fixture.session.bulkTaskArchiving.hasPreview)
        try await loadPreview(fixture, try await fixture.client.next(), tasks: [])
        XCTAssertTrue(fixture.session.bulkTaskArchiving.hasPreview)

        fixture.session.refreshBulkArchivePreview()
        let refresh = try await fixture.client.next()
        XCTAssertTrue(fixture.session.bulkTaskArchiving.hasPreview)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.tasks.isEmpty)
        XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
        try await loadPreview(fixture, refresh, tasks: [])
        XCTAssertTrue(fixture.session.bulkTaskArchiving.hasPreview)
        XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
    }

    @MainActor
    func testChangingPeriodClearsPreviouslyLoadedCandidates() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        try await openPreview(fixture)

        fixture.session.updateBulkArchiveDays("30")
        let changed = try await fixture.client.next()
        XCTAssertTrue(fixture.session.bulkTaskArchiving.tasks.isEmpty)
        XCTAssertFalse(fixture.session.bulkTaskArchiving.hasPreview)
        XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
        try await loadPreview(fixture, changed)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.hasPreview)
        fixture.session.updateBulkArchiveDays("0")
        XCTAssertFalse(fixture.session.bulkTaskArchiving.hasPreview)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.tasks.isEmpty)
        fixture.session.cancelBulkTaskArchiving()
        fixture.session.openBulkTaskArchiving()
        XCTAssertEqual(fixture.session.bulkTaskArchiving.daysText, "14")
        XCTAssertFalse(fixture.session.bulkTaskArchiving.hasPreview)
        try await loadPreview(fixture, try await fixture.client.next())
    }

    @MainActor
    func testRefreshFailureInvalidatesRetainedCandidatesAndRequiresRetry() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        try await openPreview(fixture)

        fixture.session.refreshBulkArchivePreview()
        let refresh = try await fixture.client.next()
        try await failPreview(fixture, refresh, BridgeFailure(message: "Preview unavailable"))
        XCTAssertFalse(fixture.session.bulkTaskArchiving.hasPreview)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.tasks.isEmpty)
        XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
        XCTAssertFalse(fixture.session.bulkTaskArchiving.isLoading)
        XCTAssertEqual(fixture.session.bulkTaskArchiving.error, "Preview unavailable")
        let requests = fixture.client.operations.count
        fixture.session.submitBulkTaskArchiving()
        XCTAssertEqual(fixture.client.operations.count, requests)

        fixture.session.refreshBulkArchivePreview()
        let retry = try await fixture.client.next()
        XCTAssertNil(fixture.session.bulkTaskArchiving.error)
        XCTAssertFalse(fixture.session.bulkTaskArchiving.hasPreview)
        try await loadPreview(fixture, retry)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.hasPreview)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.canSubmit)
    }

    @MainActor
    func testInvalidDaysClearPreviewAndDoNotSendRequests() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        try await openPreview(fixture)
        let count = fixture.client.operations.count
        for text in ["", "0", "-1", "+14", "1.5", " 14", "4294967296", "abc", "💠"] {
            fixture.session.updateBulkArchiveDays(text)
            XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit, text)
            XCTAssertTrue(fixture.session.bulkTaskArchiving.tasks.isEmpty, text)
            XCTAssertNotNil(fixture.session.bulkTaskArchiving.error, text)
            XCTAssertFalse(fixture.session.bulkTaskArchiving.isLoading, text)
        }
        XCTAssertEqual(fixture.client.operations.count, count)
        fixture.session.updateBulkArchiveDays("4294967295")
        let valid = try await fixture.client.next()
        guard case .inactivePreview(let days, _) = valid.operation else { return XCTFail("Expected preview") }
        XCTAssertEqual(days, Int(UInt32.max))
        try await failPreview(fixture, valid, BridgeFailure(message: "Cutoff is outside the supported date range."))
        XCTAssertNotNil(fixture.session.bulkTaskArchiving.error)
        XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
    }

    @MainActor
    func testConfirmationReusesCapturedPreviewAndReportsActualCountWithoutChangingTimer() async throws {
        for actualCount in [0, 2] {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            let resources = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
            try await fixture.start(resources)
            let preview = try await openPreview(fixture)
            fixture.clock.now.addTimeInterval(10)
            fixture.session.submitBulkTaskArchiving()
            fixture.session.submitBulkTaskArchiving()
            fixture.session.cancelBulkTaskArchiving()
            let command = try await fixture.client.next()
            XCTAssertEqual(command.operation, .archiveInactive(preview))
            XCTAssertTrue(fixture.session.bulkTaskArchiving.isSubmitting)
            XCTAssertTrue(fixture.session.bulkTaskArchiving.isPresented)
            try await archivedInactive(fixture, command, count: actualCount, resources: resources)
            XCTAssertEqual(fixture.session.bulkTaskArchiving.archivedCount, actualCount)
            XCTAssertEqual(fixture.session.active, activeWorklog)
            XCTAssertEqual(fixture.session.selectedTaskID, firstTask.id)
            XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
            XCTAssertEqual(fixture.client.maximumOutstandingRequests, 1)
            XCTAssertEqual(
                fixture.client.operations.filter {
                    if case .archiveInactive = $0 { return true }; return false
                }.count, 1)
        }
    }

    @MainActor
    func testConfirmIsDisabledWhileSubmittingAndOtherFeaturesStayUsable() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: nil)
        try await fixture.start(resources)
        let preview = try await openPreview(fixture)
        fixture.session.submitBulkTaskArchiving()
        let command = try await fixture.client.next()
        XCTAssertEqual(command.operation, .archiveInactive(preview))
        XCTAssertTrue(fixture.session.bulkTaskArchiving.isSubmitting)
        XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
        let requests = fixture.client.operations.count
        fixture.session.updateBulkArchiveDays("30")
        fixture.session.refreshBulkArchivePreview()
        fixture.session.cancelBulkTaskArchiving()
        XCTAssertEqual(fixture.client.operations.count, requests)
        XCTAssertEqual(fixture.session.bulkTaskArchiving.daysText, "14")
        XCTAssertTrue(fixture.session.bulkTaskArchiving.isPresented)
        XCTAssertFalse(fixture.session.isBlockingControls)
        XCTAssertTrue(fixture.session.canOpenTaskCreation)
        XCTAssertTrue(fixture.session.canOpenTaskRename)
        XCTAssertTrue(fixture.session.canOpenWorklogCorrection)
        XCTAssertTrue(fixture.session.canOpenWorklogMove)
        XCTAssertTrue(fixture.session.canOpenWorklogDeletion)
        XCTAssertTrue(fixture.session.canArchiveTask(taskID: secondTask.id))
        XCTAssertTrue(fixture.session.canUnarchiveTask(taskID: archivedTask.id))
        XCTAssertTrue(fixture.session.canStartTracking(taskID: firstTask.id))
        fixture.session.startTracking(taskID: firstTask.id)
        let start = try await fixture.client.next()
        XCTAssertEqual(start.operation, .start(task: firstTask.id, expected: nil, at: "2025-01-01T00:00:00.000Z"))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 2)
        start.succeed(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: activeWorklog))
        let startedHistory = try await fixture.client.next()
        XCTAssertEqual(startedHistory.operation, .history(task: firstTask.id, cursor: nil))
        startedHistory.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.active, activeWorklog)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.isSubmitting)
        let archived = TaskItem(id: secondTask.id, name: secondTask.name, archived: true, latestStart: nil)
        try await archivedInactive(
            fixture, command, count: 1,
            resources: TaskListResources(tasks: [firstTask, archived, archivedTask], active: activeWorklog))
        XCTAssertEqual(fixture.session.bulkTaskArchiving.archivedCount, 1)
        XCTAssertEqual(fixture.session.tasks, [firstTask, archived, archivedTask])
        XCTAssertEqual(fixture.session.active, activeWorklog)
    }

    @MainActor
    func testEmptyPreviewAndFailedPreviewDisableConfirmation() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        try await openPreview(fixture, tasks: [])
        XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
        fixture.session.refreshBulkArchivePreview()
        let request = try await fixture.client.next()
        try await failPreview(fixture, request, BridgeFailure(message: "Offline", kind: "unavailable"))
        XCTAssertEqual(fixture.session.bulkTaskArchiving.error, "Offline")
        XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
    }

    @MainActor
    func testFailedSubmissionShowsMessageReloadsAndRequiresNewPreview() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        try await openPreview(fixture)
        fixture.session.submitBulkTaskArchiving()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Preview is outdated", kind: "conflict"))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .taskList)
        XCTAssertFalse(fixture.session.bulkTaskArchiving.isSubmitting)
        try await fixture.update("the reloaded task list") {
            reload.succeed(resources)
        }
        XCTAssertTrue(fixture.session.bulkTaskArchiving.isPresented)
        XCTAssertEqual(
            fixture.session.bulkTaskArchiving.error, "Preview is outdated Refresh the preview before trying again.")
        XCTAssertNil(fixture.session.bulkTaskArchiving.archivedCount)
        XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.tasks.isEmpty)
        XCTAssertFalse(fixture.session.isStale)
        let count = fixture.client.operations.count
        fixture.session.submitBulkTaskArchiving()
        XCTAssertEqual(fixture.client.operations.count, count)
        fixture.session.refreshBulkArchivePreview()
        let refreshed = try await fixture.client.next()
        XCTAssertEqual(refreshed.operation, .inactivePreview(days: 14, at: "2025-01-01T00:00:00.000Z"))
        try await loadPreview(fixture, refreshed)
        XCTAssertNil(fixture.session.bulkTaskArchiving.error)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.canSubmit)
    }

    @MainActor
    func testUncertainSubmissionFailureAsksTheUserToCheckTheReloadedData() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        try await openPreview(fixture)
        fixture.session.submitBulkTaskArchiving()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Response lost", kind: "unavailable", uncertain: true))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .taskList)
        try await fixture.update("the reloaded task list") {
            reload.succeed(emptyTaskListResources)
        }
        XCTAssertEqual(
            fixture.session.bulkTaskArchiving.error,
            "Could not confirm the change. Check the reloaded data before trying again. Response lost"
                + " Refresh the preview before trying again.")
        XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.tasks.isEmpty)
    }

    @MainActor
    func testOpenDialogDoesNotBlockOtherEditorsTrackingOrConnections() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: activeWorklog))
        try await openPreview(fixture)
        XCTAssertFalse(fixture.session.isBlockingControls)
        XCTAssertTrue(fixture.session.canOpenTaskCreation)
        XCTAssertTrue(fixture.session.canOpenTaskRename)
        XCTAssertTrue(fixture.session.canOpenWorklogCorrection)
        XCTAssertTrue(fixture.session.canOpenWorklogMove)
        XCTAssertTrue(fixture.session.canArchiveTask(taskID: secondTask.id))
        XCTAssertTrue(fixture.session.canUnarchiveTask(taskID: archivedTask.id))
        XCTAssertTrue(fixture.session.canStartTracking(taskID: secondTask.id))
        XCTAssertTrue(fixture.session.canStopTracking)
        fixture.session.openTaskArchive(taskID: secondTask.id)
        XCTAssertTrue(fixture.session.taskArchiving.isPresented)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.isPresented)
        fixture.session.cancelTaskArchiving()

        let testing = Task { try await fixture.session.testConnection(.local) }
        let test = try await fixture.client.next()
        XCTAssertEqual(test.operation, .test(.local))
        test.tested()
        try await fixture.taskValue(testing)
        let connecting = Task { await fixture.session.connect(serverSettings) }
        let connection = try await fixture.client.next()
        XCTAssertEqual(connection.operation, .connect(serverSettings))
        connection.succeed(emptyTaskListResources)
        let connected = try await fixture.taskValue(connecting)
        XCTAssertTrue(connected)
        XCTAssertEqual(fixture.session.connectionSettings, serverSettings)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.isPresented)
    }

    @MainActor
    func testCancelIgnoresLateResponses() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openBulkTaskArchiving()
        let request = try await fixture.client.next()
        fixture.session.cancelBulkTaskArchiving()
        try await loadPreview(fixture, request)
        XCTAssertFalse(fixture.session.bulkTaskArchiving.isPresented)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.tasks.isEmpty)
        XCTAssertFalse(fixture.session.bulkTaskArchiving.isLoading)
        fixture.session.openBulkTaskArchiving()
        let reopened = try await fixture.client.next()
        try await loadPreview(fixture, reopened)
        XCTAssertEqual(fixture.session.bulkTaskArchiving.tasks, [secondTask])
    }

    @MainActor
    func testPreviewIsSentWhileAPollRuns() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.scheduler.poll?.fire()
        let refresh = try await fixture.client.next()
        fixture.session.openBulkTaskArchiving()
        XCTAssertTrue(fixture.session.bulkTaskArchiving.isLoading)
        let preview = try await fixture.client.next()
        XCTAssertEqual(preview.operation, .inactivePreview(days: 14, at: "2025-01-01T00:00:00.000Z"))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 2)
        try await loadPreview(fixture, preview)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.canSubmit)
        refresh.succeed(emptyTaskListResources)
        try await fixture.settled()
        XCTAssertTrue(fixture.session.bulkTaskArchiving.canSubmit)
    }

    @MainActor
    func testObserverRetainsSheetContentWhileDismissalAnimates() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        let harness = PresentationHarness(session: fixture.session)
        let updates = harness.counter(PresentationReads.bulkTaskArchiving)
        try await openPreview(fixture)
        harness.update()
        harness.attach()
        let content = harness.presentation.bulkTaskArchivingSheetContent
        fixture.session.cancelBulkTaskArchiving()
        XCTAssertFalse(fixture.session.bulkTaskArchiving.isPresented)
        XCTAssertEqual(harness.presentation.bulkTaskArchivingSheetContent, content)
        XCTAssertGreaterThan(updates.count, 1)
    }

    @MainActor
    func testInvalidPreviewResponsesCannotBeConfirmed() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openBulkTaskArchiving()
        let request = try await fixture.client.next()
        try await fixture.update("the invalid preview") {
            request.inactivePreview(InactiveTaskPreview(asOf: "invalid", inactiveDays: 14, tasks: [secondTask]))
        }
        XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
        XCTAssertEqual(fixture.session.bulkTaskArchiving.error, "The tracker returned an invalid archive preview.")
        XCTAssertTrue(fixture.session.isStale)
        fixture.session.refreshBulkArchivePreview()
        let duplicate = try await fixture.client.next()
        XCTAssertEqual(duplicate.operation, .inactivePreview(days: 14, at: "2025-01-01T00:00:00.000Z"))
        try await loadPreview(fixture, duplicate, tasks: [secondTask, secondTask])
        XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
        fixture.session.refreshBulkArchivePreview()
        let archived = try await fixture.client.next()
        try await loadPreview(fixture, archived, tasks: [archivedTask])
        XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
        XCTAssertNotNil(fixture.session.bulkTaskArchiving.error)
    }

    @MainActor
    func testBulkDialogOpensWhileOtherEditorsAreOpen() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        fixture.session.openWorklogCorrection(worklogID: activeWorklog.id)
        fixture.session.openTaskCreation()
        fixture.session.openTaskArchive(taskID: secondTask.id)
        XCTAssertTrue(fixture.session.canOpenBulkTaskArchiving)
        try await openPreview(fixture)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.isPresented)
        XCTAssertTrue(fixture.session.worklogCorrection.isPresented)
        XCTAssertTrue(fixture.session.taskCreation.isPresented)
        XCTAssertTrue(fixture.session.taskArchiving.isPresented)
    }

    @MainActor
    func testSleepDuringSubmissionAcceptsResultWithoutRepeatingCommand() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        try await openPreview(fixture)
        fixture.session.submitBulkTaskArchiving()
        let command = try await fixture.client.next()
        fixture.session.sleep()
        XCTAssertTrue(fixture.session.bulkTaskArchiving.isSubmitting)
        try await archivedInactive(fixture, command, count: 1, resources: emptyTaskListResources)
        XCTAssertEqual(fixture.session.bulkTaskArchiving.archivedCount, 1)
        XCTAssertFalse(fixture.session.bulkTaskArchiving.isSubmitting)
        let writes = fixture.client.operations.filter {
            if case .archiveInactive = $0 { return true }; return false
        }
        XCTAssertEqual(writes.count, 1)
    }

    @MainActor
    func testFailedReloadAfterFailedSubmissionMarksConnectionStaleAndPreviewLoadsWithoutRefresh() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        try await openPreview(fixture)
        fixture.session.submitBulkTaskArchiving()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Archive rejected", kind: "validation"))
        let reload = try await fixture.client.next()
        try await fixture.update("the failed reload") {
            reload.fail(BridgeFailure(message: "Offline", kind: "unavailable"))
        } until: {
            fixture.session.isStale
        }
        XCTAssertEqual(fixture.session.connectionMessage, "Offline")
        XCTAssertEqual(
            fixture.session.bulkTaskArchiving.error, "Archive rejected Refresh the preview before trying again.")
        fixture.session.refreshBulkArchivePreview()
        let preview = try await fixture.client.next()
        XCTAssertEqual(preview.operation, .inactivePreview(days: 14, at: "2025-01-01T00:00:00.000Z"))
        try await loadPreview(fixture, preview)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.canSubmit)
    }
}
