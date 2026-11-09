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
    func testChangingTheTabOrTaskLoadsTheHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: activeWorklog))

        fixture.sync.selectTask(secondTask.id)
        XCTAssertEqual(fixture.features.taskCatalog.selectedTaskID, secondTask.id)
        let selected = try await fixture.client.next()
        XCTAssertEqual(selected.operation, .history(task: secondTask.id, cursor: nil))
        selected.succeed(emptyPage)
        try await fixture.settled()

        fixture.sync.changeTab(.archived)
        XCTAssertEqual(fixture.features.taskCatalog.tab, .archived)
        let changed = try await fixture.client.next()
        XCTAssertEqual(changed.operation, .history(task: archivedTask.id, cursor: nil))
        changed.succeed(emptyPage)
        try await fixture.settled()
    }

    @MainActor
    func testNavigationUpdatesTheListsAndTheHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: nil))
        let lists = ObservationProbe { ViewReads.lists(fixture.features) }
        let catalog = fixture.features.taskCatalog
        let worklogHistory = fixture.features.worklogHistory

        fixture.sync.selectTask(secondTask.id)
        let history = try await fixture.client.next()
        XCTAssertTrue(worklogHistory.isLoading)
        XCTAssertTrue(lists.consume())
        history.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "older", reset: false))
        try await fixture.settled()
        XCTAssertEqual(catalog.selectedTask, secondTask)
        XCTAssertEqual(worklogHistory.worklogs, [oldWorklog])
        XCTAssertTrue(worklogHistory.hasMore)
        XCTAssertTrue(lists.consume())

        fixture.sync.changeTab(.archived)
        XCTAssertEqual(catalog.tab, .archived)
        XCTAssertEqual(catalog.visibleTasks, [archivedTask])
        XCTAssertEqual(catalog.selectedTask, archivedTask)
        XCTAssertTrue(lists.consume())
        try await fixture.client.next().succeed(emptyPage)
        try await fixture.settled()
        fixture.sync.changeTab(.active)
        XCTAssertEqual(catalog.selectedTask, secondTask)
        XCTAssertTrue(lists.consume())
        try await fixture.client.next().succeed(emptyPage)
        try await fixture.settled()
    }

    @MainActor
    func testShowingATaskInTheOtherTabChangesTabAndSelectionAndLoadsItsHistoryOnce() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: nil))
        fixture.sync.changeTab(.archived)
        let archivedHistory = try await fixture.client.next()
        archivedHistory.succeed(emptyPage)
        try await fixture.settled()

        fixture.sync.showTask(secondTask)

        XCTAssertEqual(fixture.features.taskCatalog.tab, .active)
        XCTAssertEqual(fixture.features.taskCatalog.selectedTaskID, secondTask.id)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: secondTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.client.operations.filter { $0 == .history(task: secondTask.id, cursor: nil) }.count, 1)
        XCTAssertEqual(fixture.client.operations.last, .history(task: secondTask.id, cursor: nil))
    }

    @MainActor
    func testKeepingTheTabOrTaskSendsNothing() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))

        fixture.sync.selectTask(firstTask.id)
        fixture.sync.changeTab(.active)

        XCTAssertFalse(fixture.client.hasPendingRequests)
    }
}
