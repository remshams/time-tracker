import XCTest
@testable import TrackerClient

final class TaskOrderTests: XCTestCase {
    private let older = TaskItem(
        id: "task-older", name: "Older task", archived: false, latestStart: "2024-12-30T09:00:00.000Z")
    private let newer = TaskItem(
        id: "task-newer", name: "Newer task", archived: false, latestStart: "2024-12-31T09:00:00.000Z")
    private let tiedLater = TaskItem(
        id: "task-zulu", name: "Zulu task", archived: false, latestStart: "2024-12-31T09:00:00.000Z")
    private let tiedEarlier = TaskItem(
        id: "task-alpha", name: "Zulu task", archived: false, latestStart: "2024-12-31T09:00:00.000Z")
    private let unlogged = TaskItem(id: "task-unlogged", name: "Alpha unlogged", archived: false, latestStart: nil)
    private let unreadable = TaskItem(
        id: "task-unreadable", name: "Beta unreadable", archived: false, latestStart: "not a timestamp")

    private var shuffled: [TaskItem] { [unlogged, older, tiedLater, unreadable, newer, tiedEarlier] }

    @MainActor
    func testNewestFirstPutsLatestLoggedTasksFirstAndUnloggedTasksLast() {
        XCTAssertEqual(
            TaskOrder.newestFirst.sorted(shuffled).map(\.id),
            [newer, tiedEarlier, tiedLater, older, unlogged, unreadable].map(\.id))
    }

    @MainActor
    func testOldestFirstPutsEarliestLoggedTasksFirstAndUnloggedTasksLast() {
        XCTAssertEqual(
            TaskOrder.oldestFirst.sorted(shuffled).map(\.id),
            [older, newer, tiedEarlier, tiedLater, unlogged, unreadable].map(\.id))
    }

    @MainActor
    func testNameOrderIgnoresLoggedTimes() {
        XCTAssertEqual(
            TaskOrder.name.sorted(shuffled).map(\.id),
            [unlogged, unreadable, newer, older, tiedEarlier, tiedLater].map(\.id))
    }

    @MainActor
    func testPersistedValuesAndTitlesStayStable() {
        XCTAssertEqual(TaskOrder.allCases.map(\.rawValue), ["newestFirst", "oldestFirst", "name"])
        XCTAssertEqual(TaskOrder.allCases.map(\.id), TaskOrder.allCases)
        XCTAssertEqual(
            TaskOrder.allCases.map(\.title),
            ["Most recently logged first", "Least recently logged first", "By name"])
    }

    @MainActor
    func testMenuSectionsFollowTheCatalogOrderAndAnOpenMenuKeepsItsSnapshotUntilClosed() async throws {
        let order = MemoryTaskOrder(.newestFirst)
        let fixture = Fixture(reports: true, taskOrder: order)
        defer { fixture.cleanup() }
        let running = TaskItem(
            id: firstTask.id, name: firstTask.name, archived: false, latestStart: activeWorklog.start)
        let earlierToday = TaskItem(
            id: "task-earlier", name: "Another task", archived: false, latestStart: "2024-12-31T12:00:00.000Z")
        let recentIdle = TaskItem(
            id: "task-recent", name: "Recent idle task", archived: false, latestStart: "2024-12-30T10:00:00.000Z")
        let staleIdle = TaskItem(
            id: "task-stale", name: "Stale idle task", archived: false, latestStart: "2024-12-01T10:00:00.000Z")
        try await fixture.start(
            TaskListResources(
                tasks: [running, staleIdle, secondTask, earlierToday, recentIdle], active: activeWorklog),
            rows: [
                TaskReportTotal(taskId: running.id, durationMicroseconds: 10_000_000),
                TaskReportTotal(taskId: earlierToday.id, durationMicroseconds: 20_000_000),
            ])
        let catalog = fixture.features.taskCatalog
        let menu = MenuHarness(fixture, showDailyTotal: true)
        let observer = menu.observer
        XCTAssertEqual(catalog.taskOrder, .newestFirst)
        XCTAssertEqual(observer.content.todayTasks.map(\.id), [running.id, earlierToday.id])
        XCTAssertEqual(observer.content.otherTasks.map(\.id), [recentIdle.id, staleIdle.id, secondTask.id])

        observer.menuOpened()
        let captured = observer.content
        catalog.setTaskOrder(.name)
        try await menu.settled()
        XCTAssertEqual(observer.content, captured, "An open menu must keep its rows in place.")
        observer.menuClosed()
        XCTAssertEqual(observer.content.todayTasks.map(\.id), [earlierToday.id, running.id])
        XCTAssertEqual(observer.content.otherTasks.map(\.id), [recentIdle.id, secondTask.id, staleIdle.id])

        catalog.setTaskOrder(.oldestFirst)
        try await menu.settled()
        XCTAssertEqual(observer.content.todayTasks.map(\.id), [earlierToday.id, running.id])
        XCTAssertEqual(observer.content.otherTasks.map(\.id), [staleIdle.id, recentIdle.id, secondTask.id])
        XCTAssertEqual(order.writes, [.name, .oldestFirst])

        let created = TrackerMenuPresentationObserver(sources: menu.sources, showDailyTotal: true)
        XCTAssertEqual(created.content, observer.content)
    }
}
