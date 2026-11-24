import XCTest
@testable import TrackerClient

final class DurationLineTextTests: XCTestCase {
    @MainActor
    func testDurationUsesTheWindowFormatAndSplitsOffTheSecondsToDim() async {
        let duration = DurationLineText(seconds: 3867)
        XCTAssertEqual(duration.text, "1:04:27")
        XCTAssertEqual(duration.seconds, ":27")
        XCTAssertEqual(duration.spoken, "1 hour, 4 minutes")
    }

    @MainActor
    func testCachedDurationIsMarkedInTheTextAndTheSpokenForm() async {
        let duration = DurationLineText(seconds: 42, cached: true)
        XCTAssertEqual(duration.text, "~0:00:42")
        XCTAssertEqual(duration.seconds, ":42")
        XCTAssertEqual(duration.spoken, "about 42 seconds")
    }

    @MainActor
    func testPlaceholderHasNoSecondsAndReadsAsShown() async {
        let placeholder = DurationLineText(placeholder: "Unavailable")
        XCTAssertEqual(placeholder.text, "Unavailable")
        XCTAssertEqual(placeholder.seconds, "")
        XCTAssertEqual(placeholder.spoken, "Unavailable")
    }
}
