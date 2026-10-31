import Foundation
import XCTest
@testable import TrackerClient

final class WorklogMoveTests: XCTestCase {
    private let precise = WorklogItem(
        id: "precise", taskId: firstTask.id,
        start: "2024-12-30T09:00:12.123456Z", end: "2024-12-30T10:00:45.654321Z")
    private let candidate = WorklogMoveCandidate(id: secondTask.id, name: secondTask.name)
    private let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)

    @MainActor
    private func open(_ fixture: Fixture, worklog: WorklogItem? = nil) async throws -> WorklogItem {
        let log = worklog ?? precise
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: log.end == nil ? log : nil))
        if log.end != nil {
            fixture.session.retryHistory()
            let page = try await fixture.client.next()
            page.succeed(HistoryPage(worklogs: [log], nextCursor: "older", reset: false))
            try await fixture.settled()
        }
        fixture.session.openWorklogMove(worklogID: log.id)
        XCTAssertEqual(fixture.session.worklogMove.sourceTaskName, firstTask.name)
        XCTAssertTrue(fixture.session.worklogMove.isSearching)
        let search = try await fixture.client.next()
        XCTAssertEqual(search.operation, .candidates(source: firstTask.id, query: ""))
        try await loadCandidates(fixture, search)
        return log
    }

    /// Answers a candidate search and waits until the session publishes the result.
    @MainActor
    private func loadCandidates(
        _ fixture: Fixture, _ request: FakeClient.Request, _ values: [WorklogMoveCandidate]? = nil
    ) async throws {
        try await fixture.update("the move candidates") {
            request.candidates(values ?? [candidate])
        } until: {
            !fixture.session.worklogMove.isSearching
        }
    }

    @MainActor
    private func finishHistory(
        _ fixture: Fixture, task: String = firstTask.id, rows: [WorklogItem] = []
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
    private func moves(_ fixture: Fixture) -> Int {
        fixture.client.operations.filter {
            if case .move = $0 { return true }
            return false
        }.count
    }

    @MainActor
    private func searches(_ fixture: Fixture) -> Int {
        fixture.client.operations.filter {
            if case .candidates = $0 { return true }
            return false
        }.count
    }

    @MainActor
    func testSuccessfulMoveClosesTheEditorBeforeReloadingAndTheRowLeavesAfterTheHistoryReload() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture)
        fixture.session.retryHistory()
        let initial = try await fixture.client.next()
        initial.succeed(HistoryPage(worklogs: [log, oldWorklog], nextCursor: "older", reset: false))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.worklogMove.canSubmit)
        fixture.session.submitWorklogMove()
        let command = try await fixture.client.next()
        XCTAssertEqual(command.operation, .move(expected: log, destination: secondTask.id))
        let harness = PresentationHarness(session: fixture.session)
        let moveDismissals = harness.recorder(PresentationReads.worklogMove) {
            !harness.presentation.worklogMove.isPresented
        }
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: log.end)
        command.moved(worklog: moved)
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connectionSettings))
        XCTAssertFalse(fixture.session.worklogMove.isPresented)
        XCTAssertEqual(moveDismissals.values.last, true)
        XCTAssertEqual(fixture.session.worklogs, [log, oldWorklog])
        reload.succeed(resources)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        XCTAssertEqual(fixture.session.worklogs, [log, oldWorklog])
        XCTAssertTrue(fixture.session.isHistoryLoading)
        history.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.worklogs, [oldWorklog])
        XCTAssertEqual(
            Array(fixture.client.operations.suffix(3)),
            [
                command.operation, .refresh(fixture.session.connectionSettings),
                .history(task: firstTask.id, cursor: nil),
            ])
    }

    @MainActor
    func testFailedHistoryReloadAfterMoveKeepsShownRowsAndMarksHistoryUnavailable() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture)
        fixture.session.submitWorklogMove()
        let command = try await fixture.client.next()
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: log.end)
        command.moved(worklog: moved, resources: resources)
        let history = try await fixture.client.next()
        XCTAssertFalse(fixture.session.worklogMove.isPresented)
        XCTAssertEqual(fixture.session.worklogs, [log])
        XCTAssertTrue(fixture.session.isHistoryLoading)
        history.fail(BridgeFailure(message: "History unavailable"))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.worklogs, [log])
        XCTAssertTrue(fixture.session.historyUnavailable)
        XCTAssertFalse(fixture.session.isHistoryLoading)
        XCTAssertEqual(moves(fixture), 1)
    }

    @MainActor
    func testMoveKeepsSelectedDestinationRowsUntilTheHistoryReloadReplacesThem() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        fixture.session.select(secondTask.id)
        let destination = try await fixture.client.next()
        let retained = WorklogItem(
            id: "destination-history", taskId: secondTask.id,
            start: oldWorklog.start, end: oldWorklog.end)
        destination.succeed(HistoryPage(worklogs: [retained], nextCursor: "old-destination", reset: false))
        try await fixture.settled()
        fixture.session.submitWorklogMove()
        let command = try await fixture.client.next()
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: nil)
        command.moved(worklog: moved, resources: TaskListResources(tasks: [firstTask, secondTask], active: moved))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: secondTask.id, cursor: nil))
        XCTAssertEqual(fixture.session.worklogs, [retained])
        XCTAssertEqual(fixture.session.nextCursor, "old-destination")
        XCTAssertTrue(fixture.session.isHistoryLoading)
        history.succeed(HistoryPage(worklogs: [moved, retained], nextCursor: "new-destination", reset: false))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.worklogs, [moved, retained])
        XCTAssertEqual(fixture.session.nextCursor, "new-destination")
        XCTAssertTrue(fixture.session.hasMoreHistory)
    }

    @MainActor
    func testRunningMoveKeepsIdentityStartAndElapsedTime() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        let elapsed = fixture.session.elapsed
        fixture.session.submitWorklogMove()
        let command = try await fixture.client.next()
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: nil)
        command.moved(worklog: moved, resources: TaskListResources(tasks: [firstTask, secondTask], active: moved))
        try await finishHistory(fixture)
        XCTAssertEqual(fixture.session.active, moved)
        XCTAssertEqual(fixture.session.elapsed, elapsed)
        XCTAssertEqual(fixture.session.runningTaskName, secondTask.name)
    }

    @MainActor
    func testEachQuerySendsASearchAndOutdatedResponsesAreIgnored() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await open(fixture)
        fixture.session.setWorklogMoveQuery("s")
        let first = try await fixture.client.next()
        XCTAssertEqual(first.operation, .candidates(source: firstTask.id, query: "s"))
        fixture.session.setWorklogMoveQuery("se")
        let second = try await fixture.client.next()
        XCTAssertEqual(second.operation, .candidates(source: firstTask.id, query: "se"))
        fixture.session.setWorklogMoveQuery("second")
        let latest = try await fixture.client.next()
        XCTAssertEqual(latest.operation, .candidates(source: firstTask.id, query: "second"))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 3)
        XCTAssertNil(fixture.session.worklogMove.selectedTaskID)
        XCTAssertFalse(fixture.session.worklogMove.canSubmit)

        try await fixture.update("the outdated candidates") {
            first.candidates([WorklogMoveCandidate(id: "obsolete", name: "Obsolete")])
        }
        XCTAssertTrue(fixture.session.worklogMove.isSearching)
        XCTAssertEqual(fixture.session.worklogMove.candidates, [candidate])
        XCTAssertNil(fixture.session.worklogMove.selectedTaskID)

        try await loadCandidates(fixture, latest)
        XCTAssertEqual(fixture.session.worklogMove.selectedTaskID, secondTask.id)
        try await fixture.update("the late candidates") {
            second.candidates([WorklogMoveCandidate(id: "late", name: "Late")])
        }
        XCTAssertEqual(fixture.session.worklogMove.candidates, [candidate])
        XCTAssertEqual(fixture.session.worklogMove.selectedTaskID, secondTask.id)
        XCTAssertNil(fixture.session.worklogMove.error)
        XCTAssertTrue(fixture.session.worklogMove.canSubmit)
        XCTAssertFalse(fixture.client.operations.dropFirst(2).contains(.refresh(fixture.session.connectionSettings)))
    }

    @MainActor
    func testOutdatedSearchFailureDoesNotReplaceTheLatestResult() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        fixture.session.openWorklogMove(worklogID: activeWorklog.id)
        let opening = try await fixture.client.next()
        XCTAssertEqual(opening.operation, .candidates(source: firstTask.id, query: ""))
        fixture.session.setWorklogMoveQuery("latest")
        let search = try await fixture.client.next()
        XCTAssertEqual(search.operation, .candidates(source: firstTask.id, query: "latest"))
        try await fixture.update("the outdated failure") {
            opening.fail(BridgeFailure(message: "Outdated failure"))
        }
        XCTAssertNil(fixture.session.worklogMove.error)
        XCTAssertTrue(fixture.session.worklogMove.isSearching)
        try await fixture.update("the latest failure") {
            search.fail(BridgeFailure(message: "Search failed"))
        } until: {
            !fixture.session.worklogMove.isSearching
        }
        XCTAssertEqual(fixture.session.worklogMove.error, "Search failed")
        XCTAssertTrue(fixture.session.worklogMove.canEdit)
        XCTAssertFalse(fixture.session.worklogMove.canSubmit)
    }

    @MainActor
    func testSearchRetainsCandidatesButBlocksSelectionUntilFreshResultsArrive() throws {
        let state = WorklogMoveState()
        let initial = try XCTUnwrap(state.open(precise, sourceTaskName: firstTask.name))
        XCTAssertEqual(initial.sourceTaskID, firstTask.id)
        XCTAssertEqual(initial.query, "")
        state.accept([candidate], search: initial)
        let search = try XCTUnwrap(state.updateQuery("Another destination"))
        XCTAssertEqual(search.query, "Another destination")
        XCTAssertGreaterThan(search.revision, initial.revision)
        XCTAssertNil(state.updateQuery("Another destination"))
        XCTAssertEqual(state.presentation.candidates, [candidate])
        XCTAssertTrue(state.presentation.isSearching)
        XCTAssertNil(state.presentation.selectedTaskID)
        XCTAssertFalse(state.presentation.canSubmit)
        state.select(candidate.id)
        state.moveSelection(by: 1)
        XCTAssertNil(state.presentation.selectedTaskID)
        XCTAssertNil(state.submit())
        state.failSearch(BridgeFailure(message: "Search unavailable"), search: search)
        XCTAssertEqual(state.presentation.candidates, [])
        XCTAssertFalse(state.presentation.isSearching)
        XCTAssertFalse(state.presentation.canSubmit)
    }

    @MainActor
    func testSearchIsSentWhileAPollRuns() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let running = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(running)
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        XCTAssertTrue(fixture.session.canOpenWorklogMove)
        fixture.session.openWorklogMove(worklogID: activeWorklog.id)
        let search = try await fixture.client.next()
        XCTAssertEqual(search.operation, .candidates(source: firstTask.id, query: ""))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 2)
        try await loadCandidates(fixture, search)
        XCTAssertTrue(fixture.session.worklogMove.canSubmit)
        try await fixture.update("the poll result") { poll.succeed(running) }
        XCTAssertTrue(fixture.session.worklogMove.canSubmit)
        XCTAssertEqual(fixture.session.worklogMove.selectedTaskID, secondTask.id)
    }

    @MainActor
    func testSearchFailureCanBeRetriedAndEmptyResultsCannotSubmit() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await open(fixture)
        fixture.session.setWorklogMoveQuery("missing")
        let search = try await fixture.client.next()
        try await fixture.update("the failed search") {
            search.fail(BridgeFailure(message: "Search unavailable"))
        }
        XCTAssertEqual(fixture.session.worklogMove.error, "Search unavailable")
        XCTAssertTrue(fixture.session.worklogMove.canEdit)
        XCTAssertFalse(fixture.session.worklogMove.canSubmit)
        fixture.session.retryWorklogMoveCandidates()
        XCTAssertNil(fixture.session.worklogMove.error)
        let retry = try await fixture.client.next()
        XCTAssertEqual(retry.operation, .candidates(source: firstTask.id, query: "missing"))
        try await loadCandidates(fixture, retry, [])
        XCTAssertFalse(fixture.session.worklogMove.isSearching)
        XCTAssertNil(fixture.session.worklogMove.error)
        XCTAssertFalse(fixture.session.worklogMove.canSubmit)
        XCTAssertEqual(fixture.client.operations.filter { $0 == .refresh(fixture.session.connectionSettings) }.count, 0)
    }

    @MainActor
    func testFailureShowsServerMessageKeepsSelectionReloadsAndResubmitSendsANewMove() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture)
        fixture.session.submitWorklogMove()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "The destination task is archived.", kind: "validation"))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connectionSettings))
        XCTAssertFalse(fixture.session.worklogMove.isSubmitting)
        XCTAssertEqual(fixture.session.worklogMove.error, "The destination task is archived.")
        let archived = TaskItem(id: secondTask.id, name: secondTask.name, archived: true, latestStart: nil)
        reload.succeed(TaskListResources(tasks: [firstTask, archived], active: nil))
        try await finishHistory(fixture, rows: [log])

        XCTAssertEqual(fixture.session.tasks, [firstTask, archived])
        XCTAssertTrue(fixture.session.worklogMove.isPresented)
        XCTAssertEqual(fixture.session.worklogMove.original, log)
        XCTAssertEqual(fixture.session.worklogMove.selectedTaskID, secondTask.id)
        XCTAssertEqual(fixture.session.worklogMove.error, "The destination task is archived.")
        XCTAssertTrue(fixture.session.worklogMove.canEdit)
        XCTAssertTrue(fixture.session.worklogMove.canSubmit)
        XCTAssertFalse(fixture.session.isStale)

        fixture.session.submitWorklogMove()
        XCTAssertNil(fixture.session.worklogMove.error)
        let retry = try await fixture.client.next()
        XCTAssertEqual(retry.operation, .move(expected: log, destination: secondTask.id))
        XCTAssertEqual(moves(fixture), 2)
    }

    @MainActor
    func testUnavailableAndProtocolWriteFailuresKeepTheMoveEditable() async throws {
        for kind in ["unavailable", "protocol"] {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            let log = try await open(fixture, worklog: activeWorklog)
            fixture.session.submitWorklogMove()
            let command = try await fixture.client.next()
            command.fail(BridgeFailure(message: "Move response unavailable", kind: kind))
            let reload = try await fixture.client.next()
            XCTAssertEqual(reload.operation, .refresh(fixture.session.connectionSettings), kind)
            reload.succeed(TaskListResources(tasks: [firstTask, secondTask], active: log))
            try await finishHistory(fixture)

            XCTAssertTrue(fixture.session.worklogMove.canEdit, kind)
            XCTAssertTrue(fixture.session.worklogMove.canSubmit, kind)
            XCTAssertEqual(fixture.session.worklogMove.original, log, kind)
            XCTAssertEqual(fixture.session.worklogMove.error, "Move response unavailable", kind)
        }
    }

    @MainActor
    func testUncertainFailureAsksTheUserToCheckTheReloadedData() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture)
        fixture.session.submitWorklogMove()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Response lost", uncertain: true))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.session.connectionSettings))
        reload.succeed(resources)
        try await finishHistory(fixture)

        XCTAssertEqual(fixture.session.worklogs, [])
        XCTAssertTrue(fixture.session.worklogMove.isPresented)
        XCTAssertEqual(fixture.session.worklogMove.original, log)
        XCTAssertEqual(
            fixture.session.worklogMove.error,
            "Could not confirm the change. Check the reloaded data before trying again. Response lost")
        XCTAssertFalse(fixture.session.isStale)
        XCTAssertEqual(moves(fixture), 1)
    }

    @MainActor
    func testFailedReloadAfterRejectedMoveKeepsTheMoveAndMarksConnectionStale() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        fixture.session.submitWorklogMove()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Move rejected", kind: "conflict"))
        let reload = try await fixture.client.next()
        reload.fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        let history = try await fixture.client.next()
        history.fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        try await fixture.settled()

        XCTAssertTrue(fixture.session.isStale)
        XCTAssertEqual(fixture.session.connectionStatusText, "Unavailable")
        XCTAssertEqual(fixture.session.worklogMove.error, "Move rejected")
        XCTAssertEqual(fixture.session.worklogMove.original, log)
        XCTAssertTrue(fixture.session.worklogMove.canSubmit)
        XCTAssertEqual(fixture.session.active, log)
    }

    @MainActor
    func testCorrectionRunsWhileAMoveRuns() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        fixture.session.retryHistory()
        let page = try await fixture.client.next()
        page.succeed(HistoryPage(worklogs: [precise], nextCursor: nil, reset: false))
        try await fixture.settled()
        fixture.session.submitWorklogMove()
        let move = try await fixture.client.next()
        XCTAssertEqual(move.operation, .move(expected: log, destination: secondTask.id))
        XCTAssertTrue(fixture.session.worklogMove.isSubmitting)
        XCTAssertFalse(fixture.session.worklogMove.canSubmit)
        XCTAssertFalse(fixture.session.worklogMove.canEdit)
        fixture.session.submitWorklogMove()
        fixture.session.cancelWorklogMove()
        XCTAssertTrue(fixture.session.worklogMove.isPresented)
        XCTAssertTrue(fixture.session.canStopTracking)
        XCTAssertTrue(fixture.session.canOpenWorklogCorrection)
        XCTAssertTrue(fixture.session.canOpenWorklogDeletion)

        fixture.session.openWorklogCorrection(worklogID: precise.id)
        fixture.session.setWorklogCorrectionStart(try XCTUnwrap(parseTimestamp("2024-12-30T08:00:00.000Z")))
        fixture.session.submitWorklogCorrection()
        let correction = try await fixture.client.next()
        XCTAssertEqual(
            correction.operation,
            .correct(
                expected: precise, start: "2024-12-30T08:00:00.000Z",
                end: precise.end, at: "2025-01-01T00:00:00.000Z"))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 2)
        XCTAssertTrue(fixture.session.worklogMove.isSubmitting)

        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: nil)
        let afterMove = TaskListResources(tasks: [firstTask, secondTask], active: moved)
        move.moved(worklog: moved, resources: afterMove)
        try await finishHistory(fixture, rows: [precise])
        XCTAssertFalse(fixture.session.worklogMove.isPresented)
        XCTAssertEqual(fixture.session.active, moved)
        XCTAssertTrue(fixture.session.worklogCorrection.isSubmitting)

        let corrected = WorklogItem(
            id: precise.id, taskId: precise.taskId, start: "2024-12-30T08:00:00.000000Z", end: precise.end)
        correction.corrected(worklog: corrected, resources: afterMove)
        try await finishHistory(fixture, rows: [corrected])
        XCTAssertFalse(fixture.session.worklogCorrection.isPresented)
        XCTAssertEqual(fixture.session.worklogs, [corrected])
        XCTAssertEqual(moves(fixture), 1)
    }

    @MainActor
    func testOpeningWhileTheMoverIsPresentedKeepsTheDraft() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture)
        let third = WorklogMoveCandidate(id: "third", name: "Third task")
        fixture.session.setWorklogMoveQuery("task")
        let search = try await fixture.client.next()
        try await loadCandidates(fixture, search, [candidate, third])
        fixture.session.selectWorklogMoveDestination(taskID: third.id)
        let operations = fixture.client.operations.count

        fixture.session.openWorklogMove(worklogID: log.id)
        fixture.session.openWorklogMove(worklogID: oldWorklog.id)

        XCTAssertEqual(fixture.client.operations.count, operations)
        XCTAssertEqual(fixture.session.worklogMove.original, log)
        XCTAssertEqual(fixture.session.worklogMove.query, "task")
        XCTAssertEqual(fixture.session.worklogMove.candidates, [candidate, third])
        XCTAssertEqual(fixture.session.worklogMove.selectedTaskID, third.id)
        XCTAssertTrue(fixture.session.worklogMove.canSubmit)
    }

    @MainActor
    func testObserverFreezesDismissedContentAndCandidateChangesDoNotRedrawHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await open(fixture)
        let harness = PresentationHarness(session: fixture.session)
        let contentChanges = harness.counter(PresentationReads.content)
        let moveChanges = harness.counter(PresentationReads.worklogMove)
        fixture.session.setWorklogMoveQuery("second")
        let search = try await fixture.client.next()
        try await loadCandidates(fixture, search)
        harness.attach()
        let visible = harness.presentation.worklogMoveSheetContent
        fixture.session.cancelWorklogMove()
        XCTAssertEqual(harness.presentation.worklogMoveSheetContent, visible)
        XCTAssertGreaterThan(moveChanges.count, 0)
        XCTAssertEqual(contentChanges.count, 0)
        fixture.session.openTaskCreation()
        XCTAssertTrue(fixture.session.canOpenWorklogMove)
    }

    @MainActor
    func testSelectionUsesWorkerRankingAndClampsKeyboardNavigation() throws {
        let state = WorklogMoveState()
        let search = try XCTUnwrap(state.open(precise, sourceTaskName: firstTask.name))
        let third = WorklogMoveCandidate(id: "third", name: "Third")
        let source = WorklogMoveCandidate(id: firstTask.id, name: firstTask.name)
        state.accept([third, source, candidate], search: search)
        XCTAssertEqual(state.presentation.candidates, [third, candidate])
        XCTAssertEqual(state.presentation.selectedTaskID, third.id)
        state.moveSelection(by: 1)
        XCTAssertEqual(state.presentation.selectedTaskID, candidate.id)
        state.moveSelection(by: 100)
        XCTAssertEqual(state.presentation.selectedTaskID, candidate.id)
        state.moveSelection(by: -100)
        XCTAssertEqual(state.presentation.selectedTaskID, third.id)
        state.select("missing")
        XCTAssertEqual(state.presentation.selectedTaskID, third.id)
    }

    @MainActor
    func testCandidateResponseThatArrivesDuringSleepIsAccepted() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await open(fixture)
        fixture.session.setWorklogMoveQuery("Destination")
        let search = try await fixture.client.next()
        fixture.session.sleep()
        try await loadCandidates(fixture, search)

        XCTAssertFalse(fixture.session.worklogMove.isSearching)
        XCTAssertEqual(fixture.session.worklogMove.selectedTaskID, secondTask.id)
        XCTAssertEqual(searches(fixture), 2)
    }

    @MainActor
    func testHistoryReloadDoesNotChangeTheOpenMover() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture)
        fixture.session.retryHistory()
        let history = try await fixture.client.next()
        let changed = WorklogItem(
            id: log.id, taskId: log.taskId, start: log.start, end: "2024-12-30T10:30:00.000000Z")
        history.succeed(HistoryPage(worklogs: [changed], nextCursor: nil, reset: false))
        try await fixture.settled()

        XCTAssertEqual(fixture.session.worklogs, [changed])
        XCTAssertEqual(fixture.session.worklogMove.original, log)
        XCTAssertNil(fixture.session.worklogMove.error)
        XCTAssertTrue(fixture.session.worklogMove.canSubmit)
    }

    @MainActor
    func testRemoteMoveReceiptAcceptsNewerTrackingAndDestinationMetadata() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        fixture.session.submitWorklogMove()
        let command = try await fixture.client.next()
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: nil)
        let archived = TaskItem(id: secondTask.id, name: secondTask.name, archived: true, latestStart: nil)
        command.moved(
            worklog: moved,
            resources: TaskListResources(tasks: [firstTask, archived], active: nil))
        try await fixture.waitUntil("the reloaded task list") { fixture.session.active == nil }
        XCTAssertNil(fixture.session.active)
        XCTAssertEqual(fixture.session.tasks, [firstTask, archived])
        XCTAssertNil(fixture.session.worklogMove.error)
        XCTAssertFalse(fixture.session.worklogMove.isPresented)
    }

    @MainActor
    func testCommittedMoveSurvivesFailedResourceRefresh() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        fixture.session.submitWorklogMove()
        let command = try await fixture.client.next()
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: nil)
        command.moved(worklog: moved)
        let refresh = try await fixture.client.next()
        XCTAssertEqual(refresh.operation, .refresh(fixture.session.connectionSettings))
        refresh.fail(BridgeFailure(message: "Server unavailable", kind: "unavailable", requiresRefresh: true))
        try await finishHistory(fixture)

        XCTAssertEqual(fixture.session.active, log)
        XCTAssertFalse(fixture.session.worklogMove.isPresented)
        XCTAssertNil(fixture.session.worklogMove.error)
        XCTAssertTrue(fixture.session.isStale)
        XCTAssertEqual(moves(fixture), 1)
    }

    @MainActor
    func testSuccessfulMovePreservesUnrelatedSelectedHistoryWhenReloadFails() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture)
        let third = TaskItem(id: "third", name: "Third task", archived: false, latestStart: nil)
        let tasks = [firstTask, secondTask, third]
        fixture.session.refresh()
        let refresh = try await fixture.client.next()
        refresh.succeed(TaskListResources(tasks: tasks, active: nil))
        try await fixture.settled()
        fixture.session.select(third.id)
        let selectedHistory = try await fixture.client.next()
        let unrelated = WorklogItem(id: "unrelated", taskId: third.id, start: log.start, end: log.end)
        selectedHistory.succeed(HistoryPage(worklogs: [unrelated], nextCursor: "older-third", reset: false))
        try await fixture.settled()
        fixture.session.submitWorklogMove()
        let command = try await fixture.client.next()
        XCTAssertEqual(command.operation, .move(expected: log, destination: secondTask.id))
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: log.end)
        command.moved(worklog: moved, resources: TaskListResources(tasks: tasks, active: nil))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: third.id, cursor: nil))
        XCTAssertEqual(fixture.session.worklogs, [unrelated])
        XCTAssertEqual(fixture.session.nextCursor, "older-third")
        history.fail(BridgeFailure(message: "History unavailable"))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.selectedTaskID, third.id)
        XCTAssertEqual(fixture.session.worklogs, [unrelated])
    }

    @MainActor
    func testCancelledOpeningIgnoresLateCandidatesAndReopenedSheetSearchesAgain() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        fixture.session.openWorklogMove(worklogID: activeWorklog.id)
        let opening = try await fixture.client.next()
        fixture.session.cancelWorklogMove()
        try await fixture.update("the late candidates") {
            opening.candidates([candidate])
        }
        XCTAssertFalse(fixture.session.worklogMove.isPresented)
        XCTAssertEqual(fixture.session.worklogMove.candidates, [])
        fixture.session.openWorklogMove(worklogID: activeWorklog.id)
        let search = try await fixture.client.next()
        XCTAssertEqual(search.operation, .candidates(source: firstTask.id, query: ""))
        try await loadCandidates(fixture, search)
        XCTAssertTrue(fixture.session.worklogMove.canSubmit)
    }

    @MainActor
    func testMoveIsSentWhileAPollRunsAndTrackingStaysUsable() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        let running = TaskListResources(tasks: [firstTask, secondTask], active: log)
        fixture.session.refresh()
        let poll = try await fixture.client.next()
        fixture.session.submitWorklogMove()
        let command = try await fixture.client.next()
        XCTAssertEqual(command.operation, .move(expected: log, destination: secondTask.id))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 2)
        fixture.session.selectWorklogMoveDestination(taskID: "ignored")
        fixture.session.setWorklogMoveQuery("ignored")
        XCTAssertEqual(fixture.session.worklogMove.selectedTaskID, secondTask.id)
        XCTAssertEqual(fixture.session.worklogMove.query, "")
        XCTAssertTrue(fixture.session.canStopTracking)
        try await fixture.update("the poll result") { poll.succeed(running) }
        XCTAssertTrue(fixture.session.worklogMove.isSubmitting)
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: nil)
        command.moved(worklog: moved, resources: TaskListResources(tasks: [firstTask, secondTask], active: moved))
        try await finishHistory(fixture)
        XCTAssertEqual(fixture.session.active, moved)
        XCTAssertFalse(fixture.session.worklogMove.isPresented)
    }

    @MainActor
    func testSuccessfulMoveFreezesSubmittingContentDuringDismissal() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        let harness = PresentationHarness(session: fixture.session)
        fixture.session.submitWorklogMove()
        let command = try await fixture.client.next()
        let visible = harness.presentation.worklogMoveSheetContent
        XCTAssertTrue(visible.isSubmitting)
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: nil)
        command.moved(worklog: moved, resources: TaskListResources(tasks: [firstTask, secondTask], active: moved))
        let history = try await fixture.client.next()
        XCTAssertEqual(harness.presentation.worklogMoveSheetContent, visible)
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(harness.presentation.worklogMoveSheetContent, visible)
    }

    @MainActor
    func testKeyboardSelectionMovesFromTheSelectedCandidateAndStopsAtTheBounds() throws {
        let state = WorklogMoveState()
        let search = try XCTUnwrap(state.open(precise, sourceTaskName: firstTask.name))
        let candidates = [
            WorklogMoveCandidate(id: "destination-one", name: "First destination"),
            WorklogMoveCandidate(id: "destination-two", name: "Second destination"),
            WorklogMoveCandidate(id: "destination-three", name: "Third destination"),
        ]
        state.accept(candidates, search: search)
        state.select(candidates[1].id)

        state.moveSelection(by: 1)
        XCTAssertEqual(state.presentation.selectedTaskID, candidates[2].id)
        state.moveSelection(by: 1)
        XCTAssertEqual(state.presentation.selectedTaskID, candidates[2].id)
        state.moveSelection(by: -1)
        XCTAssertEqual(state.presentation.selectedTaskID, candidates[1].id)
        state.moveSelection(by: -2)
        XCTAssertEqual(state.presentation.selectedTaskID, candidates[0].id)
        state.moveSelection(by: -1)
        XCTAssertEqual(state.presentation.selectedTaskID, candidates[0].id)
    }

    @MainActor
    func testExactTimestampComparisonKeepsMicrosecondsAndEquivalentOffsets() {
        XCTAssertTrue(sameWorklogTimestamp("2024-12-30T09:00:12.123456Z", "2024-12-30T10:00:12.123456000+01:00"))
        XCTAssertFalse(sameWorklogTimestamp("2024-12-30T09:00:12.123456Z", "2024-12-30T09:00:12.123457Z"))
        XCTAssertTrue(sameWorklogTimestamp("2024-12-30T09:00:12Z", "2024-12-30T09:00:12.000Z"))
        XCTAssertFalse(sameWorklogTimestamp("invalid", "also invalid"))
        XCTAssertFalse(sameWorklogTimestamp("2024-12-30T09:00:12.Z", "2024-12-30T09:00:12.000Z"))
        XCTAssertFalse(sameWorklogTimestamp("2024-12-30T09:00:12.1234567890Z", "2024-12-30T09:00:12.000Z"))
        XCTAssertFalse(sameWorklogTimestamp(nil, "2024-12-30T09:00:12.000Z"))
    }
}
