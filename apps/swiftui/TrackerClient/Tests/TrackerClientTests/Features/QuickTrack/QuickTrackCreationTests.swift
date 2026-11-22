import XCTest
@testable import TrackerClient

final class QuickTrackCreationTests: XCTestCase {
    private let created = TaskItem(id: "task-new", name: "New task", archived: false, latestStart: nil)

    /// Opens the dialog with the first task in the catalog, types `query` and answers the search with no match.
    @MainActor
    private func typed(_ query: String) async throws -> Fixture {
        let fixture = Fixture()
        fixture.sync.setWindowVisible(true)
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        let store = fixture.features.quickTrack
        store.open()
        let first = try await fixture.client.next()
        first.found([firstTask.id])
        store.setQuery(query)
        let search = try await fixture.client.next()
        try await fixture.update("the search result") {
            search.found([])
        } until: {
            !store.isSearching
        }
        return fixture
    }

    /// Answers requests until the next one isn't a history page, and returns that one.
    @MainActor
    private func skippingHistory(_ fixture: Fixture) async throws -> FakeClient.Request {
        while true {
            let request = try await fixture.client.next()
            guard case .history = request.operation else { return request }
            request.succeed(emptyPage)
        }
    }

    @MainActor
    func testReturnOnTheCreateRowCreatesStartsAndCloses() async throws {
        let fixture = try await typed("  New task ")
        defer { fixture.cleanup() }
        let store = fixture.features.quickTrack
        XCTAssertEqual(store.content.highlight, .create)
        store.activate()
        XCTAssertTrue(store.isCreating)
        XCTAssertFalse(store.content.canCreate)
        let creation = try await fixture.client.next()
        XCTAssertEqual(creation.operation, .create(name: "New task", at: "2025-01-01T00:00:00.000Z"))
        creation.created(task: created)
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(.local))
        XCTAssertTrue(store.isPresented, "The dialog stays open until the task starts.")
        reload.succeed(TaskListResources(tasks: [firstTask, created], active: nil))
        let start = try await skippingHistory(fixture)
        guard case .start(let taskID, nil, _) = start.operation else {
            return XCTFail("Expected a start request, got \(start.operation)")
        }
        XCTAssertEqual(taskID, created.id)
        XCTAssertFalse(store.isPresented)
        XCTAssertFalse(store.isCreating)
    }

    @MainActor
    func testRejectedNameKeepsTheTextAndShowsTheError() async throws {
        let fixture = try await typed("New task")
        defer { fixture.cleanup() }
        let store = fixture.features.quickTrack
        store.activate()
        let creation = try await fixture.client.next()
        creation.fail(BridgeFailure(message: "Task names must be at most 200 characters."))
        let reload = try await fixture.client.next()
        XCTAssertEqual(reload.operation, .refresh(.local))
        try await fixture.update("the reload") {
            reload.succeed(TaskListResources(tasks: [firstTask], active: nil))
        } until: {
            !store.isCreating
        }
        XCTAssertTrue(store.isPresented)
        XCTAssertEqual(store.query, "New task")
        XCTAssertEqual(store.content.statusLines, ["Task names must be at most 200 characters."])
        XCTAssertEqual(store.content.highlight, .create)
        store.setQuery("New task 2")
        XCTAssertEqual(store.content.statusLines, [], "Typing clears the error.")
    }

    @MainActor
    func testCreatedTaskThatCannotStartShowsAnError() async throws {
        let fixture = try await typed("New task")
        defer { fixture.cleanup() }
        let store = fixture.features.quickTrack
        store.activate()
        let creation = try await fixture.client.next()
        creation.created(task: created)
        let reload = try await fixture.client.next()
        // The reloaded list doesn't contain the task yet, so it can't start.
        try await fixture.update("the reload") {
            reload.succeed(TaskListResources(tasks: [firstTask], active: nil))
        } until: {
            !store.isCreating
        }
        XCTAssertTrue(store.isPresented)
        XCTAssertEqual(store.content.statusLines, ["Created New task, but tracking couldn't start."])
        XCTAssertFalse(fixture.client.operations.contains { if case .start = $0 { true } else { false } })
    }

    @MainActor
    func testNullCharacterIsRejectedWithoutARequest() async throws {
        let fixture = try await typed("New\u{0}task")
        defer { fixture.cleanup() }
        let store = fixture.features.quickTrack
        var rejections = 0
        store.onReject = { rejections += 1 }
        store.activate()
        XCTAssertEqual(rejections, 1)
        XCTAssertEqual(store.content.statusLines, ["Task names must not contain control characters."])
        XCTAssertFalse(store.isCreating)
        XCTAssertFalse(fixture.client.operations.contains { if case .create = $0 { true } else { false } })
    }

    @MainActor
    func testReturnWhileCreatingIsRejected() async throws {
        let fixture = try await typed("New task")
        defer { fixture.cleanup() }
        let store = fixture.features.quickTrack
        var rejections = 0
        store.onReject = { rejections += 1 }
        store.activate()
        _ = try await fixture.client.next()
        store.activate()
        XCTAssertEqual(rejections, 1)
        XCTAssertEqual(fixture.client.operations.count { if case .create = $0 { true } else { false } }, 1)
    }
}
