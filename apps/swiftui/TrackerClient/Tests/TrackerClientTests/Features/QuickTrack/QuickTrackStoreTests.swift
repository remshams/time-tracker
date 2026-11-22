import XCTest
@testable import TrackerClient

final class QuickTrackStoreTests: XCTestCase {
    /// Starts the fixture with the window shown, so opening the dialog sends only its search.
    @MainActor
    private func started(_ resources: TaskListResources) async throws -> Fixture {
        let fixture = Fixture()
        fixture.sync.setWindowVisible(true)
        try await fixture.start(resources)
        return fixture
    }

    /// Opens the dialog and answers its first search.
    @MainActor
    private func open(_ fixture: Fixture, results: [String]) async throws {
        let store = fixture.features.quickTrack
        store.open()
        XCTAssertTrue(store.isPresented)
        try await answer(fixture, query: "", with: results)
    }

    @MainActor
    private func answer(_ fixture: Fixture, query: String, with results: [String]) async throws {
        let search = try await fixture.client.next()
        XCTAssertEqual(search.operation, .search(query: query))
        try await fixture.update("the search result") {
            search.found(results)
        } until: {
            !fixture.features.quickTrack.isSearching
        }
    }

    @MainActor
    func testOpeningSearchesWithAnEmptyQueryAndListsTheResult() async throws {
        let fixture = try await started(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        defer { fixture.cleanup() }
        let store = fixture.features.quickTrack
        try await open(fixture, results: [secondTask.id, firstTask.id])
        XCTAssertEqual(store.content.tasks.map(\.id), [secondTask.id, firstTask.id])
        XCTAssertNil(store.content.highlight)
        XCTAssertEqual(store.query, "")
    }

    @MainActor
    func testOpeningCountsAsVisibleUIForTheSync() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        let store = fixture.features.quickTrack
        store.open()
        let operations = [try await fixture.client.next(), try await fixture.client.next()].map(\.operation)
        XCTAssertTrue(operations.contains(.search(query: "")))
        XCTAssertTrue(operations.contains(.refresh(.local)), "Showing the dialog refreshes like the window does.")
        store.open()
        XCTAssertEqual(fixture.client.operations.count { $0 == .search(query: "") }, 1, "A second open is ignored.")
    }

