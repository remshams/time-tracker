import XCTest
@testable import TrackerClient

@MainActor
final class TimeFormattingTests: XCTestCase {
    func testTimestampRejectsMissingOrInvalidInput() async {
        XCTAssertNil(timestamp(nil))
        XCTAssertNil(timestamp("Invalid timestamp"))
    }

    func testDurationClampsNegativeValuesAndKeepsHoursBeyondOneDay() async {
        XCTAssertEqual(clockDuration(-1), "00:00:00")
        XCTAssertEqual(clockDuration(90_061), "25:01:01")
    }
}
