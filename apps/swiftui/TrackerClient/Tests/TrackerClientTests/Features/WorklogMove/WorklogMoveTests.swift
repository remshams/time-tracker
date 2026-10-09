import Foundation
import XCTest
@testable import TrackerClient

final class WorklogMoveTests: XCTestCase {
    private let precise = WorklogItem(
        id: "precise", taskId: firstTask.id,
        start: "2024-12-30T09:00:12.123456Z", end: "2024-12-30T10:00:45.654321Z")
    private let candidate = WorklogMoveCandidate(id: secondTask.id, name: secondTask.name)

    @MainActor
    private func open(_ fixture: Fixture, worklog: WorklogItem? = nil) async throws -> WorklogItem {
        let log = worklog ?? precise
        try await fixture.start(TrackerSnapshot(tasks: [firstTask, secondTask], active: log.end == nil ? log : nil))
        if log.end != nil {
            fixture.session.retryHistory()
            let page = try await fixture.client.next()
            page.succeed(HistoryPage(worklogs: [log], nextCursor: "older", reset: false))
            try await fixture.settled()
        }
        fixture.session.openWorklogMove(worklogID: log.id)
        XCTAssertEqual(fixture.session.worklogMove.sourceTaskName, firstTask.name)
        let refresh = try await fixture.client.next()
        XCTAssertEqual(refresh.operation, .snapshot)
        refresh.succeed(TrackerSnapshot(tasks: [firstTask, secondTask], active: log.end == nil ? log : nil))
        let search = try await fixture.client.next()
        XCTAssertEqual(search.operation, .candidates(source: firstTask.id, query: ""))
        search.candidates([candidate])
        try await fixture.settled()
        return log
    }

    @MainActor
    private func preflight(
        _ fixture: Fixture, log: WorklogItem, committed: Bool = false, recovery: Bool = false,
        tasks: [TaskItem] = [firstTask, secondTask]
    ) async throws {
        let snapshot = try await fixture.client.next()
        XCTAssertEqual(snapshot.operation, .snapshot)
        snapshot.succeed(TrackerSnapshot(tasks: tasks, active: log.end == nil ? log : nil))
        if log.end != nil {
            if committed || recovery {
                let destination = try await fixture.client.next()
                XCTAssertEqual(destination.operation, .history(task: secondTask.id, cursor: nil))
                destination.succeed(committed ? HistoryPage(worklogs: [log], nextCursor: nil, reset: false) : emptyPage)
            }
            if !committed {
                let source = try await fixture.client.next()
                XCTAssertEqual(source.operation, .history(task: firstTask.id, cursor: nil))
                source.succeed(HistoryPage(worklogs: [log], nextCursor: nil, reset: false))
            }
        }
    }

    @MainActor
    private func finishHistory(_ fixture: Fixture, rows: [WorklogItem] = []) async throws {
        let history = try await fixture.client.next()
        history.succeed(HistoryPage(worklogs: rows, nextCursor: nil, reset: false))
        try await fixture.settled()
    }