    @MainActor
    func testTypingSearchesTheTrimmedQueryAndDropsOlderResults() async throws {
        let fixture = try await started(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        defer { fixture.cleanup() }
        let store = fixture.features.quickTrack
        try await open(fixture, results: [firstTask.id, secondTask.id])
        store.setQuery(" fir")
        let older = try await fixture.client.next()
        XCTAssertEqual(older.operation, .search(query: "fir"))
        store.setQuery(" sec ")
        store.setQuery(" sec ")
        let newer = try await fixture.client.next()
        XCTAssertEqual(newer.operation, .search(query: "sec"))
        older.found([firstTask.id])
        for _ in 0..<20 { await Task.yield() }
        XCTAssertTrue(store.isSearching)
        XCTAssertEqual(store.content.tasks.map(\.id), [firstTask.id, secondTask.id])
        try await fixture.update("the newer result") {
            newer.found([secondTask.id])
        } until: {
            !store.isSearching
        }
        XCTAssertEqual(store.content.tasks.map(\.id), [secondTask.id])
        XCTAssertEqual(store.content.highlight, .task(secondTask.id))
        XCTAssertEqual(fixture.client.operations.count { $0 == .search(query: "sec") }, 1)
    }

    @MainActor
    func testReturnStartsTheHighlightedTaskAndCloses() async throws {
        let fixture = try await started(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        defer { fixture.cleanup() }
        let store = fixture.features.quickTrack
        try await open(fixture, results: [firstTask.id, secondTask.id])
        store.moveHighlight(down: true)
        XCTAssertEqual(store.content.highlight, .task(secondTask.id))
        store.activate()
        XCTAssertFalse(store.isPresented)
        let start = try await fixture.client.next()
        guard case .start(let task, let expected, _) = start.operation else {
            return XCTFail("Expected a start request, got \(start.operation)")
        }
        XCTAssertEqual(task, secondTask.id)
        XCTAssertEqual(expected, activeWorklog.id)
    }

    @MainActor
    func testReturnOnTheHeaderStopsTrackingAndCloses() async throws {
        let fixture = try await started(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        defer { fixture.cleanup() }
        let store = fixture.features.quickTrack
        try await open(fixture, results: [secondTask.id])
        store.moveHighlight(down: false)
        XCTAssertEqual(store.content.highlight, .header)
        store.activate()
        XCTAssertFalse(store.isPresented)
        let stop = try await fixture.client.next()
        guard case .stop(let worklog, _) = stop.operation else {
            return XCTFail("Expected a stop request, got \(stop.operation)")
        }
        XCTAssertEqual(worklog, activeWorklog.id)
    }

    @MainActor
    func testReturnWithoutAnActionableHighlightRejectsAndStaysOpen() async throws {
        let fixture = try await started(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        defer { fixture.cleanup() }
        let store = fixture.features.quickTrack
        var rejections = 0
        store.onReject = { rejections += 1 }
        try await open(fixture, results: [firstTask.id])
        store.activate()
        XCTAssertEqual(rejections, 1, "Nothing is highlighted with an empty query.")
        store.highlight(.header)
        store.activate()
        XCTAssertEqual(rejections, 2, "The header can't stop anything while idle.")
        store.highlight(.task(firstTask.id))
        fixture.features.tracking.beginSubmitting()
        store.activate()
        XCTAssertEqual(rejections, 3, "A task can't start while another request runs.")
        fixture.features.tracking.endSubmitting()
        XCTAssertTrue(store.isPresented)
        XCTAssertFalse(fixture.client.operations.contains { if case .start = $0 { true } else { false } })
    }

    @MainActor
    func testReturnWhileSearchingActsOnTheNewResult() async throws {
        let fixture = try await started(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        defer { fixture.cleanup() }
        let store = fixture.features.quickTrack
        try await open(fixture, results: [firstTask.id, secondTask.id])
        store.setQuery("sec")
        let search = try await fixture.client.next()
        store.activate()
        XCTAssertTrue(store.isPresented, "Return waits for the current query's result.")
        search.found([secondTask.id])
        let start = try await fixture.client.next()
        guard case .start(let task, _, _) = start.operation else {
            return XCTFail("Expected a start request, got \(start.operation)")
        }
        XCTAssertEqual(task, secondTask.id)
        XCTAssertFalse(store.isPresented)
    }

    @MainActor
    func testTypingAgainCancelsAPendingReturn() async throws {
        let fixture = try await started(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        defer { fixture.cleanup() }
        let store = fixture.features.quickTrack
        try await open(fixture, results: [firstTask.id, secondTask.id])
        store.setQuery("s")
        let first = try await fixture.client.next()
        store.activate()
        store.setQuery("se")
        let second = try await fixture.client.next()
        first.found([secondTask.id])
        try await fixture.update("the newer result") {
            second.found([secondTask.id])
        } until: {
            !store.isSearching
        }
        XCTAssertTrue(store.isPresented)
        XCTAssertFalse(fixture.client.operations.contains { if case .start = $0 { true } else { false } })
    }

    @MainActor
    func testEscapeClearsTheQueryThenCloses() async throws {
        let fixture = try await started(TaskListResources(tasks: [firstTask], active: nil))
        defer { fixture.cleanup() }
        let store = fixture.features.quickTrack
        try await open(fixture, results: [firstTask.id])
        store.setQuery("fir")
        try await answer(fixture, query: "fir", with: [firstTask.id])
        store.cancel()
        XCTAssertEqual(store.query, "")
        XCTAssertTrue(store.isPresented)
        try await answer(fixture, query: "", with: [firstTask.id])
        store.cancel()
        XCTAssertFalse(store.isPresented)
    }

    @MainActor
    func testClosingDropsALateResultAndReopeningStartsFresh() async throws {
        let fixture = try await started(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        defer { fixture.cleanup() }
        let store = fixture.features.quickTrack
        try await open(fixture, results: [firstTask.id, secondTask.id])
        store.moveHighlight(down: true)
        store.setQuery("sec")
        let late = try await fixture.client.next()
        store.close()
        XCTAssertFalse(store.isSearching)
        late.found([secondTask.id])
        for _ in 0..<20 { await Task.yield() }
        XCTAssertEqual(store.results, [firstTask.id, secondTask.id], "A result after closing is dropped.")
        store.setQuery("ignored")
        store.moveHighlight(down: true)
        store.highlight(.header)
        store.activate()
        XCTAssertEqual(store.query, "sec", "A closed dialog ignores input.")
        try await open(fixture, results: [secondTask.id])
        XCTAssertEqual(store.query, "")
        XCTAssertNil(store.content.highlight)
    }

    @MainActor
    func testFailedSearchShowsTheErrorAndAnEmptyList() async throws {
        let fixture = try await started(TaskListResources(tasks: [firstTask], active: nil))
        defer { fixture.cleanup() }
        let store = fixture.features.quickTrack
        store.open()
        let search = try await fixture.client.next()
        try await fixture.update("the failed search") {
            search.fail(BridgeFailure(message: "offline"))
        } until: {
            !store.isSearching
        }
        XCTAssertEqual(store.content.tasks, [])
        XCTAssertEqual(store.content.statusLines, ["Search failed: offline"])
        store.setQuery("f")
        try await answer(fixture, query: "f", with: [firstTask.id])
        XCTAssertEqual(store.content.statusLines, [])
    }

    @MainActor
    func testClickingARowDuringASearchStartsThatRow() async throws {
        let fixture = try await started(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        defer { fixture.cleanup() }
        let store = fixture.features.quickTrack
        try await open(fixture, results: [firstTask.id, secondTask.id])
        store.setQuery("sec")
        _ = try await fixture.client.next()
        store.activate(.task(firstTask.id))
        XCTAssertFalse(store.isPresented)
        let start = try await fixture.client.next()
        guard case .start(let task, _, _) = start.operation else {
            return XCTFail("Expected a start request, got \(start.operation)")
        }
        XCTAssertEqual(task, firstTask.id, "The click acts on its row, not on the next search's first match.")
    }
}
