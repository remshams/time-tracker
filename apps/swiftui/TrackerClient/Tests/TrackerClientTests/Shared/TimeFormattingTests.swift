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

    func testDurationTextFromAnIntervalDropsFractionsAndClampsNegativeValues() async {
        XCTAssertEqual(DurationText(duration: 3867.9), DurationText(seconds: 3867))
        XCTAssertEqual(DurationText(duration: -1), DurationText(seconds: 0))
    }

    func testDurationTextDropsTheHourZeroAndSplitsOffTheSeconds() async {
        XCTAssertEqual(DurationText(seconds: 0).hoursAndMinutes, "0:00")
        XCTAssertEqual(DurationText(seconds: 0).seconds, ":00")
        XCTAssertEqual(joined(DurationText(seconds: 3867)), "1:04:27")
        XCTAssertEqual(joined(DurationText(seconds: 90_061)), "25:01:01")
        XCTAssertEqual(joined(DurationText(seconds: -5)), "0:00:00")
    }

    func testSpokenDurationReadsHoursAndMinutesAndSecondsOnlyUnderAMinute() async {
        XCTAssertEqual(spokenDuration(-1), "0 seconds")
        XCTAssertEqual(spokenDuration(1), "1 second")
        XCTAssertEqual(spokenDuration(59), "59 seconds")
        XCTAssertEqual(spokenDuration(60), "1 minute")
        XCTAssertEqual(spokenDuration(61), "1 minute")
        XCTAssertEqual(spokenDuration(150), "2 minutes")
        XCTAssertEqual(spokenDuration(3600), "1 hour")
        XCTAssertEqual(spokenDuration(3867), "1 hour, 4 minutes")
        XCTAssertEqual(spokenDuration(7260), "2 hours, 1 minute")
    }

    private func joined(_ duration: DurationText) -> String { duration.hoursAndMinutes + duration.seconds }
}
