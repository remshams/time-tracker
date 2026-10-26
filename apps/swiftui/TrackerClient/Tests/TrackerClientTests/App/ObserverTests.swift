import Foundation
import XCTest
@testable import TrackerClient

final class ObserverTests: XCTestCase {
    @MainActor
    func testRefreshKeepsCommandsEnabledAndCancelsScheduledPoll() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources)
        let previousPoll = try XCTUnwrap(fixture.scheduler.poll)
        XCTAssertTrue(fixture.session.canStartSelectedTask)
        fixture.session.refresh()
        XCTAssertTrue(fixture.session.canStartSelectedTask)
        XCTAssertTrue(previousPoll.cancelled)
        XCTAssertNil(fixture.scheduler.poll)
        let request = try await fixture.client.next()
        request.succeed(resources)
        try await fixture.settled()
    }

    @MainActor
    func testVisibleStartupStartsDisplayTimerAndTickNotifiesWithoutFetchingData() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        fixture.session.setWindowVisible(true)
        try await fixture.start(TaskListResources(tasks: [firstTask], active: activeWorklog))
        let display = try XCTUnwrap(fixture.scheduler.display)
        let operationCount = fixture.client.operations.count
        var observedTimerText: String?
        fixture.session.onChange = { observedTimerText = fixture.session.timerDisplayText }

        fixture.clock.now.addTimeInterval(4)
        fixture.clock.uptime += 4
        display.fire()
        XCTAssertEqual(observedTimerText, "00:00:34")
        XCTAssertEqual(fixture.client.operations.count, operationCount)
    }

    @MainActor
    func testShowingWindowDuringRefreshNotifiesUpdatedTimeAndSchedulesDisplay() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(resources)
        fixture.session.refresh()
        let request = try await fixture.client.next()
        let operationCount = fixture.client.operations.count
        fixture.clock.now.addTimeInterval(4)
        fixture.clock.uptime += 4
        var observedTimerText: String?
        var observedBusy: Bool?
        var observedDisplayScheduled: Bool?
        fixture.session.onChange = {
            observedTimerText = fixture.session.timerDisplayText
            observedBusy = fixture.client.hasPendingRequests
            observedDisplayScheduled = fixture.scheduler.display != nil
        }

        fixture.session.setWindowVisible(true)
        XCTAssertEqual(observedTimerText, "00:00:34")
        XCTAssertEqual(observedBusy, true)
        XCTAssertEqual(observedDisplayScheduled, true)
        XCTAssertEqual(fixture.client.operations.count, operationCount)
        request.succeed(resources)
        try await fixture.settled()
    }

    @MainActor
    func testResourceObserverCanChooseTaskWhileInitialHistoryIsInFlight() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        var choseTask = false
        fixture.session.onChange = {
            guard !choseTask, fixture.session.tasks.contains(secondTask) else { return }
            choseTask = true
            fixture.session.select(secondTask.id)
        }
        fixture.session.start()
        let open = try await fixture.client.next()
        open.succeed(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        let history = try await fixture.client.next()
        XCTAssertTrue(choseTask)
        XCTAssertEqual(fixture.session.selectedTaskID, secondTask.id)
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        history.succeed(emptyPage)
        let selectedHistory = try await fixture.client.next()
        XCTAssertEqual(selectedHistory.operation, .history(task: secondTask.id, cursor: nil))
        selectedHistory.succeed(emptyPage)
        try await fixture.settled()
    }
}
