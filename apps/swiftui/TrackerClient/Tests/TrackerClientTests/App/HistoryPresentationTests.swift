import XCTest
@testable import TrackerClient

final class HistoryPresentationTests: XCTestCase {
    @MainActor
    func testEmptyHistoryCompletionPublishesContentAfterSelectingAnotherTask() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        let harness = PresentationHarness(session: fixture.session)
        let content = harness.counter(PresentationReads.content)

        fixture.session.select(secondTask.id)
        let request = try await fixture.client.next()
        let changesWhileWaiting = content.count
        XCTAssertTrue(harness.presentation.isHistoryLoading)
        request.succeed(emptyPage)
        try await harness.settled(fixture)
        XCTAssertFalse(fixture.client.hasPendingRequests)
        XCTAssertFalse(harness.presentation.isHistoryLoading)
        XCTAssertGreaterThan(
            content.count, changesWhileWaiting,
            "Completing an empty history read must replace the loading presentation.")
    }

}
