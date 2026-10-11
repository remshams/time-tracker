import XCTest
@testable import TrackerClient

final class WorklogHistoryStateTests: XCTestCase {
    @MainActor
    func testDestinationCursorInvalidationPreservesAnAlreadyObservedMovedRow() async {
        let history = WorklogHistoryState()
        history.request(selectedTaskID: secondTask.id)
        let moved = WorklogItem(id: oldWorklog.id, taskId: secondTask.id, start: oldWorklog.start, end: oldWorklog.end)
        history.accept(HistoryPage(worklogs: [moved], nextCursor: "old-cursor", reset: false), cursor: nil)
        history.invalidateCursor()
        XCTAssertEqual(history.worklogs, [moved])
        XCTAssertNil(history.nextCursor)
    }

}
