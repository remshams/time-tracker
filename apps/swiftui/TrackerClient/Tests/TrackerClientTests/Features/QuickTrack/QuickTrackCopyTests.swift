import XCTest
@testable import TrackerClient

final class QuickTrackCopyTests: XCTestCase {
    /// Opens the dialog over a running first task. Only the second task has time today.
    @MainActor
    private func opened() async throws -> Fixture {
        let fixture = Fixture(reports: true)
        fixture.sync.setWindowVisible(true)
        try await fixture.start(
            TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog),
            rows: [TaskReportTotal(taskId: secondTask.id, durationMicroseconds: 5_025_000_000)])
        let store = fixture.features.quickTrack
        store.open()
        let search = try await fixture.client.next()
        try await fixture.update("the search result") {
            search.found([secondTask.id])
        } until: {
            !store.isSearching
        }
        return fixture
    }

    @MainActor
    func testCopiesTheHighlightedTask() async throws {
        let fixture = try await opened()
        defer { fixture.cleanup() }
        let store = fixture.features.quickTrack
        XCTAssertNil(store.copyValue(.copyName), "Nothing is highlighted yet.")
        store.moveHighlight(down: true)
        XCTAssertEqual(store.copyValue(.copyName), secondTask.name)
        XCTAssertEqual(store.copyValue(.copyExact), "1h 23m 45s")
        XCTAssertEqual(store.copyValue(.copyRounded), "1h 30m")
        XCTAssertTrue(store.isPresented, "Copying keeps the dialog open.")
    }

    @MainActor
    func testTheHeaderCopiesTheRunningTask() async throws {
        let fixture = try await opened()
        defer { fixture.cleanup() }
        let store = fixture.features.quickTrack
        store.moveHighlight(down: false)
        XCTAssertEqual(store.copyValue(.copyName), firstTask.name)
    }

    @MainActor
    func testCopiesAClickedRowAndNothingWhileClosed() async throws {
        let fixture = try await opened()
        defer { fixture.cleanup() }
        let store = fixture.features.quickTrack
        XCTAssertEqual(store.copyValue(.copyName, taskID: secondTask.id), secondTask.name)
        store.close()
        XCTAssertNil(store.copyValue(.copyName, taskID: secondTask.id))
    }
}
