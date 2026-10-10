import XCTest
@testable import TrackerClient

final class RefreshPresentationTests: XCTestCase {
    @MainActor
    func testSelectedResourcesPublishTogetherAndPreserveLoadedIdleState() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        XCTAssertNil(fixture.session.taskCatalogResource)
        XCTAssertNil(fixture.session.trackingResource)
        try await fixture.start()
        XCTAssertEqual(fixture.session.taskCatalogResource?.value, [])
        XCTAssertNotNil(fixture.session.trackingResource)
        XCTAssertNil(fixture.session.trackingResource?.value)
        var observations: [(String?, String?)] = []
        fixture.session.onChange = {
            observations.append(
                (
                    fixture.session.taskCatalogResource?.revision,
                    fixture.session.trackingResource?.revision
                ))
        }
        fixture.session.refresh()
        let refresh = try await fixture.client.next()
        refresh.succeed(TaskListResources(tasks: [], active: nil, tasksRevision: "next", trackingRevision: "next"))
        try await settleWithoutReplacingObserver(fixture)
        XCTAssertTrue(observations.contains { $0.0 == "next" })
        XCTAssertTrue(observations.allSatisfy { $0.0 == $0.1 })
    }

    @MainActor
    func testRunningTotalTicksInvalidateOnlyRunningTaskRow() async throws {
        let fixture = Fixture(saved: serverSettings, reports: true)
        defer { fixture.cleanup() }
        fixture.session.setWindowVisible(true)
        try await fixture.start(
            TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog),
            rows: [
                TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 10_000_000),
                TaskReportTotal(taskId: secondTask.id, durationMicroseconds: 20_000_000),
            ])
        let observer = TrackerPresentationObserver(session: fixture.session)
        var redraws: [Set<String>] = []
        observer.onTaskDailyTotalsChange = { redraws.append($0) }
        fixture.session.onChange = { observer.update(from: fixture.session) }
        let display = try XCTUnwrap(fixture.scheduler.display)
        for _ in 0..<3 {
            fixture.clock.now.addTimeInterval(1)
            fixture.clock.uptime += 1
            display.fire()
        }
        XCTAssertEqual(
            redraws, Array(repeating: Set([firstTask.id]), count: 3),
            "A running duration must leave completed task rows unchanged.")
    }

    @MainActor
    func testServerPollUpdatesOnlyChangedTotalWithoutInvalidatingLists() async throws {
        let fixture = Fixture(saved: serverSettings, reports: true)
        defer { fixture.cleanup() }
        let snapshot = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(
            snapshot,
            rows: [
                TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 10_000_000),
                TaskReportTotal(taskId: secondTask.id, durationMicroseconds: 20_000_000),
            ])
        let observer = TrackerPresentationObserver(session: fixture.session)
        var redraws: [Set<String>] = []
        var listUpdates = 0
        observer.onTaskDailyTotalsChange = { redraws.append($0) }
        observer.onContentChange = { listUpdates += 1 }
        fixture.session.onChange = { observer.update(from: fixture.session) }
        fixture.scheduler.poll?.fire()
        let poll = try await fixture.client.next()
        poll.succeed(
            TaskListTotalsRefresh(
                snapshot: snapshot,
                rows: [
                    TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 10_000_000),
                    TaskReportTotal(taskId: secondTask.id, durationMicroseconds: 25_000_000),
                ]))
        try await settleWithoutReplacingObserver(fixture)
        XCTAssertEqual(redraws, [Set([secondTask.id])])
        XCTAssertEqual(listUpdates, 0)
        XCTAssertEqual(observer.taskDailyTotals[firstTask.id]?.text, "00:00:10")
        XCTAssertEqual(observer.taskDailyTotals[secondTask.id]?.text, "00:00:25")
        observer.update(from: fixture.session)
        XCTAssertEqual(redraws.count, 1, "An unchanged report must not publish row updates again.")
    }

    @MainActor
    func testPollKeepsCompletedRowStableWhileRunningProjectionAdvances() async throws {
        let fixture = Fixture(saved: serverSettings, reports: true)
        defer { fixture.cleanup() }
        let snapshot = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(
            snapshot,
            rows: [
                TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 10_000_000),
                TaskReportTotal(taskId: secondTask.id, durationMicroseconds: 20_000_000),
            ])
        let observer = TrackerPresentationObserver(session: fixture.session)
        var redraws: [Set<String>] = []
        var listUpdates = 0
        observer.onTaskDailyTotalsChange = { redraws.append($0) }
        observer.onContentChange = { listUpdates += 1 }
        fixture.session.onChange = { observer.update(from: fixture.session) }
        fixture.clock.now.addTimeInterval(5)
        fixture.clock.uptime += 5
        fixture.scheduler.poll?.fire()
        let poll = try await fixture.client.next()
        poll.succeed(
            TaskListTotalsRefresh(
                snapshot: snapshot,
                rows: [
                    TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 15_000_000),
                    TaskReportTotal(taskId: secondTask.id, durationMicroseconds: 20_000_000),
                ]))
        try await settleWithoutReplacingObserver(fixture)
        XCTAssertEqual(redraws, [Set([firstTask.id])])
        XCTAssertEqual(listUpdates, 0)
        XCTAssertEqual(observer.taskDailyTotals[firstTask.id]?.text, "00:00:15")
        XCTAssertEqual(observer.taskDailyTotals[secondTask.id]?.text, "00:00:20")
    }

    @MainActor
    func testCachedReportNotifiesAllRowsOfChangedExplanation() async throws {
        let fixture = Fixture(saved: serverSettings, reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(
            TaskListResources(tasks: [firstTask, secondTask], active: nil),
            rows: [
                TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 10_000_000),
                TaskReportTotal(taskId: secondTask.id, durationMicroseconds: 20_000_000),
            ])
        let observer = TrackerPresentationObserver(session: fixture.session)
        var redraws: [Set<String>] = []
        observer.onTaskDailyTotalsChange = { redraws.append($0) }
        fixture.session.onChange = { observer.update(from: fixture.session) }
        fixture.scheduler.poll?.fire()
        let poll = try await fixture.client.next()
        poll.fail(BridgeFailure(message: "Server unavailable", kind: "unavailable"))
        try await settleWithoutReplacingObserver(fixture)
        XCTAssertEqual(redraws, [Set([firstTask.id, secondTask.id])])
        XCTAssertEqual(observer.taskDailyTotals[firstTask.id]?.text, "00:00:10")
        XCTAssertEqual(observer.taskDailyTotals[firstTask.id]?.explanation, fixture.session.dailyTotalsExplanation)
        XCTAssertTrue(observer.taskDailyTotals[secondTask.id]?.explanation.contains("cached") == true)
    }

    @MainActor
    func testAddedAndRemovedTasksNotifyTheirTotalAdapters() async throws {
        let fixture = Fixture(saved: serverSettings, reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(
            TaskListResources(tasks: [firstTask, secondTask], active: nil),
            rows: [
                TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 10_000_000),
                TaskReportTotal(taskId: secondTask.id, durationMicroseconds: 20_000_000),
            ])
        let observer = TrackerPresentationObserver(session: fixture.session)
        var redraws: [Set<String>] = []
        observer.onTaskDailyTotalsChange = { redraws.append($0) }
        fixture.session.onChange = { observer.update(from: fixture.session) }
        let added = TaskItem(id: "task-three", name: "New task", archived: false, latestStart: nil)
        fixture.scheduler.poll?.fire()
        let poll = try await fixture.client.next()
        poll.succeed(
            TaskListTotalsRefresh(
                snapshot: TaskListResources(tasks: [firstTask, added], active: nil),
                rows: [
                    TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 10_000_000),
                    TaskReportTotal(taskId: added.id, durationMicroseconds: 30_000_000),
                ]))
        try await settleWithoutReplacingObserver(fixture)
        XCTAssertEqual(redraws, [Set([secondTask.id, added.id])])
        XCTAssertNil(observer.taskDailyTotals[secondTask.id])
        XCTAssertEqual(observer.taskDailyTotals[added.id]?.text, "00:00:30")
    }

    @MainActor
    private func settleWithoutReplacingObserver(_ fixture: Fixture) async throws {
        let finished = Task { @MainActor in
            while fixture.session.isBusy && !Task.isCancelled { await Task.yield() }
        }
        try await fixture.taskValue(finished)
    }

}
