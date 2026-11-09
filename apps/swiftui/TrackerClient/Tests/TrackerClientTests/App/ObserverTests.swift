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
        XCTAssertTrue(fixture.canStartSelectedTask)
        fixture.sync.refresh()
        XCTAssertTrue(fixture.canStartSelectedTask)
        XCTAssertTrue(previousPoll.cancelled)
        XCTAssertNil(fixture.scheduler.poll)
        let request = try await fixture.client.next()
        request.succeed(resources)
        try await fixture.settled()
    }

    @MainActor
    func testVisibleStartupStartsDisplayTimerAndTickUpdatesTheTimerWithoutFetchingData() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        fixture.sync.setWindowVisible(true)
        try await fixture.start(TaskListResources(tasks: [firstTask], active: activeWorklog))
        let display = try XCTUnwrap(fixture.scheduler.display)
        let operationCount = fixture.client.operations.count

        fixture.advanceClock(by: 4)
        display.fire()
        XCTAssertEqual(fixture.session.tracking.timerText, "00:00:34")
        XCTAssertEqual(fixture.client.operations.count, operationCount)
    }

    @MainActor
    func testShowingWindowDuringRefreshUpdatesTheTimeAndSchedulesDisplay() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: activeWorklog)
        try await fixture.start(resources)
        fixture.sync.refresh()
        let request = try await fixture.client.next()
        let operationCount = fixture.client.operations.count
        fixture.advanceClock(by: 4)

        fixture.sync.setWindowVisible(true)
        XCTAssertEqual(fixture.session.tracking.timerText, "00:00:34")
        XCTAssertNotNil(fixture.scheduler.display)
        XCTAssertEqual(fixture.client.operations.count, operationCount)
        request.succeed(resources)
        try await fixture.settled()
    }

    @MainActor
    func testChoosingATaskWhileInitialHistoryIsInFlightLoadsItsHistoryAndDropsTheOlderPage() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        fixture.sync.start()
        let open = try await fixture.client.next()
        open.succeed(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        fixture.sync.selectTask(secondTask.id)
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, secondTask.id)
        let selectedHistory = try await fixture.client.next()
        XCTAssertEqual(selectedHistory.operation, .history(task: secondTask.id, cursor: nil))
        history.succeed(HistoryPage(worklogs: [activeWorklog], nextCursor: nil, reset: false))
        selectedHistory.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [oldWorklog])
    }
}
