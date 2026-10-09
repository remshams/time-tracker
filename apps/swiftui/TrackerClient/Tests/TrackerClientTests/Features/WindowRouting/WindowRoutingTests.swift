import Foundation
import XCTest
@testable import TrackerClient

final class WindowRoutingTests: XCTestCase {
    func testShowTrackerReusesPreferredWindowAndFallsBackAfterItCloses() {
        var routing = TrackerWindowRoutingState()
        let first = UUID(), second = UUID()
        routing.appeared(first, isActive: true)
        routing.appeared(second, isActive: true)
        routing.prefer(first)
        XCTAssertEqual(routing.showTracker(), TrackerWindowOpenRequest(id: first, shouldOpen: true))
        routing.disappeared(first)
        XCTAssertEqual(routing.showTracker(), TrackerWindowOpenRequest(id: second, shouldOpen: true))
        XCTAssertEqual(routing.activePresentationWindowID, second)
    }

    func testRepeatedOpenRequestsWaitForTheSameSceneIdentity() {
        var routing = TrackerWindowRoutingState()
        let id = UUID()
        XCTAssertEqual(routing.showTracker { id }, TrackerWindowOpenRequest(id: id, shouldOpen: true))
        XCTAssertEqual(
            routing.showTracker {
                XCTFail("Do not create another scene"); return UUID()
            },
            TrackerWindowOpenRequest(id: id, shouldOpen: false))
        routing.appeared(id, isActive: true)
        XCTAssertEqual(routing.showTracker(), TrackerWindowOpenRequest(id: id, shouldOpen: true))
    }

    func testBackgroundScenesWaitForActivationBeforePresentingErrors() {
        var routing = TrackerWindowRoutingState()
        let id = UUID()
        routing.appeared(id, isActive: false)
        XCTAssertNil(routing.activePresentationWindowID)
        routing.setActive(id, isActive: true)
        XCTAssertEqual(routing.activePresentationWindowID, id)
        routing.setActive(id, isActive: false)
        XCTAssertNil(routing.activePresentationWindowID)
        XCTAssertEqual(routing.showTracker(), TrackerWindowOpenRequest(id: id, shouldOpen: true))
    }

    func testClosingOneSceneDoesNotRemoveAnotherActiveScene() {
        var routing = TrackerWindowRoutingState()
        let first = UUID(), second = UUID()
        routing.appeared(first, isActive: true)
        routing.appeared(second, isActive: true)
        routing.disappeared(second)
        routing.setActive(second, isActive: true)
        routing.prefer(second)
        XCTAssertEqual(routing.activePresentationWindowID, first)
        XCTAssertEqual(routing.showTracker(), TrackerWindowOpenRequest(id: first, shouldOpen: true))
    }

    func testClosingAllScenesCreatesANewIdentityAndDuplicateAppearanceIsHarmless() {
        var routing = TrackerWindowRoutingState()
        let first = UUID(), second = UUID()
        routing.appeared(first, isActive: true)
        routing.appeared(first, isActive: true)
        routing.disappeared(first)
        XCTAssertNil(routing.activePresentationWindowID)
        XCTAssertEqual(routing.showTracker { second }, TrackerWindowOpenRequest(id: second, shouldOpen: true))
    }
}
