import XCTest
@testable import TrackerClient

final class WorklogHistoryStoreTests: XCTestCase {
    @MainActor
    func testReloadForTheSameTaskKeepsShownRowsUntilTheNextPageArrives() async {
        let history = makeStore(client: FakeClient()).history
        history.request(selectedTaskID: firstTask.id)
        history.accept(HistoryPage(worklogs: [oldWorklog], nextCursor: "old-cursor", reset: false), cursor: nil)

        history.request(selectedTaskID: firstTask.id)

        XCTAssertEqual(history.worklogs, [oldWorklog])
        XCTAssertEqual(history.nextCursor, "old-cursor")
        XCTAssertTrue(history.isLoading)
        history.accept(emptyPage, cursor: nil)
        XCTAssertEqual(history.worklogs, [])
        XCTAssertNil(history.nextCursor)
        XCTAssertFalse(history.isLoading)
    }

    @MainActor
    func testRequestForAnotherTaskClearsShownRows() async {
        let history = makeStore(client: FakeClient()).history
        history.request(selectedTaskID: firstTask.id)
        history.accept(HistoryPage(worklogs: [oldWorklog], nextCursor: "old-cursor", reset: false), cursor: nil)

        history.request(selectedTaskID: secondTask.id)

        XCTAssertEqual(history.worklogs, [])
        XCTAssertNil(history.nextCursor)
        XCTAssertTrue(history.isLoading)
    }

    @MainActor
    func testFailureIsReportedToTheCallerAndKeepsLoadedPages() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let (history, _, connection) = makeStore(client: fixture.client)
        connection.acceptTaskList()
        let reports = PageReports()
        history.recordReadFailure = { reports.values.append($0.localizedDescription) }
        history.loadOlder()

        history.reload(selectedTaskID: firstTask.id)
        let first = try await fixture.client.next()
        XCTAssertEqual(first.operation, .history(task: firstTask.id, cursor: nil))
        first.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "older", reset: false))
        try await settled(fixture, history)
        XCTAssertEqual(reports.values, [])

        history.loadOlder()
        XCTAssertTrue(history.isLoading)
        history.loadOlder()
        let older = try await fixture.client.next()
        XCTAssertEqual(older.operation, .history(task: firstTask.id, cursor: "older"))
        older.fail(BridgeFailure(message: "Tracker unavailable", kind: "unavailable"))
        try await settled(fixture, history)

        XCTAssertEqual(reports.values, ["Tracker unavailable"])
        XCTAssertEqual(history.worklogs, [oldWorklog])
        XCTAssertTrue(history.unavailable)
        XCTAssertEqual(history.error, "Tracker unavailable")
        XCTAssertEqual(fixture.client.operations.count, 2)
    }

    @MainActor
    func testPageOrFailureForATaskNoLongerSelectedIsDroppedWithoutReport() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let history = makeStore(client: fixture.client).history
        let reports = PageReports()
        history.recordReadFailure = { reports.values.append($0.localizedDescription) }
        let currentPage = HistoryPage(worklogs: [activeWorklog], nextCursor: nil, reset: false)

        history.reload(selectedTaskID: firstTask.id)
        let stalePage = try await fixture.client.next()
        history.reload(selectedTaskID: secondTask.id)
        let current = try await fixture.client.next()
        XCTAssertEqual(stalePage.operation, .history(task: firstTask.id, cursor: nil))
        XCTAssertEqual(current.operation, .history(task: secondTask.id, cursor: nil))
        stalePage.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "older", reset: false))
        current.succeed(currentPage)
        try await settled(fixture, history)
        XCTAssertEqual(history.worklogs, [activeWorklog])
        XCTAssertFalse(history.hasMore)

        history.reload(selectedTaskID: firstTask.id)
        let staleFailure = try await fixture.client.next()
        history.reload(selectedTaskID: secondTask.id)
        let reloaded = try await fixture.client.next()
        staleFailure.fail(BridgeFailure(message: "Tracker unavailable", kind: "unavailable"))
        reloaded.succeed(currentPage)
        try await settled(fixture, history)

        XCTAssertEqual(reports.values, [])
        XCTAssertEqual(history.worklogs, [activeWorklog])
        XCTAssertFalse(history.unavailable)
        XCTAssertNil(history.error)
    }

    @MainActor
    func testReloadRequestsTheSelectedTasksFirstPage() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let (history, catalog, _) = makeStore(client: fixture.client)
        XCTAssertTrue(catalog.apply([firstTask, secondTask], previousActive: nil, active: nil))
        XCTAssertTrue(catalog.switchSelection(to: secondTask.id))

        history.reload()

        let page = try await fixture.client.next()
        XCTAssertEqual(page.operation, .history(task: secondTask.id, cursor: nil))
        page.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: nil, reset: false))
        try await settled(fixture, history)
        XCTAssertEqual(history.worklogs, [oldWorklog])
    }

    @MainActor
    func testLoadOlderSendsNothingWhileTheConnectionIsStale() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let (history, _, connection) = makeStore(client: fixture.client)
        connection.acceptTaskList()
        history.reload(selectedTaskID: firstTask.id)
        try await fixture.client.next().succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "older", reset: false))
        try await settled(fixture, history)
        connection.recordFailure(BridgeFailure(message: "Tracker unavailable", kind: "unavailable"))

        history.loadOlder()

        XCTAssertFalse(history.isLoading)
        XCTAssertFalse(fixture.client.hasPendingRequests)
        XCTAssertEqual(fixture.client.operations.count, 1)
    }

    @MainActor
    private func makeStore(
        client: FakeClient
    ) -> (history: WorklogHistoryStore, catalog: TaskCatalogStore, connection: ConnectionStore) {
        let catalog = TaskCatalogStore()
        let connection = ConnectionStore(settings: .local)
        return (WorklogHistoryStore(client: client, catalog: catalog, connection: connection), catalog, connection)
    }

    @MainActor
    private func settled(_ fixture: Fixture, _ history: WorklogHistoryStore) async throws {
        let finished = Task { @MainActor in
            while (fixture.client.hasPendingRequests || history.isLoading) && !Task.isCancelled {
                await Task.yield()
            }
        }
        try await fixture.taskValue(finished)
    }
}

/// Records the failures a history store reported to its caller.
@MainActor
private final class PageReports {
    var values: [String] = []
}
