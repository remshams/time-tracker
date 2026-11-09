import XCTest
@testable import TrackerClient

final class HistoryPresentationTests: XCTestCase {
    @MainActor
    func testEmptyHistoryCompletionPublishesContentAfterSelectingAnotherTask() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        let content = ObservationProbe { ViewReads.content(fixture.features) }

        fixture.features.taskCatalog.select(secondTask.id)
        let request = try await fixture.client.next()
        XCTAssertTrue(content.consume())
        XCTAssertTrue(fixture.features.worklogHistory.isLoading)
        request.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertFalse(fixture.client.hasPendingRequests)
        XCTAssertFalse(fixture.features.worklogHistory.isLoading)
        XCTAssertTrue(content.consume(), "Completing an empty history read must replace the loading presentation.")
    }

}
