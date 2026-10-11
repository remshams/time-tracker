import XCTest
@testable import TrackerClient

final class HistoryPresentationTests: XCTestCase {
    @MainActor
    func testEmptyHistoryCompletionPublishesContentAfterSelectingAnotherTask() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        let observer = TrackerPresentationObserver(session: fixture.session)
        var contentChanges = 0
        observer.onContentChange = { contentChanges += 1 }
        fixture.session.onChange = { observer.update(from: fixture.session) }

        fixture.session.select(secondTask.id)
        let request = try await fixture.client.next()
        let changesWhileWaiting = contentChanges
        let completion = XCTestExpectation(description: "Empty history completion publishes content")
        observer.onContentChange = {
            contentChanges += 1
            completion.fulfill()
        }
        request.succeed(emptyPage)
        let result = await XCTWaiter.fulfillment(of: [completion], timeout: 2)
        XCTAssertEqual(result, .completed)
        XCTAssertFalse(fixture.session.isBusy)
        XCTAssertGreaterThan(
            contentChanges, changesWhileWaiting,
            "Completing an empty history read must replace the loading presentation.")
    }

}
