import Foundation
import XCTest
@testable import TrackerClient

@MainActor
final class WindowRoutingTests: XCTestCase {
    func testShowTrackerReusesPreferredWindowAndFallsBackAfterItCloses() async {
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

    func testRepeatedOpenRequestsWaitForTheSameSceneIdentity() async {
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

    func testUnknownWindowCannotReplaceThePreferredWindow() async {
        var routing = TrackerWindowRoutingState()
        let first = UUID(), second = UUID()
        routing.appeared(first, isActive: true)
        routing.appeared(second, isActive: true)
        routing.prefer(first)

        routing.prefer(UUID())

        XCTAssertEqual(routing.showTracker(), TrackerWindowOpenRequest(id: first, shouldOpen: true))
        XCTAssertEqual(routing.activePresentationWindowID, first)
    }

    func testUnrelatedSceneAppearanceDoesNotClearAPendingOpenRequest() async {
        var routing = TrackerWindowRoutingState()
        let pending = UUID(), unrelated = UUID()
        _ = routing.showTracker { pending }
        routing.appeared(unrelated, isActive: false)
        routing.disappeared(unrelated)

        XCTAssertEqual(routing.showTracker(), TrackerWindowOpenRequest(id: pending, shouldOpen: false))
    }

    func testClosingAPreviouslyPendingSceneAllowsANewOpenRequest() async {
        var routing = TrackerWindowRoutingState()
        let first = UUID(), second = UUID()
        _ = routing.showTracker { first }
        routing.appeared(first, isActive: true)
        routing.disappeared(first)

        XCTAssertEqual(routing.showTracker { second }, TrackerWindowOpenRequest(id: second, shouldOpen: true))
    }

    func testReopeningAClosedSceneWaitsForActivationBeforePresenting() async {
        var routing = TrackerWindowRoutingState()
        let id = UUID()
        routing.appeared(id, isActive: true)
        routing.disappeared(id)

        XCTAssertEqual(routing.showTracker { id }, TrackerWindowOpenRequest(id: id, shouldOpen: true))
        XCTAssertNil(routing.activePresentationWindowID)
    }

    func testClosingAnUnrelatedWindowPreservesThePreferredWindow() async {
        var routing = TrackerWindowRoutingState()
        let first = UUID(), second = UUID(), third = UUID()
        routing.appeared(first, isActive: true)
        routing.appeared(second, isActive: true)
        routing.appeared(third, isActive: true)
        routing.prefer(second)

        routing.disappeared(first)
        XCTAssertEqual(routing.showTracker(), TrackerWindowOpenRequest(id: second, shouldOpen: true))
        XCTAssertEqual(routing.activePresentationWindowID, second)
    }

    func testBackgroundScenesWaitForActivationBeforePresentingErrors() async {
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

    func testClosingOneSceneDoesNotRemoveAnotherActiveScene() async {
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

    func testClosingAllScenesCreatesANewIdentityAndDuplicateAppearanceIsHarmless() async {
        var routing = TrackerWindowRoutingState()
        let first = UUID(), second = UUID()
        routing.appeared(first, isActive: true)
        routing.appeared(first, isActive: true)
        routing.disappeared(first)
        XCTAssertNil(routing.activePresentationWindowID)
        XCTAssertEqual(routing.showTracker { second }, TrackerWindowOpenRequest(id: second, shouldOpen: true))
    }
}
