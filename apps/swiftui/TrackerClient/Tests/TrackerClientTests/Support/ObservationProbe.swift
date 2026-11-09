import Observation
import XCTest
@testable import TrackerClient

/// Records whether the observable values read by `read` changed, as a SwiftUI view body would observe them.
@MainActor
final class ObservationProbe {
    private let read: @MainActor () -> Void
    private var generation = 0
    private var changed = false

    init(_ read: @escaping @MainActor () -> Void) {
        self.read = read
        observe()
    }

    /// Returns whether a read value changed since the previous call, then observes the current values.
    func consume() -> Bool {
        let result = changed
        if result { observe() }
        return result
    }

    private func observe() {
        changed = false
        generation += 1
        let current = generation
        withObservationTracking {
            read()
        } onChange: { [weak self] in
            MainActor.assumeIsolated {
                guard let self, self.generation == current else { return }
                self.changed = true
            }
        }
    }
}

/// Collects which task total labels changed.
@MainActor
final class TaskTotalProbes {
    private let probes: [String: ObservationProbe]

    init(_ store: DailyTotalsStore, taskIDs: [String]) {
        probes = Dictionary(
            uniqueKeysWithValues: taskIDs.map { taskID in
                let total = store.taskTotal(taskID: taskID)
                return (taskID, ObservationProbe { _ = total.content })
            })
    }

    func changed() -> Set<String> { Set(probes.filter { $0.value.consume() }.keys) }
}

/// Reads the values the window's views read, so a probe sees the changes that would redraw them.
@MainActor
enum ViewReads {
    /// Reads the values the task list and worklog list display.
    static func lists(_ session: TrackerSession) {
        _ = session.taskCatalog.visibleTasks
        _ = session.taskCatalog.selectedTaskID
        _ = session.taskCatalog.selectedTask
        _ = session.taskCatalog.tab
        _ = session.tracking.active
        _ = session.connection.stale
        _ = session.dailyTotals.status
        _ = session.worklogHistory.worklogs
        _ = session.worklogHistory.hasMore
        _ = session.worklogHistory.isLoading
        _ = session.worklogHistory.error
        _ = session.worklogHistory.unavailable
        _ = session.trackingAutomation.statusText
    }

    /// Reads every window value outside the request controls, timer, totals, and feature sheets.
    static func content(_ session: TrackerSession) {
        lists(session)
        _ = session.tracking.error
        _ = session.connection.settings
        _ = session.connection.statusText
        _ = session.connection.message
        _ = session.connection.changing
        _ = session.trackingAutomation.enabled
    }

    static func controls(_ session: TrackerSession) {
        _ = session.trackingAvailability.canStop
        for task in session.taskCatalog.tasks { _ = session.trackingAvailability.canStart(taskID: task.id) }
    }

    static func timer(_ session: TrackerSession) { _ = session.tracking.timerText }

    static func taskCreation(_ session: TrackerSession) {
        let store = session.taskCreation
        _ = store.isPresented
        _ = store.name
        _ = store.isSubmitting
        _ = store.error
        _ = store.canOpen
    }

    static func taskRename(_ session: TrackerSession) {
        let store = session.taskRename
        _ = store.isPresented
        _ = store.originalName
        _ = store.name
        _ = store.isSubmitting
        _ = store.error
        _ = store.canOpen
    }

    static func taskArchiving(_ session: TrackerSession) {
        let store = session.taskArchiving
        _ = store.isPresented
        _ = store.taskName
        _ = store.action
        _ = store.isSubmitting
        _ = store.error
    }

    static func bulkTaskArchiving(_ session: TrackerSession) {
        let store = session.bulkTaskArchiving
        _ = store.isPresented
        _ = store.daysText
        _ = store.isLoading
        _ = store.isSubmitting
        _ = store.hasPreview
        _ = store.tasks
        _ = store.error
        _ = store.archivedCount
        _ = store.canOpen
    }

    static func worklogCorrection(_ session: TrackerSession) {
        let store = session.worklogCorrection
        _ = store.isPresented
        _ = store.taskName
        _ = store.start
        _ = store.end
        _ = store.timezone
        _ = store.isSubmitting
        _ = store.error
        _ = store.canOpen
    }

    static func worklogMove(_ session: TrackerSession) {
        let store = session.worklogMove
        _ = store.isPresented
        _ = store.original
        _ = store.sourceTaskName
        _ = store.query
        _ = store.candidates
        _ = store.selectedTaskID
        _ = store.isSearching
        _ = store.isSubmitting
        _ = store.error
        _ = store.canOpen
    }

    static func worklogDeletion(_ session: TrackerSession) {
        let store = session.worklogDeletion
        _ = store.isPresented
        _ = store.taskName
        _ = store.original
        _ = store.isSubmitting
        _ = store.error
        _ = store.canOpen
    }
}

/// Checks that the last publish updated the tracking store's timer text, the auto-pause status and the daily totals
/// store's values. Call it only while the clock hasn't moved since that publish.
@MainActor
func assertPublished(_ session: TrackerSession, file: StaticString = #filePath, line: UInt = #line) {
    let automation = session.trackingAutomation
    XCTAssertEqual(automation.statusText, automation.pausedStatusText, file: file, line: line)
    let totals = session.dailyTotals
    XCTAssertEqual(totals.status, totals.shownStatus(clock: session.clock), file: file, line: line)
    XCTAssertEqual(
        totals.dayTotal, totals.totalDuration(active: session.active, clock: session.clock), file: file, line: line)
    let explanation = totals.explanation(clock: session.clock)
    XCTAssertEqual(totals.totalsExplanation, explanation, file: file, line: line)
    XCTAssertEqual(
        session.tracking.timerText,
        session.tracking.displayText(clock: session.clock, confirmed: session.connection.confirmed),
        file: file, line: line)
    for task in session.tasks {
        XCTAssertEqual(totals.taskDurations[task.id], session.dailyDuration(taskID: task.id), file: file, line: line)
        XCTAssertEqual(
            totals.taskTotal(taskID: task.id).content?.text,
            session.dailyDuration(taskID: task.id).map(formatClockDuration) ?? "-", file: file, line: line)
        XCTAssertEqual(totals.taskTotal(taskID: task.id).content?.explanation, explanation, file: file, line: line)
    }
}
