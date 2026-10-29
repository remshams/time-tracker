import Foundation
import XCTest
@testable import TrackerClient

final class WorklogCorrectionTests: XCTestCase {
    private let preciseLog = WorklogItem(
        id: "precise-log", taskId: firstTask.id,
        start: "2024-12-30T09:00:12.123456Z",
        end: "2024-12-30T10:00:45.654321Z")
    private let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)

    @MainActor
    private func openCompleted(_ fixture: Fixture, worklog: WorklogItem? = nil) async throws -> WorklogItem {
        let log = worklog ?? preciseLog
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        fixture.session.retryHistory()
        let history = try await fixture.client.next()
        history.succeed(HistoryPage(worklogs: [log], nextCursor: "older", reset: false))
        try await fixture.settled()
        fixture.session.openWorklogCorrection(worklogID: log.id)
        return log
    }

    /// Answers the history reload that follows a correction and waits for the session to settle.
    @MainActor
    private func finishHistory(
        _ fixture: Fixture, task: String = firstTask.id, rows: [WorklogItem]
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
    private func corrections(_ fixture: Fixture) -> Int {
        fixture.client.operations.filter {
            if case .correct = $0 { return true }
            return false
        }.count
    }

    @MainActor
    func testCancellationKeepsLastSheetContentUntilAnotherWorklogOpens() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await openCompleted(fixture)
        let harness = PresentationHarness(session: fixture.session)
        fixture.session.setWorklogCorrectionStart(timestamp("2024-12-30T08:45:00.000Z")!)
        let visible = fixture.session.worklogCorrection
        XCTAssertEqual(harness.presentation.worklogCorrectionSheetContent, visible)

        fixture.session.cancelWorklogCorrection()
        XCTAssertFalse(fixture.session.worklogCorrection.isPresented)
        XCTAssertNil(fixture.session.worklogCorrection.original)
        XCTAssertTrue(fixture.session.canOpenTaskCreation)
        XCTAssertEqual(harness.presentation.worklogCorrectionSheetContent, visible)
        fixture.session.refresh()
        let refresh = try await fixture.client.next()
        let running = WorklogItem(
            id: "running-log", taskId: secondTask.id,
            start: "2024-12-31T23:58:12.123456Z", end: nil)
        refresh.succeed(TaskListResources(tasks: [firstTask, secondTask], active: running))
        try await fixture.settled()
        harness.attach()
        XCTAssertEqual(harness.presentation.worklogCorrectionSheetContent, visible)

        fixture.session.openWorklogCorrection(worklogID: running.id)
        XCTAssertEqual(harness.presentation.worklogCorrectionSheetContent, fixture.session.worklogCorrection)
        XCTAssertEqual(harness.presentation.worklogCorrectionSheetContent.original, running)
        XCTAssertEqual(harness.presentation.worklogCorrectionSheetContent.taskName, secondTask.name)
        XCTAssertNil(harness.presentation.worklogCorrectionSheetContent.end)
    }

    @MainActor
    func testSuccessfulSaveKeepsSubmittingSheetContentDuringDismissal() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        let harness = PresentationHarness(session: fixture.session)
        fixture.session.setWorklogCorrectionStart(timestamp("2024-12-30T08:45:00.000Z")!)
        fixture.session.submitWorklogCorrection()
        let command = try await fixture.client.next()
        let visible = fixture.session.worklogCorrection
        XCTAssertTrue(visible.isSubmitting)
        XCTAssertEqual(harness.presentation.worklogCorrectionSheetContent, visible)
        let corrected = WorklogItem(
            id: log.id, taskId: log.taskId,
            start: "2024-12-30T08:45:00.000000Z", end: log.end)
        command.corrected(worklog: corrected, resources: resources)
        let history = try await fixture.client.next()
        XCTAssertFalse(fixture.session.worklogCorrection.isPresented)
        XCTAssertFalse(fixture.session.worklogCorrection.isSubmitting)
        XCTAssertNil(fixture.session.worklogCorrection.original)
        XCTAssertEqual(harness.presentation.worklogCorrectionSheetContent, visible)
        history.succeed(HistoryPage(worklogs: [corrected], nextCursor: nil, reset: false))
        try await fixture.settled()
        harness.attach()
        XCTAssertEqual(harness.presentation.worklogCorrectionSheetContent, visible)
        fixture.session.openWorklogCorrection(worklogID: corrected.id)
        XCTAssertEqual(harness.presentation.worklogCorrectionSheetContent, fixture.session.worklogCorrection)
        XCTAssertEqual(harness.presentation.worklogCorrectionSheetContent.original, corrected)
        XCTAssertFalse(harness.presentation.worklogCorrectionSheetContent.isSubmitting)
    }

    @MainActor
    func testCompletedEditSendsCorrectionFirstThenReloadsTaskListAndHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        XCTAssertEqual(fixture.session.worklogCorrection.original, log)
        XCTAssertEqual(fixture.session.worklogCorrection.taskName, firstTask.name)
        XCTAssertFalse(fixture.session.worklogCorrection.canSubmit)
        fixture.session.setWorklogCorrectionStart(timestamp("2024-12-30T08:45:59.999Z")!)
        fixture.session.submitWorklogCorrection()
        let command = try await fixture.client.next()
        XCTAssertEqual(
            command.operation,
            .correct(
                expected: log, start: "2024-12-30T08:45:00.000Z",
                end: log.end, at: "2025-01-01T00:00:00.000Z"))
        fixture.session.cancelWorklogCorrection()
        fixture.session.setWorklogCorrectionEnd(fixture.clock.now)
        XCTAssertTrue(fixture.session.worklogCorrection.isPresented)
        XCTAssertEqual(fixture.session.worklogCorrection.end, timestamp(log.end))
        let corrected = WorklogItem(
            id: log.id, taskId: log.taskId,
            start: "2024-12-30T08:45:00.000000Z", end: log.end)
        command.corrected(worklog: corrected, resources: resources)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: log.taskId, cursor: nil))
        XCTAssertEqual(fixture.session.worklogs, [log])
        history.succeed(HistoryPage(worklogs: [corrected], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertFalse(fixture.session.worklogCorrection.isPresented)
        XCTAssertEqual(fixture.session.worklogs, [corrected])
        XCTAssertNil(fixture.session.nextCursor)
        XCTAssertEqual(
            Array(fixture.client.operations.suffix(3)),
            [command.operation, .refresh(fixture.session.connectionSettings), .history(task: log.taskId, cursor: nil)])
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
        let initial = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(initial)
        fixture.session.openWorklogCorrection(worklogID: activeWorklog.id)
        fixture.session.setWorklogCorrectionEnd(fixture.clock.now)
        XCTAssertNil(fixture.session.worklogCorrection.end)
        fixture.session.setWorklogCorrectionStart(timestamp("2024-12-31T23:58:00.000Z")!)
        fixture.session.submitWorklogCorrection()
        let command = try await fixture.client.next()
        XCTAssertEqual(
            command.operation,
            .correct(
                expected: activeWorklog, start: "2024-12-31T23:58:00.000Z",
                end: nil, at: "2025-01-01T00:00:00.000Z"))
        let corrected = WorklogItem(
            id: activeWorklog.id, taskId: firstTask.id,
            start: "2024-12-31T23:58:00.000000Z", end: nil)
        command.corrected(worklog: corrected, resources: TaskListResources(tasks: [firstTask], active: corrected))
        try await fixture.waitUntil("the corrected timer") { fixture.session.active == corrected }
        XCTAssertEqual(fixture.session.elapsed, 120)
        XCTAssertFalse(fixture.session.worklogCorrection.isPresented)
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
        XCTAssertEqual(fixture.session.worklogCorrection.error, "End must be after Start.")
        fixture.session.setWorklogCorrectionEnd(fixture.clock.now.addingTimeInterval(60))
        fixture.session.submitWorklogCorrection()
        XCTAssertEqual(fixture.session.worklogCorrection.error, "End must not be in the future.")
        XCTAssertTrue(fixture.session.worklogCorrection.isPresented)
        XCTAssertFalse(fixture.session.worklogCorrection.isSubmitting)
        XCTAssertEqual(fixture.client.operations.count, operations)
    }

    @MainActor
    func testCompletedDurationUsesStoredMicrosecondsAcrossOffsets() {
        let start = "2024-12-30T10:00:00.123456000Z"
        XCTAssertTrue(hasPositiveWorklogDuration(start: start, end: "2024-12-30T11:00:00.123457000+01:00"))
        XCTAssertFalse(hasPositiveWorklogDuration(start: start, end: "2024-12-30T11:00:00.123456999+01:00"))
        XCTAssertFalse(hasPositiveWorklogDuration(start: start, end: "2024-12-30T10:00:00.123455999Z"))
        XCTAssertTrue(hasPositiveWorklogDuration(start: start, end: "2024-12-30T10:00:01.000000Z"))
        XCTAssertFalse(hasPositiveWorklogDuration(start: start, end: "2024-12-30T09:59:59.999999Z"))
        XCTAssertFalse(hasPositiveWorklogDuration(start: "invalid", end: start))
        XCTAssertFalse(hasPositiveWorklogDuration(start: start, end: "invalid"))
    }

    @MainActor
    func testZeroDurationDraftsStayOpenWithoutSendingRequests() async throws {
        for end in ["2024-12-30T10:00:00.000000000Z", "2024-12-30T10:00:00.000000999Z"] {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            let original = WorklogItem(
                id: "precise-log", taskId: firstTask.id,
                start: "2024-12-30T09:00:00.000000Z", end: end)
            _ = try await openCompleted(fixture, worklog: original)
            let operations = fixture.client.operations.count
            let start = try XCTUnwrap(timestamp("2024-12-30T10:00:00.000000Z"))
            fixture.session.setWorklogCorrectionStart(start)

            fixture.session.submitWorklogCorrection()
            try await fixture.settled()

            XCTAssertEqual(fixture.session.worklogCorrection.error, "End must be after Start.")
            XCTAssertTrue(fixture.session.worklogCorrection.isPresented)
            XCTAssertTrue(fixture.session.worklogCorrection.canEdit)
            XCTAssertFalse(fixture.session.worklogCorrection.isSubmitting)
            XCTAssertEqual(fixture.session.worklogCorrection.original, original)
            XCTAssertEqual(fixture.session.worklogCorrection.start, start)
            XCTAssertEqual(fixture.session.worklogCorrection.end, timestamp(end))
            XCTAssertEqual(fixture.client.operations.count, operations)
        }
    }

    @MainActor
    func testCorrectionAllowsOneMicrosecondOfDuration() throws {
        let state = WorklogCorrectionState()
        let original = WorklogItem(
            id: "precise-log", taskId: firstTask.id,
            start: "2024-12-30T09:00:00.000000Z", end: "2024-12-30T10:00:00.000001Z")
        state.open(original, taskName: firstTask.name, timezone: TimeZone(secondsFromGMT: 0)!)
        state.updateStart(try XCTUnwrap(timestamp("2024-12-30T10:00:00.000000Z")))

        let request = try XCTUnwrap(state.submit(at: try XCTUnwrap(timestamp("2025-01-01T00:00:00.000000Z"))))

        XCTAssertEqual(request.expected, original)
        XCTAssertEqual(request.replacementEnd, original.end)
        XCTAssertTrue(sameWorklogTimestamp(request.replacementStart, "2024-12-30T10:00:00.000000Z"))
    }

    @MainActor
    func testSubmitIsDisabledWhileSubmittingAndTaskCreationAndTrackingStayUsable() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        let draft = try XCTUnwrap(timestamp("2024-12-30T08:00:00.000Z"))
        fixture.session.setWorklogCorrectionStart(draft)
        fixture.session.submitWorklogCorrection()
        let command = try await fixture.client.next()
        XCTAssertTrue(fixture.session.worklogCorrection.isSubmitting)
        XCTAssertFalse(fixture.session.worklogCorrection.canSubmit)
        XCTAssertFalse(fixture.session.worklogCorrection.canEdit)
        fixture.session.submitWorklogCorrection()
        fixture.session.setWorklogCorrectionStart(try XCTUnwrap(timestamp("2024-12-30T07:00:00.000Z")))
        fixture.session.cancelWorklogCorrection()
        XCTAssertTrue(fixture.session.worklogCorrection.isPresented)
        XCTAssertEqual(fixture.session.worklogCorrection.start, draft)
        XCTAssertTrue(fixture.session.canOpenTaskCreation)
        XCTAssertTrue(fixture.session.canOpenWorklogMove)
        XCTAssertTrue(fixture.session.canOpenWorklogDeletion)
        XCTAssertTrue(fixture.session.canStartTracking(taskID: secondTask.id))

        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("Parallel task")
        fixture.session.submitTaskCreation()
        let create = try await fixture.client.next()
        XCTAssertEqual(create.operation, .create(name: "Parallel task", at: "2025-01-01T00:00:00.000Z"))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 2)
        XCTAssertTrue(fixture.session.taskCreation.isSubmitting)
        XCTAssertTrue(fixture.session.worklogCorrection.isSubmitting)

        let corrected = WorklogItem(
            id: log.id, taskId: log.taskId, start: "2024-12-30T08:00:00.000000Z", end: log.end)
        command.corrected(worklog: corrected, resources: resources)
        try await finishHistory(fixture, rows: [corrected])
        XCTAssertFalse(fixture.session.worklogCorrection.isPresented)
        XCTAssertTrue(fixture.session.taskCreation.isSubmitting)
        XCTAssertEqual(corrections(fixture), 1)

        let created = TaskItem(id: "task-created", name: "Parallel task", archived: false, latestStart: nil)
        try await fixture.update("the created task") {
            create.created(
                taskID: created.id, resources: TaskListResources(tasks: [firstTask, secondTask, created], active: nil))
        } until: {
            !fixture.session.taskCreation.isSubmitting
        }
        XCTAssertFalse(fixture.session.taskCreation.isPresented)
        XCTAssertEqual(fixture.session.tasks, [firstTask, secondTask, created])
    }

    @MainActor
    func testFailureShowsServerMessageKeepsDraftReloadsAndResubmitUsesNewTimestamp() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        let draft = try XCTUnwrap(timestamp("2024-12-30T08:00:00.000Z"))
        fixture.session.setWorklogCorrectionStart(draft)
        fixture.session.submitWorklogCorrection()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "The worklog changed on another client.", kind: "worklog_changed"))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connectionSettings))
        XCTAssertFalse(fixture.session.worklogCorrection.isSubmitting)
        XCTAssertEqual(fixture.session.worklogCorrection.error, "The worklog changed on another client.")
        reload.succeed(resources)
        let changed = WorklogItem(
            id: log.id, taskId: log.taskId, start: log.start, end: "2024-12-30T10:30:00.000000Z")
        try await finishHistory(fixture, rows: [changed])

        XCTAssertEqual(fixture.session.worklogs, [changed])
        XCTAssertTrue(fixture.session.worklogCorrection.isPresented)
        XCTAssertEqual(fixture.session.worklogCorrection.original, log)
        XCTAssertEqual(fixture.session.worklogCorrection.start, draft)
        XCTAssertEqual(fixture.session.worklogCorrection.end, timestamp(log.end))
        XCTAssertEqual(fixture.session.worklogCorrection.error, "The worklog changed on another client.")
        XCTAssertTrue(fixture.session.worklogCorrection.canEdit)
        XCTAssertTrue(fixture.session.worklogCorrection.canSubmit)
        XCTAssertFalse(fixture.session.isStale)

        fixture.clock.now.addTimeInterval(90)
        fixture.session.submitWorklogCorrection()
        XCTAssertNil(fixture.session.worklogCorrection.error)
        let retry = try await fixture.client.next()
        XCTAssertEqual(
            retry.operation,
            .correct(
                expected: log, start: "2024-12-30T08:00:00.000Z",
                end: log.end, at: "2025-01-01T00:01:30.000Z"))
        XCTAssertEqual(corrections(fixture), 2)
    }

    @MainActor
    func testUnavailableAndProtocolWriteFailuresKeepTheDraftEditable() async throws {
        for kind in ["unavailable", "protocol"] {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            let log = try await openCompleted(fixture)
            let draft = try XCTUnwrap(timestamp("2024-12-30T08:00:00.000000Z"))
            fixture.session.setWorklogCorrectionStart(draft)
            fixture.session.submitWorklogCorrection()
            let command = try await fixture.client.next()
            command.fail(BridgeFailure(message: "Correction response unavailable", kind: kind))
            let reload = try await fixture.client.next()
            XCTAssertEqual(reload.operation, .refresh(fixture.session.connectionSettings), kind)
            reload.succeed(resources)
            try await finishHistory(fixture, rows: [log])

            XCTAssertTrue(fixture.session.worklogCorrection.canEdit, kind)
            XCTAssertTrue(fixture.session.worklogCorrection.canSubmit, kind)
            XCTAssertEqual(fixture.session.worklogCorrection.start, draft, kind)
            XCTAssertEqual(fixture.session.worklogCorrection.error, "Correction response unavailable", kind)
        }
    }

    @MainActor
    func testUncertainFailureAsksTheUserToCheckTheReloadedData() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        fixture.session.setWorklogCorrectionStart(timestamp("2024-12-30T08:00:00.000Z")!)
        fixture.session.submitWorklogCorrection()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Response lost", uncertain: true))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connectionSettings))
        reload.succeed(resources)
        let corrected = WorklogItem(id: log.id, taskId: log.taskId, start: "2024-12-30T08:00:00.000000Z", end: log.end)
        try await finishHistory(fixture, rows: [corrected])

        XCTAssertEqual(fixture.session.worklogs, [corrected])
        XCTAssertTrue(fixture.session.worklogCorrection.isPresented)
        XCTAssertTrue(fixture.session.worklogCorrection.canEdit)
        XCTAssertEqual(
            fixture.session.worklogCorrection.error,
            "Could not confirm the change. Check the reloaded data before trying again. Response lost")
        XCTAssertFalse(fixture.session.isStale)
        XCTAssertEqual(corrections(fixture), 1)
    }

    @MainActor
    func testFailedReloadAfterRejectedCorrectionKeepsDraftAndMarksConnectionStale() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await openCompleted(fixture)
        let draft = try XCTUnwrap(timestamp("2024-12-30T08:00:00.000Z"))
        fixture.session.setWorklogCorrectionStart(draft)
        fixture.session.submitWorklogCorrection()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Overlap", kind: "worklog_overlap"))
        let reload = try await fixture.client.next()
        reload.fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        history.fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        try await fixture.settled()

        XCTAssertTrue(fixture.session.isStale)
        XCTAssertEqual(fixture.session.connectionMessage, "Server offline")
        XCTAssertEqual(fixture.session.worklogCorrection.error, "Overlap")
        XCTAssertEqual(fixture.session.worklogCorrection.start, draft)
        XCTAssertTrue(fixture.session.worklogCorrection.canSubmit)
    }

    @MainActor
    func testOpeningWhileTheEditorIsPresentedKeepsTheDraft() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        fixture.session.retryHistory()
        let history = try await fixture.client.next()
        history.succeed(HistoryPage(worklogs: [preciseLog], nextCursor: nil, reset: false))
        try await fixture.settled()
        fixture.session.openWorklogCorrection(worklogID: preciseLog.id)
        let draft = try XCTUnwrap(timestamp("2024-12-30T08:00:00.000Z"))
        fixture.session.setWorklogCorrectionStart(draft)

        fixture.session.openWorklogCorrection(worklogID: activeWorklog.id)
        fixture.session.openWorklogCorrection(worklogID: preciseLog.id)

        XCTAssertEqual(fixture.session.worklogCorrection.original, preciseLog)
        XCTAssertEqual(fixture.session.worklogCorrection.start, draft)
        XCTAssertTrue(fixture.session.worklogCorrection.canSubmit)
    }

    @MainActor
    func testHistoryReloadDoesNotChangeTheOpenDraft() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        let draft = try XCTUnwrap(timestamp("2024-12-30T08:00:00.000Z"))
        fixture.session.setWorklogCorrectionStart(draft)
        fixture.session.retryHistory()
        let changed = WorklogItem(
            id: log.id, taskId: log.taskId, start: log.start, end: "2024-12-30T10:30:00.000000Z")
        let history = try await fixture.client.next()
        history.succeed(HistoryPage(worklogs: [changed], nextCursor: nil, reset: false))
        try await fixture.settled()

        XCTAssertEqual(fixture.session.worklogs, [changed])
        XCTAssertEqual(fixture.session.worklogCorrection.original, log)
        XCTAssertEqual(fixture.session.worklogCorrection.start, draft)
        XCTAssertEqual(fixture.session.worklogCorrection.end, timestamp(log.end))
        XCTAssertNil(fixture.session.worklogCorrection.error)
        XCTAssertTrue(fixture.session.worklogCorrection.canSubmit)
    }

    @MainActor
    func testInvalidSubmissionPublishesItsValidationErrorToTheSheetObserver() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await openCompleted(fixture)
        fixture.session.setWorklogCorrectionStart(fixture.clock.now.addingTimeInterval(60))
        let harness = PresentationHarness(session: fixture.session)
        let changes = harness.counter(PresentationReads.worklogCorrection)
        let requests = fixture.client.operations.count

        fixture.session.submitWorklogCorrection()

        XCTAssertEqual(changes.count, 1)
        XCTAssertEqual(harness.presentation.worklogCorrectionSheetContent.error, "Start must not be in the future.")
        XCTAssertEqual(fixture.client.operations.count, requests)
    }

    @MainActor
    func testCorrectionObserverPublishesDraftWithoutRedrawingContent() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await openCompleted(fixture)
        let harness = PresentationHarness(session: fixture.session)
        let corrections = harness.counter(PresentationReads.worklogCorrection)
        let content = harness.counter(PresentationReads.content)
        fixture.session.setWorklogCorrectionStart(timestamp("2024-12-30T08:00:00.000Z")!)
        XCTAssertEqual(corrections.count, 1)
        XCTAssertEqual(content.count, 0)
    }

    @MainActor
    func testMinutePrecisionUsesFrozenLocalTimezoneForHistoricalSecondOffsets() throws {
        let state = WorklogCorrectionState()
        let timezone = TimeZone(identifier: "Europe/Paris")!
        let log = WorklogItem(
            id: "historical-log", taskId: firstTask.id,
            start: "1900-01-01T09:00:12.123456Z", end: "1900-01-01T11:00:45.654321Z")
        state.open(log, taskName: "Historical task", timezone: timezone)
        state.updateStart(timestamp("1900-01-01T10:00:00.000Z")!)
        let request = try XCTUnwrap(state.submit(at: timestamp("1900-01-02T00:00:00.000Z")!))
        XCTAssertEqual(request.replacementStart, "1900-01-01T09:59:39.000Z")
        XCTAssertEqual(request.replacementEnd, log.end)
        XCTAssertEqual(request.occurredAt, "1900-01-02T00:00:00.000Z")
        XCTAssertEqual(state.presentation.timezoneIdentifier, timezone.identifier)
    }

    @MainActor
    func testFailedHistoryReloadAfterCorrectionKeepsShownRowsAndMarksHistoryUnavailable() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        fixture.session.setWorklogCorrectionStart(timestamp("2024-12-30T08:00:00.000Z")!)
        fixture.session.submitWorklogCorrection()
        let command = try await fixture.client.next()
        let corrected = WorklogItem(id: log.id, taskId: log.taskId, start: "2024-12-30T08:00:00.000000Z", end: log.end)
        command.corrected(worklog: corrected, resources: resources)
        let history = try await fixture.client.next()
        XCTAssertEqual(fixture.session.worklogs, [log])
        XCTAssertTrue(fixture.session.isHistoryLoading)
        history.fail(BridgeFailure(message: "History unavailable"))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.worklogs, [log])
        XCTAssertTrue(fixture.session.historyUnavailable)
        XCTAssertFalse(fixture.session.isHistoryLoading)
        XCTAssertFalse(fixture.session.worklogCorrection.isPresented)
    }

    @MainActor
    func testCorrectionReloadsTheHistoryOfTheTaskSelectedAfterOpening() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        fixture.session.select(secondTask.id)
        let other = WorklogItem(
            id: "other-log", taskId: secondTask.id,
            start: log.start, end: log.end)
        let selectedHistory = try await fixture.client.next()
        selectedHistory.succeed(HistoryPage(worklogs: [other], nextCursor: "other-older", reset: false))
        try await fixture.settled()
        fixture.session.setWorklogCorrectionStart(timestamp("2024-12-30T08:00:00.000Z")!)
        fixture.session.submitWorklogCorrection()
        let command = try await fixture.client.next()
        XCTAssertEqual(
            command.operation,
            .correct(
                expected: log, start: "2024-12-30T08:00:00.000Z",
                end: log.end, at: "2025-01-01T00:00:00.000Z"))
        let corrected = WorklogItem(
            id: log.id, taskId: log.taskId,
            start: "2024-12-30T08:00:00.000000Z", end: log.end)
        command.corrected(worklog: corrected, resources: resources)
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .history(task: secondTask.id, cursor: nil))
        XCTAssertEqual(fixture.session.selectedTaskID, secondTask.id)
        XCTAssertEqual(fixture.session.worklogs, [other])
        XCTAssertEqual(fixture.session.nextCursor, "other-older")
        reload.succeed(HistoryPage(worklogs: [other], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.worklogs, [other])
        XCTAssertEqual(
            Array(fixture.client.operations.suffix(4)),
            [
                .history(task: secondTask.id, cursor: nil), command.operation,
                .refresh(fixture.session.connectionSettings),
                .history(task: secondTask.id, cursor: nil),
            ])
    }
}
