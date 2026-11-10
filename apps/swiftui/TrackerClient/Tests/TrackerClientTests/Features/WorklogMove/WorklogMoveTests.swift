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
    private func open(
        _ fixture: Fixture, worklog: WorklogItem? = nil, candidates: [WorklogMoveCandidate]? = nil
    ) async throws -> WorklogItem {
        let log = worklog ?? precise
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: log.end == nil ? log : nil))
        if log.end != nil {
            fixture.features.worklogHistory.reload()
            let page = try await fixture.client.next()
            page.succeed(HistoryPage(worklogs: [log], nextCursor: "older", reset: false))
            try await fixture.settled()
        }
        fixture.worklogMove.open(worklogID: log.id)
        XCTAssertEqual(fixture.worklogMove.sourceTaskName, firstTask.name)
        XCTAssertTrue(fixture.worklogMove.isSearching)
        let search = try await fixture.client.next()
        XCTAssertEqual(search.operation, .candidates(source: firstTask.id, query: ""))
        try await loadCandidates(fixture, search, candidates)
        return log
    }

    @MainActor
    private func searchFinished(_ fixture: Fixture) async throws {
        try await fixture.waitUntil("the search result") { !fixture.worklogMove.isSearching }
    }

    /// Answers a candidate search and waits until the store shows the result.
    @MainActor
    private func loadCandidates(
        _ fixture: Fixture, _ request: FakeClient.Request, _ values: [WorklogMoveCandidate]? = nil
    ) async throws {
        request.candidates(values ?? [candidate])
        try await searchFinished(fixture)
    }

    /// Lets a reply reach the store when the store must drop it, which leaves nothing to wait on.
    @MainActor
    private func deliverDroppedReply() async {
        for _ in 0..<20 { await Task.yield() }
    }

    @MainActor
    private func finishHistory(_ fixture: Fixture, rows: [WorklogItem] = []) async throws {
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        try await fixture.update("the reloaded history") {
            history.succeed(HistoryPage(worklogs: rows, nextCursor: nil, reset: false))
        } until: {
            !fixture.features.worklogHistory.isLoading
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
        fixture.features.worklogHistory.reload()
        let initial = try await fixture.client.next()
        initial.succeed(HistoryPage(worklogs: [log, oldWorklog], nextCursor: "older", reset: false))
        try await fixture.settled()
        XCTAssertTrue(fixture.worklogMove.canSubmit)
        fixture.worklogMove.submit()
        let command = try await fixture.client.next()
        XCTAssertEqual(command.operation, .move(expected: log, destination: secondTask.id))
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: log.end)
        command.moved(worklog: moved)
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.features.connection.settings))
        XCTAssertFalse(fixture.worklogMove.isPresented)
        XCTAssertEqual(fixture.features.worklogHistory.worklogs, [log, oldWorklog])
        reload.succeed(resources)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        XCTAssertEqual(fixture.features.worklogHistory.worklogs, [log, oldWorklog])
        XCTAssertTrue(fixture.features.worklogHistory.isLoading)
        history.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertEqual(fixture.features.worklogHistory.worklogs, [oldWorklog])
        XCTAssertEqual(
            Array(fixture.client.operations.suffix(3)),
            [
                command.operation, .refresh(fixture.features.connection.settings),
                .history(task: firstTask.id, cursor: nil),
            ])
    }

    @MainActor
    func testFailedHistoryReloadAfterMoveKeepsShownRowsAndMarksHistoryUnavailable() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture)
        fixture.worklogMove.submit()
        let command = try await fixture.client.next()
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: log.end)
        command.moved(worklog: moved, resources: resources)
        let history = try await fixture.client.next()
        XCTAssertFalse(fixture.worklogMove.isPresented)
        XCTAssertEqual(fixture.features.worklogHistory.worklogs, [log])
        XCTAssertTrue(fixture.features.worklogHistory.isLoading)
        history.fail(BridgeFailure(message: "History unavailable"))
        try await fixture.settled()
        XCTAssertEqual(fixture.features.worklogHistory.worklogs, [log])
        XCTAssertTrue(fixture.features.worklogHistory.unavailable)
        XCTAssertFalse(fixture.features.worklogHistory.isLoading)
        XCTAssertEqual(moves(fixture), 1)
    }

    @MainActor
    func testMoveKeepsSelectedDestinationRowsUntilTheHistoryReloadReplacesThem() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        fixture.features.taskCatalog.select(secondTask.id)
        let destination = try await fixture.client.next()
        let retained = WorklogItem(
            id: "destination-history", taskId: secondTask.id,
            start: oldWorklog.start, end: oldWorklog.end)
        destination.succeed(HistoryPage(worklogs: [retained], nextCursor: "old-destination", reset: false))
        try await fixture.settled()
        fixture.worklogMove.submit()
        let command = try await fixture.client.next()
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: nil)
        command.moved(worklog: moved, resources: TaskListResources(tasks: [firstTask, secondTask], active: moved))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: secondTask.id, cursor: nil))
        XCTAssertEqual(fixture.features.worklogHistory.worklogs, [retained])
        XCTAssertEqual(fixture.features.worklogHistory.nextCursor, "old-destination")
        XCTAssertTrue(fixture.features.worklogHistory.isLoading)
        history.succeed(HistoryPage(worklogs: [moved, retained], nextCursor: "new-destination", reset: false))
        try await fixture.settled()
        XCTAssertEqual(fixture.features.worklogHistory.worklogs, [moved, retained])
        XCTAssertEqual(fixture.features.worklogHistory.nextCursor, "new-destination")
        XCTAssertTrue(fixture.features.worklogHistory.hasMore)
    }

    @MainActor
    func testRunningMoveKeepsIdentityStartAndElapsedTime() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        let elapsed = fixture.features.tracking.elapsed(clock: fixture.clock)
        fixture.worklogMove.submit()
        let command = try await fixture.client.next()
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: nil)
        command.moved(worklog: moved, resources: TaskListResources(tasks: [firstTask, secondTask], active: moved))
        try await finishHistory(fixture)
        XCTAssertEqual(fixture.features.tracking.active, moved)
        XCTAssertEqual(fixture.features.tracking.elapsed(clock: fixture.clock), elapsed)
        XCTAssertEqual(fixture.features.menuSources.read().runningTaskName, secondTask.name)
    }

    @MainActor
    func testEachQuerySendsASearchAndOutdatedResponsesAreIgnored() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await open(fixture)
        fixture.worklogMove.setQuery("s")
        let first = try await fixture.client.next()
        XCTAssertEqual(first.operation, .candidates(source: firstTask.id, query: "s"))
        fixture.worklogMove.setQuery("se")
        let second = try await fixture.client.next()
        XCTAssertEqual(second.operation, .candidates(source: firstTask.id, query: "se"))
        fixture.worklogMove.setQuery("second")
        let latest = try await fixture.client.next()
        XCTAssertEqual(latest.operation, .candidates(source: firstTask.id, query: "second"))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 3)
        XCTAssertNil(fixture.worklogMove.selectedTaskID)
        XCTAssertFalse(fixture.worklogMove.canSubmit)

        first.candidates([WorklogMoveCandidate(id: "obsolete", name: "Obsolete")])
        await deliverDroppedReply()
        XCTAssertTrue(fixture.worklogMove.isSearching)
        XCTAssertEqual(fixture.worklogMove.candidates, [candidate])
        XCTAssertNil(fixture.worklogMove.selectedTaskID)

        try await loadCandidates(fixture, latest)
        XCTAssertEqual(fixture.worklogMove.selectedTaskID, secondTask.id)
        second.candidates([WorklogMoveCandidate(id: "late", name: "Late")])
        await deliverDroppedReply()
        XCTAssertEqual(fixture.worklogMove.candidates, [candidate])
        XCTAssertEqual(fixture.worklogMove.selectedTaskID, secondTask.id)
        XCTAssertNil(fixture.worklogMove.error)
        XCTAssertTrue(fixture.worklogMove.canSubmit)
        XCTAssertFalse(fixture.client.operations.dropFirst(2).contains(.refresh(fixture.features.connection.settings)))
    }

    @MainActor
    func testOutdatedSearchFailureDoesNotReplaceTheLatestResult() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        fixture.worklogMove.open(worklogID: activeWorklog.id)
        let opening = try await fixture.client.next()
        XCTAssertEqual(opening.operation, .candidates(source: firstTask.id, query: ""))
        fixture.worklogMove.setQuery("latest")
        let search = try await fixture.client.next()
        XCTAssertEqual(search.operation, .candidates(source: firstTask.id, query: "latest"))
        opening.fail(BridgeFailure(message: "Outdated failure"))
        await deliverDroppedReply()
        XCTAssertNil(fixture.worklogMove.error)
        XCTAssertTrue(fixture.worklogMove.isSearching)
        search.fail(BridgeFailure(message: "Search failed"))
        try await searchFinished(fixture)
        XCTAssertEqual(fixture.worklogMove.error, "Search failed")
        XCTAssertTrue(fixture.worklogMove.canEdit)
        XCTAssertFalse(fixture.worklogMove.canSubmit)
    }

    @MainActor
    func testSearchRetainsCandidatesButBlocksSelectionUntilFreshResultsArrive() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await open(fixture)
        fixture.worklogMove.setQuery("Another destination")
        let search = try await fixture.client.next()
        XCTAssertEqual(search.operation, .candidates(source: firstTask.id, query: "Another destination"))
        fixture.worklogMove.setQuery("Another destination")
        XCTAssertEqual(searches(fixture), 2)
        XCTAssertEqual(fixture.worklogMove.candidates, [candidate])
        XCTAssertTrue(fixture.worklogMove.isSearching)
        XCTAssertNil(fixture.worklogMove.selectedTaskID)
        XCTAssertFalse(fixture.worklogMove.canSubmit)
        fixture.worklogMove.select(taskID: candidate.id)
        fixture.worklogMove.moveSelection(by: 1)
        XCTAssertNil(fixture.worklogMove.selectedTaskID)
        fixture.worklogMove.submit()
        XCTAssertEqual(moves(fixture), 0)
        search.fail(BridgeFailure(message: "Search unavailable"))
        try await searchFinished(fixture)
        XCTAssertEqual(fixture.worklogMove.candidates, [])
        XCTAssertFalse(fixture.worklogMove.isSearching)
        XCTAssertFalse(fixture.worklogMove.canSubmit)
    }

    @MainActor
    func testSearchIsSentWhileAPollRuns() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let running = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(running)
        fixture.sync.refresh()
        let poll = try await fixture.client.next()
        XCTAssertTrue(fixture.worklogMove.canOpen)
        fixture.worklogMove.open(worklogID: activeWorklog.id)
        let search = try await fixture.client.next()
        XCTAssertEqual(search.operation, .candidates(source: firstTask.id, query: ""))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 2)
        try await loadCandidates(fixture, search)
        XCTAssertTrue(fixture.worklogMove.canSubmit)
        try await fixture.update("the poll result") { poll.succeed(running) }
        XCTAssertTrue(fixture.worklogMove.canSubmit)
        XCTAssertEqual(fixture.worklogMove.selectedTaskID, secondTask.id)
    }

    @MainActor
    func testSearchFailureCanBeRetriedAndEmptyResultsCannotSubmit() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await open(fixture)
        fixture.worklogMove.setQuery("missing")
        let search = try await fixture.client.next()
        search.fail(BridgeFailure(message: "Search unavailable"))
        try await searchFinished(fixture)
        XCTAssertEqual(fixture.worklogMove.error, "Search unavailable")
        XCTAssertTrue(fixture.worklogMove.canEdit)
        XCTAssertFalse(fixture.worklogMove.canSubmit)
        fixture.worklogMove.retryCandidates()
        XCTAssertNil(fixture.worklogMove.error)
        let retry = try await fixture.client.next()
        XCTAssertEqual(retry.operation, .candidates(source: firstTask.id, query: "missing"))
        try await loadCandidates(fixture, retry, [])
        XCTAssertFalse(fixture.worklogMove.isSearching)
        XCTAssertNil(fixture.worklogMove.error)
        XCTAssertFalse(fixture.worklogMove.canSubmit)
        XCTAssertEqual(
            fixture.client.operations.filter { $0 == .refresh(fixture.features.connection.settings) }.count, 0)
    }

    @MainActor
    func testFailureShowsServerMessageKeepsSelectionReloadsAndResubmitSendsANewMove() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture)
        fixture.worklogMove.submit()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "The destination task is archived.", kind: "validation"))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.features.connection.settings))
        XCTAssertFalse(fixture.worklogMove.isSubmitting)
        XCTAssertEqual(fixture.worklogMove.error, "The destination task is archived.")
        let archived = TaskItem(id: secondTask.id, name: secondTask.name, archived: true, latestStart: nil)
        reload.succeed(TaskListResources(tasks: [firstTask, archived], active: nil))
        try await finishHistory(fixture, rows: [log])

        XCTAssertEqual(fixture.features.taskCatalog.tasks, [firstTask, archived])
        XCTAssertTrue(fixture.worklogMove.isPresented)
        XCTAssertEqual(fixture.worklogMove.original, log)
        XCTAssertEqual(fixture.worklogMove.selectedTaskID, secondTask.id)
        XCTAssertEqual(fixture.worklogMove.error, "The destination task is archived.")
        XCTAssertTrue(fixture.worklogMove.canEdit)
        XCTAssertTrue(fixture.worklogMove.canSubmit)
        XCTAssertFalse(fixture.features.connection.stale)

        fixture.worklogMove.submit()
        XCTAssertNil(fixture.worklogMove.error)
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
            fixture.worklogMove.submit()
            let command = try await fixture.client.next()
            command.fail(BridgeFailure(message: "Move response unavailable", kind: kind))
            let reload = try await fixture.client.next()
            XCTAssertEqual(reload.operation, .refresh(fixture.features.connection.settings), kind)
            reload.succeed(TaskListResources(tasks: [firstTask, secondTask], active: log))
            try await finishHistory(fixture)

            XCTAssertTrue(fixture.worklogMove.canEdit, kind)
            XCTAssertTrue(fixture.worklogMove.canSubmit, kind)
            XCTAssertEqual(fixture.worklogMove.original, log, kind)
            XCTAssertEqual(fixture.worklogMove.error, "Move response unavailable", kind)
        }
    }

    @MainActor
    func testUncertainFailureAsksTheUserToCheckTheReloadedData() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture)
        fixture.worklogMove.submit()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Response lost", uncertain: true))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(fixture.features.connection.settings))
        reload.succeed(resources)
        try await finishHistory(fixture)

        XCTAssertEqual(fixture.features.worklogHistory.worklogs, [])
        XCTAssertTrue(fixture.worklogMove.isPresented)
        XCTAssertEqual(fixture.worklogMove.original, log)
        XCTAssertEqual(
            fixture.worklogMove.error,
            "Could not confirm the change. Check the reloaded data before trying again. Response lost")
        XCTAssertFalse(fixture.features.connection.stale)
        XCTAssertEqual(moves(fixture), 1)
    }

    @MainActor
    func testFailedReloadAfterRejectedMoveKeepsTheMoveAndMarksConnectionStale() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        fixture.worklogMove.submit()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Move rejected", kind: "conflict"))
        let reload = try await fixture.client.next()
        reload.fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        let history = try await fixture.client.next()
        history.fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        try await fixture.settled()

        XCTAssertTrue(fixture.features.connection.stale)
        XCTAssertEqual(fixture.features.connection.statusText, "Unavailable")
        XCTAssertEqual(fixture.worklogMove.error, "Move rejected")
        XCTAssertEqual(fixture.worklogMove.original, log)
        XCTAssertTrue(fixture.worklogMove.canSubmit)
        XCTAssertEqual(fixture.features.tracking.active, log)
    }

    @MainActor
    func testCorrectionWaitsUntilTheMoveDialogCloses() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        fixture.features.worklogHistory.reload()
        let page = try await fixture.client.next()
        page.succeed(HistoryPage(worklogs: [precise], nextCursor: nil, reset: false))
        try await fixture.settled()
        fixture.worklogMove.submit()
        let move = try await fixture.client.next()
        XCTAssertEqual(move.operation, .move(expected: log, destination: secondTask.id))
        XCTAssertTrue(fixture.worklogMove.isSubmitting)
        XCTAssertFalse(fixture.worklogMove.canSubmit)
        XCTAssertFalse(fixture.worklogMove.canEdit)
        fixture.worklogMove.submit()
        fixture.worklogMove.cancel()
        XCTAssertTrue(fixture.worklogMove.isPresented)
        XCTAssertTrue(fixture.trackingAvailability.canStop)
        XCTAssertFalse(fixture.worklogCorrection.canOpen)
        XCTAssertFalse(fixture.worklogDeletion.canOpen)
        fixture.worklogCorrection.open(worklogID: precise.id)
        XCTAssertFalse(fixture.worklogCorrection.isPresented)

        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: nil)
        let afterMove = TaskListResources(tasks: [firstTask, secondTask], active: moved)
        move.moved(worklog: moved, resources: afterMove)
        try await finishHistory(fixture, rows: [precise])
        XCTAssertFalse(fixture.worklogMove.isPresented)
        XCTAssertEqual(fixture.features.tracking.active, moved)
        XCTAssertEqual(moves(fixture), 1)
        XCTAssertTrue(fixture.worklogCorrection.canOpen)
        fixture.worklogCorrection.open(worklogID: precise.id)
        XCTAssertTrue(fixture.worklogCorrection.isPresented)
    }

    @MainActor
    func testOpeningWhileTheMoverIsPresentedKeepsTheDraft() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture)
        let third = WorklogMoveCandidate(id: "third", name: "Third task")
        fixture.worklogMove.setQuery("task")
        let search = try await fixture.client.next()
        try await loadCandidates(fixture, search, [candidate, third])
        fixture.worklogMove.select(taskID: third.id)
        let operations = fixture.client.operations.count

        fixture.worklogMove.open(worklogID: log.id)
        fixture.worklogMove.open(worklogID: oldWorklog.id)

        XCTAssertEqual(fixture.client.operations.count, operations)
        XCTAssertEqual(fixture.worklogMove.original, log)
        XCTAssertEqual(fixture.worklogMove.query, "task")
        XCTAssertEqual(fixture.worklogMove.candidates, [candidate, third])
        XCTAssertEqual(fixture.worklogMove.selectedTaskID, third.id)
        XCTAssertTrue(fixture.worklogMove.canSubmit)
    }

    @MainActor
    func testAvailabilityNotifiesOnlyWhenItChanges() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let move = ObservationProbe { ViewReads.worklogMove(fixture.features) }
        XCTAssertFalse(fixture.worklogMove.canOpen)
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        XCTAssertTrue(move.consume())
        XCTAssertTrue(fixture.worklogMove.canOpen)
        fixture.taskCreation.open()
        XCTAssertTrue(move.consume())
        XCTAssertFalse(fixture.worklogMove.canOpen)
        fixture.taskCreation.setName("Draft")
        XCTAssertFalse(move.consume())
        fixture.taskCreation.cancel()
        XCTAssertTrue(move.consume())
        XCTAssertTrue(fixture.worklogMove.canOpen)
    }

    @MainActor
    func testCandidateChangesDoNotRedrawHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await open(fixture)
        let content = ObservationProbe { ViewReads.content(fixture.features) }
        let move = ObservationProbe { ViewReads.worklogMove(fixture.features) }
        fixture.worklogMove.setQuery("second")
        let search = try await fixture.client.next()
        try await loadCandidates(fixture, search)
        fixture.worklogMove.cancel()
        XCTAssertTrue(move.consume())
        XCTAssertFalse(content.consume())
    }

    @MainActor
    func testClosedDialogKeepsItsContentUntilTheNextOpenAndSendsNothing() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture)
        fixture.worklogMove.setQuery("second")
        let search = try await fixture.client.next()
        try await loadCandidates(fixture, search)
        let operations = fixture.client.operations.count

        fixture.worklogMove.cancel()

        XCTAssertFalse(fixture.worklogMove.isPresented)
        XCTAssertEqual(fixture.worklogMove.original, log)
        XCTAssertEqual(fixture.worklogMove.sourceTaskName, firstTask.name)
        XCTAssertEqual(fixture.worklogMove.query, "second")
        XCTAssertEqual(fixture.worklogMove.candidates, [candidate])
        XCTAssertEqual(fixture.worklogMove.selectedTaskID, secondTask.id)
        XCTAssertFalse(fixture.worklogMove.canEdit)
        XCTAssertFalse(fixture.worklogMove.canSubmit)
        fixture.worklogMove.setQuery("Hidden query")
        fixture.worklogMove.retryCandidates()
        fixture.worklogMove.submit()
        XCTAssertEqual(fixture.worklogMove.query, "second")
        XCTAssertEqual(fixture.client.operations.count, operations)

        fixture.worklogMove.open(worklogID: log.id)

        XCTAssertTrue(fixture.worklogMove.isPresented)
        XCTAssertEqual(fixture.worklogMove.query, "")
        XCTAssertEqual(fixture.worklogMove.candidates, [])
        XCTAssertNil(fixture.worklogMove.selectedTaskID)
        XCTAssertTrue(fixture.worklogMove.isSearching)
        let reopened = try await fixture.client.next()
        XCTAssertEqual(reopened.operation, .candidates(source: firstTask.id, query: ""))
    }

    @MainActor
    func testUnconfirmedConnectionAndUnknownWorklogKeepTheDialogClosed() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        XCTAssertFalse(fixture.worklogMove.canOpen)
        fixture.worklogMove.open(worklogID: activeWorklog.id)
        XCTAssertFalse(fixture.worklogMove.isPresented)
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        XCTAssertTrue(fixture.worklogMove.canOpen)
        let operations = fixture.client.operations.count
        fixture.worklogMove.open(worklogID: "missing")
        XCTAssertFalse(fixture.worklogMove.isPresented)
        XCTAssertEqual(fixture.client.operations.count, operations)
    }

    @MainActor
    func testSourceTaskMissingFromTheCatalogShowsAnUnknownTask() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let running = WorklogItem(id: "running", taskId: "gone", start: activeWorklog.start, end: nil)
        try await fixture.start(TaskListResources(tasks: [firstTask], active: running))
        fixture.worklogMove.open(worklogID: running.id)
        XCTAssertEqual(fixture.worklogMove.sourceTaskName, "Unknown task")
        let search = try await fixture.client.next()
        XCTAssertEqual(search.operation, .candidates(source: "gone", query: ""))
    }

    @MainActor
    func testSelectingADestinationClearsTheWriteError() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await open(fixture)
        fixture.worklogMove.submit()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Move rejected", kind: "conflict"))
        let reload = try await fixture.client.next()
        reload.succeed(resources)
        try await finishHistory(fixture)
        XCTAssertEqual(fixture.worklogMove.error, "Move rejected")
        fixture.worklogMove.select(taskID: secondTask.id)
        XCTAssertNil(fixture.worklogMove.error)
    }

    @MainActor
    func testKeyboardSelectionClearsTheWriteError() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await open(fixture)
        fixture.worklogMove.submit()
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Move rejected", kind: "conflict"))
        let reload = try await fixture.client.next()
        reload.succeed(resources)
        try await finishHistory(fixture)
        XCTAssertEqual(fixture.worklogMove.error, "Move rejected")
        fixture.worklogMove.moveSelection(by: 1)
        XCTAssertNil(fixture.worklogMove.error)
    }

    @MainActor
    func testSelectionUsesWorkerRankingAndClampsKeyboardNavigation() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let third = WorklogMoveCandidate(id: "third", name: "Third")
        let source = WorklogMoveCandidate(id: firstTask.id, name: firstTask.name)
        _ = try await open(fixture, candidates: [third, source, candidate])
        let move = fixture.worklogMove
        XCTAssertEqual(move.candidates, [third, candidate])
        XCTAssertEqual(move.selectedTaskID, third.id)
        move.moveSelection(by: 1)
        XCTAssertEqual(move.selectedTaskID, candidate.id)
        move.moveSelection(by: 100)
        XCTAssertEqual(move.selectedTaskID, candidate.id)
        move.moveSelection(by: -100)
        XCTAssertEqual(move.selectedTaskID, third.id)
        move.select(taskID: "missing")
        XCTAssertEqual(move.selectedTaskID, third.id)
    }

    @MainActor
    func testCandidateResponseThatArrivesDuringSleepIsAccepted() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await open(fixture)
        fixture.worklogMove.setQuery("Destination")
        let search = try await fixture.client.next()
        fixture.sync.sleep()
        try await loadCandidates(fixture, search)

        XCTAssertFalse(fixture.worklogMove.isSearching)
        XCTAssertEqual(fixture.worklogMove.selectedTaskID, secondTask.id)
        XCTAssertEqual(searches(fixture), 2)
    }

    @MainActor
    func testHistoryReloadDoesNotChangeTheOpenMover() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture)
        fixture.features.worklogHistory.reload()
        let history = try await fixture.client.next()
        let changed = WorklogItem(
            id: log.id, taskId: log.taskId, start: log.start, end: "2024-12-30T10:30:00.000000Z")
        history.succeed(HistoryPage(worklogs: [changed], nextCursor: nil, reset: false))
        try await fixture.settled()

        XCTAssertEqual(fixture.features.worklogHistory.worklogs, [changed])
        XCTAssertEqual(fixture.worklogMove.original, log)
        XCTAssertNil(fixture.worklogMove.error)
        XCTAssertTrue(fixture.worklogMove.canSubmit)
    }

    @MainActor
    func testRemoteMoveReceiptAcceptsNewerTrackingAndDestinationMetadata() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        fixture.worklogMove.submit()
        let command = try await fixture.client.next()
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: nil)
        let archived = TaskItem(id: secondTask.id, name: secondTask.name, archived: true, latestStart: nil)
        command.moved(
            worklog: moved,
            resources: TaskListResources(tasks: [firstTask, archived], active: nil))
        try await fixture.waitUntil("the reloaded task list") { fixture.features.tracking.active == nil }
        XCTAssertNil(fixture.features.tracking.active)
        XCTAssertEqual(fixture.features.taskCatalog.tasks, [firstTask, archived])
        XCTAssertNil(fixture.worklogMove.error)
        XCTAssertFalse(fixture.worklogMove.isPresented)
    }

    @MainActor
    func testCommittedMoveSurvivesFailedResourceRefresh() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        fixture.worklogMove.submit()
        let command = try await fixture.client.next()
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: nil)
        command.moved(worklog: moved)
        let refresh = try await fixture.client.next()
        XCTAssertEqual(refresh.operation, .refresh(fixture.features.connection.settings))
        refresh.fail(BridgeFailure(message: "Server unavailable", kind: "unavailable", requiresRefresh: true))
        try await finishHistory(fixture)

        XCTAssertEqual(fixture.features.tracking.active, log)
        XCTAssertFalse(fixture.worklogMove.isPresented)
        XCTAssertNil(fixture.worklogMove.error)
        XCTAssertTrue(fixture.features.connection.stale)
        XCTAssertEqual(moves(fixture), 1)
    }

    @MainActor
    func testSuccessfulMovePreservesUnrelatedSelectedHistoryWhenReloadFails() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture)
        let third = TaskItem(id: "third", name: "Third task", archived: false, latestStart: nil)
        let tasks = [firstTask, secondTask, third]
        fixture.sync.refresh()
        let refresh = try await fixture.client.next()
        refresh.succeed(TaskListResources(tasks: tasks, active: nil))
        try await fixture.settled()
        fixture.features.taskCatalog.select(third.id)
        let selectedHistory = try await fixture.client.next()
        let unrelated = WorklogItem(id: "unrelated", taskId: third.id, start: log.start, end: log.end)
        selectedHistory.succeed(HistoryPage(worklogs: [unrelated], nextCursor: "older-third", reset: false))
        try await fixture.settled()
        fixture.worklogMove.submit()
        let command = try await fixture.client.next()
        XCTAssertEqual(command.operation, .move(expected: log, destination: secondTask.id))
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: log.end)
        command.moved(worklog: moved, resources: TaskListResources(tasks: tasks, active: nil))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: third.id, cursor: nil))
        XCTAssertEqual(fixture.features.worklogHistory.worklogs, [unrelated])
        XCTAssertEqual(fixture.features.worklogHistory.nextCursor, "older-third")
        history.fail(BridgeFailure(message: "History unavailable"))
        try await fixture.settled()
        XCTAssertEqual(fixture.features.taskCatalog.selectedTaskID, third.id)
        XCTAssertEqual(fixture.features.worklogHistory.worklogs, [unrelated])
    }

    @MainActor
    func testCancelledOpeningIgnoresLateCandidatesAndReopenedSheetSearchesAgain() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        fixture.worklogMove.open(worklogID: activeWorklog.id)
        let opening = try await fixture.client.next()
        fixture.worklogMove.cancel()
        opening.candidates([candidate])
        await deliverDroppedReply()
        XCTAssertFalse(fixture.worklogMove.isPresented)
        XCTAssertEqual(fixture.worklogMove.candidates, [])
        fixture.worklogMove.open(worklogID: activeWorklog.id)
        let search = try await fixture.client.next()
        XCTAssertEqual(search.operation, .candidates(source: firstTask.id, query: ""))
        try await loadCandidates(fixture, search)
        XCTAssertTrue(fixture.worklogMove.canSubmit)
    }

    @MainActor
    func testCancelledOpeningIgnoresALateSearchFailure() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        fixture.worklogMove.open(worklogID: activeWorklog.id)
        let opening = try await fixture.client.next()
        fixture.worklogMove.cancel()
        opening.fail(BridgeFailure(message: "Late failure"))
        await deliverDroppedReply()
        XCTAssertFalse(fixture.worklogMove.isPresented)
        XCTAssertNil(fixture.worklogMove.error)
    }

    @MainActor
    func testMoveIsSentWhileAPollRunsAndTrackingStaysUsable() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        let running = TaskListResources(tasks: [firstTask, secondTask], active: log)
        fixture.sync.refresh()
        let poll = try await fixture.client.next()
        fixture.worklogMove.submit()
        let command = try await fixture.client.next()
        XCTAssertEqual(command.operation, .move(expected: log, destination: secondTask.id))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 2)
        fixture.worklogMove.select(taskID: "ignored")
        fixture.worklogMove.setQuery("ignored")
        XCTAssertEqual(fixture.worklogMove.selectedTaskID, secondTask.id)
        XCTAssertEqual(fixture.worklogMove.query, "")
        XCTAssertTrue(fixture.trackingAvailability.canStop)
        try await fixture.update("the poll result") { poll.succeed(running) }
        XCTAssertTrue(fixture.worklogMove.isSubmitting)
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: nil)
        command.moved(worklog: moved, resources: TaskListResources(tasks: [firstTask, secondTask], active: moved))
        try await finishHistory(fixture)
        XCTAssertEqual(fixture.features.tracking.active, moved)
        XCTAssertFalse(fixture.worklogMove.isPresented)
    }

    @MainActor
    func testSuccessfulMoveKeepsTheContentDuringDismissal() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        fixture.worklogMove.submit()
        let command = try await fixture.client.next()
        XCTAssertTrue(fixture.worklogMove.isSubmitting)
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: nil)
        command.moved(worklog: moved, resources: TaskListResources(tasks: [firstTask, secondTask], active: moved))
        let history = try await fixture.client.next()
        XCTAssertFalse(fixture.worklogMove.isPresented)
        XCTAssertFalse(fixture.worklogMove.isSubmitting)
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.worklogMove.original, log)
        XCTAssertEqual(fixture.worklogMove.sourceTaskName, firstTask.name)
        XCTAssertEqual(fixture.worklogMove.candidates, [candidate])
        XCTAssertEqual(fixture.worklogMove.selectedTaskID, secondTask.id)
        XCTAssertFalse(fixture.worklogMove.canSubmit)
    }

    @MainActor
    func testKeyboardSelectionMovesFromTheSelectedCandidateAndStopsAtTheBounds() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let candidates = [
            WorklogMoveCandidate(id: "destination-one", name: "First destination"),
            WorklogMoveCandidate(id: "destination-two", name: "Second destination"),
            WorklogMoveCandidate(id: "destination-three", name: "Third destination"),
        ]
        _ = try await open(fixture, candidates: candidates)
        let move = fixture.worklogMove
        move.select(taskID: candidates[1].id)

        move.moveSelection(by: 1)
        XCTAssertEqual(move.selectedTaskID, candidates[2].id)
        move.moveSelection(by: 1)
        XCTAssertEqual(move.selectedTaskID, candidates[2].id)
        move.moveSelection(by: -1)
        XCTAssertEqual(move.selectedTaskID, candidates[1].id)
        move.moveSelection(by: -2)
        XCTAssertEqual(move.selectedTaskID, candidates[0].id)
        move.moveSelection(by: -1)
        XCTAssertEqual(move.selectedTaskID, candidates[0].id)
    }

    @MainActor
    func testExactTimestampComparisonKeepsMicrosecondsAndEquivalentOffsets() async {
        XCTAssertTrue(sameWorklogTimestamp("2024-12-30T09:00:12.123456Z", "2024-12-30T10:00:12.123456000+01:00"))
        XCTAssertFalse(sameWorklogTimestamp("2024-12-30T09:00:12.123456Z", "2024-12-30T09:00:12.123457Z"))
        XCTAssertTrue(sameWorklogTimestamp("2024-12-30T09:00:12Z", "2024-12-30T09:00:12.000Z"))
        XCTAssertFalse(sameWorklogTimestamp("invalid", "also invalid"))
        XCTAssertFalse(sameWorklogTimestamp("2024-12-30T09:00:12.Z", "2024-12-30T09:00:12.000Z"))
        XCTAssertFalse(sameWorklogTimestamp("2024-12-30T09:00:12.1234567890Z", "2024-12-30T09:00:12.000Z"))
        XCTAssertFalse(sameWorklogTimestamp(nil, "2024-12-30T09:00:12.000Z"))
    }
}
