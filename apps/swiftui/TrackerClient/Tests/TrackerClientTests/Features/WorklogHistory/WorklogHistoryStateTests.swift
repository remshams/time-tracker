import XCTest
@testable import TrackerClient

final class WorklogHistoryStateTests: XCTestCase {
    @MainActor
    func testReloadForTheSameTaskKeepsShownRowsUntilTheNextPageArrives() async {
        let history = WorklogHistoryState()
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
        let history = WorklogHistoryState()
        history.request(selectedTaskID: firstTask.id)
        history.accept(HistoryPage(worklogs: [oldWorklog], nextCursor: "old-cursor", reset: false), cursor: nil)

        history.request(selectedTaskID: secondTask.id)

        XCTAssertEqual(history.worklogs, [])
        XCTAssertNil(history.nextCursor)
        XCTAssertTrue(history.isLoading)
    }
}
