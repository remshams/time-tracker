import Foundation
import XCTest
@testable import TrackerClient

final class ModelsTests: XCTestCase {
    func testSnapshotDecodesRustCamelCaseFieldsAndFractionalTimestamps() throws {
        let json = #"{"tasks":[{"id":"task-one","name":"First task","archived":false,"latestStart":"2024-12-31T23:59:30.123456Z"}],"active":{"id":"worklog-one","taskId":"task-one","start":"2024-12-31T23:59:30.123456Z","end":null}}"#
        let snapshot = try JSONDecoder().decode(TrackerSnapshot.self, from: Data(json.utf8))
        XCTAssertEqual(snapshot.tasks.map(\.id), ["task-one"])
        XCTAssertEqual(snapshot.tasks.first?.latestStart, snapshot.active?.start)
        XCTAssertEqual(snapshot.active?.taskId, "task-one")
        XCTAssertNil(snapshot.active?.end)
        let instant = try XCTUnwrap(timestamp(snapshot.active?.start))
        XCTAssertEqual(instant.timeIntervalSince1970, 1_735_689_570.123456, accuracy: 0.001)
    }

    func testHistoryDecodesOpaqueCursorAndResetFlag() throws {
        let json = #"{"worklogs":[{"id":"worklog-one","taskId":"task-one","start":"2024-12-30T09:00:00.000000Z","end":"2024-12-30T10:00:00.000000Z"}],"nextCursor":"{\"taskId\":\"task-one\"}","reset":true}"#
        let page = try JSONDecoder().decode(HistoryPage.self, from: Data(json.utf8))
        XCTAssertEqual(page.worklogs.first?.end, "2024-12-30T10:00:00.000000Z")
        XCTAssertEqual(page.nextCursor, #"{"taskId":"task-one"}"#)
        XCTAssertTrue(page.reset)
    }

    func testMalformedSnapshotCannotBecomeConfirmedIdleState() {
        for json in [#"{"active":null}"#, #"{"tasks":[],"active":{"id":"worklog-one"}}"#] {
            XCTAssertThrowsError(try JSONDecoder().decode(TrackerSnapshot.self, from: Data(json.utf8)))
        }
    }

    func testTimestampRejectsMissingOrInvalidInput() {
        XCTAssertNil(timestamp(nil))
        XCTAssertNil(timestamp("Invalid timestamp"))
    }

    func testDurationClampsNegativeValuesAndKeepsHoursBeyondOneDay() {
        XCTAssertEqual(clockDuration(-1), "00:00:00")
        XCTAssertEqual(clockDuration(90_061), "25:01:01")
    }
}
