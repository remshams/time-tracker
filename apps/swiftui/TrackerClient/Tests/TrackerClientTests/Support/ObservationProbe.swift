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

/// Records a value after each presentation update that changed the tracked values.
@MainActor
final class ObservationRecorder<Value> {
    private let probe: ObservationProbe
    private let value: @MainActor () -> Value
    private(set) var values: [Value] = []
    var count: Int { values.count }

    init(track: @escaping @MainActor () -> Void, value: @escaping @MainActor () -> Value) {
        probe = ObservationProbe(track)
        self.value = value
    }

    func sample() {
        if probe.consume() { values.append(value()) }
    }
}

/// Updates a presentation after each session change and checks that it mirrors the session.
@MainActor
final class PresentationHarness {
    let presentation: TrackerPresentation
    var catalog: TaskCatalogStore { session.taskCatalog }
    var history: WorklogHistoryStore { session.worklogHistory }
    var connection: ConnectionStore { session.connection }
    var tracking: TrackingStore { session.tracking }
    var dailyTotals: DailyTotalsStore { session.dailyTotals }
    var automation: TrackingAutomationStore { session.trackingAutomation }
    private let session: TrackerSession
    private var samplers: [@MainActor () -> Void] = []

    init(session: TrackerSession) {
        self.session = session
        presentation = TrackerPresentation(session: session)
        attach()
    }

    /// Restores the session change handler after a fixture helper replaced it.
    func attach() { session.sync.onChange = { [weak self] in self?.update() } }

    func update() {
        presentation.update(from: session)
        assertMirrors(presentation, session)
        for sample in samplers { sample() }
    }

    /// Counts the updates that changed the values read by `track`.
    func counter(_ track: @escaping @MainActor (PresentationHarness) -> Void) -> ObservationRecorder<Void> {
        recorder(track) { () }
    }

    /// Records `value` after each update that changed the values read by `track`.
    func recorder<Value>(
        _ track: @escaping @MainActor (PresentationHarness) -> Void, value: @escaping @MainActor () -> Value
    ) -> ObservationRecorder<Value> {
        let recorder = ObservationRecorder(track: { [weak self] in if let self { track(self) } }, value: value)
        samplers.append { recorder.sample() }
        return recorder
    }
}

