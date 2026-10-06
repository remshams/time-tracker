import Foundation
import XCTest
@testable import TrackerClient

final class WorklogCorrectionTests: XCTestCase {
    private let preciseLog = WorklogItem(id: "precise-log", taskId: firstTask.id,
                                         start: "2024-12-30T09:00:12.123456Z",
                                         end: "2024-12-30T10:00:45.654321Z")

    @MainActor
    private func openCompleted(_ fixture: Fixture, worklog: WorklogItem? = nil) async throws -> WorklogItem {
        let log = worklog ?? preciseLog
        try await fixture.start(TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        fixture.session.retryHistory()
        let history = try await fixture.client.next()
        history.succeed(HistoryPage(worklogs: [log], nextCursor: "older", reset: false))
        try await fixture.settled()
        fixture.session.openWorklogCorrection(worklogID: log.id)
        return log
    }

    @MainActor
    private func completePreflight(_ fixture: Fixture, worklog: WorklogItem,
                                   snapshot: TrackerSnapshot? = nil) async throws {
        let snapshot = snapshot ?? TrackerSnapshot(tasks: [firstTask, secondTask], active: nil)
        let read = try await fixture.client.next()
        XCTAssertEqual(read.operation, .snapshot)
        read.succeed(snapshot)
        if snapshot.active?.id != worklog.id {
            let history = try await fixture.client.next()
            XCTAssertEqual(history.operation, .history(task: worklog.taskId, cursor: nil))
            history.succeed(HistoryPage(worklogs: [worklog], nextCursor: nil, reset: false))
        }
    }

    @MainActor
    func testCorrectionQueuedDuringRefreshBlocksTrackingAndRunsWithoutOverlappingRequests() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(snapshot)
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        XCTAssertFalse(fixture.session.isBlockingControls)
        fixture.session.openWorklogCorrection(worklogID: activeWorklog.id)
        fixture.session.setWorklogCorrectionStart(timestamp("2024-12-31T23:58:00.000Z")!)
        fixture.session.submitWorklogCorrection()
        XCTAssertTrue(fixture.session.isBlockingControls)
        XCTAssertFalse(fixture.session.canStopTracking)
        XCTAssertFalse(fixture.session.canStartTracking(taskID: secondTask.id))
        fixture.session.stopTracking(worklogID: activeWorklog.id)
        poll.succeed(snapshot)
        try await completePreflight(fixture, worklog: activeWorklog, snapshot: snapshot)
        let command = try await fixture.client.next()
        XCTAssertEqual(command.operation, .correct(expected: activeWorklog, start: "2024-12-31T23:58:00.000Z",
                                                   end: nil, at: "2025-01-01T00:00:00.000Z"))
        let corrected = WorklogItem(id: activeWorklog.id, taskId: firstTask.id,
                                   start: "2024-12-31T23:58:00.000000Z", end: nil)
        command.corrected(worklog: corrected, snapshot: TrackerSnapshot(tasks: snapshot.tasks, active: corrected))
        let history = try await fixture.client.next()
        history.succeed(HistoryPage(worklogs: [corrected], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertFalse(fixture.session.isBlockingControls)
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 1)
        XCTAssertFalse(fixture.client.operations.contains { if case .stop = $0 { return true }; return false })
    }

    @MainActor
    func testCorrectionOpenedWhileConnectionChangeWaitsPreventsSourceSwitch() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(snapshot)
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        let connection = Task { await fixture.session.connect(.local) }
        try await fixture.waitUntil("connection change to queue") { fixture.session.isBlockingControls }
        XCTAssertTrue(fixture.session.isBlockingControls)
        fixture.session.openWorklogCorrection(worklogID: activeWorklog.id)
        XCTAssertTrue(fixture.session.worklogCorrection.isPresented)
        poll.succeed(snapshot)
        let connected = try await fixture.taskValue(connection)
        XCTAssertFalse(connected)
        try await fixture.settled()
        XCTAssertTrue(fixture.session.worklogCorrection.isPresented)
        XCTAssertFalse(fixture.client.operations.contains { if case .connect = $0 { return true }; return false })
    }

