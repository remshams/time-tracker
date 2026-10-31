import XCTest
@testable import TrackerClient

@MainActor
final class TimeFormattingTests: XCTestCase {
    func testParseTimestampRejectsMissingOrInvalidInput() async {
        XCTAssertNil(parseTimestamp(nil))
        XCTAssertNil(parseTimestamp("Invalid timestamp"))
    }

    func testFormatClockDurationClampsNegativeValuesAndKeepsHoursBeyondOneDay() async {
        XCTAssertEqual(formatClockDuration(-1), "00:00:00")
        XCTAssertEqual(formatClockDuration(90_061), "25:01:01")
    }
}
