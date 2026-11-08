import XCTest
@testable import TrackerClient

final class RefreshPresentationTests: XCTestCase {
    @MainActor
    func testSelectedResourcesPublishTogetherAndPreserveLoadedIdleState() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        XCTAssertTrue(fixture.session.tasks.isEmpty)
        XCTAssertEqual(TrackerMenuInputs.current(fixture.session).runningTaskName, "Waiting for tracker state")
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        XCTAssertEqual(fixture.session.tasks, [firstTask])
        XCTAssertNil(fixture.session.active)
        XCTAssertEqual(TrackerMenuInputs.current(fixture.session).runningTaskName, "No timer running")
        var observations: [([TaskItem], WorklogItem?)] = []
        fixture.sync.onChange = { observations.append((fixture.session.tasks, fixture.session.active)) }
        fixture.sync.refresh()
        let refresh = try await fixture.client.next()
        let running = WorklogItem(id: "worklog-second", taskId: secondTask.id, start: activeWorklog.start, end: nil)
        let next = TaskListResources(tasks: [firstTask, secondTask], active: running)
        refresh.succeed(next)
        try await settleWithoutReplacingObserver(fixture)
        XCTAssertTrue(observations.contains { $0.0 == next.catalog.value })
        XCTAssertTrue(observations.allSatisfy { ($0.0 == next.catalog.value) == ($0.1 == next.tracking.value) })
    }

    @MainActor
    func testRunningTotalTicksInvalidateOnlyRunningTaskRow() async throws {
        let fixture = Fixture(saved: serverSettings, reports: true)
        defer { fixture.cleanup() }
        fixture.sync.setWindowVisible(true)
        try await fixture.start(
            TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog),
            rows: [
                TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 10_000_000),
                TaskReportTotal(taskId: secondTask.id, durationMicroseconds: 20_000_000),
            ])
        let harness = PresentationHarness(session: fixture.session)
        let totals = TaskTotalProbes(harness.dailyTotals, taskIDs: [firstTask.id, secondTask.id])
        var redraws: [Set<String>] = []
        let display = try XCTUnwrap(fixture.scheduler.display)
        for _ in 0..<3 {
            fixture.clock.now.addTimeInterval(1)
            fixture.clock.uptime += 1
            display.fire()
            redraws.append(totals.changed())
        }
        XCTAssertEqual(
            redraws, Array(repeating: Set([firstTask.id]), count: 3),
            "A running duration must leave completed task rows unchanged.")
    }

    @MainActor
    func testServerPollUpdatesOnlyChangedTotalWithoutInvalidatingLists() async throws {
        let fixture = Fixture(saved: serverSettings, reports: true)
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(
            resources,
            rows: [
                TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 10_000_000),
                TaskReportTotal(taskId: secondTask.id, durationMicroseconds: 20_000_000),
            ])
        let harness = PresentationHarness(session: fixture.session)
        let totals = TaskTotalProbes(harness.dailyTotals, taskIDs: [firstTask.id, secondTask.id])
        let lists = harness.counter(PresentationReads.content)
        fixture.scheduler.poll?.fire()
        let poll = try await fixture.client.next()
        poll.succeed(
            TaskListTotalsRefresh(
                taskList: resources,
                rows: [
                    TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 10_000_000),
                    TaskReportTotal(taskId: secondTask.id, durationMicroseconds: 25_000_000),
                ]))
        try await harness.settled(fixture)
        XCTAssertEqual(totals.changed(), Set([secondTask.id]))
        XCTAssertEqual(lists.count, 0)
        XCTAssertEqual(harness.dailyTotals.taskTotal(taskID: firstTask.id).content?.text, "00:00:10")
        XCTAssertEqual(harness.dailyTotals.taskTotal(taskID: secondTask.id).content?.text, "00:00:25")
        // Closing the menu that isn't open publishes again without changing any value.
        fixture.sync.setMenuVisible(false)
        XCTAssertEqual(totals.changed(), [], "An unchanged report must not publish row updates again.")
    }

    @MainActor
    func testPollKeepsCompletedRowStableWhileRunningProjectionAdvances() async throws {
        let fixture = Fixture(saved: serverSettings, reports: true)
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
        try await fixture.start(
            resources,
            rows: [
                TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 10_000_000),
                TaskReportTotal(taskId: secondTask.id, durationMicroseconds: 20_000_000),
            ])
        let harness = PresentationHarness(session: fixture.session)
        let totals = TaskTotalProbes(harness.dailyTotals, taskIDs: [firstTask.id, secondTask.id])
        let lists = harness.counter(PresentationReads.content)
        fixture.clock.now.addTimeInterval(5)
        fixture.clock.uptime += 5
        fixture.scheduler.poll?.fire()
        let poll = try await fixture.client.next()
        poll.succeed(
            TaskListTotalsRefresh(
                taskList: resources,
                rows: [
                    TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 15_000_000),
                    TaskReportTotal(taskId: secondTask.id, durationMicroseconds: 20_000_000),
                ]))
        try await harness.settled(fixture)
        XCTAssertEqual(totals.changed(), Set([firstTask.id]))
        XCTAssertEqual(lists.count, 0)
        XCTAssertEqual(harness.dailyTotals.taskTotal(taskID: firstTask.id).content?.text, "00:00:15")
        XCTAssertEqual(harness.dailyTotals.taskTotal(taskID: secondTask.id).content?.text, "00:00:20")
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
        let harness = PresentationHarness(session: fixture.session)
        let totals = TaskTotalProbes(harness.dailyTotals, taskIDs: [firstTask.id, secondTask.id])
        fixture.scheduler.poll?.fire()
        let poll = try await fixture.client.next()
        poll.fail(BridgeFailure(message: "Server unavailable", kind: "unavailable"))
        try await harness.settled(fixture)
        XCTAssertEqual(totals.changed(), Set([firstTask.id, secondTask.id]))
        let first = harness.dailyTotals.taskTotal(taskID: firstTask.id).content
        XCTAssertEqual(first?.text, "00:00:10")
        XCTAssertEqual(first?.explanation, harness.dailyTotals.totalsExplanation)
        let second = harness.dailyTotals.taskTotal(taskID: secondTask.id).content
        XCTAssertTrue(second?.explanation.contains("cached") == true)
    }

    @MainActor
    func testAddedTasksReadCurrentTotalsAndRemovedTasksStopUpdating() async throws {
        let fixture = Fixture(saved: serverSettings, reports: true)
        defer { fixture.cleanup() }
        try await fixture.start(
            TaskListResources(tasks: [firstTask, secondTask], active: nil),
            rows: [
                TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 10_000_000),
                TaskReportTotal(taskId: secondTask.id, durationMicroseconds: 20_000_000),
            ])
        let harness = PresentationHarness(session: fixture.session)
        let removedTotal = harness.dailyTotals.taskTotal(taskID: secondTask.id)
        let totals = TaskTotalProbes(harness.dailyTotals, taskIDs: [firstTask.id, secondTask.id])
        let added = TaskItem(id: "task-three", name: "New task", archived: false, latestStart: nil)
        fixture.scheduler.poll?.fire()
        let poll = try await fixture.client.next()
        poll.succeed(
            TaskListTotalsRefresh(
                taskList: TaskListResources(tasks: [firstTask, added], active: nil),
                rows: [
                    TaskReportTotal(taskId: firstTask.id, durationMicroseconds: 10_000_000),
                    TaskReportTotal(taskId: added.id, durationMicroseconds: 30_000_000),
                ]))
        try await harness.settled(fixture)
        XCTAssertEqual(totals.changed(), [], "A removed task's label keeps its last total.")
        XCTAssertEqual(removedTotal.content?.text, "00:00:20")
        let replacement = harness.dailyTotals.taskTotal(taskID: secondTask.id)
        XCTAssertFalse(replacement === removedTotal, "A removed task's total must stop receiving updates.")
        XCTAssertNil(replacement.content)
        XCTAssertEqual(harness.dailyTotals.taskTotal(taskID: added.id).content?.text, "00:00:30")
    }

    @MainActor
    private func settleWithoutReplacingObserver(_ fixture: Fixture) async throws {
        let finished = Task { @MainActor in
            while fixture.client.hasPendingRequests && !Task.isCancelled { await Task.yield() }
        }
        try await fixture.taskValue(finished)
    }

}
