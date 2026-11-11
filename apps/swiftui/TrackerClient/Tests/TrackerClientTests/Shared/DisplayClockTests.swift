import XCTest
@testable import TrackerClient

final class DisplayClockTests: XCTestCase {
    @MainActor
    func testSecondTimerRunsOnlyWhileAwakeVisibleAndAWorklogRuns() async {
        let scheduler = FakeScheduler()
        let clock = DisplayClock(scheduler: scheduler)
        clock.setVisible(true)
        clock.setWorklogRunning(true)
        XCTAssertNil(scheduler.display, "The clock waits for the sync to start.")
        clock.setAwake(true)
        let timer = scheduler.display
        XCTAssertEqual(timer?.tolerance, 0.2)
        clock.setAwake(true)
        XCTAssertEqual(scheduler.active.count, 1, "An unchanged input must not schedule a second timer.")
        clock.setWorklogRunning(false)
        XCTAssertTrue(timer?.cancelled == true)
        XCTAssertNil(scheduler.display)
        clock.setWorklogRunning(true)
        XCTAssertNotNil(scheduler.display)
        clock.setVisible(false)
        XCTAssertNil(scheduler.display)
        clock.setVisible(true)
        clock.setAwake(false)
        XCTAssertNil(scheduler.display)
        XCTAssertTrue(scheduler.active.isEmpty)
    }

    @MainActor
    func testMinuteTimerRunsOnlyWhileAwakeShownAndAWorklogRuns() async {
        let scheduler = FakeScheduler()
        let clock = DisplayClock(scheduler: scheduler)
        clock.setAwake(true)
        clock.setWorklogRunning(true)
        XCTAssertNil(scheduler.menuBarClock)
        clock.setMenuBarTotalShown(true)
        let timer = scheduler.menuBarClock
        XCTAssertEqual(timer?.tolerance, 5)
        XCTAssertNil(scheduler.display, "The menu bar total alone needs no second timer.")
        clock.setMenuBarTotalShown(true)
        XCTAssertEqual(scheduler.active.count, 1)
        clock.setAwake(false)
        XCTAssertTrue(timer?.cancelled == true)
        clock.setAwake(true)
        XCTAssertNotNil(scheduler.menuBarClock)
        clock.setWorklogRunning(false)
        XCTAssertNil(scheduler.menuBarClock)
        clock.setWorklogRunning(true)
        clock.setMenuBarTotalShown(false)
        XCTAssertTrue(scheduler.active.isEmpty)
    }

    @MainActor
    func testTicksAndVisibilityReportsRunEveryRefreshInOrder() async {
        let scheduler = FakeScheduler()
        let clock = DisplayClock(scheduler: scheduler)
        let calls = RefreshCalls()
        clock.onTick { calls.names.append("first") }
        clock.onTick { calls.names.append("second") }
        clock.setAwake(true)
        clock.setWorklogRunning(true)
        clock.setMenuBarTotalShown(true)
        XCTAssertEqual(calls.names, [], "Only a tick or a visibility report refreshes.")

        clock.setVisible(true)
        XCTAssertEqual(calls.names, ["first", "second"])
        clock.setVisible(true)
        XCTAssertEqual(calls.names.count, 4, "A menu that opens over the visible window refreshes too.")
        XCTAssertEqual(scheduler.active.count, 2, "Another report must not schedule a second timer.")
        scheduler.display?.fire()
        scheduler.menuBarClock?.fire()
        XCTAssertEqual(calls.names, Array(repeating: ["first", "second"], count: 4).flatMap { $0 })
        clock.setVisible(false)
        XCTAssertEqual(calls.names.count, 10, "Hiding the window refreshes too.")
    }
}

@MainActor
private final class RefreshCalls {
    var names: [String] = []
}
