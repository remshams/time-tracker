import XCTest
@testable import TrackerClient

final class FixtureCleanupTests: XCTestCase {
    @MainActor
    func testCleanupRejectsReloadsStartedByCancelledWrites() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.session.startTracking(taskID: firstTask.id)
        let write = try await fixture.client.next()
        XCTAssertEqual(write.operation, .start(task: firstTask.id, expected: nil, at: "2025-01-01T00:00:00.000Z"))
        let count = fixture.client.operations.count
        fixture.cleanup()
        try await fixture.settled()
        XCTAssertFalse(fixture.client.hasPendingRequests)
        XCTAssertEqual(fixture.client.operations.count, count)
        XCTAssertTrue(fixture.scheduler.active.isEmpty)
        write.succeed(TaskListResources(tasks: [firstTask], active: activeWorklog))
        XCTAssertEqual(fixture.client.operations.count, count)
        XCTAssertNil(fixture.session.active)
    }
}