    @MainActor
    func testCompletedMovePreservesRawTimesAndClearsSourceHistoryBeforeReload() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture)
        XCTAssertTrue(fixture.session.worklogMove.canSubmit)
        fixture.session.submitWorklogMove()
        try await preflight(fixture, log: log)
        let command = try await fixture.client.next()
        XCTAssertEqual(command.operation, .move(expected: log, destination: secondTask.id))
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: log.end)
        command.moved(worklog: moved, snapshot: TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        let history = try await fixture.client.next()
        XCTAssertFalse(fixture.session.worklogMove.isPresented)
        XCTAssertEqual(fixture.session.worklogs, [])
        XCTAssertNil(fixture.session.nextCursor)
        history.fail(BridgeFailure(message: "History unavailable"))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.worklogs, [])
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 1)
    }

    @MainActor
    func testRunningMoveKeepsIdentityStartAndElapsedTime() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        let elapsed = fixture.session.elapsed
        fixture.session.submitWorklogMove()
        try await preflight(fixture, log: log)
        let command = try await fixture.client.next()
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: nil)
        command.moved(worklog: moved, snapshot: TrackerSnapshot(tasks: [firstTask, secondTask], active: moved))
        try await finishHistory(fixture)
        XCTAssertEqual(fixture.session.active, moved)
        XCTAssertEqual(fixture.session.elapsed, elapsed)
        XCTAssertEqual(fixture.session.runningTaskName, secondTask.name)
    }

    @MainActor
    func testSearchCoalescesRevisionsAndIgnoresOldResponseWithoutAnotherRefresh() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await open(fixture)
        fixture.session.setWorklogMoveQuery("s")
        let first = try await fixture.client.next()
        fixture.session.setWorklogMoveQuery("se")
        fixture.session.setWorklogMoveQuery("second")
        XCTAssertNil(fixture.session.worklogMove.selectedTaskID)
        XCTAssertFalse(fixture.session.worklogMove.canSubmit)
        first.candidates([WorklogMoveCandidate(id: "obsolete", name: "Obsolete")])
        let latest = try await fixture.client.next()
        XCTAssertEqual(latest.operation, .candidates(source: firstTask.id, query: "second"))
        XCTAssertEqual(fixture.session.worklogMove.candidates, [])
        latest.candidates([candidate])
        try await fixture.settled()
        XCTAssertEqual(fixture.session.worklogMove.selectedTaskID, secondTask.id)
        XCTAssertEqual(fixture.client.operations.filter { $0 == .snapshot }.count, 1)
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 1)
    }

    @MainActor
    func testOpeningDuringRefreshCoalescesSearchAndQueuedConnectionCannotChangeSource() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(snapshot)
        fixture.session.refresh()
        let refresh = try await fixture.client.next()
        let connection = Task { await fixture.session.connect(serverSettings) }
        try await fixture.waitUntil("queued connection") { fixture.session.isBlockingControls }
        fixture.session.openWorklogMove(worklogID: activeWorklog.id)
        fixture.session.setWorklogMoveQuery("latest")
        refresh.succeed(snapshot)
        let connected = try await fixture.taskValue(connection)
        XCTAssertFalse(connected)
        let opening = try await fixture.client.next()
        XCTAssertEqual(opening.operation, .snapshot)
        fixture.session.setWorklogMoveQuery("final")
        opening.succeed(snapshot)
        let search = try await fixture.client.next()
        XCTAssertEqual(search.operation, .candidates(source: firstTask.id, query: "final"))
        search.candidates([candidate])
        try await fixture.settled()
        XCTAssertEqual(fixture.session.connectionSettings, .local)
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 1)
    }

    @MainActor
    func testSearchFailureCanReloadAndEmptyResultsCannotSubmit() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await open(fixture)
        fixture.session.setWorklogMoveQuery("missing")
        let search = try await fixture.client.next()
        search.fail(BridgeFailure(message: "Search unavailable"))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.worklogMove.error, "Search unavailable")
        XCTAssertTrue(fixture.session.worklogMove.canEdit)
        XCTAssertFalse(fixture.session.worklogMove.canSubmit)
        fixture.session.retryWorklogMoveCandidates()
        let refresh = try await fixture.client.next()
        refresh.succeed(TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        let retry = try await fixture.client.next()
        retry.candidates([])
        try await fixture.settled()
        XCTAssertFalse(fixture.session.worklogMove.isSearching)
        XCTAssertNil(fixture.session.worklogMove.error)
        XCTAssertFalse(fixture.session.worklogMove.canSubmit)
    }

    @MainActor
    func testChangedWorklogRequiresReviewBeforeMove() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        fixture.session.submitWorklogMove()
        let stopped = WorklogItem(
            id: log.id, taskId: firstTask.id, start: log.start, end: "2025-01-01T00:00:00.000000Z")
        try await preflight(fixture, log: stopped)
        try await finishHistory(fixture, rows: [stopped])
        XCTAssertTrue(fixture.session.worklogMove.requiresReview)
        XCTAssertFalse(fixture.session.worklogMove.canSubmit)
        XCTAssertEqual(fixture.session.worklogMove.latest, stopped)
        fixture.session.reviewLatestWorklogMove()
        XCTAssertEqual(fixture.session.worklogMove.sourceTaskName, firstTask.name)
        let refresh = try await fixture.client.next()
        refresh.succeed(TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        let search = try await fixture.client.next()
        search.candidates([candidate])
        try await fixture.settled()
        XCTAssertEqual(fixture.session.worklogMove.original, stopped)
        XCTAssertTrue(fixture.session.worklogMove.canSubmit)
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .move = $0 { return true }; return false
            })
    }

    @MainActor
    func testArchivedDestinationFailsBeforeWriteAndLeavesDraftEditable() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        fixture.session.submitWorklogMove()
        let archived = TaskItem(id: secondTask.id, name: secondTask.name, archived: true, latestStart: nil)
        try await preflight(fixture, log: log, tasks: [firstTask, archived])
        try await fixture.settled()
        XCTAssertTrue(fixture.session.worklogMove.canEdit)
        XCTAssertNotNil(fixture.session.worklogMove.error)
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .move = $0 { return true }; return false
            })
    }

    @MainActor
    func testLostCompletedResponseFindsDestinationAndDoesNotWriteTwice() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture)
        fixture.session.submitWorklogMove()
        try await preflight(fixture, log: log)
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Response lost", uncertain: true))
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: log.end)
        try await preflight(fixture, log: moved, committed: true)
        try await finishHistory(fixture)
        XCTAssertFalse(fixture.session.worklogMove.isPresented)
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .move = $0 { return true }; return false
            }.count, 1)
    }

    @MainActor
    func testUncertainFailureFreezesOriginalIntentAcrossDismissAndRetry() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        fixture.session.submitWorklogMove()
        try await preflight(fixture, log: log)
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Response lost", uncertain: true))
        let reconciliation = try await fixture.client.next()
        reconciliation.fail(BridgeFailure(message: "Offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertFalse(fixture.session.worklogMove.canEdit)
        XCTAssertTrue(fixture.session.worklogMove.canSubmit)
        fixture.session.cancelWorklogMove()
        XCTAssertFalse(fixture.session.canOpenTaskCreation)
        XCTAssertFalse(fixture.session.canOpenTaskRename)
        XCTAssertFalse(fixture.session.canOpenWorklogCorrection)
        let connecting = Task { await fixture.session.connect(serverSettings) }
        let connected = try await fixture.taskValue(connecting)
        XCTAssertFalse(connected)
        fixture.session.openWorklogMove(worklogID: "ignored")
        fixture.session.setWorklogMoveQuery("ignored")
        fixture.session.submitWorklogMove()
        try await preflight(fixture, log: log)
        let retry = try await fixture.client.next()
        XCTAssertEqual(retry.operation, command.operation)
        retry.fail(BridgeFailure(message: "Conflict", kind: "conflict"))
        try await preflight(fixture, log: log)
        try await fixture.settled()
        XCTAssertTrue(fixture.session.worklogMove.canEdit)
    }

    @MainActor
    func testSleepDefersPreflightWriteAndShutdownIgnoresLateResponse() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        fixture.session.submitWorklogMove()
        let snapshot = try await fixture.client.next()
        fixture.session.sleep()
        snapshot.succeed(TrackerSnapshot(tasks: [firstTask, secondTask], active: log))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.worklogMove.isSubmitting)
        fixture.session.wake()
        try await preflight(fixture, log: log)
        let command = try await fixture.client.next()
        fixture.session.shutdown()
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: nil)
        command.moved(worklog: moved, snapshot: TrackerSnapshot(tasks: [firstTask, secondTask], active: moved))
        await Task.yield()
        XCTAssertEqual(fixture.session.active, log)
        XCTAssertFalse(fixture.session.worklogMove.isPresented)
    }

    @MainActor
    func testObserverFreezesDismissedContentAndCandidateChangesDoNotRedrawHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await open(fixture)
        let observer = TrackerPresentationObserver(session: fixture.session)
        var contentChanges = 0
        var moveChanges = 0
        observer.onContentChange = { contentChanges += 1 }
        observer.onWorklogMoveChange = { moveChanges += 1 }
        fixture.session.onChange = { observer.update(from: fixture.session) }
        fixture.session.setWorklogMoveQuery("second")
        let search = try await fixture.client.next()
        search.candidates([candidate])
        try await fixture.settled()
        fixture.session.onChange = { observer.update(from: fixture.session) }
        let visible = observer.worklogMoveSheetContent
        fixture.session.cancelWorklogMove()
        XCTAssertEqual(observer.worklogMoveSheetContent, visible)
        XCTAssertGreaterThan(moveChanges, 0)
        XCTAssertEqual(contentChanges, 0)
        fixture.session.openTaskCreation()
        XCTAssertFalse(fixture.session.canOpenWorklogMove)
    }

    @MainActor
    func testOpeningRefreshCannotClearChangedWorklogReviewWithCandidates() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog))
        fixture.session.openWorklogMove(worklogID: activeWorklog.id)
        let refresh = try await fixture.client.next()
        let changed = WorklogItem(id: activeWorklog.id, taskId: secondTask.id, start: activeWorklog.start, end: nil)
        refresh.succeed(TrackerSnapshot(tasks: [firstTask, secondTask], active: changed))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.worklogMove.requiresReview)
        XCTAssertEqual(fixture.session.worklogMove.error, "This worklog changed. Review it before moving.")
        XCTAssertFalse(fixture.session.worklogMove.isSearching)
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .candidates = $0 { return true }; return false
            })
    }

    @MainActor
    func testCoalescedOpeningSearchFailureReportsLatestQueryError() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(snapshot)
        fixture.session.openWorklogMove(worklogID: activeWorklog.id)
        let refresh = try await fixture.client.next()
        fixture.session.setWorklogMoveQuery("latest")
        refresh.succeed(snapshot)
        let search = try await fixture.client.next()
        XCTAssertEqual(search.operation, .candidates(source: firstTask.id, query: "latest"))
        search.fail(BridgeFailure(message: "Search failed"))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.worklogMove.error, "Search failed")
        XCTAssertFalse(fixture.session.worklogMove.isSearching)
    }

    @MainActor
    func testInFlightSearchCannotClearReviewErrorAfterWorklogStops() {
        let state = WorklogMoveState()
        state.open(activeWorklog, sourceTaskName: firstTask.name, historyPageLimit: 2)
        let search = state.takeSearch()!
        let stopped = WorklogItem(
            id: activeWorklog.id, taskId: firstTask.id,
            start: activeWorklog.start, end: "2025-01-01T00:00:00.000000Z")
        state.observe(stopped)
        state.accept([candidate], search: search)
        state.failSearch(BridgeFailure(message: "Obsolete failure"), search: search)
        XCTAssertTrue(state.presentation.requiresReview)
        XCTAssertEqual(state.presentation.error, "This worklog changed. Review it before moving.")
        XCTAssertFalse(state.presentation.isSearching)
        XCTAssertNil(state.takeSearch())
    }

    @MainActor
    func testSelectionUsesWorkerRankingAndClampsKeyboardNavigation() {
        let state = WorklogMoveState()
        state.open(precise, sourceTaskName: firstTask.name, historyPageLimit: 2)
        let search = state.takeSearch()!
        let third = WorklogMoveCandidate(id: "third", name: "Third")
        state.accept([third, candidate], search: search)
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
    func testMalformedMoveResultReconcilesBeforeAllowingRetry() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        fixture.session.submitWorklogMove()
        try await preflight(fixture, log: log)
        let command = try await fixture.client.next()
        let invalid = WorklogItem(id: log.id, taskId: secondTask.id, start: "2024-12-31T23:58:30.000Z", end: nil)
        command.moved(worklog: invalid, snapshot: TrackerSnapshot(tasks: [firstTask, secondTask], active: invalid))
        try await preflight(fixture, log: log)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.active, log)
        XCTAssertFalse(fixture.session.worklogMove.canEdit)
        XCTAssertTrue(fixture.session.worklogMove.canSubmit)
        XCTAssertEqual(fixture.session.worklogMove.error, "The move response does not contain the moved worklog.")
    }

    @MainActor
    func testPreflightConnectionFailureIsReportedWithoutSendingAMove() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await open(fixture, worklog: activeWorklog)
        fixture.session.submitWorklogMove()
        let snapshot = try await fixture.client.next()
        snapshot.fail(BridgeFailure(message: "Move preflight offline", kind: "unavailable"))
        try await fixture.settled()

        XCTAssertTrue(fixture.session.isStale)
        XCTAssertEqual(fixture.session.connectionStatusText, "Unavailable")
        XCTAssertEqual(fixture.session.error, "Move preflight offline")
        XCTAssertEqual(fixture.session.worklogMove.error, "Move preflight offline")
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .move = $0 { return true }; return false
            })
    }

    @MainActor
    func testCandidateSearchRetriesWhenItsInitialSnapshotArrivesDuringSleep() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(snapshot)
        fixture.session.openWorklogMove(worklogID: activeWorklog.id)
        let openingSnapshot = try await fixture.client.next()
        XCTAssertEqual(openingSnapshot.operation, .snapshot)
        fixture.session.sleep()
        openingSnapshot.succeed(snapshot)
        try await fixture.settled()
        XCTAssertTrue(fixture.session.worklogMove.isSearching)

        fixture.session.wake()
        let refresh = try await fixture.client.next()
        refresh.succeed(snapshot)
        let retry = try await fixture.client.next()
        XCTAssertEqual(retry.operation, .candidates(source: firstTask.id, query: ""))
        retry.candidates([candidate])
        try await fixture.settled()

        XCTAssertFalse(fixture.session.worklogMove.isSearching)
        XCTAssertEqual(fixture.session.worklogMove.selectedTaskID, secondTask.id)
    }

    @MainActor
    func testCandidateSearchRetriesAfterItsResponseArrivesDuringSleep() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await open(fixture)
        fixture.session.setWorklogMoveQuery("Destination")
        let search = try await fixture.client.next()
        fixture.session.sleep()
        search.candidates([candidate])
        try await fixture.settled()
        XCTAssertTrue(fixture.session.worklogMove.isSearching)

        fixture.session.wake()
        let refresh = try await fixture.client.next()
        refresh.succeed(TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        let retry = try await fixture.client.next()
        XCTAssertEqual(retry.operation, .candidates(source: firstTask.id, query: "Destination"))
        retry.candidates([candidate])
        try await fixture.settled()

        XCTAssertFalse(fixture.session.worklogMove.isSearching)
        XCTAssertEqual(fixture.session.worklogMove.selectedTaskID, secondTask.id)
    }

    @MainActor
    func testAnOpenMoverPublishesWhyAConnectionChangeIsBlocked() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await open(fixture)
        var publishedMessage: String?
        fixture.session.onChange = { publishedMessage = fixture.session.connectionMessage }

        let connecting = Task { await fixture.session.connect(serverSettings) }
        let connected = try await fixture.taskValue(connecting)

        XCTAssertFalse(connected)
        XCTAssertEqual(publishedMessage, "Finish or retry worklog moving before changing connections.")
        XCTAssertEqual(fixture.session.connectionSettings, .local)
    }

    @MainActor
    func testHistoryRefreshRequiresReviewWhenTheCompletedWorklogChanged() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture)
        fixture.session.retryHistory()
        let history = try await fixture.client.next()
        let changed = WorklogItem(
            id: log.id, taskId: log.taskId, start: log.start, end: "2024-12-30T10:30:00.000000Z")
        history.succeed(HistoryPage(worklogs: [changed], nextCursor: nil, reset: false))
        try await fixture.settled()

        XCTAssertTrue(fixture.session.worklogMove.requiresReview)
        XCTAssertEqual(fixture.session.worklogMove.latest, changed)
        XCTAssertFalse(fixture.session.worklogMove.canSubmit)
    }

    @MainActor
    func testPreflightIncludesHistoryLoadedAfterTheMoverOpened() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture)
        fixture.session.loadOlder()
        let older = try await fixture.client.next()
        let rows = (0..<100).map {
            WorklogItem(id: "older-worklog-\($0)", taskId: log.taskId, start: log.start, end: log.end)
        }
        older.succeed(HistoryPage(worklogs: rows, nextCursor: "more-history", reset: false))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.worklogs.count, 101)
        fixture.session.submitWorklogMove()
        let snapshot = try await fixture.client.next()
        snapshot.succeed(TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        for cursor in ["second-page", "third-page"] {
            let page = try await fixture.client.next()
            page.succeed(HistoryPage(worklogs: [], nextCursor: cursor, reset: false))
        }
        let third = try await fixture.client.next()
        XCTAssertEqual(third.operation, .history(task: firstTask.id, cursor: "third-page"))
        third.succeed(HistoryPage(worklogs: [log], nextCursor: nil, reset: false))
        let command = try await fixture.client.next()
        XCTAssertEqual(command.operation, .move(expected: log, destination: secondTask.id))
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: log.end)
        command.moved(worklog: moved, snapshot: TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        try await finishHistory(fixture)

        XCTAssertFalse(fixture.session.worklogMove.isPresented)
    }

    @MainActor
    func testDeletedWorklogRequiresReviewWithoutSendingAMove() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await open(fixture)
        fixture.session.submitWorklogMove()
        let snapshot = try await fixture.client.next()
        snapshot.succeed(TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()

        XCTAssertTrue(fixture.session.worklogMove.requiresReview)
        XCTAssertFalse(fixture.session.worklogMove.canSubmit)
        XCTAssertNil(fixture.session.worklogMove.latest)
        XCTAssertEqual(
            fixture.session.worklogMove.error,
            "This worklog was moved or deleted. Cancel the mover and refresh history.")
    }

    @MainActor
    func testMissingDestinationDoesNotMisreportAnExistingWorklogAsDeleted() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture)
        fixture.session.submitWorklogMove()
        try await preflight(fixture, log: log, tasks: [firstTask])
        try await fixture.settled()

        XCTAssertFalse(fixture.session.worklogMove.requiresReview)
        XCTAssertTrue(fixture.session.worklogMove.canEdit)
        XCTAssertNotNil(fixture.session.worklogMove.error)
        XCTAssertEqual(fixture.session.worklogMove.original, log)
    }

    @MainActor
    func testMovePreflightFailuresOnlyMarkTheConnectionStaleWhenRequested() async throws {
        for refresh in [false, true] {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            _ = try await open(fixture, worklog: activeWorklog)
            fixture.session.submitWorklogMove()
            let snapshot = try await fixture.client.next()
            snapshot.fail(BridgeFailure(message: "Move preflight rejected", kind: "conflict", requiresRefresh: refresh))
            try await fixture.settled()

            XCTAssertEqual(fixture.session.isStale, refresh)
            XCTAssertEqual(fixture.session.error, refresh ? "Move preflight rejected" : nil)
            XCTAssertTrue(fixture.session.worklogMove.canEdit)
        }
    }

    @MainActor
    func testUnavailableAndProtocolWriteFailuresRetainTheMoveForRetry() async throws {
        for kind in ["unavailable", "protocol"] {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            let log = try await open(fixture, worklog: activeWorklog)
            fixture.session.submitWorklogMove()
            try await preflight(fixture, log: log)
            let command = try await fixture.client.next()
            command.fail(BridgeFailure(message: "Move response unavailable", kind: kind))
            try await preflight(fixture, log: log)
            try await fixture.settled()

            XCTAssertFalse(fixture.session.worklogMove.canEdit, kind)
            XCTAssertTrue(fixture.session.worklogMove.canSubmit, kind)
            XCTAssertEqual(fixture.session.worklogMove.original, log, kind)
            XCTAssertEqual(fixture.session.worklogMove.error, "Move response unavailable", kind)
        }
    }

    @MainActor
    func testRemoteMoveReceiptAcceptsNewerTrackingAndDestinationMetadata() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        fixture.session.submitWorklogMove()
        try await preflight(fixture, log: log)
        let command = try await fixture.client.next()
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: nil)
        let archived = TaskItem(id: secondTask.id, name: secondTask.name, archived: true, latestStart: nil)
        command.moved(
            worklog: moved,
            snapshot: TrackerSnapshot(
                tasks: [firstTask, archived], active: nil,
                tasksRevision: "epoch:3", trackingRevision: "epoch:3"))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertNil(fixture.session.active)
        XCTAssertNil(fixture.session.worklogMove.error)
        XCTAssertFalse(fixture.session.worklogMove.isPresented)
    }

    @MainActor
    func testMoveResponseWithAnArchivedDestinationRequiresReconciliation() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        fixture.session.submitWorklogMove()
        try await preflight(fixture, log: log)
        let command = try await fixture.client.next()
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: nil)
        let archived = TaskItem(id: secondTask.id, name: secondTask.name, archived: true, latestStart: nil)
        command.moved(worklog: moved, snapshot: TrackerSnapshot(tasks: [firstTask, archived], active: moved))
        try await preflight(fixture, log: log)
        try await fixture.settled()

        XCTAssertEqual(fixture.session.active, log)
        XCTAssertFalse(fixture.session.worklogMove.canEdit)
        XCTAssertTrue(fixture.session.worklogMove.canSubmit)
        XCTAssertEqual(fixture.session.worklogMove.error, "The move response does not contain the moved worklog.")
    }

    @MainActor
    func testEntryConflictRequiresReviewEvenWhenLatestMatchesOriginal() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        fixture.session.submitWorklogMove()
        try await preflight(fixture, log: log)
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Worklog changed", kind: "worklog_changed"))
        try await preflight(fixture, log: log)
        try await fixture.settled()
        XCTAssertTrue(fixture.session.worklogMove.requiresReview)
        XCTAssertEqual(fixture.session.worklogMove.latest, log)
        XCTAssertFalse(fixture.session.worklogMove.canSubmit)
    }

    @MainActor
    func testUncertainRetryFindsCommittedDestinationBeforeReadingSource() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture)
        fixture.session.submitWorklogMove()
        try await preflight(fixture, log: log)
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Lost response", uncertain: true))
        let offline = try await fixture.client.next()
        offline.fail(BridgeFailure(message: "Offline", kind: "unavailable"))
        try await fixture.settled()
        fixture.session.submitWorklogMove()
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: log.end)
        try await preflight(fixture, log: moved, committed: true)
        try await finishHistory(fixture)
        XCTAssertFalse(fixture.session.worklogMove.isPresented)
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .move = $0 { return true }; return false
            }.count, 1)
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
        refresh.succeed(TrackerSnapshot(tasks: tasks, active: nil))
        try await fixture.settled()
        fixture.session.select(third.id)
        let selectedHistory = try await fixture.client.next()
        let unrelated = WorklogItem(id: "unrelated", taskId: third.id, start: log.start, end: log.end)
        selectedHistory.succeed(HistoryPage(worklogs: [unrelated], nextCursor: "older-third", reset: false))
        try await fixture.settled()
        fixture.session.submitWorklogMove()
        try await preflight(fixture, log: log, tasks: tasks)
        let command = try await fixture.client.next()
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: log.end)
        command.moved(worklog: moved, snapshot: TrackerSnapshot(tasks: tasks, active: nil))
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
    func testCompletedPreflightChecksSourceWithoutScanningBusyDestination() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture)
        fixture.session.submitWorklogMove()
        try await preflight(fixture, log: log)
        let command = try await fixture.client.next()
        XCTAssertEqual(command.operation, .move(expected: log, destination: secondTask.id))
        XCTAssertFalse(fixture.client.operations.contains(.history(task: secondTask.id, cursor: nil)))
    }

    @MainActor
    func testUncertainReconciliationContinuesPastDestinationLimitAndFindsSource() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture)
        fixture.session.submitWorklogMove()
        try await preflight(fixture, log: log)
        let command = try await fixture.client.next()
        command.fail(BridgeFailure(message: "Lost response", uncertain: true))
        let snapshot = try await fixture.client.next()
        snapshot.succeed(TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        for cursor in ["older-destination", "unverified-destination"] {
            let page = try await fixture.client.next()
            page.succeed(HistoryPage(worklogs: [], nextCursor: cursor, reset: false))
        }
        let source = try await fixture.client.next()
        XCTAssertEqual(source.operation, .history(task: firstTask.id, cursor: nil))
        source.succeed(HistoryPage(worklogs: [log], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertFalse(fixture.session.worklogMove.requiresReview)
        XCTAssertTrue(fixture.session.worklogMove.canSubmit)
        fixture.session.submitWorklogMove()
        try await preflight(fixture, log: log, recovery: true)
        let retry = try await fixture.client.next()
        XCTAssertEqual(retry.operation, command.operation)
    }

    @MainActor
    func testBoundedLookupNeverCallsUnverifiedEntryDeleted() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        _ = try await open(fixture)
        fixture.session.submitWorklogMove()
        let snapshot = try await fixture.client.next()
        snapshot.succeed(TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        for cursor in ["older-source", "unverified-source"] {
            let page = try await fixture.client.next()
            page.succeed(HistoryPage(worklogs: [], nextCursor: cursor, reset: false))
        }
        try await fixture.settled()
        XCTAssertFalse(fixture.session.worklogMove.requiresReview)
        XCTAssertTrue(fixture.session.worklogMove.canEdit)
        XCTAssertTrue(fixture.session.worklogMove.error?.contains("history limit") == true)
    }

    @MainActor
    func testCancelledOpeningDoesNotSearchOrOverwriteReopenedSheet() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog))
        fixture.session.openWorklogMove(worklogID: activeWorklog.id)
        let opening = try await fixture.client.next()
        fixture.session.cancelWorklogMove()
        opening.succeed(TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog))
        try await fixture.settled()
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .candidates = $0 { return true }; return false
            })
        fixture.session.openWorklogMove(worklogID: activeWorklog.id)
        let refresh = try await fixture.client.next()
        refresh.succeed(TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog))
        let search = try await fixture.client.next()
        search.candidates([candidate])
        try await fixture.settled()
        XCTAssertTrue(fixture.session.worklogMove.canSubmit)
    }

    @MainActor
    func testMoveQueuedDuringRefreshKeepsDestinationAndBlocksTracking() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        let snapshot = TrackerSnapshot(tasks: [firstTask, secondTask], active: log)
        fixture.session.refresh()
        let refresh = try await fixture.client.next()
        fixture.session.submitWorklogMove()
        fixture.session.selectWorklogMoveDestination(taskID: "ignored")
        XCTAssertTrue(fixture.session.isBlockingControls)
        XCTAssertFalse(fixture.session.canStopTracking)
        refresh.succeed(snapshot)
        try await preflight(fixture, log: log)
        let command = try await fixture.client.next()
        XCTAssertEqual(command.operation, .move(expected: log, destination: secondTask.id))
        XCTAssertEqual(fixture.client.maximumOutstandingRequests, 1)
    }

    @MainActor
    func testSuccessfulMoveFreezesSubmittingContentDuringDismissal() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let log = try await open(fixture, worklog: activeWorklog)
        let observer = TrackerPresentationObserver(session: fixture.session)
        fixture.session.onChange = { observer.update(from: fixture.session) }
        fixture.session.submitWorklogMove()
        try await preflight(fixture, log: log)
        let command = try await fixture.client.next()
        let visible = observer.worklogMoveSheetContent
        XCTAssertTrue(visible.isSubmitting)
        let moved = WorklogItem(id: log.id, taskId: secondTask.id, start: log.start, end: nil)
        command.moved(worklog: moved, snapshot: TrackerSnapshot(tasks: [firstTask, secondTask], active: moved))
        let history = try await fixture.client.next()
        XCTAssertEqual(observer.worklogMoveSheetContent, visible)
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(observer.worklogMoveSheetContent, visible)
    }

    @MainActor
    func testKeyboardSelectionMovesFromTheSelectedCandidateAndStopsAtTheBounds() throws {
        let state = WorklogMoveState()
        state.open(precise, sourceTaskName: firstTask.name, historyPageLimit: 2)
        let search = try XCTUnwrap(state.takeSearch())
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
