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
        fixture.sync.retryHistory()
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
            !fixture.session.worklogHistory.isLoading
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
        fixture.session.setWorklogCorrectionStart(parseTimestamp("2024-12-30T08:45:00.000Z")!)
        let visible = fixture.session.worklogCorrection
        XCTAssertEqual(harness.presentation.worklogCorrectionSheetContent, visible)

        fixture.session.cancelWorklogCorrection()
        XCTAssertFalse(fixture.session.worklogCorrection.isPresented)
        XCTAssertNil(fixture.session.worklogCorrection.original)
        XCTAssertTrue(fixture.taskCreation.canOpen)
        XCTAssertEqual(harness.presentation.worklogCorrectionSheetContent, visible)
        fixture.sync.refresh()
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
        fixture.session.setWorklogCorrectionStart(parseTimestamp("2024-12-30T08:45:00.000Z")!)
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
        fixture.session.setWorklogCorrectionStart(parseTimestamp("2024-12-30T08:45:59.999Z")!)
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
        XCTAssertEqual(fixture.session.worklogCorrection.end, parseTimestamp(log.end))
        let corrected = WorklogItem(
            id: log.id, taskId: log.taskId,
            start: "2024-12-30T08:45:00.000000Z", end: log.end)
        command.corrected(worklog: corrected, resources: resources)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: log.taskId, cursor: nil))
        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [log])
        history.succeed(HistoryPage(worklogs: [corrected], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertFalse(fixture.session.worklogCorrection.isPresented)
        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [corrected])
        XCTAssertNil(fixture.session.worklogHistory.nextCursor)
        XCTAssertEqual(
            Array(fixture.client.operations.suffix(3)),
            [command.operation, .refresh(fixture.session.connection.settings), .history(task: log.taskId, cursor: nil)])
    }

    @MainActor
    func testReturningToOriginalMinuteKeepsExactOriginalStringsAndDisablesSave() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        fixture.session.setWorklogCorrectionStart(parseTimestamp("2024-12-30T08:00:00.000Z")!)
        fixture.session.setWorklogCorrectionStart(parseTimestamp(log.start)!)
        fixture.session.setWorklogCorrectionEnd(parseTimestamp(log.end)!)
        XCTAssertFalse(fixture.session.worklogCorrection.canSubmit)
        XCTAssertEqual(fixture.session.worklogCorrection.start, parseTimestamp(log.start))
        XCTAssertEqual(fixture.session.worklogCorrection.end, parseTimestamp(log.end))
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
        fixture.session.setWorklogCorrectionStart(parseTimestamp("2024-12-31T23:58:00.000Z")!)
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
        XCTAssertEqual(fixture.session.tracking.elapsed(clock: fixture.session.clock), 120)
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
        fixture.session.setWorklogCorrectionStart(parseTimestamp("2024-12-30T11:00:00.000Z")!)
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
            let start = try XCTUnwrap(parseTimestamp("2024-12-30T10:00:00.000000Z"))
            fixture.session.setWorklogCorrectionStart(start)

            fixture.session.submitWorklogCorrection()
            try await fixture.settled()

            XCTAssertEqual(fixture.session.worklogCorrection.error, "End must be after Start.")
            XCTAssertTrue(fixture.session.worklogCorrection.isPresented)
            XCTAssertTrue(fixture.session.worklogCorrection.canEdit)
            XCTAssertFalse(fixture.session.worklogCorrection.isSubmitting)
            XCTAssertEqual(fixture.session.worklogCorrection.original, original)
            XCTAssertEqual(fixture.session.worklogCorrection.start, start)
            XCTAssertEqual(fixture.session.worklogCorrection.end, parseTimestamp(end))
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
        state.updateStart(try XCTUnwrap(parseTimestamp("2024-12-30T10:00:00.000000Z")))

        let request = try XCTUnwrap(state.submit(at: try XCTUnwrap(parseTimestamp("2025-01-01T00:00:00.000000Z"))))

        XCTAssertEqual(request.expected, original)
        XCTAssertEqual(request.replacementEnd, original.end)
        XCTAssertTrue(sameWorklogTimestamp(request.replacementStart, "2024-12-30T10:00:00.000000Z"))
    }

    @MainActor
    func testSubmitIsDisabledWhileSubmittingAndTaskCreationAndTrackingStayUsable() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        let draft = try XCTUnwrap(parseTimestamp("2024-12-30T08:00:00.000Z"))
        fixture.session.setWorklogCorrectionStart(draft)
        fixture.session.submitWorklogCorrection()
        let command = try await fixture.client.next()
        XCTAssertTrue(fixture.session.worklogCorrection.isSubmitting)
        XCTAssertFalse(fixture.session.worklogCorrection.canSubmit)
        XCTAssertFalse(fixture.session.worklogCorrection.canEdit)
        fixture.session.submitWorklogCorrection()
        fixture.session.setWorklogCorrectionStart(try XCTUnwrap(parseTimestamp("2024-12-30T07:00:00.000Z")))
        fixture.session.cancelWorklogCorrection()
        XCTAssertTrue(fixture.session.worklogCorrection.isPresented)
        XCTAssertEqual(fixture.session.worklogCorrection.start, draft)
        XCTAssertTrue(fixture.taskCreation.canOpen)
        XCTAssertTrue(fixture.session.canOpenWorklogMove)
        XCTAssertTrue(fixture.worklogDeletion.canOpen)
        XCTAssertTrue(fixture.trackingAvailability.canStart(taskID: secondTask.id))

        fixture.taskCreation.open()
        fixture.taskCreation.setName("Parallel task")
        fixture.taskCreation.submit()
        let create = try await fixture.client.next()
        XCTAssertEqual(create.operation, .create(name: "Parallel task", at: "2025-01-01T00:00:00.000Z"))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 2)
        XCTAssertTrue(fixture.taskCreation.isSubmitting)
        XCTAssertTrue(fixture.session.worklogCorrection.isSubmitting)

        let corrected = WorklogItem(
            id: log.id, taskId: log.taskId, start: "2024-12-30T08:00:00.000000Z", end: log.end)
        command.corrected(worklog: corrected, resources: resources)
        try await finishHistory(fixture, rows: [corrected])
        XCTAssertFalse(fixture.session.worklogCorrection.isPresented)
        XCTAssertTrue(fixture.taskCreation.isSubmitting)
        XCTAssertEqual(corrections(fixture), 1)

        let created = TaskItem(id: "task-created", name: "Parallel task", archived: false, latestStart: nil)
        try await fixture.update("the created task") {
            create.created(
                taskID: created.id, resources: TaskListResources(tasks: [firstTask, secondTask, created], active: nil))
        } until: {
            !fixture.taskCreation.isSubmitting
        }
        XCTAssertFalse(fixture.taskCreation.isPresented)
        XCTAssertEqual(fixture.session.tasks, [firstTask, secondTask, created])
    }

    @MainActor
    func testFailureShowsServerMessageKeepsDraftReloadsAndResubmitUsesNewTimestamp() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        let draft = try XCTUnwrap(parseTimestamp("2024-12-30T08:00:00.000Z"))
        fixture.session.setWorklogCorrectionStart(draft)
        fixture.session.submitWorklogCorrection()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "The worklog changed on another client.", kind: "worklog_changed"))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connection.settings))
        XCTAssertFalse(fixture.session.worklogCorrection.isSubmitting)
        XCTAssertEqual(fixture.session.worklogCorrection.error, "The worklog changed on another client.")
        reload.succeed(resources)
        let changed = WorklogItem(
            id: log.id, taskId: log.taskId, start: log.start, end: "2024-12-30T10:30:00.000000Z")
        try await finishHistory(fixture, rows: [changed])

        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [changed])
        XCTAssertTrue(fixture.session.worklogCorrection.isPresented)
        XCTAssertEqual(fixture.session.worklogCorrection.original, log)
        XCTAssertEqual(fixture.session.worklogCorrection.start, draft)
        XCTAssertEqual(fixture.session.worklogCorrection.end, parseTimestamp(log.end))
        XCTAssertEqual(fixture.session.worklogCorrection.error, "The worklog changed on another client.")
        XCTAssertTrue(fixture.session.worklogCorrection.canEdit)
        XCTAssertTrue(fixture.session.worklogCorrection.canSubmit)
        XCTAssertFalse(fixture.session.connection.stale)

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
            let draft = try XCTUnwrap(parseTimestamp("2024-12-30T08:00:00.000000Z"))
            fixture.session.setWorklogCorrectionStart(draft)
            fixture.session.submitWorklogCorrection()
            let command = try await fixture.client.next()
            command.fail(BridgeFailure(message: "Correction response unavailable", kind: kind))
            let reload = try await fixture.client.next()
            XCTAssertEqual(reload.operation, .refresh(fixture.session.connection.settings), kind)
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
        fixture.session.setWorklogCorrectionStart(parseTimestamp("2024-12-30T08:00:00.000Z")!)
        fixture.session.submitWorklogCorrection()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Response lost", uncertain: true))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connection.settings))
        reload.succeed(resources)
        let corrected = WorklogItem(id: log.id, taskId: log.taskId, start: "2024-12-30T08:00:00.000000Z", end: log.end)
        try await finishHistory(fixture, rows: [corrected])

        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [corrected])
        XCTAssertTrue(fixture.session.worklogCorrection.isPresented)
        XCTAssertTrue(fixture.session.worklogCorrection.canEdit)
        XCTAssertEqual(
            fixture.session.worklogCorrection.error,
            "Could not confirm the change. Check the reloaded data before trying again. Response lost")
        XCTAssertFalse(fixture.session.connection.stale)
        XCTAssertEqual(corrections(fixture), 1)
    }

    @MainActor
    func testFailedReloadAfterRejectedCorrectionKeepsDraftAndMarksConnectionStale() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await openCompleted(fixture)
        let draft = try XCTUnwrap(parseTimestamp("2024-12-30T08:00:00.000Z"))
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

        XCTAssertTrue(fixture.session.connection.stale)
        XCTAssertEqual(fixture.session.connection.message, "Server offline")
        XCTAssertEqual(fixture.session.worklogCorrection.error, "Overlap")
        XCTAssertEqual(fixture.session.worklogCorrection.start, draft)
        XCTAssertTrue(fixture.session.worklogCorrection.canSubmit)
    }

    @MainActor
    func testOpeningWhileTheEditorIsPresentedKeepsTheDraft() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        fixture.sync.retryHistory()
        let history = try await fixture.client.next()
        history.succeed(HistoryPage(worklogs: [preciseLog], nextCursor: nil, reset: false))
        try await fixture.settled()
        fixture.session.openWorklogCorrection(worklogID: preciseLog.id)
        let draft = try XCTUnwrap(parseTimestamp("2024-12-30T08:00:00.000Z"))
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
        let draft = try XCTUnwrap(parseTimestamp("2024-12-30T08:00:00.000Z"))
        fixture.session.setWorklogCorrectionStart(draft)
        fixture.sync.retryHistory()
        let changed = WorklogItem(
            id: log.id, taskId: log.taskId, start: log.start, end: "2024-12-30T10:30:00.000000Z")
        let history = try await fixture.client.next()
        history.succeed(HistoryPage(worklogs: [changed], nextCursor: nil, reset: false))
        try await fixture.settled()

        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [changed])
        XCTAssertEqual(fixture.session.worklogCorrection.original, log)
        XCTAssertEqual(fixture.session.worklogCorrection.start, draft)
        XCTAssertEqual(fixture.session.worklogCorrection.end, parseTimestamp(log.end))
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
        fixture.session.setWorklogCorrectionStart(parseTimestamp("2024-12-30T08:00:00.000Z")!)
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
        state.updateStart(parseTimestamp("1900-01-01T10:00:00.000Z")!)
        let request = try XCTUnwrap(state.submit(at: parseTimestamp("1900-01-02T00:00:00.000Z")!))
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
        fixture.session.setWorklogCorrectionStart(parseTimestamp("2024-12-30T08:00:00.000Z")!)
        fixture.session.submitWorklogCorrection()
        let command = try await fixture.client.next()
        let corrected = WorklogItem(id: log.id, taskId: log.taskId, start: "2024-12-30T08:00:00.000000Z", end: log.end)
        command.corrected(worklog: corrected, resources: resources)
        let history = try await fixture.client.next()
        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [log])
        XCTAssertTrue(fixture.session.worklogHistory.isLoading)
        history.fail(BridgeFailure(message: "History unavailable"))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [log])
        XCTAssertTrue(fixture.session.worklogHistory.unavailable)
        XCTAssertFalse(fixture.session.worklogHistory.isLoading)
        XCTAssertFalse(fixture.session.worklogCorrection.isPresented)
    }

    @MainActor
    func testCorrectionReloadsTheHistoryOfTheTaskSelectedAfterOpening() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        fixture.sync.selectTask(secondTask.id)
        let other = WorklogItem(
            id: "other-log", taskId: secondTask.id,
            start: log.start, end: log.end)
        let selectedHistory = try await fixture.client.next()
        selectedHistory.succeed(HistoryPage(worklogs: [other], nextCursor: "other-older", reset: false))
        try await fixture.settled()
        fixture.session.setWorklogCorrectionStart(parseTimestamp("2024-12-30T08:00:00.000Z")!)
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
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, secondTask.id)
        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [other])
        XCTAssertEqual(fixture.session.worklogHistory.nextCursor, "other-older")
        reload.succeed(HistoryPage(worklogs: [other], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [other])
        XCTAssertEqual(
            Array(fixture.client.operations.suffix(4)),
            [
                .history(task: secondTask.id, cursor: nil), command.operation,
                .refresh(fixture.session.connection.settings),
                .history(task: secondTask.id, cursor: nil),
            ])
    }
}
