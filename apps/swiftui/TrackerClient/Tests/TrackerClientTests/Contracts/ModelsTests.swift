import Foundation
import XCTest
@testable import TrackerClient

final class ModelsTests: XCTestCase {
    func testConnectionModeLabelsNameLocalStorageAndServer() {
        XCTAssertEqual(ConnectionMode.local.label, "Local database")
        XCTAssertEqual(ConnectionMode.server.label, "Server")
    }

    func testResourceObservationsDecodeRustFieldsAndFractionalTimestamps() throws {
        let tasksJSON =
            #"{"value":[{"id":"task-one","name":"First task","archived":false,"latestStart":"2024-12-31T23:59:30.123456Z"}]}"#
        let trackingJSON =
            #"{"value":{"id":"worklog-one","taskId":"task-one","start":"2024-12-31T23:59:30.123456Z","end":null}}"#
        let tasks = try JSONDecoder().decode(TaskCatalogObservation.self, from: Data(tasksJSON.utf8))
        let tracking = try JSONDecoder().decode(TrackingObservation.self, from: Data(trackingJSON.utf8))
        XCTAssertEqual(tasks.value.map(\.id), ["task-one"])
        XCTAssertEqual(tasks.value.first?.latestStart, tracking.value?.start)
        XCTAssertEqual(tracking.value?.taskId, "task-one")
        XCTAssertNil(tracking.value?.end)
        let instant = try XCTUnwrap(timestamp(tracking.value?.start))
        XCTAssertEqual(instant.timeIntervalSince1970, 1_735_689_570.123456, accuracy: 0.001)
    }

    func testUnloadedTrackingDiffersFromConfirmedIdle() throws {
        let unloaded = try JSONDecoder().decode(TrackingObservation?.self, from: Data("null".utf8))
        let idle = try JSONDecoder().decode(
            TrackingObservation?.self,
            from: Data(#"{"value":null}"#.utf8))
        XCTAssertNil(unloaded)
        XCTAssertNotNil(idle)
        XCTAssertNil(idle?.value)
    }

    func testFocusedCommandAndReportPayloadsDecodeWithoutTaskLists() throws {
        let taskJSON =
            #"{"task":{"id":"created","name":"Created task","archived":false,"latestStart":null},"receipt":{"requestId":"request","replayed":true}}"#
        let task = try JSONDecoder().decode(TaskCommandResult.self, from: Data(taskJSON.utf8))
        XCTAssertEqual(task.task.id, "created")
        XCTAssertEqual(task.receipt, CommandReceipt(requestId: "request", replayed: true))
        let trackingJSON = #"{"active":null,"stopped":null,"didStop":false,"receipt":null}"#
        let tracking = try JSONDecoder().decode(TrackingCommandResult.self, from: Data(trackingJSON.utf8))
        XCTAssertEqual(tracking, TrackingCommandResult(active: nil))
        let reportJSON =
            #"{"rows":[{"taskId":"metadata-not-loaded","durationMicroseconds":12}],"now":"2025-01-01T00:00:00Z"}"#
        let report = try JSONDecoder().decode(TrackerReport.self, from: Data(reportJSON.utf8))
        let resources = DailyTotalsResources(report: report, tracking: TrackingObservation(value: nil))
        XCTAssertEqual(resources.report.rows.first?.taskId, "metadata-not-loaded")
        XCTAssertEqual(resources.report.now, "2025-01-01T00:00:00Z")
    }

    func testHistoryDecodesOpaqueCursorAndResetFlag() throws {
        let json =
            #"{"worklogs":[{"id":"worklog-one","taskId":"task-one","start":"2024-12-30T09:00:00.000000Z","end":"2024-12-30T10:00:00.000000Z"}],"nextCursor":"{\"taskId\":\"task-one\"}","reset":true}"#
        let page = try JSONDecoder().decode(HistoryPage.self, from: Data(json.utf8))
        XCTAssertEqual(page.worklogs.first?.end, "2024-12-30T10:00:00.000000Z")
        XCTAssertEqual(page.nextCursor, #"{"taskId":"task-one"}"#)
        XCTAssertTrue(page.reset)
    }

    func testMalformedResourcesCannotBecomeConfirmedIdleState() {
        for json in [#"{}"#, #"{"value":{"id":"worklog-one"}}"#] {
            XCTAssertThrowsError(try JSONDecoder().decode(TrackingObservation.self, from: Data(json.utf8)))
        }
        XCTAssertThrowsError(try JSONDecoder().decode(TaskCatalogObservation.self, from: Data(#"{}"#.utf8)))
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
