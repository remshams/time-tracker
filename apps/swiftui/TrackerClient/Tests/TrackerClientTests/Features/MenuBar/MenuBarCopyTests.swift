import XCTest
@testable import TrackerClient

final class MenuBarCopyTests: XCTestCase {
    @MainActor
    func testCopyRejectsDataSourceMismatch() async throws {
        let fixture = Fixture(reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(
            TaskListResources(tasks: [firstTask], active: nil),
            rows: [TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 60_000_000)])
        for action in TaskCopyAction.allCases {
            XCTAssertNotNil(fixture.features.menuBar.menuCopyValue(action, taskID: firstTask.id, connection: .local))
            XCTAssertNil(
                fixture.features.menuBar.menuCopyValue(action, taskID: firstTask.id, connection: serverSettings))
        }
    }
}
