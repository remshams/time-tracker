import Foundation
#if canImport(CoreGraphics)
import CoreGraphics
#endif
import XCTest
@testable import TrackerClient

final class MenuPanelPlacementTests: XCTestCase {
    private let content = CGSize(width: 360, height: 560)

    func testPanelAlignsToStatusItemBelowTheMenuBar() {
        let frame = TrackerMenuPanelPlacement.frame(
            contentSize: content,
            anchor: CGRect(x: 1100, y: 878, width: 24, height: 22),
            visibleFrame: CGRect(x: 0, y: 30, width: 1440, height: 848))
        XCTAssertEqual(frame, CGRect(x: 764, y: 312, width: 360, height: 560))
    }

    func testPanelFitsBothEdgesOfItsScreen() {
        let screen = CGRect(x: 0, y: 0, width: 1280, height: 778)
        let left = TrackerMenuPanelPlacement.frame(
            contentSize: content, anchor: CGRect(x: 4, y: 778, width: 24, height: 22),
            visibleFrame: screen)
        let right = TrackerMenuPanelPlacement.frame(
            contentSize: content, anchor: CGRect(x: 1270, y: 778, width: 24, height: 22),
            visibleFrame: screen)
        XCTAssertEqual(left.minX, 0)
        XCTAssertEqual(right.maxX, 1280)
        XCTAssertTrue(screen.contains(left))
        XCTAssertTrue(screen.contains(right))
    }

    func testPanelUsesSecondaryScreenCoordinatesAndAvoidsDockArea() {
        let screen = CGRect(x: -1920, y: -200, width: 1920, height: 1050)
        let frame = TrackerMenuPanelPlacement.frame(
            contentSize: content, anchor: CGRect(x: -400, y: 850, width: 24, height: 24),
            visibleFrame: screen)
        XCTAssertEqual(frame, CGRect(x: -736, y: 284, width: 360, height: 560))
        XCTAssertTrue(screen.contains(frame))
    }

    func testPanelShrinksToSmallVisibleArea() {
        let screen = CGRect(x: 80, y: 50, width: 300, height: 400)
        let frame = TrackerMenuPanelPlacement.frame(
            contentSize: content, anchor: CGRect(x: 320, y: 450, width: 24, height: 22),
            visibleFrame: screen)
        XCTAssertEqual(frame, CGRect(x: 80, y: 50, width: 300, height: 394))
        XCTAssertTrue(screen.contains(frame))
    }

    func testPanelStaysInsideVisibleFrameWhenMenuBarAndVisibleTopDiffer() {
        let screen = CGRect(x: 0, y: 0, width: 1440, height: 900)
        let frame = TrackerMenuPanelPlacement.frame(
            contentSize: content, anchor: CGRect(x: 1200, y: 930, width: 24, height: 22),
            visibleFrame: screen)
        XCTAssertEqual(frame.maxY, screen.maxY)
        XCTAssertTrue(screen.contains(frame))
    }
}
