import XCTest
@testable import TrackerClient

final class QuickTrackContentTests: XCTestCase {
    private let thirdTask = TaskItem(id: "task-three", name: "Third task", archived: false, latestStart: nil)

    private func inputs(
        active: WorklogItem? = nil, stale: Bool = false, durations: [String: TimeInterval] = [:],
        startable: Set<String>? = nil, autoPause: String? = nil, trackingError: String? = nil
    ) -> TrackerMenuInputs {
        let tasks = [firstTask, secondTask, thirdTask, archivedTask]
        return TrackerMenuInputs(
            connectionSettings: .local, connectionStatusText: stale ? "Unavailable" : "Local database",
            isStale: stale, tasks: tasks, active: active, elapsedSeconds: active == nil ? nil : 65,
            trackingError: trackingError, runningTaskName: active == nil ? "No timer running" : firstTask.name,
            autoPauseStatusText: autoPause, canStopTracking: active != nil,
            startableTaskIDs: startable ?? Set(tasks.filter { !$0.archived && $0.id != active?.taskId }.map(\.id)),
            dailyTotalsStatus: .current, dayTotal: durations.values.reduce(0, +),
            totalsExplanation: "Totals for today", taskDurations: durations, lastTrackedTaskID: nil)
    }

    private let allResults = ["task-two", "task-archived", "task-one", "task-three", "missing"]

    @MainActor
    private func content(
        _ inputs: TrackerMenuInputs, query: String = "", results: [String]? = nil,
        highlight: QuickTrackHighlight? = nil, searchError: String? = nil
    ) -> QuickTrackContent {
        QuickTrackContent(
            inputs, query: query, results: results ?? allResults, highlight: highlight, searchError: searchError)
    }

    @MainActor
    func testEmptyQueryListsTodaysTasksFirstAndLeavesOutTheRunningArchivedAndUnknownTasks() {
        let content = content(inputs(active: activeWorklog, durations: ["task-three": 600, "task-one": 60]))
        XCTAssertEqual(content.tasks.map(\.id), ["task-three", "task-two"])
        XCTAssertEqual(content.tasks[0].durationText, MenuDurationText(seconds: 600))
        XCTAssertNil(content.tasks[1].durationText)
        XCTAssertEqual(content.tasks.map(\.canStart), [true, true])
        XCTAssertNil(content.highlight)
    }

    @MainActor
    func testQueryKeepsTheSearchOrderAndHighlightsTheFirstMatch() {
        let content = content(inputs(durations: ["task-three": 600]), query: "task")
        XCTAssertEqual(content.tasks.map(\.id), ["task-two", "task-one", "task-three"])
        XCTAssertEqual(content.tasks.map(\.durationText), [nil, nil, MenuDurationText(seconds: 600)])
        XCTAssertEqual(content.highlight, .task("task-two"))
    }

    @MainActor
    func testBlankQueryCountsAsEmpty() {
        let content = content(inputs(durations: ["task-three": 600]), query: "  ")
        XCTAssertEqual(content.tasks.map(\.id), ["task-three", "task-two", "task-one"])
        XCTAssertNil(content.highlight)
    }

    @MainActor
    func testHeaderShowsTheRunningTaskOrNotTracking() {
        let running = content(inputs(active: activeWorklog, durations: ["task-one": 65])).header
        XCTAssertEqual(running.taskID, firstTask.id)
        XCTAssertEqual(running.title, firstTask.name)
        XCTAssertEqual(running.elapsedText, MenuDurationText(seconds: 65))
        XCTAssertTrue(running.isRunning)
        XCTAssertTrue(running.canStop)
        XCTAssertEqual(running.totalText, MenuDurationText(seconds: 65))
        XCTAssertEqual(running.totalsExplanation, "Totals for today")

        let idle = content(inputs()).header
        XCTAssertNil(idle.taskID)
        XCTAssertEqual(idle.title, "Not tracking")
        XCTAssertNil(idle.elapsedText)
        XCTAssertFalse(idle.isRunning)
        XCTAssertFalse(idle.canStop)
    }

    @MainActor
    func testStaleStateDisablesStartAndStopAndExplainsWhy() {
        let content = content(inputs(active: activeWorklog, stale: true))
        XCTAssertEqual(content.statusLines, ["Unavailable", "Showing last confirmed state"])
        XCTAssertFalse(content.header.canStop)
        XCTAssertEqual(content.tasks.map(\.canStart), [false, false])
    }

    @MainActor
    func testStatusLinesListAutoPauseTrackingAndSearchMessages() {
        let content = content(
            inputs(autoPause: "Paused while the screen is locked", trackingError: "Tracking failed"),
            searchError: "Search failed: offline")
        XCTAssertEqual(
            content.statusLines,
            ["Paused while the screen is locked", "Tracking failed", "Search failed: offline"])
        XCTAssertEqual(self.content(inputs()).statusLines, [])
    }

    @MainActor
    func testATaskThatCannotStartIsMarked() {
        let content = content(inputs(startable: ["task-two"]))
        XCTAssertEqual(content.tasks.map(\.canStart), [true, false, false])
    }

    @MainActor
    func testHighlightIsKeptWhileItsRowExists() {
        let running = inputs(active: activeWorklog)
        XCTAssertEqual(content(running, highlight: .header).highlight, .header)
        XCTAssertEqual(content(running, highlight: .task("task-three")).highlight, .task("task-three"))
        XCTAssertNil(content(inputs(), highlight: .header).highlight, "The header can't be highlighted while idle.")
        XCTAssertNil(content(running, highlight: .task("task-one")).highlight, "The running task has no row.")
        XCTAssertEqual(
            content(running, query: "t", highlight: .task("gone")).highlight, .task("task-two"),
            "A highlight whose row is gone falls back to the first match.")
        XCTAssertNil(content(running, query: "t", results: []).highlight)
    }

    @MainActor
    func testArrowsMoveThroughTheHeaderAndTheTasks() {
        let running = inputs(active: activeWorklog)
        func moved(_ highlight: QuickTrackHighlight?, down: Bool) -> QuickTrackHighlight? {
            content(running, highlight: highlight).movingHighlight(down: down)
        }
        XCTAssertEqual(moved(nil, down: false), .header)
        XCTAssertEqual(moved(nil, down: true), .task("task-two"))
        XCTAssertEqual(moved(.header, down: false), .header)
        XCTAssertEqual(moved(.header, down: true), .task("task-two"))
        XCTAssertEqual(moved(.task("task-two"), down: true), .task("task-three"))
        XCTAssertEqual(moved(.task("task-three"), down: true), .task("task-three"))
        XCTAssertEqual(moved(.task("task-three"), down: false), .task("task-two"))
        XCTAssertEqual(moved(.task("task-two"), down: false), .header)
    }

    @MainActor
    func testArrowsSkipTheHeaderWhileIdle() {
        func moved(_ highlight: QuickTrackHighlight?, down: Bool) -> QuickTrackHighlight? {
            content(inputs(), highlight: highlight).movingHighlight(down: down)
        }
        XCTAssertNil(moved(nil, down: false))
        XCTAssertEqual(moved(nil, down: true), .task("task-two"))
        XCTAssertEqual(moved(.task("task-two"), down: false), .task("task-two"))
        XCTAssertNil(content(inputs(), results: []).movingHighlight(down: true))
    }
}
