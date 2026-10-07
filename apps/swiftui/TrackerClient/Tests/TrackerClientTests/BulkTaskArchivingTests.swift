import Foundation
import XCTest
@testable import TrackerClient

final class BulkTaskArchivingTests: XCTestCase {
    @MainActor
    private func answerPreview(_ request: FakeClient.Request, tasks: [TaskItem] = [secondTask]) throws -> InactiveTaskPreview {
        guard case .inactivePreview(let days, let at) = request.operation else {
            throw TestTimeout(description: "Expected an inactive task preview.")
        }
        let preview = InactiveTaskPreview(asOf: at, inactiveDays: days, tasks: tasks)
        request.inactivePreview(preview)
        return preview
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
        _ = try answerPreview(request)
        try await fixture.settled()
        XCTAssertTrue(fixture.session.bulkTaskArchiving.canSubmit)
        fixture.session.cancelBulkTaskArchiving()
        fixture.session.submitBulkTaskArchiving()
        XCTAssertFalse(fixture.session.bulkTaskArchiving.isPresented)
        XCTAssertEqual(fixture.client.operations.count, 2)
    }

    @MainActor
    func testChangingDaysIgnoresOldResponseAndUsesLatestPeriod() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openBulkTaskArchiving()
        let old = try await fixture.client.next()
        fixture.session.updateBulkArchiveDays("30")
        _ = try answerPreview(old)
        let latest = try await fixture.client.next()
        guard case .inactivePreview(let days, _) = latest.operation else { return XCTFail("Expected preview") }
        XCTAssertEqual(days, 30)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.tasks.isEmpty)
        _ = try answerPreview(latest)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.bulkTaskArchiving.tasks, [secondTask])
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 1)
    }

    @MainActor
    func testInvalidDaysClearPreviewAndDoNotSendRequests() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openBulkTaskArchiving()
        _ = try answerPreview(try await fixture.client.next())
        try await fixture.settled()
        let count = fixture.client.operations.count
        for text in ["", "0", "-1", "1.5", " 14", "4294967296", "abc", "💠"] {
            fixture.session.updateBulkArchiveDays(text)
            XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit, text)
            XCTAssertTrue(fixture.session.bulkTaskArchiving.tasks.isEmpty, text)
            XCTAssertNotNil(fixture.session.bulkTaskArchiving.error, text)
        }
        XCTAssertEqual(fixture.client.operations.count, count)
        fixture.session.updateBulkArchiveDays("4294967295")
        let valid = try await fixture.client.next()
        guard case .inactivePreview(let days, _) = valid.operation else { return XCTFail("Expected preview") }
        XCTAssertEqual(days, Int(UInt32.max))
        valid.fail(BridgeFailure(message: "Cutoff is outside the supported date range."))
        try await fixture.settled()
        XCTAssertNotNil(fixture.session.bulkTaskArchiving.error)
        XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
    }

    @MainActor
    func testConfirmationReusesCapturedPreviewAndReportsActualCountWithoutChangingTimer() async throws {
        for actualCount in [0, 2] {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            let snapshot = TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog)
            try await fixture.start(snapshot)
            fixture.session.openBulkTaskArchiving()
            let preview = try answerPreview(try await fixture.client.next())
            try await fixture.settled()
            fixture.clock.now.addTimeInterval(10)
            fixture.session.submitBulkTaskArchiving()
            fixture.session.submitBulkTaskArchiving()
            fixture.session.cancelBulkTaskArchiving()
            let command = try await fixture.client.next()
            XCTAssertEqual(command.operation, .archiveInactive(preview))
            XCTAssertTrue(fixture.session.bulkTaskArchiving.isSubmitting)
            command.archivedInactive(count: actualCount, snapshot: snapshot)
            try await fixture.settled()
            XCTAssertEqual(fixture.session.bulkTaskArchiving.archivedCount, actualCount)
            XCTAssertEqual(fixture.session.active, activeWorklog)
            XCTAssertEqual(fixture.session.selectedTaskID, firstTask.id)
            XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
            XCTAssertEqual(fixture.client.maximumOutstandingRequests, 1)
        }
    }

    @MainActor
    func testEmptyPreviewAndFailedPreviewDisableConfirmation() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openBulkTaskArchiving()
        _ = try answerPreview(try await fixture.client.next(), tasks: [])
        try await fixture.settled()
        XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
        fixture.session.refreshBulkArchivePreview()
        let request = try await fixture.client.next()
        request.fail(BridgeFailure(message: "Offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.bulkTaskArchiving.error, "Offline")
        XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
    }

    @MainActor
    func testFailedSubmissionReconcilesAndRequiresNewPreview() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openBulkTaskArchiving()
        _ = try answerPreview(try await fixture.client.next())
        try await fixture.settled()
        fixture.session.submitBulkTaskArchiving()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Response lost", kind: "unavailable", uncertain: true))
        let reconcile = try await fixture.client.next()
        XCTAssertEqual(reconcile.operation, .snapshot)
        reconcile.succeed(emptySnapshot)
        try await fixture.settled()
        XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.tasks.isEmpty)
        let count = fixture.client.operations.count
        fixture.session.submitBulkTaskArchiving()
        XCTAssertEqual(fixture.client.operations.count, count)
        fixture.session.refreshBulkArchivePreview()
        _ = try answerPreview(try await fixture.client.next())
        try await fixture.settled()
        XCTAssertTrue(fixture.session.bulkTaskArchiving.canSubmit)
    }

    @MainActor
    func testDialogBlocksOtherEditorsTrackingAndConnections() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask, secondTask, archivedTask], active: activeWorklog))
        fixture.session.openBulkTaskArchiving()
        _ = try answerPreview(try await fixture.client.next())
        try await fixture.settled()
        XCTAssertFalse(fixture.session.canOpenTaskCreation)
        XCTAssertFalse(fixture.session.canOpenTaskRename)
        XCTAssertFalse(fixture.session.canOpenWorklogCorrection)
        XCTAssertFalse(fixture.session.canOpenWorklogMove)
        XCTAssertFalse(fixture.session.canArchiveTask(taskID: secondTask.id))
        XCTAssertFalse(fixture.session.canUnarchiveTask(taskID: archivedTask.id))
        XCTAssertFalse(fixture.session.canStartTracking(taskID: secondTask.id))
        XCTAssertFalse(fixture.session.canStopTracking)
        XCTAssertFalse(fixture.session.canOpenBulkTaskArchiving)
        let changed = await fixture.session.connect(.local)
        XCTAssertFalse(changed)
        do { try await fixture.session.testConnection(.local); XCTFail("Connection test must be blocked") }
        catch { XCTAssertTrue(error.localizedDescription.contains("Close the archive dialog")) }
        fixture.session.cancelBulkTaskArchiving()
        XCTAssertTrue(fixture.session.canOpenTaskCreation)
        fixture.session.openTaskArchive(taskID: secondTask.id)
        XCTAssertFalse(fixture.session.canOpenBulkTaskArchiving)
    }

    @MainActor
    func testCancelAndShutdownIgnoreLateResponses() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openBulkTaskArchiving()
        let request = try await fixture.client.next()
        fixture.session.cancelBulkTaskArchiving()
        _ = try answerPreview(request)
        try await fixture.settled()
        XCTAssertFalse(fixture.session.bulkTaskArchiving.isPresented)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.tasks.isEmpty)
        fixture.session.openBulkTaskArchiving()
        let late = try await fixture.client.next()
        fixture.session.shutdown()
        _ = try answerPreview(late)
        await Task.yield()
        XCTAssertFalse(fixture.session.bulkTaskArchiving.isPresented)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.tasks.isEmpty)
    }

    @MainActor
    func testSleepInvalidatesReadyPreviewAndWakeLoadsFreshPreview() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openBulkTaskArchiving()
        _ = try answerPreview(try await fixture.client.next())
        try await fixture.settled()
        fixture.session.sleep()
        XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.tasks.isEmpty)
        fixture.session.wake()
        let refresh = try await fixture.client.next()
        XCTAssertEqual(refresh.operation, .refresh(.local))
        refresh.succeed(emptySnapshot)
        _ = try answerPreview(try await fixture.client.next())
        try await fixture.settled()
        XCTAssertTrue(fixture.session.bulkTaskArchiving.canSubmit)
    }

    @MainActor
    func testPreviewQueuesBehindBackgroundReadAndCancelledQueuedSubmitDoesNotWriteAfterSleep() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.scheduler.poll?.fire()
        let refresh = try await fixture.client.next()
        fixture.session.openBulkTaskArchiving()
        XCTAssertTrue(fixture.session.bulkTaskArchiving.isLoading)
        refresh.succeed(emptySnapshot)
        _ = try answerPreview(try await fixture.client.next())
        try await fixture.settled()
        fixture.scheduler.poll?.fire()
        let background = try await fixture.client.next()
        fixture.session.submitBulkTaskArchiving()
        fixture.session.sleep()
        background.succeed(emptySnapshot)
        try await fixture.settled()
        XCTAssertFalse(fixture.session.bulkTaskArchiving.isSubmitting)
        XCTAssertFalse(fixture.client.operations.contains { if case .archiveInactive = $0 { return true }; return false })
    }

    @MainActor
    func testObserverRetainsSheetContentWhileDismissalAnimates() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        let observer = TrackerPresentationObserver(session: fixture.session)
        var updates = 0
        observer.onBulkTaskArchivingChange = { updates += 1 }
        fixture.session.onChange = { observer.update(from: fixture.session) }
        fixture.session.openBulkTaskArchiving()
        _ = try answerPreview(try await fixture.client.next())
        try await fixture.settled()
        observer.update(from: fixture.session)
        fixture.session.onChange = { observer.update(from: fixture.session) }
        let content = observer.bulkTaskArchivingSheetContent
        fixture.session.cancelBulkTaskArchiving()
        XCTAssertFalse(fixture.session.bulkTaskArchiving.isPresented)
        XCTAssertEqual(observer.bulkTaskArchivingSheetContent, content)
        XCTAssertGreaterThan(updates, 1)
    }

    @MainActor
    func testInvalidPreviewResponsesCannotBeConfirmed() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openBulkTaskArchiving()
        let request = try await fixture.client.next()
        request.inactivePreview(InactiveTaskPreview(asOf: "invalid", inactiveDays: 14, tasks: [secondTask]))
        try await fixture.settled()
        XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
        XCTAssertNotNil(fixture.session.bulkTaskArchiving.error)
        fixture.session.refreshBulkArchivePreview()
        let recovery = try await fixture.client.next()
        XCTAssertEqual(recovery.operation, .snapshot)
        recovery.succeed(emptySnapshot)
        let duplicate = try await fixture.client.next()
        _ = try answerPreview(duplicate, tasks: [secondTask, secondTask])
        try await fixture.settled()
        XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
    }
    @MainActor
    func testBulkDialogCannotOpenWhileOtherEditorsOwnPresentation() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog))
        fixture.session.openTaskCreation()
        XCTAssertFalse(fixture.session.canOpenBulkTaskArchiving)
        fixture.session.cancelTaskCreation()
        fixture.session.openTaskRename(taskID: firstTask.id)
        XCTAssertFalse(fixture.session.canOpenBulkTaskArchiving)
        fixture.session.cancelTaskRename()
        fixture.session.openWorklogCorrection(worklogID: activeWorklog.id)
        XCTAssertFalse(fixture.session.canOpenBulkTaskArchiving)
        fixture.session.cancelWorklogCorrection()
        fixture.session.openWorklogMove(worklogID: activeWorklog.id)
        XCTAssertFalse(fixture.session.canOpenBulkTaskArchiving)
        let moveSnapshot = try await fixture.client.next()
        moveSnapshot.succeed(TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog))
        let search = try await fixture.client.next()
        search.candidates([])
        try await fixture.settled()
        fixture.session.cancelWorklogMove()
        XCTAssertTrue(fixture.session.canOpenBulkTaskArchiving)
    }

    @MainActor
    func testSleepDuringPreviewIgnoresOldResponseAndRetriesAfterWake() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openBulkTaskArchiving()
        let initial = try await fixture.client.next()
        fixture.session.sleep()
        _ = try answerPreview(initial)
        try await fixture.settled()
        XCTAssertTrue(fixture.session.bulkTaskArchiving.tasks.isEmpty)
        fixture.session.wake()
        let refresh = try await fixture.client.next()
        refresh.succeed(emptySnapshot)
        _ = try answerPreview(try await fixture.client.next())
        try await fixture.settled()
        XCTAssertTrue(fixture.session.bulkTaskArchiving.canSubmit)
    }

    @MainActor
    func testSleepDuringCommittedSubmissionAcceptsResultWithoutRepeatingCommand() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openBulkTaskArchiving()
        _ = try answerPreview(try await fixture.client.next())
        try await fixture.settled()
        fixture.session.submitBulkTaskArchiving()
        let command = try await fixture.client.next()
        fixture.session.sleep()
        command.archivedInactive(count: 1, snapshot: emptySnapshot)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.bulkTaskArchiving.archivedCount, 1)
        XCTAssertFalse(fixture.session.bulkTaskArchiving.isSubmitting)
        let writes = fixture.client.operations.filter { if case .archiveInactive = $0 { return true }; return false }
        XCTAssertEqual(writes.count, 1)
    }

    @MainActor
    func testFailedReconciliationRefreshesSnapshotBeforeLoadingNewPreview() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openBulkTaskArchiving()
        _ = try answerPreview(try await fixture.client.next())
        try await fixture.settled()
        fixture.session.submitBulkTaskArchiving()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Offline", kind: "unavailable", uncertain: true))
        let reconciliation = try await fixture.client.next()
        reconciliation.fail(BridgeFailure(message: "Offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.isStale)
        fixture.session.refreshBulkArchivePreview()
        let recovery = try await fixture.client.next()
        XCTAssertEqual(recovery.operation, .snapshot)
        recovery.succeed(emptySnapshot)
        _ = try answerPreview(try await fixture.client.next())
        try await fixture.settled()
        XCTAssertFalse(fixture.session.isStale)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.canSubmit)
    }

    @MainActor
    func testNegativeArchiveCountIsRejectedAndDoesNotReportSuccess() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openBulkTaskArchiving()
        _ = try answerPreview(try await fixture.client.next())
        try await fixture.settled()
        fixture.session.submitBulkTaskArchiving()
        let command = try await fixture.client.next()
        command.archivedInactive(count: -1, snapshot: emptySnapshot)
        let reconciliation = try await fixture.client.next()
        reconciliation.succeed(emptySnapshot)
        try await fixture.settled()
        XCTAssertNil(fixture.session.bulkTaskArchiving.archivedCount)
        XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
        XCTAssertNotNil(fixture.session.bulkTaskArchiving.error)
    }

    @MainActor
    func testSleepBeforeSubmissionTaskStartsCancelsPreparedWrite() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openBulkTaskArchiving()
        _ = try answerPreview(try await fixture.client.next())
        try await fixture.settled()
        fixture.session.submitBulkTaskArchiving()
        fixture.session.sleep()
        try await fixture.settled()
        XCTAssertFalse(fixture.session.bulkTaskArchiving.isSubmitting)
        XCTAssertFalse(fixture.session.bulkTaskArchiving.canSubmit)
        XCTAssertTrue(fixture.session.bulkTaskArchiving.tasks.isEmpty)
        XCTAssertFalse(fixture.client.operations.contains { if case .archiveInactive = $0 { return true }; return false })
        fixture.session.wake()
        let refresh = try await fixture.client.next()
        refresh.succeed(emptySnapshot)
        _ = try answerPreview(try await fixture.client.next())
        try await fixture.settled()
        XCTAssertTrue(fixture.session.bulkTaskArchiving.canSubmit)
    }

}
