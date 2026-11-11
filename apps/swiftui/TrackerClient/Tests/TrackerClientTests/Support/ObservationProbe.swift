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
    static func lists(_ features: TrackerFeatures) {
        _ = features.taskCatalog.visibleTasks
        _ = features.taskCatalog.selectedTaskID
        _ = features.taskCatalog.selectedTask
        _ = features.taskCatalog.tab
        _ = features.tracking.active
        _ = features.connection.stale
        _ = features.dailyTotals.status
        _ = features.worklogHistory.worklogs
        _ = features.worklogHistory.hasMore
        _ = features.worklogHistory.isLoading
        _ = features.worklogHistory.error
        _ = features.worklogHistory.unavailable
        _ = features.trackingAutomation.statusText
    }

    /// Reads every window value outside the request controls, timer, totals, and feature sheets.
    static func content(_ features: TrackerFeatures) {
        lists(features)
        _ = features.tracking.error
        _ = features.connection.settings
        _ = features.connection.statusText
        _ = features.connection.message
        _ = features.connection.changing
        _ = features.trackingAutomation.enabled
    }

    static func controls(_ features: TrackerFeatures) {
        _ = features.trackingAvailability.canStop
        for task in features.taskCatalog.tasks { _ = features.trackingAvailability.canStart(taskID: task.id) }
    }

    static func timer(_ features: TrackerFeatures) { _ = features.tracking.timerText }

    static func taskCreation(_ features: TrackerFeatures) {
        let store = features.taskCreation
        _ = store.isPresented
        _ = store.name
        _ = store.isSubmitting
        _ = store.error
        _ = store.canOpen
    }

    static func taskRename(_ features: TrackerFeatures) {
        let store = features.taskRename
        _ = store.isPresented
        _ = store.originalName
        _ = store.name
        _ = store.isSubmitting
        _ = store.error
        _ = store.canOpen
    }

    static func taskArchiving(_ features: TrackerFeatures) {
        let store = features.taskArchiving
        _ = store.isPresented
        _ = store.taskName
        _ = store.action
        _ = store.isSubmitting
        _ = store.error
    }

    static func bulkTaskArchiving(_ features: TrackerFeatures) {
        let store = features.bulkTaskArchiving
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

    static func worklogCorrection(_ features: TrackerFeatures) {
        let store = features.worklogCorrection
        _ = store.isPresented
        _ = store.taskName
        _ = store.start
        _ = store.end
        _ = store.timezone
        _ = store.isSubmitting
        _ = store.error
        _ = store.canOpen
    }

    static func worklogMove(_ features: TrackerFeatures) {
        let store = features.worklogMove
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

    static func worklogDeletion(_ features: TrackerFeatures) {
        let store = features.worklogDeletion
        _ = store.isPresented
        _ = store.taskName
        _ = store.original
        _ = store.isSubmitting
        _ = store.error
        _ = store.canOpen
    }
}

/// Checks that the tracking store's timer text, the auto-pause status and the daily totals store's values match their
/// inputs. Call it only while the clock hasn't moved since the stores last refreshed.
@MainActor
func assertRefreshed(_ fixture: Fixture, file: StaticString = #filePath, line: UInt = #line) {
    let features = fixture.features
    let automation = features.trackingAutomation
    XCTAssertEqual(automation.statusText, automation.pausedStatusText, file: file, line: line)
    let totals = features.dailyTotals
    XCTAssertEqual(totals.status, totals.shownStatus(), file: file, line: line)
    XCTAssertEqual(
        totals.dayTotal, totals.totalDuration(active: features.tracking.active),
        file: file, line: line)
    let explanation = totals.explanation()
    XCTAssertEqual(totals.totalsExplanation, explanation, file: file, line: line)
    XCTAssertEqual(
        features.tracking.timerText,
        features.tracking.displayText(),
        file: file, line: line)
    for task in features.taskCatalog.tasks {
        XCTAssertEqual(
            totals.taskDurations[task.id], features.menuBar.dailyDuration(taskID: task.id), file: file, line: line)
        XCTAssertEqual(
            totals.taskTotal(taskID: task.id).content?.text,
            features.menuBar.dailyDuration(taskID: task.id).map(formatClockDuration) ?? "-", file: file, line: line)
        XCTAssertEqual(totals.taskTotal(taskID: task.id).content?.explanation, explanation, file: file, line: line)
    }
}
