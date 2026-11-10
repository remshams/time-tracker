import XCTest
@testable import TrackerClient

final class TrackerSyncSelectionAndReloadTests: XCTestCase {
    @MainActor
    func testTaskWriteReloadRunsThenAfterTheTaskListArrives() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: activeWorklog))
        // The closure type a task dialog store declares.
        let reload: @MainActor (_ then: () -> Void) async -> Void = fixture.sync.reloadAfterTaskWrite(then:)
        var tasksInThen: [TaskItem] = []
        var timerInThen: String?

        let finished = Task { @MainActor in
            await reload {
                tasksInThen = fixture.features.taskCatalog.tasks
                timerInThen = fixture.features.tracking.timerText
            }
        }
        let refresh = try await fixture.client.next()
        XCTAssertEqual(refresh.operation, .refresh(fixture.features.connection.settings))
        fixture.advanceClock(by: 4)
        refresh.succeed(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
        try await fixture.taskValue(finished)

        XCTAssertEqual(tasksInThen, [firstTask, secondTask])
        XCTAssertEqual(timerInThen, "00:00:34", "The reloaded task list must show before `then` runs.")
        XCTAssertFalse(fixture.client.hasPendingRequests, "A task write must not reload an unchanged history.")
    }

    @MainActor
    func testWorklogWriteReloadRefreshesTheTaskListThenTheHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let running = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(running)
        // The closure type a worklog dialog store declares.
        let reload: @MainActor () async -> Void = fixture.sync.reloadAfterWorklogWrite

        let finished = Task { @MainActor in await reload() }
        let refresh = try await fixture.client.next()
        XCTAssertEqual(refresh.operation, .refresh(fixture.features.connection.settings))
        fixture.advanceClock(by: 4)
        refresh.succeed(running)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        try await fixture.taskValue(finished)
        XCTAssertEqual(fixture.features.tracking.timerText, "00:00:34", "The reloaded task list must show.")
        history.succeed(emptyPage)
        try await fixture.settled()
    }

    @MainActor
    func testShowingATaskInTheOtherTabChangesTabAndSelectionAndLoadsItsHistoryOnce() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: nil))
        fixture.features.taskCatalog.changeTab(.archived)
        let archivedHistory = try await fixture.client.next()
        archivedHistory.succeed(emptyPage)
        try await fixture.settled()

        fixture.features.taskCatalog.showTask(secondTask)

        XCTAssertEqual(fixture.features.taskCatalog.tab, .active)
        XCTAssertEqual(fixture.features.taskCatalog.selectedTaskID, secondTask.id)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: secondTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.client.operations.filter { $0 == .history(task: secondTask.id, cursor: nil) }.count, 1)
        XCTAssertEqual(fixture.client.operations.last, .history(task: secondTask.id, cursor: nil))
    }
}
