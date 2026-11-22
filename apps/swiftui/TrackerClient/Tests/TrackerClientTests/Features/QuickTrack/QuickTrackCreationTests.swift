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

    /// Answers history pages and the search that follows a create, and returns the next other request.
    @MainActor
    private func skippingHistory(_ fixture: Fixture) async throws -> FakeClient.Request {
        while true {
            let request = try await fixture.client.next()
            switch request.operation {
            case .history: request.succeed(emptyPage)
            case .search: request.found([created.id])
            default: return request
            }
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
        XCTAssertTrue(store.isPresented, "The dialog waits until the new task runs.")
        let running = WorklogItem(id: "worklog-new", taskId: created.id, start: activeWorklog.start, end: nil)
        try await fixture.update("the started task") {
            start.succeed(TaskListResources(tasks: [firstTask, created], active: running))
        } until: {
            !store.isCreating
        }
        XCTAssertFalse(store.isPresented)
    }

    @MainActor
    func testFailedStartAfterCreatingKeepsTheDialogOpenWithTheError() async throws {
        let fixture = try await typed("New task")
        defer { fixture.cleanup() }
        let store = fixture.features.quickTrack
        store.activate()
        (try await fixture.client.next()).created(task: created)
        let reload = try await fixture.client.next()
        reload.succeed(TaskListResources(tasks: [firstTask, created], active: nil))
        let start = try await skippingHistory(fixture)
        start.fail(BridgeFailure(message: "The tracker changed."))
        let reloadAfterStart = try await skippingHistory(fixture)
        try await fixture.update("the reload after the failed start") {
            reloadAfterStart.succeed(TaskListResources(tasks: [firstTask, created], active: nil))
        } until: {
            !store.isCreating
        }
        XCTAssertTrue(store.isPresented)
        XCTAssertEqual(store.content.statusLines.last, "Created New task, but tracking couldn't start.")
        XCTAssertEqual(store.content.highlight, .task(created.id), "The new task is listed, so Return can try again.")
    }

    @MainActor
    func testCreateErrorAfterReopeningIsDropped() async throws {
        let fixture = try await typed("New task")
        defer { fixture.cleanup() }
        let store = fixture.features.quickTrack
        store.activate()
        let creation = try await fixture.client.next()
        store.close()
        store.open()
        let search = try await fixture.client.next()
        XCTAssertEqual(search.operation, .search(query: ""))
        search.found([firstTask.id])
        creation.fail(BridgeFailure(message: "Rejected"))
        let reload = try await fixture.client.next()
        try await fixture.update("the reload") {
            reload.succeed(TaskListResources(tasks: [firstTask], active: nil))
        } until: {
            !store.isCreating
        }
        XCTAssertTrue(store.isPresented)
        XCTAssertEqual(store.content.statusLines, [], "The error belongs to the earlier opening.")
    }

    @MainActor
    func testReturnOnATaskWhileCreatingIsRejected() async throws {
        let fixture = try await typed("New task")
        defer { fixture.cleanup() }
        let store = fixture.features.quickTrack
        var rejections = 0
        store.onReject = { rejections += 1 }
        store.activate()
        _ = try await fixture.client.next()
        store.activate(.task(firstTask.id))
        XCTAssertEqual(rejections, 1)
        XCTAssertFalse(fixture.client.operations.contains { if case .start = $0 { true } else { false } })
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
        store.activate()
        XCTAssertTrue(store.isCreating)
        XCTAssertEqual(store.content.statusLines, [], "Trying again hides the earlier error while the request runs.")
        _ = try await fixture.client.next()
    }

    @MainActor
    func testTypingClearsACreateError() async throws {
        let fixture = try await typed("New task")
        defer { fixture.cleanup() }
        let store = fixture.features.quickTrack
        store.activate()
        let creation = try await fixture.client.next()
        creation.fail(BridgeFailure(message: "Rejected"))
        let reload = try await fixture.client.next()
        try await fixture.update("the reload") {
            reload.succeed(TaskListResources(tasks: [firstTask], active: nil))
        } until: {
            !store.isCreating
        }
        XCTAssertEqual(store.content.statusLines, ["Rejected"])
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