extension PresentationHarness {
    /// Waits for the session to finish its operation without replacing the presentation's change handler.
    func settled(_ fixture: Fixture) async throws {
        let finished = Task { @MainActor in
            while fixture.client.hasPendingRequests && !Task.isCancelled { await Task.yield() }
        }
        try await fixture.taskValue(finished)
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

@MainActor
enum PresentationReads {
    /// Reads the values the task list and worklog list display.
    static func lists(_ views: PresentationHarness) {
        _ = views.catalog.visibleTasks
        _ = views.catalog.selectedTaskID
        _ = views.catalog.selectedTask
        _ = views.catalog.tab
        _ = views.tracking.active
        _ = views.connection.stale
        _ = views.dailyTotals.status
        _ = views.history.worklogs
        _ = views.history.hasMore
        _ = views.history.isLoading
        _ = views.history.error
        _ = views.history.unavailable
        _ = views.automation.statusText
    }

    /// Reads every window value outside the request controls, timer, totals, and feature sheets.
    static func content(_ views: PresentationHarness) {
        lists(views)
        _ = views.tracking.error
        _ = views.connection.settings
        _ = views.connection.statusText
        _ = views.connection.message
        _ = views.connection.changing
        _ = views.automation.enabled
    }

    static func controls(_ views: PresentationHarness) {
        _ = views.presentation.canStopTracking
        _ = views.presentation.startableTaskIDs
    }

    static func timer(_ views: PresentationHarness) { _ = views.tracking.timerText }

    static func taskCreation(_ views: PresentationHarness) {
        _ = views.presentation.taskCreation
        _ = views.presentation.canOpenTaskCreation
    }

    static func taskRename(_ views: PresentationHarness) {
        _ = views.presentation.taskRename
        _ = views.presentation.canOpenTaskRename
    }

    static func taskArchiving(_ views: PresentationHarness) {
        _ = views.presentation.taskArchiving
        _ = views.presentation.archivableTaskIDs
        _ = views.presentation.unarchivableTaskIDs
    }

    static func bulkTaskArchiving(_ views: PresentationHarness) {
        _ = views.presentation.bulkTaskArchiving
        _ = views.presentation.canOpenBulkTaskArchiving
    }

    static func worklogCorrection(_ views: PresentationHarness) {
        _ = views.presentation.worklogCorrection
        _ = views.presentation.canOpenWorklogCorrection
    }

    static func worklogMove(_ views: PresentationHarness) {
        _ = views.presentation.worklogMove
        _ = views.presentation.canOpenWorklogMove
    }

    static func worklogDeletion(_ views: PresentationHarness) {
        _ = views.presentation.worklogDeletion
        _ = views.presentation.canOpenWorklogDeletion
    }
}

/// Checks that every presentation value equals the session value it copies, and that publishing updated the
/// tracking store's timer text, the auto-pause status and the daily totals store's values.
@MainActor
func assertMirrors(
    _ presentation: TrackerPresentation, _ session: TrackerSession,
    file: StaticString = #filePath, line: UInt = #line
) {
    let automation = session.trackingAutomation
    XCTAssertEqual(automation.statusText, automation.pausedStatusText, file: file, line: line)
    let totals = session.dailyTotals
    XCTAssertEqual(totals.status, totals.shownStatus(clock: session.clock), file: file, line: line)
    XCTAssertEqual(
        totals.dayTotal, totals.totalDuration(active: session.active, clock: session.clock), file: file, line: line)
    let explanation = totals.explanation(clock: session.clock)
    XCTAssertEqual(totals.totalsExplanation, explanation, file: file, line: line)
    XCTAssertEqual(presentation.runningTaskName, session.runningTaskName, file: file, line: line)
    let actions = session.trackingActions
    XCTAssertEqual(presentation.canStopTracking, actions.canStopTracking, file: file, line: line)
    XCTAssertEqual(
        session.tracking.timerText,
        session.tracking.displayText(clock: session.clock, confirmed: session.connection.confirmed),
        file: file, line: line)
    XCTAssertEqual(presentation.taskCreation, session.taskCreation, file: file, line: line)
    XCTAssertEqual(presentation.canOpenTaskCreation, session.canOpenTaskCreation, file: file, line: line)
    XCTAssertEqual(presentation.taskRename, session.taskRename, file: file, line: line)
    XCTAssertEqual(presentation.canOpenTaskRename, session.canOpenTaskRename, file: file, line: line)
    XCTAssertEqual(presentation.taskArchiving, session.taskArchiving, file: file, line: line)
    XCTAssertEqual(presentation.bulkTaskArchiving, session.bulkTaskArchiving, file: file, line: line)
    XCTAssertEqual(presentation.canOpenBulkTaskArchiving, session.canOpenBulkTaskArchiving, file: file, line: line)
    XCTAssertEqual(presentation.worklogCorrection, session.worklogCorrection, file: file, line: line)
    XCTAssertEqual(presentation.canOpenWorklogCorrection, session.canOpenWorklogCorrection, file: file, line: line)
    XCTAssertEqual(presentation.worklogMove, session.worklogMove, file: file, line: line)
    XCTAssertEqual(presentation.canOpenWorklogMove, session.canOpenWorklogMove, file: file, line: line)
    XCTAssertEqual(presentation.worklogDeletion, session.worklogDeletion, file: file, line: line)
    XCTAssertEqual(presentation.canOpenWorklogDeletion, session.canOpenWorklogDeletion, file: file, line: line)
    for task in session.tasks {
        XCTAssertEqual(
            presentation.canStartTracking(taskID: task.id), actions.canStartTracking(taskID: task.id),
            file: file, line: line)
        XCTAssertEqual(
            presentation.canArchiveTask(taskID: task.id), session.canArchiveTask(taskID: task.id),
            file: file, line: line)
        XCTAssertEqual(
            presentation.canUnarchiveTask(taskID: task.id), session.canUnarchiveTask(taskID: task.id),
            file: file, line: line)
        XCTAssertEqual(totals.taskDurations[task.id], session.dailyDuration(taskID: task.id), file: file, line: line)
        XCTAssertEqual(
            totals.taskTotal(taskID: task.id).content?.text,
            session.dailyDuration(taskID: task.id).map(formatClockDuration) ?? "-", file: file, line: line)
        XCTAssertEqual(totals.taskTotal(taskID: task.id).content?.explanation, explanation, file: file, line: line)
    }
    if session.taskArchiving.isPresented {
        XCTAssertEqual(presentation.taskArchivingSheetContent, session.taskArchiving, file: file, line: line)
    }
    if session.bulkTaskArchiving.isPresented {
        XCTAssertEqual(
            presentation.bulkTaskArchivingSheetContent, session.bulkTaskArchiving, file: file, line: line)
    }
    if session.worklogCorrection.isPresented {
        XCTAssertEqual(
            presentation.worklogCorrectionSheetContent, session.worklogCorrection, file: file, line: line)
    }
    if session.worklogMove.isPresented {
        XCTAssertEqual(presentation.worklogMoveSheetContent, session.worklogMove, file: file, line: line)
    }
    if session.worklogDeletion.isPresented {
        XCTAssertEqual(presentation.worklogDeletionSheetContent, session.worklogDeletion, file: file, line: line)
    }
}
