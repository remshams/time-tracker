import Foundation
import XCTest
@testable import TrackerClient

private func assertWidths(
    _ actual: [Double], _ expected: [Double], file: StaticString = #filePath, line: UInt = #line
) {
    XCTAssertEqual(actual.count, expected.count, file: file, line: line)
    for (width, wanted) in zip(actual, expected) {
        XCTAssertEqual(width, wanted, accuracy: 1e-9, file: file, line: line)
    }
}

final class DailyTotalsBarLayoutTests: XCTestCase {
    @MainActor
    func testWideSharesSplitTheBarAfterTheGaps() async {
        let layout = DailyTotalsBarLayout(fractions: [0.75, 0.25], width: 102, gap: 2, minimum: 4)
        XCTAssertEqual(layout.gap, 2)
        assertWidths(layout.widths, [75, 25])
    }

    @MainActor
    func testNarrowSharesGetTheMinimumFromTheWiderOnes() async {
        let layout = DailyTotalsBarLayout(fractions: [0.99, 0.01], width: 102, gap: 2, minimum: 4)
        assertWidths(layout.widths, [96, 4])
    }

    @MainActor
    func testRaisingOneShareCanPushAnotherBelowTheMinimum() async {
        // Once the 0.03 share takes 4 points, the 0.04 share gets 0.04 / 0.97 of the other 96 points, 3.96, below the
        // minimum.
        let layout = DailyTotalsBarLayout(fractions: [0.93, 0.04, 0.03], width: 104, gap: 2, minimum: 4)
        assertWidths(layout.widths, [92, 4, 4])
    }

    @MainActor
    func testCrowdedBarDropsTheGapsAndShrinksTheMinimumToFit() async {
        let fractions = Array(repeating: 1.0 / 60, count: 60)
        let layout = DailyTotalsBarLayout(fractions: fractions, width: 240, gap: 2, minimum: 4)
        XCTAssertEqual(layout.gap, 0)
        assertWidths(layout.widths, Array(repeating: 4, count: 60))
        let tighter = DailyTotalsBarLayout(fractions: fractions, width: 120, gap: 2, minimum: 4)
        assertWidths(tighter.widths, Array(repeating: 2, count: 60))
        XCTAssertEqual(tighter.widths.reduce(0, +), 120, accuracy: 1e-9, "Every segment stays inside the bar.")
    }

    @MainActor
    func testTheGapStaysWhileTheMinimumWidthsAndGapsJustFit() async {
        // Ten segments of 4 points and nine gaps of 2 points fill 58 points exactly.
        let fractions = Array(repeating: 0.1, count: 10)
        XCTAssertEqual(DailyTotalsBarLayout(fractions: fractions, width: 58, gap: 2, minimum: 4).gap, 2)
        XCTAssertEqual(DailyTotalsBarLayout(fractions: fractions, width: 57, gap: 2, minimum: 4).gap, 0)
    }

    @MainActor
    func testZeroWidthAndZeroFractionsGiveFiniteWidths() async {
        let empty = DailyTotalsBarLayout(fractions: [0.5, 0.5], width: 0, gap: 2, minimum: 4)
        XCTAssertEqual(empty.gap, 0)
        assertWidths(empty.widths, [0, 0])
        assertWidths(DailyTotalsBarLayout(fractions: [0, 0], width: 100, gap: 2, minimum: 4).widths, [4, 4])
    }

    @MainActor
    func testNoSharesHaveNoSegments() async {
        let layout = DailyTotalsBarLayout(fractions: [], width: 100, gap: 2, minimum: 4)
        assertWidths(layout.widths, [])
        XCTAssertEqual(layout.gap, 2)
    }
}
