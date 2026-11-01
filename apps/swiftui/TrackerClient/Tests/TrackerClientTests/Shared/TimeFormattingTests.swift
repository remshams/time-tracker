import XCTest
@testable import TrackerClient

@MainActor
final class TimeFormattingTests: XCTestCase {
    func testParseTimestampRejectsMissingOrInvalidInput() async {
        XCTAssertNil(parseTimestamp(nil))
        XCTAssertNil(parseTimestamp("Invalid timestamp"))
    }

    func testFormatTimestampWritesUtcMillisecondsThatParseTimestampReadsBack() async throws {
        // .125 is exact in binary, so truncating and rounding formatters write the same digits.
        let date = Date(timeIntervalSince1970: 1_735_545_600.125)

        let timestamp = formatTimestamp(date)

        XCTAssertEqual(timestamp, "2024-12-30T08:00:00.125Z")
        let parsed = try XCTUnwrap(parseTimestamp(timestamp))
        XCTAssertEqual(parsed.timeIntervalSince1970, date.timeIntervalSince1970, accuracy: 0.001)
    }

    func testFormatClockDurationClampsNegativeValuesAndKeepsHoursBeyondOneDay() async {
        XCTAssertEqual(formatClockDuration(-1), "00:00:00")
        XCTAssertEqual(formatClockDuration(90_061), "25:01:01")
    }
}