    @MainActor
    func testCompletedEditPreservesUntouchedMicrosecondsAndRefreshesNewestHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        XCTAssertEqual(fixture.session.worklogCorrection.original, log)
        XCTAssertEqual(fixture.session.worklogCorrection.taskName, firstTask.name)
        XCTAssertFalse(fixture.session.worklogCorrection.canSubmit)
        fixture.session.setWorklogCorrectionStart(timestamp("2024-12-30T08:45:59.999Z")!)
        fixture.session.submitWorklogCorrection()
        try await completePreflight(fixture, worklog: log)
        let command = try await fixture.client.next()
        XCTAssertEqual(command.operation, .correct(expected: log, start: "2024-12-30T08:45:00.000Z",
                                                   end: log.end, at: "2025-01-01T00:00:00.000Z"))
        fixture.session.cancelWorklogCorrection()
        fixture.session.setWorklogCorrectionEnd(fixture.clock.now)
        XCTAssertEqual(fixture.session.worklogCorrection.end, timestamp(log.end))
        let corrected = WorklogItem(id: log.id, taskId: log.taskId,
                                   start: "2024-12-30T08:45:00.000000Z", end: log.end)
        command.corrected(worklog: corrected, snapshot: TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: log.taskId, cursor: nil))
        history.succeed(HistoryPage(worklogs: [corrected], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertFalse(fixture.session.worklogCorrection.isPresented)
        XCTAssertEqual(fixture.session.worklogs, [corrected])
        XCTAssertNil(fixture.session.nextCursor)
    }

    @MainActor
    func testReturningToOriginalMinuteKeepsExactOriginalStringsAndDisablesSave() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        fixture.session.setWorklogCorrectionStart(timestamp("2024-12-30T08:00:00.000Z")!)
        fixture.session.setWorklogCorrectionStart(timestamp(log.start)!)
        fixture.session.setWorklogCorrectionEnd(timestamp(log.end)!)
        XCTAssertFalse(fixture.session.worklogCorrection.canSubmit)
        XCTAssertEqual(fixture.session.worklogCorrection.start, timestamp(log.start))
        XCTAssertEqual(fixture.session.worklogCorrection.end, timestamp(log.end))
        let operations = fixture.client.operations.count
        fixture.session.submitWorklogCorrection()
        fixture.session.cancelWorklogCorrection()
        XCTAssertEqual(fixture.client.operations.count, operations)
        XCTAssertFalse(fixture.session.worklogCorrection.isPresented)
    }

    @MainActor
    func testRunningEditChangesOnlyStartAndReanchorsElapsedTime() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let initial = TrackerSnapshot(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(initial)
        fixture.session.openWorklogCorrection(worklogID: activeWorklog.id)
        fixture.session.setWorklogCorrectionEnd(fixture.clock.now)
        XCTAssertNil(fixture.session.worklogCorrection.end)
        fixture.session.setWorklogCorrectionStart(timestamp("2024-12-31T23:58:00.000Z")!)
        fixture.session.submitWorklogCorrection()
        try await completePreflight(fixture, worklog: activeWorklog, snapshot: initial)
        let command = try await fixture.client.next()
        XCTAssertEqual(command.operation, .correct(expected: activeWorklog, start: "2024-12-31T23:58:00.000Z",
                                                   end: nil, at: "2025-01-01T00:00:00.000Z"))
        let corrected = WorklogItem(id: activeWorklog.id, taskId: firstTask.id,
                                   start: "2024-12-31T23:58:00.000000Z", end: nil)
        command.corrected(worklog: corrected, snapshot: TrackerSnapshot(tasks: [firstTask], active: corrected))
        let history = try await fixture.client.next()
        history.succeed(HistoryPage(worklogs: [corrected], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.elapsed, 120)
        XCTAssertEqual(fixture.session.active, corrected)
    }

    @MainActor
    func testInvalidIntervalsStayOpenWithDraftAndMakeNoRequest() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await openCompleted(fixture)
        let operations = fixture.client.operations.count
        fixture.session.setWorklogCorrectionStart(fixture.clock.now.addingTimeInterval(60))
        fixture.session.submitWorklogCorrection()
        XCTAssertEqual(fixture.session.worklogCorrection.error, "Start must not be in the future.")
        fixture.session.setWorklogCorrectionStart(timestamp("2024-12-30T11:00:00.000Z")!)
        fixture.session.submitWorklogCorrection()
        XCTAssertEqual(fixture.session.worklogCorrection.error, "End must not be before Start.")
        fixture.session.setWorklogCorrectionEnd(fixture.clock.now.addingTimeInterval(60))
        fixture.session.submitWorklogCorrection()
        XCTAssertEqual(fixture.session.worklogCorrection.error, "End must not be in the future.")
        XCTAssertTrue(fixture.session.worklogCorrection.isPresented)
        XCTAssertFalse(fixture.session.worklogCorrection.isSubmitting)
        XCTAssertEqual(fixture.client.operations.count, operations)
    }

    @MainActor
    func testStoppedRunningEntryRequiresReviewAndPreservesDraftStart() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(snapshot)
        fixture.session.openWorklogCorrection(worklogID: activeWorklog.id)
        let draft = timestamp("2024-12-31T23:58:00.000Z")!
        fixture.session.setWorklogCorrectionStart(draft)
        fixture.session.submitWorklogCorrection()
        let stopped = WorklogItem(id: activeWorklog.id, taskId: firstTask.id, start: activeWorklog.start,
                                 end: "2025-01-01T00:00:00.000000Z")
        try await completePreflight(fixture, worklog: stopped, snapshot: TrackerSnapshot(tasks: [firstTask], active: nil))
        let refresh = try await fixture.client.next()
        refresh.succeed(HistoryPage(worklogs: [stopped], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.worklogCorrection.requiresReview)
        XCTAssertFalse(fixture.session.worklogCorrection.canSubmit)
        XCTAssertEqual(fixture.session.worklogCorrection.latest, stopped)
        XCTAssertEqual(fixture.session.worklogCorrection.start, draft)
        XCTAssertNil(fixture.session.worklogCorrection.end)
        fixture.session.reviewLatestWorklogCorrection()
        XCTAssertEqual(fixture.session.worklogCorrection.original, stopped)
        XCTAssertEqual(fixture.session.worklogCorrection.start, draft)
        XCTAssertEqual(fixture.session.worklogCorrection.end, timestamp(stopped.end))
        XCTAssertTrue(fixture.session.worklogCorrection.canSubmit)
        XCTAssertFalse(fixture.client.operations.contains { if case .correct = $0 { return true }; return false })
    }

    @MainActor
    func testGlobalRevisionConflictAllowsExplicitRetryWithoutDiscardingDraft() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        let draft = timestamp("2024-12-30T08:00:00.000Z")!
        fixture.session.setWorklogCorrectionStart(draft)
        fixture.session.submitWorklogCorrection()
        try await completePreflight(fixture, worklog: log)
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Tracker state changed. Save again.", kind: "conflict", requiresRefresh: true))
        try await completePreflight(fixture, worklog: log)
        try await fixture.settled()
        XCTAssertFalse(fixture.session.worklogCorrection.requiresReview)
        XCTAssertTrue(fixture.session.worklogCorrection.canSubmit)
        XCTAssertTrue(fixture.session.worklogCorrection.canEdit)
        XCTAssertEqual(fixture.session.worklogCorrection.start, draft)
        XCTAssertEqual(fixture.client.operations.filter { if case .correct = $0 { return true }; return false }.count, 1)
    }

    @MainActor
    func testEntryConflictRequiresReviewEvenIfLatestTimesMatchOriginal() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        fixture.session.setWorklogCorrectionStart(timestamp("2024-12-30T08:00:00.000Z")!)
        fixture.session.submitWorklogCorrection()
        try await completePreflight(fixture, worklog: log)
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Worklog changed", kind: "worklog_changed"))
        try await completePreflight(fixture, worklog: log)
        try await fixture.settled()
        XCTAssertTrue(fixture.session.worklogCorrection.requiresReview)
        XCTAssertEqual(fixture.session.worklogCorrection.latest, log)
        XCTAssertFalse(fixture.session.worklogCorrection.canEdit)
        fixture.session.reviewLatestWorklogCorrection()
        XCTAssertTrue(fixture.session.worklogCorrection.canSubmit)
    }

    @MainActor
    func testLostResponseConfirmsCommittedTimesWithoutAnotherWrite() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        fixture.session.setWorklogCorrectionStart(timestamp("2024-12-30T08:00:00.000Z")!)
        fixture.session.submitWorklogCorrection()
        try await completePreflight(fixture, worklog: log)
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Response lost", uncertain: true))
        let corrected = WorklogItem(id: log.id, taskId: log.taskId, start: "2024-12-30T08:00:00.000000Z", end: log.end)
        try await completePreflight(fixture, worklog: corrected)
        let history = try await fixture.client.next()
        history.succeed(HistoryPage(worklogs: [corrected], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertFalse(fixture.session.worklogCorrection.isPresented)
        XCTAssertEqual(fixture.session.worklogs, [corrected])
        XCTAssertEqual(fixture.client.operations.filter { if case .correct = $0 { return true }; return false }.count, 1)
    }

    @MainActor
    func testUncertainFailureFreezesIntentAcrossDismissAndBlocksSourceChangeAndOtherEditors() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        fixture.session.setWorklogCorrectionStart(timestamp("2024-12-30T08:00:00.000Z")!)
        fixture.session.submitWorklogCorrection()
        try await completePreflight(fixture, worklog: log)
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Response lost", uncertain: true))
        let reconciliation = try await fixture.client.next()
        reconciliation.fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertFalse(fixture.session.worklogCorrection.canEdit)
        XCTAssertTrue(fixture.session.worklogCorrection.canSubmit)
        fixture.session.cancelWorklogCorrection()
        XCTAssertFalse(fixture.session.canOpenTaskCreation)
        XCTAssertFalse(fixture.session.canOpenTaskRename)
        let connected = await fixture.session.connect(serverSettings)
        XCTAssertFalse(connected)
        XCTAssertEqual(fixture.session.connectionSettings, .local)
        fixture.session.select(secondTask.id)
        let selectedHistory = try await fixture.client.next()
        selectedHistory.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.worklogs, [])
        fixture.session.openWorklogCorrection(worklogID: log.id)
        XCTAssertTrue(fixture.session.worklogCorrection.isPresented)
        XCTAssertEqual(fixture.session.worklogCorrection.original, log)
        fixture.clock.now.addTimeInterval(90)
        fixture.session.submitWorklogCorrection()
        try await completePreflight(fixture, worklog: log)
        let retry = try await fixture.client.next()
        XCTAssertEqual(retry.operation, command.operation)
        retry.fail(BridgeFailure(message: "Overlap", kind: "worklog_overlap", requiresRefresh: true))
        try await completePreflight(fixture, worklog: log)
        try await fixture.settled()
        XCTAssertTrue(fixture.session.worklogCorrection.canEdit)
        XCTAssertEqual(fixture.session.worklogCorrection.error, "Overlap")
    }

    @MainActor
    func testMissingEntryRequiresReviewWithoutWritingOrScanningOtherTasks() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        fixture.session.setWorklogCorrectionStart(timestamp("2024-12-30T08:00:00.000Z")!)
        fixture.session.submitWorklogCorrection()
        let read = try await fixture.client.next()
        read.succeed(TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertTrue(fixture.session.worklogCorrection.requiresReview)
        XCTAssertNil(fixture.session.worklogCorrection.latest)
        XCTAssertEqual(fixture.session.worklogCorrection.original, log)
        XCTAssertFalse(fixture.client.operations.contains { if case .correct = $0 { return true }; return false })
        XCTAssertFalse(fixture.client.operations.contains(.history(task: secondTask.id, cursor: nil)))
    }

    @MainActor
    func testPreflightFindsOlderEntryAndSaveKeepsNewSelection() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        fixture.session.setWorklogCorrectionStart(timestamp("2024-12-30T08:00:00.000Z")!)
        fixture.session.select(secondTask.id)
        let selectedHistory = try await fixture.client.next()
        selectedHistory.succeed(emptyPage)
        try await fixture.settled()
        fixture.session.submitWorklogCorrection()
        let read = try await fixture.client.next()
        read.succeed(TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        let newest = try await fixture.client.next()
        newest.succeed(HistoryPage(worklogs: [], nextCursor: "old-page", reset: false))
        let older = try await fixture.client.next()
        XCTAssertEqual(older.operation, .history(task: log.taskId, cursor: "old-page"))
        older.succeed(HistoryPage(worklogs: [log], nextCursor: nil, reset: false))
        let command = try await fixture.client.next()
        let corrected = WorklogItem(id: log.id, taskId: log.taskId, start: "2024-12-30T08:00:00.000000Z", end: log.end)
        command.corrected(worklog: corrected, snapshot: TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        let refreshedHistory = try await fixture.client.next()
        XCTAssertEqual(refreshedHistory.operation, .history(task: secondTask.id, cursor: nil))
        refreshedHistory.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.selectedTaskID, secondTask.id)
        XCTAssertEqual(fixture.session.worklogs, [])
    }

    @MainActor
    func testSleepDuringPreflightDefersWriteAndShutdownIgnoresLateResult() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(snapshot)
        fixture.session.openWorklogCorrection(worklogID: activeWorklog.id)
        fixture.session.setWorklogCorrectionStart(timestamp("2024-12-31T23:58:00.000Z")!)
        fixture.session.submitWorklogCorrection()
        let read = try await fixture.client.next()
        fixture.session.sleep()
        read.succeed(snapshot)
        try await fixture.settled()
        XCTAssertTrue(fixture.session.worklogCorrection.isSubmitting)
        fixture.session.wake()
        try await completePreflight(fixture, worklog: activeWorklog, snapshot: snapshot)
        let command = try await fixture.client.next()
        fixture.session.shutdown()
        let corrected = WorklogItem(id: activeWorklog.id, taskId: firstTask.id, start: "2024-12-31T23:58:00.000000Z", end: nil)
        command.corrected(worklog: corrected, snapshot: TrackerSnapshot(tasks: [firstTask], active: corrected))
        await Task.yield()
        XCTAssertEqual(fixture.session.active, activeWorklog)
        XCTAssertFalse(fixture.session.worklogCorrection.isPresented)
    }

    @MainActor
    func testCorrectionObserverPublishesDraftWithoutRedrawingContentAndEditorsAreExclusive() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        let observer = TrackerPresentationObserver(session: fixture.session)
        var corrections = 0
        var content = 0
        observer.onWorklogCorrectionChange = { corrections += 1 }
        observer.onContentChange = { content += 1 }
        fixture.session.onChange = { observer.update(from: fixture.session) }
        fixture.session.setWorklogCorrectionStart(timestamp("2024-12-30T08:00:00.000Z")!)
        XCTAssertEqual(corrections, 1)
        XCTAssertEqual(content, 0)
        XCTAssertFalse(fixture.session.canOpenTaskCreation)
        XCTAssertFalse(fixture.session.canOpenTaskRename)
        fixture.session.openTaskCreation()
        XCTAssertFalse(fixture.session.taskCreation.isPresented)
        fixture.session.cancelWorklogCorrection()
        fixture.session.openTaskRename()
        XCTAssertFalse(fixture.session.canOpenWorklogCorrection)
        fixture.session.openWorklogCorrection(worklogID: log.id)
        XCTAssertFalse(fixture.session.worklogCorrection.isPresented)
    }

    @MainActor
    func testMinutePrecisionUsesFrozenLocalTimezoneForHistoricalSecondOffsets() {
        let state = WorklogCorrectionState()
        let timezone = TimeZone(identifier: "Europe/Paris")!
        let log = WorklogItem(id: "historical-log", taskId: firstTask.id,
                             start: "1900-01-01T09:00:12.123456Z", end: "1900-01-01T11:00:45.654321Z")
        state.open(log, taskName: "Historical task", timezone: timezone)
        state.updateStart(timestamp("1900-01-01T10:00:00.000Z")!)
        XCTAssertTrue(state.submit(at: timestamp("1900-01-02T00:00:00.000Z")!))
        let intent = state.takePendingIntent()
        XCTAssertEqual(intent?.replacementStart, "1900-01-01T09:59:39.000Z")
        XCTAssertEqual(intent?.replacementEnd, log.end)
        XCTAssertEqual(state.presentation.timezoneIdentifier, timezone.identifier)
    }

    @MainActor
    func testBoundedHistoryLookupDoesNotMisclassifyAnUnverifiedEntryAsDeleted() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        fixture.session.setWorklogCorrectionStart(timestamp("2024-12-30T08:00:00.000Z")!)
        fixture.session.submitWorklogCorrection()
        let snapshot = try await fixture.client.next()
        snapshot.succeed(TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        for cursor in ["second-page", "third-page"] {
            let page = try await fixture.client.next()
            page.succeed(HistoryPage(worklogs: [], nextCursor: cursor, reset: false))
        }
        try await fixture.settled()
        XCTAssertFalse(fixture.session.worklogCorrection.requiresReview)
        XCTAssertEqual(fixture.session.worklogCorrection.original, log)
        XCTAssertEqual(fixture.session.worklogCorrection.error,
                       "Could not verify this worklog within the history limit. Cancel and reopen the editor before retrying.")
        XCTAssertFalse(fixture.client.operations.contains { if case .correct = $0 { return true }; return false })
    }


    @MainActor
    func testCommittedCorrectionClearsObsoleteHistoryWhenReloadFails() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        fixture.session.setWorklogCorrectionStart(timestamp("2024-12-30T08:00:00.000Z")!)
        fixture.session.submitWorklogCorrection()
        try await completePreflight(fixture, worklog: log)
        let command = try await fixture.client.next()
        let corrected = WorklogItem(id: log.id, taskId: log.taskId, start: "2024-12-30T08:00:00.000000Z", end: log.end)
        command.corrected(worklog: corrected, snapshot: TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        let history = try await fixture.client.next()
        XCTAssertEqual(fixture.session.worklogs, [])
        XCTAssertNil(fixture.session.nextCursor)
        history.fail(BridgeFailure(message: "History unavailable"))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.worklogs, [])
        XCTAssertNil(fixture.session.nextCursor)
        XCTAssertTrue(fixture.session.historyUnavailable)
        XCTAssertFalse(fixture.session.worklogCorrection.isPresented)
    }

    @MainActor
    func testCorrectionPreservesLoadedHistoryForAnotherSelectedTask() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        fixture.session.select(secondTask.id)
        let other = WorklogItem(id: "other-log", taskId: secondTask.id,
                               start: log.start, end: log.end)
        let selectedHistory = try await fixture.client.next()
        selectedHistory.succeed(HistoryPage(worklogs: [other], nextCursor: "other-older", reset: false))
        try await fixture.settled()
        fixture.session.setWorklogCorrectionStart(timestamp("2024-12-30T08:00:00.000Z")!)
        fixture.session.submitWorklogCorrection()
        try await completePreflight(fixture, worklog: log)
        let command = try await fixture.client.next()
        let corrected = WorklogItem(id: log.id, taskId: log.taskId,
                                   start: "2024-12-30T08:00:00.000000Z", end: log.end)
        command.corrected(worklog: corrected, snapshot: TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .history(task: secondTask.id, cursor: nil))
        XCTAssertEqual(fixture.session.selectedTaskID, secondTask.id)
        XCTAssertEqual(fixture.session.worklogs, [other])
        XCTAssertEqual(fixture.session.nextCursor, "other-older")
        reload.succeed(HistoryPage(worklogs: [other], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.worklogs, [other])
    }

    @MainActor
    func testCommittedResultComparisonPreservesSubmillisecondDifferences() {
        let state = WorklogCorrectionState()
        state.open(preciseLog, taskName: firstTask.name)
        state.updateStart(timestamp("2024-12-30T08:00:00.000Z")!)
        XCTAssertTrue(state.submit(at: timestamp("2025-01-01T00:00:00.000Z")!))
        let intent = state.takePendingIntent()!
        let same = WorklogItem(id: preciseLog.id, taskId: preciseLog.taskId,
                              start: "2024-12-30T08:00:00.000000Z", end: preciseLog.end)
        let changed = WorklogItem(id: preciseLog.id, taskId: preciseLog.taskId,
                                 start: same.start, end: "2024-12-30T10:00:45.654999Z")
        XCTAssertTrue(intent.matchesReplacement(same))
        XCTAssertFalse(intent.matchesReplacement(changed))
    }

}
