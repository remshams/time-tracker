import XCTest
@testable import TrackerClient

final class MenuTaskOrderTests: XCTestCase {
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
            MenuTaskOrder.newestFirst.sorted(shuffled).map(\.id),
            [newer, tiedEarlier, tiedLater, older, unlogged, unreadable].map(\.id))
    }

    @MainActor
    func testOldestFirstPutsEarliestLoggedTasksFirstAndUnloggedTasksLast() {
        XCTAssertEqual(
            MenuTaskOrder.oldestFirst.sorted(shuffled).map(\.id),
            [older, newer, tiedEarlier, tiedLater, unlogged, unreadable].map(\.id))
    }

    @MainActor
    func testNameOrderIgnoresLoggedTimes() {
        XCTAssertEqual(
            MenuTaskOrder.name.sorted(shuffled).map(\.id),
            [unlogged, unreadable, newer, older, tiedEarlier, tiedLater].map(\.id))
    }

    @MainActor
    func testPersistedValuesAndTitlesStayStable() {
        XCTAssertEqual(MenuTaskOrder.allCases.map(\.rawValue), ["newestFirst", "oldestFirst", "name"])
        XCTAssertEqual(MenuTaskOrder.allCases.map(\.id), MenuTaskOrder.allCases)
        XCTAssertEqual(
            MenuTaskOrder.allCases.map(\.title),
            ["Most recently logged first", "Least recently logged first", "By name"])
    }

    @MainActor
    func testMenuSectionsUseTheOrderAndAnOpenMenuKeepsItsSnapshotUntilClosed() async throws {
        let fixture = Fixture(reports: true)
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
        let observer = TrackerMenuPresentationObserver(session: fixture.session, showDailyTotal: true)
        XCTAssertEqual(observer.taskOrder, .newestFirst)
        XCTAssertEqual(observer.content.todayTasks.map(\.id), [running.id, earlierToday.id])
        XCTAssertEqual(observer.content.otherTasks.map(\.id), [recentIdle.id, staleIdle.id, secondTask.id])

        observer.menuOpened(from: fixture.session, showDailyTotal: true)
        let captured = observer.content
        observer.setTaskOrder(.name, session: fixture.session, showDailyTotal: true)
        XCTAssertEqual(observer.taskOrder, .name)
        XCTAssertEqual(observer.content, captured, "An open menu must keep its rows in place.")
        observer.menuClosed(from: fixture.session, showDailyTotal: true)
        XCTAssertNotEqual(observer.content, captured)
        XCTAssertEqual(observer.content.todayTasks.map(\.id), [earlierToday.id, running.id])
        XCTAssertEqual(observer.content.otherTasks.map(\.id), [recentIdle.id, secondTask.id, staleIdle.id])

        let byName = observer.content
        observer.setTaskOrder(.oldestFirst, session: fixture.session, showDailyTotal: true)
        XCTAssertNotEqual(observer.content, byName)
        XCTAssertEqual(observer.content.todayTasks.map(\.id), [earlierToday.id, running.id])
        XCTAssertEqual(observer.content.otherTasks.map(\.id), [staleIdle.id, recentIdle.id, secondTask.id])

        let created = TrackerMenuPresentationObserver(
            session: fixture.session, showDailyTotal: true, taskOrder: .oldestFirst)
        XCTAssertEqual(created.content, observer.content)
    }
}
