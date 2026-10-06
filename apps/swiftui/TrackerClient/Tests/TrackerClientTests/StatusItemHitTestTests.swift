import Foundation
#if canImport(CoreGraphics)
import CoreGraphics
#endif
import XCTest
@testable import TrackerClient

final class StatusItemHitTestTests: XCTestCase {
    func testDotCenterAndInteriorToggleWhileImagePaddingAndTotalOpenMenu() {
        let image = CGRect(x: 7, y: 4, width: 16, height: 16)
        XCTAssertTrue(TrackerStatusItemHitTest.containsDot(CGPoint(x: 15, y: 12), in: image))
        XCTAssertTrue(TrackerStatusItemHitTest.containsDot(CGPoint(x: 18, y: 15), in: image))
        XCTAssertFalse(TrackerStatusItemHitTest.containsDot(CGPoint(x: 8, y: 12), in: image))
        XCTAssertFalse(TrackerStatusItemHitTest.containsDot(CGPoint(x: 15, y: 5), in: image))
        XCTAssertFalse(TrackerStatusItemHitTest.containsDot(CGPoint(x: 32, y: 12), in: image))
    }

    func testCircularBoundaryIncludesCardinalEdgesButExcludesBoundingBoxCorners() {
        let image = CGRect(x: 0, y: 0, width: 16, height: 16)
        for point in [CGPoint(x: 3, y: 8), CGPoint(x: 13, y: 8),
                      CGPoint(x: 8, y: 3), CGPoint(x: 8, y: 13)] {
            XCTAssertTrue(TrackerStatusItemHitTest.containsDot(point, in: image))
        }
        XCTAssertFalse(TrackerStatusItemHitTest.containsDot(CGPoint(x: 13, y: 13), in: image))
        XCTAssertFalse(TrackerStatusItemHitTest.containsDot(CGPoint(x: 13.01, y: 8), in: image))
    }

    func testHitRegionFollowsImagePlacementAndScaling() {
        let image = CGRect(x: 107, y: -12, width: 32, height: 24)
        XCTAssertTrue(TrackerStatusItemHitTest.containsDot(CGPoint(x: 123, y: 0), in: image))
        XCTAssertTrue(TrackerStatusItemHitTest.containsDot(CGPoint(x: 132, y: 0), in: image))
        XCTAssertFalse(TrackerStatusItemHitTest.containsDot(CGPoint(x: 134, y: 0), in: image))
        XCTAssertFalse(TrackerStatusItemHitTest.containsDot(CGPoint(x: 123, y: 8), in: image))
        XCTAssertFalse(TrackerStatusItemHitTest.containsDot(CGPoint(x: 15, y: 12), in: image))
    }

    func testMissingOrInvalidImageCannotToggleTracking() {
        for frame in [CGRect.zero, CGRect(x: 0, y: 0, width: 0, height: 16),
                      CGRect(x: 0, y: 0, width: 16, height: 0)] {
            XCTAssertFalse(TrackerStatusItemHitTest.containsDot(CGPoint.zero, in: frame))
        }
    }
}
