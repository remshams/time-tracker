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
    private func openCompleted(
        _ fixture: Fixture, worklog: WorklogItem? = nil, timezone: TimeZone = .current
    ) async throws -> WorklogItem {
        let log = worklog ?? preciseLog
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        fixture.sync.retryHistory()
        let history = try await fixture.client.next()
        history.succeed(HistoryPage(worklogs: [log], nextCursor: "older", reset: false))
        try await fixture.settled()
        fixture.worklogCorrection.open(worklogID: log.id, timezone: timezone)
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
    func testUnconfirmedConnectionKeepsTheDialogClosedEvenWithARunningWorklog() async throws {
        let clock = FakeClock()
        let client = FakeClient()
        let tracking = TrackingStore()
        tracking.apply(activeWorklog, clock: clock)
        let correction = WorklogCorrectionStore(
            client: client, clock: clock, connection: ConnectionStore(settings: .local), catalog: TaskCatalogStore(),
            history: WorklogHistoryStore(client: client), tracking: tracking, reloadAfterWrite: {})
        XCTAssertFalse(correction.canOpen)
        correction.open(worklogID: activeWorklog.id)
        XCTAssertFalse(correction.isPresented)
    }

    @MainActor
    func testCancellationKeepsTheContentUntilAnotherWorklogOpens() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        let draft = try XCTUnwrap(parseTimestamp("2024-12-30T11:00:00.000Z"))
        fixture.worklogCorrection.setStart(draft)
        fixture.worklogCorrection.submit()

        fixture.worklogCorrection.cancel()
        XCTAssertFalse(fixture.worklogCorrection.isPresented)
        XCTAssertFalse(fixture.worklogCorrection.canEdit)
        XCTAssertFalse(fixture.worklogCorrection.canSubmit)
        XCTAssertEqual(fixture.worklogCorrection.original, log)
        XCTAssertEqual(fixture.worklogCorrection.taskName, firstTask.name)
        XCTAssertEqual(fixture.worklogCorrection.start, draft)
        XCTAssertEqual(fixture.worklogCorrection.error, "End must be after Start.")
        XCTAssertTrue(fixture.taskCreation.canOpen)

        let operations = fixture.client.operations.count
        fixture.worklogCorrection.setStart(fixture.clock.now)
        fixture.worklogCorrection.submit()
        XCTAssertEqual(fixture.worklogCorrection.start, draft)
        XCTAssertEqual(fixture.client.operations.count, operations)

        fixture.sync.refresh()
        let refresh = try await fixture.client.next()
        let running = WorklogItem(
            id: "running-log", taskId: secondTask.id,
            start: "2024-12-31T23:58:12.123456Z", end: nil)
        refresh.succeed(TaskListResources(tasks: [firstTask, secondTask], active: running))
        try await fixture.settled()
        XCTAssertEqual(fixture.worklogCorrection.original, log)

        fixture.worklogCorrection.open(worklogID: running.id)
        XCTAssertTrue(fixture.worklogCorrection.isPresented)
        XCTAssertEqual(fixture.worklogCorrection.original, running)
        XCTAssertNil(fixture.worklogCorrection.error)
        XCTAssertEqual(fixture.worklogCorrection.taskName, secondTask.name)
        XCTAssertEqual(fixture.worklogCorrection.start, parseTimestamp(running.start))
        XCTAssertNil(fixture.worklogCorrection.end)
    }

    @MainActor
    func testSuccessfulSaveKeepsTheContentDuringDismissal() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        let draft = try XCTUnwrap(parseTimestamp("2024-12-30T08:45:00.000Z"))
        fixture.worklogCorrection.setStart(draft)
        fixture.worklogCorrection.submit()
        let command = try await fixture.client.next()
        XCTAssertTrue(fixture.worklogCorrection.isSubmitting)
        let corrected = WorklogItem(
            id: log.id, taskId: log.taskId,
            start: "2024-12-30T08:45:00.000000Z", end: log.end)
        command.corrected(worklog: corrected, resources: resources)
        let history = try await fixture.client.next()
        XCTAssertFalse(fixture.worklogCorrection.isPresented)
        XCTAssertFalse(fixture.worklogCorrection.isSubmitting)
        XCTAssertFalse(fixture.worklogCorrection.canSubmit)
        XCTAssertEqual(fixture.worklogCorrection.original, log)
        XCTAssertEqual(fixture.worklogCorrection.start, draft)
        history.succeed(HistoryPage(worklogs: [corrected], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertEqual(fixture.worklogCorrection.start, draft)
        fixture.worklogCorrection.open(worklogID: corrected.id)
        XCTAssertEqual(fixture.worklogCorrection.original, corrected)
        XCTAssertFalse(fixture.worklogCorrection.isSubmitting)
    }

    @MainActor
    func testCompletedEditSendsCorrectionFirstThenReloadsTaskListAndHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        XCTAssertEqual(fixture.worklogCorrection.original, log)
        XCTAssertEqual(fixture.worklogCorrection.taskName, firstTask.name)
        XCTAssertFalse(fixture.worklogCorrection.canSubmit)
        fixture.worklogCorrection.setStart(parseTimestamp("2024-12-30T08:45:59.999Z")!)
        fixture.worklogCorrection.submit()
        let command = try await fixture.client.next()
        XCTAssertEqual(
            command.operation,
            .correct(
                expected: log, start: "2024-12-30T08:45:00.000Z",
                end: log.end, at: "2025-01-01T00:00:00.000Z"))
        fixture.worklogCorrection.cancel()
        fixture.worklogCorrection.setEnd(fixture.clock.now)
        XCTAssertTrue(fixture.worklogCorrection.isPresented)
        XCTAssertEqual(fixture.worklogCorrection.end, parseTimestamp(log.end))
        let corrected = WorklogItem(
            id: log.id, taskId: log.taskId,
            start: "2024-12-30T08:45:00.000000Z", end: log.end)
        command.corrected(worklog: corrected, resources: resources)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: log.taskId, cursor: nil))
        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [log])
        history.succeed(HistoryPage(worklogs: [corrected], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertFalse(fixture.worklogCorrection.isPresented)
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
        fixture.worklogCorrection.setStart(parseTimestamp("2024-12-30T08:00:00.000Z")!)
        fixture.worklogCorrection.setStart(parseTimestamp(log.start)!)
        fixture.worklogCorrection.setEnd(parseTimestamp(log.end)!)
        XCTAssertFalse(fixture.worklogCorrection.canSubmit)
        XCTAssertEqual(fixture.worklogCorrection.start, parseTimestamp(log.start))
        XCTAssertEqual(fixture.worklogCorrection.end, parseTimestamp(log.end))
        let operations = fixture.client.operations.count
        fixture.worklogCorrection.submit()
        fixture.worklogCorrection.cancel()
        XCTAssertEqual(fixture.client.operations.count, operations)
        XCTAssertFalse(fixture.worklogCorrection.isPresented)
    }

    @MainActor
    func testRunningEditChangesOnlyStartAndReanchorsElapsedTime() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let initial = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(initial)
        fixture.worklogCorrection.open(worklogID: activeWorklog.id)
        fixture.worklogCorrection.setEnd(fixture.clock.now)
        XCTAssertNil(fixture.worklogCorrection.end)
        fixture.worklogCorrection.setStart(parseTimestamp("2024-12-31T23:58:00.000Z")!)
        fixture.worklogCorrection.submit()
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
        XCTAssertFalse(fixture.worklogCorrection.isPresented)
    }

    @MainActor
    func testInvalidIntervalsStayOpenWithDraftAndMakeNoRequest() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await openCompleted(fixture)
        let operations = fixture.client.operations.count
        fixture.worklogCorrection.setStart(fixture.clock.now.addingTimeInterval(60))
        fixture.worklogCorrection.submit()
        XCTAssertEqual(fixture.worklogCorrection.error, "Start must not be in the future.")
        fixture.worklogCorrection.setStart(parseTimestamp("2024-12-30T11:00:00.000Z")!)
        XCTAssertNil(fixture.worklogCorrection.error)
        fixture.worklogCorrection.submit()
        XCTAssertEqual(fixture.worklogCorrection.error, "End must be after Start.")
        fixture.worklogCorrection.setEnd(fixture.clock.now.addingTimeInterval(60))
        XCTAssertNil(fixture.worklogCorrection.error)
        fixture.worklogCorrection.submit()
        XCTAssertEqual(fixture.worklogCorrection.error, "End must not be in the future.")
        XCTAssertTrue(fixture.worklogCorrection.isPresented)
        XCTAssertFalse(fixture.worklogCorrection.isSubmitting)
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
            fixture.worklogCorrection.setStart(start)

            fixture.worklogCorrection.submit()
            try await fixture.settled()

            XCTAssertEqual(fixture.worklogCorrection.error, "End must be after Start.")
            XCTAssertTrue(fixture.worklogCorrection.isPresented)
            XCTAssertTrue(fixture.worklogCorrection.canEdit)
            XCTAssertFalse(fixture.worklogCorrection.isSubmitting)
            XCTAssertEqual(fixture.worklogCorrection.original, original)
            XCTAssertEqual(fixture.worklogCorrection.start, start)
            XCTAssertEqual(fixture.worklogCorrection.end, parseTimestamp(end))
            XCTAssertEqual(fixture.client.operations.count, operations)
        }
    }

    @MainActor
    func testCorrectionAllowsOneMicrosecondOfDuration() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let original = WorklogItem(
            id: "precise-log", taskId: firstTask.id,
            start: "2024-12-30T09:00:00.000000Z", end: "2024-12-30T10:00:00.000001Z")
        _ = try await openCompleted(fixture, worklog: original)
        fixture.worklogCorrection.setStart(try XCTUnwrap(parseTimestamp("2024-12-30T10:00:00.000000Z")))

        fixture.worklogCorrection.submit()

        let command = try await fixture.client.next()
        XCTAssertEqual(
            command.operation,
            .correct(
                expected: original, start: "2024-12-30T10:00:00.000Z",
                end: original.end, at: "2025-01-01T00:00:00.000Z"))
        XCTAssertNil(fixture.worklogCorrection.error)
    }

    @MainActor
    func testSubmitIsDisabledWhileSubmittingAndTaskCreationAndTrackingStayUsable() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        let draft = try XCTUnwrap(parseTimestamp("2024-12-30T08:00:00.000Z"))
        fixture.worklogCorrection.setStart(draft)
        fixture.worklogCorrection.submit()
        let command = try await fixture.client.next()
        XCTAssertTrue(fixture.worklogCorrection.isSubmitting)
        XCTAssertFalse(fixture.worklogCorrection.canSubmit)
        XCTAssertFalse(fixture.worklogCorrection.canEdit)
        fixture.worklogCorrection.submit()
        fixture.worklogCorrection.setStart(try XCTUnwrap(parseTimestamp("2024-12-30T07:00:00.000Z")))
        fixture.worklogCorrection.cancel()
        XCTAssertTrue(fixture.worklogCorrection.isPresented)
        XCTAssertEqual(fixture.worklogCorrection.start, draft)
        XCTAssertTrue(fixture.taskCreation.canOpen)
        XCTAssertTrue(fixture.worklogMove.canOpen)
        XCTAssertTrue(fixture.worklogDeletion.canOpen)
        XCTAssertTrue(fixture.trackingAvailability.canStart(taskID: secondTask.id))

        fixture.taskCreation.open()
        fixture.taskCreation.setName("Parallel task")
        fixture.taskCreation.submit()
        let create = try await fixture.client.next()
        XCTAssertEqual(create.operation, .create(name: "Parallel task", at: "2025-01-01T00:00:00.000Z"))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 2)
        XCTAssertTrue(fixture.taskCreation.isSubmitting)
        XCTAssertTrue(fixture.worklogCorrection.isSubmitting)

        let corrected = WorklogItem(
            id: log.id, taskId: log.taskId, start: "2024-12-30T08:00:00.000000Z", end: log.end)
        command.corrected(worklog: corrected, resources: resources)
        try await finishHistory(fixture, rows: [corrected])
        XCTAssertFalse(fixture.worklogCorrection.isPresented)
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
        fixture.worklogCorrection.setStart(draft)
        fixture.worklogCorrection.submit()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "The worklog changed on another client.", kind: "worklog_changed"))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connection.settings))
        XCTAssertFalse(fixture.worklogCorrection.isSubmitting)
        XCTAssertEqual(fixture.worklogCorrection.error, "The worklog changed on another client.")
        reload.succeed(resources)
        let changed = WorklogItem(
            id: log.id, taskId: log.taskId, start: log.start, end: "2024-12-30T10:30:00.000000Z")
        try await finishHistory(fixture, rows: [changed])

        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [changed])
        XCTAssertTrue(fixture.worklogCorrection.isPresented)
        XCTAssertEqual(fixture.worklogCorrection.original, log)
        XCTAssertEqual(fixture.worklogCorrection.start, draft)
        XCTAssertEqual(fixture.worklogCorrection.end, parseTimestamp(log.end))
        XCTAssertEqual(fixture.worklogCorrection.error, "The worklog changed on another client.")
        XCTAssertTrue(fixture.worklogCorrection.canEdit)
        XCTAssertTrue(fixture.worklogCorrection.canSubmit)
        XCTAssertFalse(fixture.session.connection.stale)

        fixture.clock.now.addTimeInterval(90)
        fixture.worklogCorrection.submit()
        XCTAssertNil(fixture.worklogCorrection.error)
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
            fixture.worklogCorrection.setStart(draft)
            fixture.worklogCorrection.submit()
            let command = try await fixture.client.next()
            command.fail(BridgeFailure(message: "Correction response unavailable", kind: kind))
            let reload = try await fixture.client.next()
            XCTAssertEqual(reload.operation, .refresh(fixture.session.connection.settings), kind)
            reload.succeed(resources)
            try await finishHistory(fixture, rows: [log])

            XCTAssertTrue(fixture.worklogCorrection.canEdit, kind)
            XCTAssertTrue(fixture.worklogCorrection.canSubmit, kind)
            XCTAssertEqual(fixture.worklogCorrection.start, draft, kind)
            XCTAssertEqual(fixture.worklogCorrection.error, "Correction response unavailable", kind)
        }
    }

    @MainActor
    func testUncertainFailureAsksTheUserToCheckTheReloadedData() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        fixture.worklogCorrection.setStart(parseTimestamp("2024-12-30T08:00:00.000Z")!)
        fixture.worklogCorrection.submit()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Response lost", uncertain: true))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connection.settings))
        reload.succeed(resources)
        let corrected = WorklogItem(id: log.id, taskId: log.taskId, start: "2024-12-30T08:00:00.000000Z", end: log.end)
        try await finishHistory(fixture, rows: [corrected])

        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [corrected])
        XCTAssertTrue(fixture.worklogCorrection.isPresented)
        XCTAssertTrue(fixture.worklogCorrection.canEdit)
        XCTAssertEqual(
            fixture.worklogCorrection.error,
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
        fixture.worklogCorrection.setStart(draft)
        fixture.worklogCorrection.submit()
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
        XCTAssertEqual(fixture.worklogCorrection.error, "Overlap")
        XCTAssertEqual(fixture.worklogCorrection.start, draft)
        XCTAssertTrue(fixture.worklogCorrection.canSubmit)
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
        fixture.worklogCorrection.open(worklogID: preciseLog.id)
        let draft = try XCTUnwrap(parseTimestamp("2024-12-30T08:00:00.000Z"))
        fixture.worklogCorrection.setStart(draft)

        fixture.worklogCorrection.open(worklogID: activeWorklog.id)
        fixture.worklogCorrection.open(worklogID: preciseLog.id)

        XCTAssertEqual(fixture.worklogCorrection.original, preciseLog)
        XCTAssertEqual(fixture.worklogCorrection.start, draft)
        XCTAssertTrue(fixture.worklogCorrection.canSubmit)
    }

    @MainActor
    func testHistoryReloadDoesNotChangeTheOpenDraft() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        let draft = try XCTUnwrap(parseTimestamp("2024-12-30T08:00:00.000Z"))
        fixture.worklogCorrection.setStart(draft)
        fixture.sync.retryHistory()
        let changed = WorklogItem(
            id: log.id, taskId: log.taskId, start: log.start, end: "2024-12-30T10:30:00.000000Z")
        let history = try await fixture.client.next()
        history.succeed(HistoryPage(worklogs: [changed], nextCursor: nil, reset: false))
        try await fixture.settled()

        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [changed])
        XCTAssertEqual(fixture.worklogCorrection.original, log)
        XCTAssertEqual(fixture.worklogCorrection.start, draft)
        XCTAssertEqual(fixture.worklogCorrection.end, parseTimestamp(log.end))
        XCTAssertNil(fixture.worklogCorrection.error)
        XCTAssertTrue(fixture.worklogCorrection.canSubmit)
    }

    @MainActor
    func testInvalidSubmissionShowsItsValidationErrorInTheSheet() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await openCompleted(fixture)
        fixture.worklogCorrection.setStart(fixture.clock.now.addingTimeInterval(60))
        let harness = PresentationHarness(session: fixture.session)
        let sheet = ObservationProbe { PresentationReads.worklogCorrection(harness) }
        let requests = fixture.client.operations.count

        fixture.worklogCorrection.submit()

        XCTAssertTrue(sheet.consume())
        XCTAssertEqual(fixture.worklogCorrection.error, "Start must not be in the future.")
        XCTAssertEqual(fixture.client.operations.count, requests)
    }

    @MainActor
    func testDraftEditRedrawsTheSheetWithoutRedrawingContent() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await openCompleted(fixture)
        let harness = PresentationHarness(session: fixture.session)
        let sheet = ObservationProbe { PresentationReads.worklogCorrection(harness) }
        let content = ObservationProbe { PresentationReads.content(harness) }
        fixture.worklogCorrection.setStart(parseTimestamp("2024-12-30T08:00:00.000Z")!)
        XCTAssertTrue(sheet.consume())
        XCTAssertFalse(content.consume())
    }

    @MainActor
    func testMinutePrecisionUsesFrozenLocalTimezoneForHistoricalSecondOffsets() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let timezone = TimeZone(identifier: "Europe/Paris")!
        let log = WorklogItem(
            id: "historical-log", taskId: firstTask.id,
            start: "1900-01-01T09:00:12.123456Z", end: "1900-01-01T11:00:45.654321Z")
        _ = try await openCompleted(fixture, worklog: log, timezone: timezone)
        XCTAssertEqual(fixture.worklogCorrection.timezone.identifier, timezone.identifier)
        fixture.worklogCorrection.setStart(parseTimestamp("1900-01-01T10:00:00.000Z")!)

        fixture.worklogCorrection.submit()

        let command = try await fixture.client.next()
        XCTAssertEqual(
            command.operation,
            .correct(
                expected: log, start: "1900-01-01T09:59:39.000Z",
                end: log.end, at: "2025-01-01T00:00:00.000Z"))
    }

    @MainActor
    func testFailedHistoryReloadAfterCorrectionKeepsShownRowsAndMarksHistoryUnavailable() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await openCompleted(fixture)
        fixture.worklogCorrection.setStart(parseTimestamp("2024-12-30T08:00:00.000Z")!)
        fixture.worklogCorrection.submit()
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
        XCTAssertFalse(fixture.worklogCorrection.isPresented)
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
        fixture.worklogCorrection.setStart(parseTimestamp("2024-12-30T08:00:00.000Z")!)
        fixture.worklogCorrection.submit()
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
