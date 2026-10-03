import XCTest
@testable import TrackerClient

final class HistoryTests: XCTestCase {
    @MainActor
    func testSelectionsAreRememberedPerTabAndRemovedSelectionFallsBack() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask, secondTask, archivedTask], active: nil)
        try await fixture.start(snapshot)
        fixture.session.select(secondTask.id)
        let selected = try await fixture.client.next()
        selected.succeed(emptyPage)
        try await fixture.settled()
        fixture.session.changeTab(.archived)
        let archived = try await fixture.client.next()
        XCTAssertEqual(archived.operation, .history(task: archivedTask.id, cursor: nil))
        archived.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.visibleTasks, [archivedTask])
        fixture.session.select(firstTask.id)
        XCTAssertEqual(fixture.session.selectedTaskID, archivedTask.id)
        fixture.session.changeTab(.active)
        let restored = try await fixture.client.next()
        XCTAssertEqual(restored.operation, .history(task: secondTask.id, cursor: nil))
        restored.succeed(emptyPage)
        try await fixture.settled()
        fixture.session.refresh()
        let refresh = try await fixture.client.next()
        refresh.succeed(TrackerSnapshot(tasks: [firstTask, archivedTask], active: nil))
        let fallback = try await fixture.client.next()
        XCTAssertEqual(fallback.operation, .history(task: firstTask.id, cursor: nil))
        fallback.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.selectedTaskID, firstTask.id)
    }

    @MainActor
    func testDelayedHistoryCannotReplaceNewSelectionOrItsErrorState() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        fixture.session.retryHistory()
        let oldRequest = try await fixture.client.next()
        fixture.session.select(secondTask.id)
        XCTAssertTrue(fixture.session.worklogs.isEmpty)
        oldRequest.fail(BridgeFailure(message: "Old selection failed", kind: "unavailable"))
        let newRequest = try await fixture.client.next()
        XCTAssertEqual(newRequest.operation, .history(task: secondTask.id, cursor: nil))
        XCTAssertFalse(fixture.session.historyUnavailable)
        XCTAssertNil(fixture.session.error)
        XCTAssertFalse(fixture.session.isStale)
        newRequest.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.selectedTaskID, secondTask.id)
        XCTAssertFalse(fixture.session.historyUnavailable)
        XCTAssertNil(fixture.session.error)
    }

    @MainActor
    func testDelayedSuccessfulHistoryCannotPopulateNewSelection() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        fixture.session.retryHistory()
        let oldRequest = try await fixture.client.next()
        fixture.session.select(secondTask.id)
        oldRequest.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "obsolete", reset: false))
        let newRequest = try await fixture.client.next()
        XCTAssertTrue(fixture.session.worklogs.isEmpty)
        XCTAssertNil(fixture.session.nextCursor)
        newRequest.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertTrue(fixture.session.worklogs.isEmpty)
    }

    @MainActor
    func testPaginationAppendsButResetReplacesPreviousRowsAndCursor() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask], active: nil))
        fixture.session.retryHistory()
        let first = try await fixture.client.next()
        first.succeed(HistoryPage(worklogs: [activeWorklog], nextCursor: "page-two", reset: false))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.hasMoreHistory)
        fixture.session.loadOlder()
        let older = try await fixture.client.next()
        XCTAssertEqual(older.operation, .history(task: firstTask.id, cursor: "page-two"))
        fixture.session.loadOlder()
        older.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "page-three", reset: false))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.worklogs, [activeWorklog, oldWorklog])
        fixture.session.loadOlder()
        let reset = try await fixture.client.next()
        XCTAssertEqual(reset.operation, .history(task: firstTask.id, cursor: "page-three"))
        reset.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: nil, reset: true))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.worklogs, [oldWorklog])
        XCTAssertFalse(fixture.session.hasMoreHistory)
    }

    @MainActor
    func testHistoryFailureRetriesCurrentSelectionAndClearsError() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask], active: nil))
        fixture.session.retryHistory()
        let failure = try await fixture.client.next()
        failure.fail(BridgeFailure(message: "History read failed"))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.historyUnavailable)
        XCTAssertEqual(fixture.session.error, "History read failed")
        XCTAssertFalse(fixture.session.isStale)
        fixture.session.retryHistory()
        let retry = try await fixture.client.next()
        XCTAssertEqual(retry.operation, .history(task: firstTask.id, cursor: nil))
        retry.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.worklogs, [oldWorklog])
        XCTAssertFalse(fixture.session.historyUnavailable)
        XCTAssertNil(fixture.session.error)
    }

    @MainActor
    func testUnavailableHistoryMarksSnapshotStaleAndRefreshRetriesHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: nil)
        try await fixture.start(snapshot)
        fixture.session.retryHistory()
        let history = try await fixture.client.next()
        history.fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.isStale)
        XCTAssertFalse(fixture.session.canStartSelectedTask)
        fixture.session.refresh()
        let refresh = try await fixture.client.next()
        refresh.succeed(snapshot)
        let retry = try await fixture.client.next()
        XCTAssertEqual(retry.operation, .history(task: firstTask.id, cursor: nil))
        retry.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertFalse(fixture.session.isStale)
        XCTAssertFalse(fixture.session.historyUnavailable)
        XCTAssertTrue(fixture.session.canStartSelectedTask)
    }
}
