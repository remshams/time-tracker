import XCTest
@testable import TrackerClient

@MainActor
final class TaskCopyFormatterTests: XCTestCase {
    func testExactDurationPreservesSecondsAndDropsLeadingZeroUnits() async {
        XCTAssertEqual(TaskCopyFormatter.exact(5_025), "1h 23m 45s")
        XCTAssertEqual(TaskCopyFormatter.exact(1_425), "23m 45s")
        XCTAssertEqual(TaskCopyFormatter.exact(45.99), "45s")
        XCTAssertEqual(TaskCopyFormatter.exact(3_600), "1h 0m 0s")
        XCTAssertEqual(TaskCopyFormatter.exact(0), "0s")
        XCTAssertEqual(TaskCopyFormatter.exact(-10), "0s")
    }

    func testRoundedDurationUsesNearestQuarterHourAndRoundsTiesUp() async {
        XCTAssertEqual(TaskCopyFormatter.rounded(449), "0m")
        XCTAssertEqual(TaskCopyFormatter.rounded(450), "15m")
        XCTAssertEqual(TaskCopyFormatter.rounded(1_349), "15m")
        XCTAssertEqual(TaskCopyFormatter.rounded(1_350), "30m")
        XCTAssertEqual(TaskCopyFormatter.rounded(5_025), "1h 30m")
        XCTAssertEqual(TaskCopyFormatter.rounded(3_600), "1h 0m")
        XCTAssertEqual(TaskCopyFormatter.rounded(-450), "0m")
    }

    func testUnavailableNonfiniteAndOversizedDurationsHaveNoCopyValue() async {
        for duration: TimeInterval? in [nil, .nan, .infinity, -.infinity, .greatestFiniteMagnitude, Double(Int64.max)] {
            XCTAssertNil(TaskCopyFormatter.exact(duration))
            XCTAssertNil(TaskCopyFormatter.rounded(duration))
        }
    }
}
