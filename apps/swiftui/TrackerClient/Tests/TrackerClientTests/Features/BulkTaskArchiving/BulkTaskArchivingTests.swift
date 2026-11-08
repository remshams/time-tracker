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

    /// A preview doesn't publish, so `Fixture.update` can't wait for it.
    @MainActor
    private func finishedLoading(_ fixture: Fixture) async throws {
        let finished = Task { @MainActor in
            while fixture.bulkTaskArchiving.isLoading && !Task.isCancelled { await Task.yield() }
        }
        try await fixture.taskValue(finished)
    }

    /// Answers the latest preview request and waits until the store applies the result.
    @MainActor
    @discardableResult
    private func loadPreview(
        _ fixture: Fixture, _ request: FakeClient.Request, tasks: [TaskItem] = [secondTask]
    ) async throws -> InactiveTaskPreview {
        let preview = try answerPreview(request, tasks: tasks)
        try await finishedLoading(fixture)
        return preview
    }

    @MainActor
    private func failPreview(_ fixture: Fixture, _ request: FakeClient.Request, _ failure: Error) async throws {
        request.fail(failure)
        try await finishedLoading(fixture)
    }

    /// Answers a preview the store must ignore. The empty task runs after the reply, so the next assertion sees
    /// the store after it handled the reply.
    @MainActor
    private func answerOutdated(_ request: FakeClient.Request) async throws {
        _ = try answerPreview(request)
        await Task { @MainActor in }.value
    }

    @MainActor
    @discardableResult
    private func openPreview(_ fixture: Fixture, tasks: [TaskItem] = [secondTask]) async throws
        -> InactiveTaskPreview
    {
        fixture.bulkTaskArchiving.open()
        return try await loadPreview(fixture, try await fixture.client.next(), tasks: tasks)
    }

    @MainActor
    private func archivedInactive(
        _ fixture: Fixture, _ request: FakeClient.Request, count: Int, resources: TaskListResources
    ) async throws {
        try await fixture.update("the archive result") {
            request.archivedInactive(count: count, resources: resources)
        } until: {
            !fixture.bulkTaskArchiving.isSubmitting
        }
    }

    @MainActor
    func testSuccessfulArchiveSendsWriteThenReloadsTaskCatalog() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        let preview = try await openPreview(fixture)
        fixture.bulkTaskArchiving.submit()
        let command = try await fixture.client.next()
        XCTAssertEqual(command.operation, .archiveInactive(preview))
        let archived = TaskItem(id: secondTask.id, name: secondTask.name, archived: true, latestStart: nil)
        try await archivedInactive(
            fixture, command, count: 1, resources: TaskListResources(tasks: [firstTask, archived], active: nil))

        XCTAssertEqual(fixture.session.tasks, [firstTask, archived])
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, firstTask.id)
        XCTAssertEqual(fixture.bulkTaskArchiving.archivedCount, 1)
        XCTAssertFalse(fixture.bulkTaskArchiving.hasPreview)
        XCTAssertTrue(fixture.bulkTaskArchiving.isPresented)
        XCTAssertEqual(
            Array(fixture.client.operations.suffix(2)),
            [.archiveInactive(preview), .refresh(fixture.session.connection.settings)])
    }

    @MainActor
    func testPreviewFailuresThatNeedReconciliationMarkTheConnectionStale() async throws {
        let failures = [
            BridgeFailure(message: "Preview unavailable", kind: "unavailable"),
            BridgeFailure(message: "Preview protocol error", kind: "protocol"),
            BridgeFailure(message: "Preview requires refresh", requiresRefresh: true),
        ]
        for failure in failures {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            try await fixture.start()
            fixture.bulkTaskArchiving.open()
            let request = try await fixture.client.next()
            try await failPreview(fixture, request, failure)

            XCTAssertTrue(fixture.session.connection.stale, failure.message)
            XCTAssertEqual(fixture.session.connection.message, failure.message)
            XCTAssertEqual(fixture.bulkTaskArchiving.error, failure.message)
            XCTAssertFalse(fixture.bulkTaskArchiving.isLoading)
        }
    }

    @MainActor
    func testDefaultPreviewWithNoSelectedTaskAndCancel() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        XCTAssertFalse(fixture.bulkTaskArchiving.canOpen)
        try await fixture.start()
        XCTAssertNil(fixture.session.taskCatalog.selectedTaskID)
        fixture.bulkTaskArchiving.open()
        XCTAssertEqual(fixture.bulkTaskArchiving.daysText, "14")
        XCTAssertTrue(fixture.bulkTaskArchiving.isLoading)
        XCTAssertFalse(fixture.bulkTaskArchiving.canSubmit)
        let request = try await fixture.client.next()
        XCTAssertEqual(request.operation, .inactivePreview(days: 14, at: "2025-01-01T00:00:00.000Z"))
        try await loadPreview(fixture, request)
        XCTAssertTrue(fixture.bulkTaskArchiving.canSubmit)
        fixture.bulkTaskArchiving.cancel()
        fixture.bulkTaskArchiving.submit()
        XCTAssertFalse(fixture.bulkTaskArchiving.isPresented)
        XCTAssertEqual(fixture.client.operations.count, 2)
    }

    @MainActor
    func testEachDayChangeSendsAPreviewAndOutdatedResponsesAreIgnored() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.bulkTaskArchiving.open()
        let first = try await fixture.client.next()
        fixture.bulkTaskArchiving.setDays("3")
        XCTAssertEqual(fixture.bulkTaskArchiving.daysText, "3")
        XCTAssertTrue(fixture.bulkTaskArchiving.isLoading)
        let second = try await fixture.client.next()
        fixture.bulkTaskArchiving.setDays("30")
        let latest = try await fixture.client.next()
        XCTAssertEqual(first.operation, .inactivePreview(days: 14, at: "2025-01-01T00:00:00.000Z"))
        XCTAssertEqual(second.operation, .inactivePreview(days: 3, at: "2025-01-01T00:00:00.000Z"))
        XCTAssertEqual(latest.operation, .inactivePreview(days: 30, at: "2025-01-01T00:00:00.000Z"))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 3)
        try await answerOutdated(first)
        XCTAssertTrue(fixture.bulkTaskArchiving.tasks.isEmpty)
        XCTAssertTrue(fixture.bulkTaskArchiving.isLoading)
        XCTAssertFalse(fixture.bulkTaskArchiving.canSubmit)
        try await loadPreview(fixture, latest, tasks: [firstTask])
        XCTAssertEqual(fixture.bulkTaskArchiving.tasks, [firstTask])
        XCTAssertFalse(fixture.bulkTaskArchiving.isLoading)
        XCTAssertTrue(fixture.bulkTaskArchiving.canSubmit)
        try await answerOutdated(second)
        XCTAssertEqual(fixture.bulkTaskArchiving.tasks, [firstTask])
        XCTAssertEqual(fixture.bulkTaskArchiving.daysText, "30")
        XCTAssertTrue(fixture.bulkTaskArchiving.canSubmit)
    }

    @MainActor
    func testOutdatedPreviewFailureDoesNotReplaceTheLatestPreview() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.bulkTaskArchiving.open()
        let old = try await fixture.client.next()
        fixture.bulkTaskArchiving.refresh()
        let latest = try await fixture.client.next()
        old.fail(BridgeFailure(message: "Old preview failed", kind: "unavailable"))
        try await loadPreview(fixture, latest)
        XCTAssertNil(fixture.bulkTaskArchiving.error)
        XCTAssertEqual(fixture.bulkTaskArchiving.tasks, [secondTask])
        XCTAssertTrue(fixture.bulkTaskArchiving.canSubmit)
        XCTAssertFalse(fixture.session.connection.stale)
    }

    @MainActor
    func testRefreshKeepsCandidatesVisibleButCannotSubmitUntilResponse() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        try await openPreview(fixture)

        fixture.bulkTaskArchiving.refresh()
        let refresh = try await fixture.client.next()
        XCTAssertTrue(fixture.bulkTaskArchiving.isLoading)
        XCTAssertTrue(fixture.bulkTaskArchiving.hasPreview)
        XCTAssertEqual(
            fixture.bulkTaskArchiving.tasks, [secondTask], "Refreshing must keep the displayed candidates.")
        XCTAssertFalse(fixture.bulkTaskArchiving.canSubmit)
        let requests = fixture.client.operations.count
        fixture.bulkTaskArchiving.submit()
        XCTAssertEqual(fixture.client.operations.count, requests)

        let updated = try await loadPreview(fixture, refresh, tasks: [firstTask])
        XCTAssertEqual(fixture.bulkTaskArchiving.tasks, [firstTask])
        XCTAssertFalse(fixture.bulkTaskArchiving.isLoading)
        XCTAssertTrue(fixture.bulkTaskArchiving.canSubmit)
        fixture.bulkTaskArchiving.submit()
        let submission = try await fixture.client.next()
        XCTAssertEqual(submission.operation, .archiveInactive(updated))
        try await archivedInactive(fixture, submission, count: 1, resources: emptyTaskListResources)
    }

    @MainActor
    func testRefreshingAnEmptyPreviewRetainsItsLoadedState() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.bulkTaskArchiving.open()
        XCTAssertFalse(fixture.bulkTaskArchiving.hasPreview)
        try await loadPreview(fixture, try await fixture.client.next(), tasks: [])
        XCTAssertTrue(fixture.bulkTaskArchiving.hasPreview)

        fixture.bulkTaskArchiving.refresh()
        let refresh = try await fixture.client.next()
        XCTAssertTrue(fixture.bulkTaskArchiving.hasPreview)
        XCTAssertTrue(fixture.bulkTaskArchiving.tasks.isEmpty)
        XCTAssertFalse(fixture.bulkTaskArchiving.canSubmit)
        try await loadPreview(fixture, refresh, tasks: [])
        XCTAssertTrue(fixture.bulkTaskArchiving.hasPreview)
        XCTAssertFalse(fixture.bulkTaskArchiving.canSubmit)
    }

    @MainActor
    func testChangingPeriodClearsPreviouslyLoadedCandidates() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        try await openPreview(fixture)

        fixture.bulkTaskArchiving.setDays("30")
        let changed = try await fixture.client.next()
        XCTAssertTrue(fixture.bulkTaskArchiving.tasks.isEmpty)
        XCTAssertFalse(fixture.bulkTaskArchiving.hasPreview)
        XCTAssertFalse(fixture.bulkTaskArchiving.canSubmit)
        try await loadPreview(fixture, changed)
        XCTAssertTrue(fixture.bulkTaskArchiving.hasPreview)
        fixture.bulkTaskArchiving.setDays("0")
        XCTAssertFalse(fixture.bulkTaskArchiving.hasPreview)
        XCTAssertTrue(fixture.bulkTaskArchiving.tasks.isEmpty)
        fixture.bulkTaskArchiving.cancel()
        fixture.bulkTaskArchiving.open()
        XCTAssertEqual(fixture.bulkTaskArchiving.daysText, "14")
        XCTAssertFalse(fixture.bulkTaskArchiving.hasPreview)
        try await loadPreview(fixture, try await fixture.client.next())
    }

    @MainActor
    func testRefreshFailureInvalidatesRetainedCandidatesAndRequiresRetry() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        try await openPreview(fixture)

        fixture.bulkTaskArchiving.refresh()
        let refresh = try await fixture.client.next()
        try await failPreview(fixture, refresh, BridgeFailure(message: "Preview unavailable"))
        XCTAssertFalse(fixture.bulkTaskArchiving.hasPreview)
        XCTAssertTrue(fixture.bulkTaskArchiving.tasks.isEmpty)
        XCTAssertFalse(fixture.bulkTaskArchiving.canSubmit)
        XCTAssertFalse(fixture.bulkTaskArchiving.isLoading)
        XCTAssertEqual(fixture.bulkTaskArchiving.error, "Preview unavailable")
        let requests = fixture.client.operations.count
        fixture.bulkTaskArchiving.submit()
        XCTAssertEqual(fixture.client.operations.count, requests)

        fixture.bulkTaskArchiving.refresh()
        let retry = try await fixture.client.next()
        XCTAssertNil(fixture.bulkTaskArchiving.error)
        XCTAssertFalse(fixture.bulkTaskArchiving.hasPreview)
        try await loadPreview(fixture, retry)
        XCTAssertTrue(fixture.bulkTaskArchiving.hasPreview)
        XCTAssertTrue(fixture.bulkTaskArchiving.canSubmit)
    }

    @MainActor
    func testInvalidDaysClearPreviewAndDoNotSendRequests() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        try await openPreview(fixture)
        let count = fixture.client.operations.count
        for text in ["", "0", "-1", "+14", "1.5", " 14", "4294967296", "abc", "💠"] {
            fixture.bulkTaskArchiving.setDays(text)
            XCTAssertFalse(fixture.bulkTaskArchiving.canSubmit, text)
            XCTAssertTrue(fixture.bulkTaskArchiving.tasks.isEmpty, text)
            XCTAssertNotNil(fixture.bulkTaskArchiving.error, text)
            XCTAssertFalse(fixture.bulkTaskArchiving.isLoading, text)
        }
        XCTAssertEqual(fixture.client.operations.count, count)
        fixture.bulkTaskArchiving.setDays("4294967295")
        let valid = try await fixture.client.next()
        guard case .inactivePreview(let days, _) = valid.operation else { return XCTFail("Expected preview") }
        XCTAssertEqual(days, Int(UInt32.max))
        try await failPreview(fixture, valid, BridgeFailure(message: "Cutoff is outside the supported date range."))
        XCTAssertNotNil(fixture.bulkTaskArchiving.error)
        XCTAssertFalse(fixture.bulkTaskArchiving.canSubmit)
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
            fixture.bulkTaskArchiving.submit()
            fixture.bulkTaskArchiving.submit()
            fixture.bulkTaskArchiving.cancel()
            let command = try await fixture.client.next()
            XCTAssertEqual(command.operation, .archiveInactive(preview))
            XCTAssertTrue(fixture.bulkTaskArchiving.isSubmitting)
            XCTAssertTrue(fixture.bulkTaskArchiving.isPresented)
            try await archivedInactive(fixture, command, count: actualCount, resources: resources)
            XCTAssertEqual(fixture.bulkTaskArchiving.archivedCount, actualCount)
            XCTAssertEqual(fixture.session.active, activeWorklog)
            XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, firstTask.id)
            XCTAssertFalse(fixture.bulkTaskArchiving.canSubmit)
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
        fixture.bulkTaskArchiving.submit()
        let command = try await fixture.client.next()
        XCTAssertEqual(command.operation, .archiveInactive(preview))
        XCTAssertTrue(fixture.bulkTaskArchiving.isSubmitting)
        XCTAssertFalse(fixture.bulkTaskArchiving.canSubmit)
        let requests = fixture.client.operations.count
        fixture.bulkTaskArchiving.setDays("30")
        fixture.bulkTaskArchiving.refresh()
        fixture.bulkTaskArchiving.cancel()
        XCTAssertEqual(fixture.client.operations.count, requests)
        XCTAssertEqual(fixture.bulkTaskArchiving.daysText, "14")
        XCTAssertTrue(fixture.bulkTaskArchiving.isPresented)
        XCTAssertTrue(fixture.taskCreation.canOpen)
        XCTAssertTrue(fixture.taskRename.canOpen)
        XCTAssertTrue(fixture.session.canOpenWorklogCorrection)
        XCTAssertTrue(fixture.worklogMove.canOpen)
        XCTAssertTrue(fixture.worklogDeletion.canOpen)
        XCTAssertTrue(fixture.taskArchiving.canArchive(taskID: secondTask.id))
        XCTAssertTrue(fixture.taskArchiving.canUnarchive(taskID: archivedTask.id))
        XCTAssertTrue(fixture.trackingAvailability.canStart(taskID: firstTask.id))
        fixture.trackingActions.startTracking(taskID: firstTask.id)
        let start = try await fixture.client.next()
        XCTAssertEqual(start.operation, .start(task: firstTask.id, expected: nil, at: "2025-01-01T00:00:00.000Z"))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 2)
        start.succeed(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: activeWorklog))
        let startedHistory = try await fixture.client.next()
        XCTAssertEqual(startedHistory.operation, .history(task: firstTask.id, cursor: nil))
        try await fixture.update("the tracking history") { startedHistory.succeed(emptyPage) }
        XCTAssertEqual(fixture.session.active, activeWorklog)
        XCTAssertTrue(fixture.bulkTaskArchiving.isSubmitting)
        let archived = TaskItem(id: secondTask.id, name: secondTask.name, archived: true, latestStart: nil)
        try await archivedInactive(
            fixture, command, count: 1,
            resources: TaskListResources(tasks: [firstTask, archived, archivedTask], active: activeWorklog))
        XCTAssertEqual(fixture.bulkTaskArchiving.archivedCount, 1)
        XCTAssertEqual(fixture.session.tasks, [firstTask, archived, archivedTask])
        XCTAssertEqual(fixture.session.active, activeWorklog)
    }

    @MainActor
    func testEmptyPreviewAndFailedPreviewDisableConfirmation() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        try await openPreview(fixture, tasks: [])
        XCTAssertFalse(fixture.bulkTaskArchiving.canSubmit)
        fixture.bulkTaskArchiving.refresh()
        let request = try await fixture.client.next()
        try await failPreview(fixture, request, BridgeFailure(message: "Offline", kind: "unavailable"))
        XCTAssertEqual(fixture.bulkTaskArchiving.error, "Offline")
        XCTAssertFalse(fixture.bulkTaskArchiving.canSubmit)
    }

    @MainActor
    func testFailedSubmissionShowsMessageReloadsAndRequiresNewPreview() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        try await openPreview(fixture)
        fixture.bulkTaskArchiving.submit()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Preview is outdated", kind: "conflict"))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connection.settings))
        XCTAssertFalse(fixture.bulkTaskArchiving.isSubmitting)
        try await fixture.update("the reloaded task list") {
            reload.succeed(resources)
        }
        XCTAssertTrue(fixture.bulkTaskArchiving.isPresented)
        XCTAssertEqual(
            fixture.bulkTaskArchiving.error, "Preview is outdated Refresh the preview before trying again.")
        XCTAssertNil(fixture.bulkTaskArchiving.archivedCount)
        XCTAssertFalse(fixture.bulkTaskArchiving.canSubmit)
        XCTAssertTrue(fixture.bulkTaskArchiving.tasks.isEmpty)
        XCTAssertFalse(fixture.session.connection.stale)
        let count = fixture.client.operations.count
        fixture.bulkTaskArchiving.submit()
        XCTAssertEqual(fixture.client.operations.count, count)
        fixture.bulkTaskArchiving.refresh()
        let refreshed = try await fixture.client.next()
        XCTAssertEqual(refreshed.operation, .inactivePreview(days: 14, at: "2025-01-01T00:00:00.000Z"))
        try await loadPreview(fixture, refreshed)
        XCTAssertNil(fixture.bulkTaskArchiving.error)
        XCTAssertTrue(fixture.bulkTaskArchiving.canSubmit)
    }

    @MainActor
    func testUncertainSubmissionFailureAsksTheUserToCheckTheReloadedData() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        try await openPreview(fixture)
        fixture.bulkTaskArchiving.submit()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Response lost", kind: "unavailable", uncertain: true))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connection.settings))
        try await fixture.update("the reloaded task list") {
            reload.succeed(emptyTaskListResources)
        }
        XCTAssertEqual(
            fixture.bulkTaskArchiving.error,
            "Could not confirm the change. Check the reloaded data before trying again. Response lost"
                + " Refresh the preview before trying again.")
        XCTAssertFalse(fixture.bulkTaskArchiving.canSubmit)
        XCTAssertTrue(fixture.bulkTaskArchiving.tasks.isEmpty)
    }

    @MainActor
    func testOpenDialogDoesNotBlockOtherEditorsTrackingOrConnections() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: activeWorklog))
        try await openPreview(fixture)
        XCTAssertTrue(fixture.taskCreation.canOpen)
        XCTAssertTrue(fixture.taskRename.canOpen)
        XCTAssertTrue(fixture.session.canOpenWorklogCorrection)
        XCTAssertTrue(fixture.worklogMove.canOpen)
        XCTAssertTrue(fixture.taskArchiving.canArchive(taskID: secondTask.id))
        XCTAssertTrue(fixture.taskArchiving.canUnarchive(taskID: archivedTask.id))
        XCTAssertTrue(fixture.trackingAvailability.canStart(taskID: secondTask.id))
        XCTAssertTrue(fixture.trackingAvailability.canStop)
        fixture.taskArchiving.openArchive(taskID: secondTask.id)
        XCTAssertTrue(fixture.taskArchiving.isPresented)
        XCTAssertTrue(fixture.bulkTaskArchiving.isPresented)
        fixture.taskArchiving.cancel()

        let testing = Task { try await testConnectionSettings(.local, client: fixture.client) }
        let test = try await fixture.client.next()
        XCTAssertEqual(test.operation, .test(.local))
        test.tested()
        try await fixture.taskValue(testing)
        let connecting = Task { await fixture.sync.connect(serverSettings) }
        let connection = try await fixture.client.next()
        XCTAssertEqual(connection.operation, .connect(serverSettings))
        connection.succeed(emptyTaskListResources)
        let connected = try await fixture.taskValue(connecting)
        XCTAssertTrue(connected)
        XCTAssertEqual(fixture.session.connection.settings, serverSettings)
        XCTAssertTrue(fixture.bulkTaskArchiving.isPresented)
    }

    @MainActor
    func testCancelIgnoresLateResponses() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.bulkTaskArchiving.open()
        let request = try await fixture.client.next()
        fixture.bulkTaskArchiving.cancel()
        try await answerOutdated(request)
        XCTAssertFalse(fixture.bulkTaskArchiving.isPresented)
        XCTAssertTrue(fixture.bulkTaskArchiving.tasks.isEmpty)
        XCTAssertFalse(fixture.bulkTaskArchiving.isLoading)
        fixture.bulkTaskArchiving.open()
        let reopened = try await fixture.client.next()
        try await loadPreview(fixture, reopened)
        XCTAssertEqual(fixture.bulkTaskArchiving.tasks, [secondTask])
    }

    @MainActor
    func testPreviewIsSentWhileAPollRuns() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.scheduler.poll?.fire()
        let refresh = try await fixture.client.next()
        fixture.bulkTaskArchiving.open()
        XCTAssertTrue(fixture.bulkTaskArchiving.isLoading)
        let preview = try await fixture.client.next()
        XCTAssertEqual(preview.operation, .inactivePreview(days: 14, at: "2025-01-01T00:00:00.000Z"))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 2)
        try await loadPreview(fixture, preview)
        XCTAssertTrue(fixture.bulkTaskArchiving.canSubmit)
        refresh.succeed(emptyTaskListResources)
        try await fixture.settled()
        XCTAssertTrue(fixture.bulkTaskArchiving.canSubmit)
    }

    @MainActor
    func testClosingKeepsTheContentUntilTheNextOpen() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        try await openPreview(fixture)
        fixture.bulkTaskArchiving.setDays("30")
        try await loadPreview(fixture, try await fixture.client.next())

        fixture.bulkTaskArchiving.cancel()
        XCTAssertFalse(fixture.bulkTaskArchiving.isPresented)
        XCTAssertEqual(fixture.bulkTaskArchiving.daysText, "30")
        XCTAssertEqual(fixture.bulkTaskArchiving.tasks, [secondTask])
        XCTAssertTrue(fixture.bulkTaskArchiving.hasPreview)
        XCTAssertFalse(fixture.bulkTaskArchiving.canSubmit, "A closed dialog must not submit.")
        let requests = fixture.client.operations.count
        fixture.bulkTaskArchiving.submit()
        fixture.bulkTaskArchiving.setDays("7")
        fixture.bulkTaskArchiving.refresh()
        XCTAssertEqual(fixture.client.operations.count, requests)
        XCTAssertEqual(fixture.bulkTaskArchiving.daysText, "30")

        fixture.bulkTaskArchiving.open()
        XCTAssertEqual(fixture.bulkTaskArchiving.daysText, "14")
        XCTAssertFalse(fixture.bulkTaskArchiving.hasPreview)
        XCTAssertTrue(fixture.bulkTaskArchiving.isLoading)
        try await loadPreview(fixture, try await fixture.client.next())
    }

    @MainActor
    func testOpeningAfterAFinishedArchiveStartsOver() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        try await openPreview(fixture)
        fixture.bulkTaskArchiving.submit()
        try await archivedInactive(
            fixture, try await fixture.client.next(), count: 1, resources: emptyTaskListResources)

        fixture.bulkTaskArchiving.cancel()
        XCTAssertEqual(fixture.bulkTaskArchiving.archivedCount, 1)
        fixture.bulkTaskArchiving.open()
        XCTAssertNil(fixture.bulkTaskArchiving.archivedCount)
        try await loadPreview(fixture, try await fixture.client.next())
        XCTAssertTrue(fixture.bulkTaskArchiving.canSubmit)
    }

    @MainActor
    func testUnconfirmedConnectionKeepsTheDialogClosed() async throws {
        let client = FakeClient()
        let archiving = BulkTaskArchivingStore(
            client: client, clock: FakeClock(), connection: ConnectionStore(settings: .local),
            recordPreviewFailure: { _ in }, reloadAfterWrite: { _ in })
        XCTAssertFalse(archiving.canOpen)
        archiving.open()
        XCTAssertFalse(archiving.isPresented)
        XCTAssertTrue(client.operations.isEmpty)
    }

    @MainActor
    func testDayEditsNotifyTheSheetButNotTheContent() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        try await openPreview(fixture)
        let harness = PresentationHarness(session: fixture.session)
        let sheet = ObservationProbe { PresentationReads.bulkTaskArchiving(harness) }
        let content = ObservationProbe { PresentationReads.content(harness) }

        fixture.bulkTaskArchiving.setDays("0")

        XCTAssertTrue(sheet.consume())
        XCTAssertFalse(content.consume())
    }

    @MainActor
    func testInvalidPreviewResponsesCannotBeConfirmed() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.bulkTaskArchiving.open()
        let request = try await fixture.client.next()
        request.inactivePreview(InactiveTaskPreview(asOf: "invalid", inactiveDays: 14, tasks: [secondTask]))
        try await finishedLoading(fixture)
        XCTAssertFalse(fixture.bulkTaskArchiving.canSubmit)
        XCTAssertEqual(fixture.bulkTaskArchiving.error, "The tracker returned an invalid archive preview.")
        XCTAssertTrue(fixture.session.connection.stale)
        fixture.bulkTaskArchiving.refresh()
        let duplicate = try await fixture.client.next()
        XCTAssertEqual(duplicate.operation, .inactivePreview(days: 14, at: "2025-01-01T00:00:00.000Z"))
        try await loadPreview(fixture, duplicate, tasks: [secondTask, secondTask])
        XCTAssertFalse(fixture.bulkTaskArchiving.canSubmit)
        fixture.bulkTaskArchiving.refresh()
        let archived = try await fixture.client.next()
        try await loadPreview(fixture, archived, tasks: [archivedTask])
        XCTAssertFalse(fixture.bulkTaskArchiving.canSubmit)
        XCTAssertNotNil(fixture.bulkTaskArchiving.error)
    }

    @MainActor
    func testBulkDialogOpensWhileOtherEditorsAreOpen() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        fixture.session.openWorklogCorrection(worklogID: activeWorklog.id)
        fixture.taskCreation.open()
        fixture.taskArchiving.openArchive(taskID: secondTask.id)
        XCTAssertTrue(fixture.bulkTaskArchiving.canOpen)
        try await openPreview(fixture)
        XCTAssertTrue(fixture.bulkTaskArchiving.isPresented)
        XCTAssertTrue(fixture.session.worklogCorrection.isPresented)
        XCTAssertTrue(fixture.taskCreation.isPresented)
        XCTAssertTrue(fixture.taskArchiving.isPresented)
    }

    @MainActor
    func testSleepDuringSubmissionAcceptsResultWithoutRepeatingCommand() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        try await openPreview(fixture)
        fixture.bulkTaskArchiving.submit()
        let command = try await fixture.client.next()
        fixture.sync.sleep()
        XCTAssertTrue(fixture.bulkTaskArchiving.isSubmitting)
        try await archivedInactive(fixture, command, count: 1, resources: emptyTaskListResources)
        XCTAssertEqual(fixture.bulkTaskArchiving.archivedCount, 1)
        XCTAssertFalse(fixture.bulkTaskArchiving.isSubmitting)
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
        fixture.bulkTaskArchiving.submit()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Archive rejected", kind: "validation"))
        let reload = try await fixture.client.next()
        try await fixture.update("the failed reload") {
            reload.fail(BridgeFailure(message: "Offline", kind: "unavailable"))
        } until: {
            fixture.session.connection.stale
        }
        XCTAssertEqual(fixture.session.connection.message, "Offline")
        XCTAssertEqual(
            fixture.bulkTaskArchiving.error, "Archive rejected Refresh the preview before trying again.")
        fixture.bulkTaskArchiving.refresh()
        let preview = try await fixture.client.next()
        XCTAssertEqual(preview.operation, .inactivePreview(days: 14, at: "2025-01-01T00:00:00.000Z"))
        try await loadPreview(fixture, preview)
        XCTAssertTrue(fixture.bulkTaskArchiving.canSubmit)
    }
}
