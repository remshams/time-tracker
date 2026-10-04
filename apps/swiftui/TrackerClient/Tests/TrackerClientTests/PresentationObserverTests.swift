import XCTest
@testable import TrackerClient

final class PresentationObserverTests: XCTestCase {
    @MainActor
    func testChangedServerContentAndPreferencesStillNotifyTheWindow() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask], active: activeWorklog))
        let observer = TrackerPresentationObserver(session: fixture.session)
        var names: [String?] = []
        observer.onContentChange = { names.append(fixture.session.selectedTask?.name) }
        fixture.session.onChange = { observer.update(from: fixture.session) }
        let renamed = TaskItem(id: firstTask.id, name: "Renamed task", archived: false, latestStart: nil)
        fixture.scheduler.poll?.fire()
        let refresh = try await fixture.client.next()
        refresh.succeed(TrackerSnapshot(tasks: [renamed], active: activeWorklog))
        let finished = Task { @MainActor in
            while fixture.session.isBusy && !Task.isCancelled { await Task.yield() }
        }
        try await fixture.taskValue(finished)
        XCTAssertEqual(names, ["Renamed task"])
        fixture.session.setPauseOnScreenLock(true)
        XCTAssertEqual(names, ["Renamed task", "Renamed task"])
        observer.update(from: fixture.session)
        XCTAssertEqual(names.count, 2)
    }

    @MainActor
    func testClockTicksUpdateOnlyElapsedTextWithoutInvalidatingLists() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        fixture.session.setWindowVisible(true)
        try await fixture.start(TrackerSnapshot(tasks: [firstTask], active: activeWorklog))
        let observer = TrackerPresentationObserver(session: fixture.session)
        var contentUpdates = 0
        var activityUpdates = 0
        var timerUpdates = 0
        observer.onContentChange = { contentUpdates += 1 }
        observer.onActivityChange = { activityUpdates += 1 }
        observer.onTimerChange = { timerUpdates += 1 }
        fixture.session.onChange = { observer.update(from: fixture.session) }
        let display = try XCTUnwrap(fixture.scheduler.display)
        let operationCount = fixture.client.operations.count

        for _ in 0..<3 {
            fixture.clock.now.addTimeInterval(1)
            fixture.clock.uptime += 1
            display.fire()
        }
        XCTAssertEqual(timerUpdates, 3)
        XCTAssertEqual(contentUpdates, 0)
        XCTAssertEqual(activityUpdates, 0)
        XCTAssertEqual(fixture.client.operations.count, operationCount)
        observer.update(from: fixture.session)
        XCTAssertEqual(timerUpdates, 3, "Repeating the same formatted second must not redraw the timer.")
    }

    @MainActor
    func testUnchangedServerPollsUpdateRequestControlsWithoutInvalidatingLists() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        let snapshot = TrackerSnapshot(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(snapshot)
        let observer = TrackerPresentationObserver(session: fixture.session)
        var contentUpdates = 0
        var activityUpdates = 0
        var timerUpdates = 0
        observer.onContentChange = { contentUpdates += 1 }
        observer.onActivityChange = { activityUpdates += 1 }
        observer.onTimerChange = { timerUpdates += 1 }
        fixture.session.onChange = { observer.update(from: fixture.session) }

        for _ in 0..<3 {
            fixture.scheduler.poll?.fire()
            let refresh = try await fixture.client.next()
            XCTAssertTrue(fixture.session.isBusy)
            XCTAssertFalse(fixture.session.canStopTracking)
            refresh.succeed(snapshot)
            let settled = XCTestExpectation(description: "The poll finishes")
            let completed = Task { @MainActor in
                while fixture.session.isBusy && !Task.isCancelled { await Task.yield() }
                if !Task.isCancelled { settled.fulfill() }
            }
            let result = await XCTWaiter.fulfillment(of: [settled], timeout: 2)
            XCTAssertEqual(result, .completed)
            completed.cancel()
            guard result == .completed else { return }
            XCTAssertTrue(fixture.session.canStopTracking)
        }
        XCTAssertEqual(contentUpdates, 0)
        XCTAssertEqual(timerUpdates, 0)
        XCTAssertEqual(activityUpdates, 6)
    }
}
