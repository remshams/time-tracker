import XCTest
@testable import TrackerClient

final class TrackerSyncSelectionAndReloadTests: XCTestCase {
    private let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)

    @MainActor
    func testTaskWriteReloadRunsThenAfterTheTaskListArrivesAndPublishesAroundIt() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        // The closure type a task dialog store declares.
        let reload: @MainActor (_ then: () -> Void) async -> Void = fixture.sync.reloadAfterTaskWrite(then:)
        var events: [String] = []
        var tasksInThen: [TaskItem] = []
        fixture.sync.onChange = { events.append("publish") }

        let finished = Task { @MainActor in
            await reload {
                events.append("then")
                tasksInThen = fixture.session.tasks
            }
        }
        let refresh = try await fixture.client.next()
        XCTAssertEqual(refresh.operation, .refresh(fixture.session.connection.settings))
        XCTAssertEqual(events, ["publish"], "The write result must show before the task list reloads.")
        refresh.succeed(resources)
        try await fixture.taskValue(finished)

        XCTAssertEqual(tasksInThen, [firstTask, secondTask])
        XCTAssertEqual(events, ["publish", "publish", "then", "publish"])
        XCTAssertFalse(fixture.client.hasPendingRequests, "A task write must not reload an unchanged history.")
    }

    @MainActor
    func testWorklogWriteReloadRefreshesTheTaskListThenTheHistory() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(resources)
        // The closure type a worklog dialog store declares.
        let reload: @MainActor () async -> Void = fixture.sync.reloadAfterWorklogWrite
        var publishes = 0
        fixture.sync.onChange = { publishes += 1 }

        let finished = Task { @MainActor in await reload() }
        let refresh = try await fixture.client.next()
        XCTAssertEqual(refresh.operation, .refresh(fixture.session.connection.settings))
        XCTAssertEqual(publishes, 1, "The write result must show before the task list reloads.")
        refresh.succeed(resources)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        try await fixture.taskValue(finished)
        XCTAssertEqual(publishes, 3)
        history.succeed(emptyPage)
        try await fixture.settled()
    }

    @MainActor
    func testChangingTheTabOrTaskLoadsTheHistoryAndPublishes() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: nil))
        var publishes = 0
        fixture.sync.onChange = { publishes += 1 }

        fixture.sync.selectTask(secondTask.id)
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, secondTask.id)
        XCTAssertEqual(publishes, 1)
        let selected = try await fixture.client.next()
        XCTAssertEqual(selected.operation, .history(task: secondTask.id, cursor: nil))
        selected.succeed(emptyPage)
        try await fixture.settled()

        publishes = 0
        fixture.sync.changeTab(.archived)
        XCTAssertEqual(fixture.session.taskCatalog.tab, .archived)
        XCTAssertEqual(publishes, 1)
        let changed = try await fixture.client.next()
        XCTAssertEqual(changed.operation, .history(task: archivedTask.id, cursor: nil))
        changed.succeed(emptyPage)
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

        XCTAssertEqual(fixture.session.taskCatalog.tab, .active)
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, secondTask.id)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: secondTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.client.operations.filter { $0 == .history(task: secondTask.id, cursor: nil) }.count, 1)
        XCTAssertEqual(fixture.client.operations.last, .history(task: secondTask.id, cursor: nil))
    }

    @MainActor
    func testKeepingTheTabOrTaskSendsNothingAndDoesNotPublish() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(resources)
        var publishes = 0
        fixture.sync.onChange = { publishes += 1 }

        fixture.sync.selectTask(firstTask.id)
        fixture.sync.changeTab(.active)

        XCTAssertEqual(publishes, 0)
        XCTAssertFalse(fixture.client.hasPendingRequests)
    }
}
