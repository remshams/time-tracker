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
    private let session: TrackerSession
    private var samplers: [@MainActor () -> Void] = []

    init(session: TrackerSession) {
        self.session = session
        presentation = TrackerPresentation(session: session)
        attach()
    }

    /// Restores the session change handler after a fixture helper replaced it.
    func attach() { session.onChange = { [weak self] in self?.update() } }

    func update() {
        presentation.update(from: session)
        assertMirrors(presentation, session)
        for sample in samplers { sample() }
    }

    /// Counts the updates that changed the values read by `track`.
    func counter(_ track: @escaping @MainActor (TrackerPresentation) -> Void) -> ObservationRecorder<Void> {
        recorder(track) { () }
    }

    /// Records `value` after each update that changed the values read by `track`.
    func recorder<Value>(
        _ track: @escaping @MainActor (TrackerPresentation) -> Void, value: @escaping @MainActor () -> Value
    ) -> ObservationRecorder<Value> {
        let presentation = presentation
        let recorder = ObservationRecorder(track: { track(presentation) }, value: value)
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

    init(_ presentation: TrackerPresentation, taskIDs: [String]) {
        probes = Dictionary(
            uniqueKeysWithValues: taskIDs.map { taskID in
                let total = presentation.taskDailyTotal(taskID: taskID)
                return (taskID, ObservationProbe { _ = total.content })
            })
    }

    func changed() -> Set<String> { Set(probes.filter { $0.value.consume() }.keys) }
}

@MainActor
enum PresentationReads {
    /// Reads the values the task list and worklog list display.
    static func lists(_ presentation: TrackerPresentation) {
        _ = presentation.visibleTasks
        _ = presentation.selectedTaskID
        _ = presentation.selectedTask
        _ = presentation.tab
        _ = presentation.active
        _ = presentation.isStale
        _ = presentation.dailyTotalsStatus
        _ = presentation.worklogs
        _ = presentation.hasMoreHistory
        _ = presentation.isHistoryLoading
        _ = presentation.error
        _ = presentation.historyUnavailable
        _ = presentation.autoPauseStatusText
    }

    /// Reads every window value outside the request controls, timer, totals, and feature sheets.
    static func content(_ presentation: TrackerPresentation) {
        lists(presentation)
        _ = presentation.trackingError
        _ = presentation.connectionSettings
        _ = presentation.connectionStatusText
        _ = presentation.connectionMessage
        _ = presentation.isChangingConnection
        _ = presentation.pauseOnScreenLock
    }

    static func controls(_ presentation: TrackerPresentation) {
        _ = presentation.canStopTracking
        _ = presentation.startableTaskIDs
    }

    static func timer(_ presentation: TrackerPresentation) { _ = presentation.timerDisplayText }

    static func taskCreation(_ presentation: TrackerPresentation) {
        _ = presentation.taskCreation
        _ = presentation.canOpenTaskCreation
    }

    static func taskRename(_ presentation: TrackerPresentation) {
        _ = presentation.taskRename
        _ = presentation.canOpenTaskRename
    }

    static func taskArchiving(_ presentation: TrackerPresentation) {
        _ = presentation.taskArchiving
        _ = presentation.archivableTaskIDs
        _ = presentation.unarchivableTaskIDs
    }

    static func bulkTaskArchiving(_ presentation: TrackerPresentation) {
        _ = presentation.bulkTaskArchiving
        _ = presentation.canOpenBulkTaskArchiving
    }

    static func worklogCorrection(_ presentation: TrackerPresentation) {
        _ = presentation.worklogCorrection
        _ = presentation.canOpenWorklogCorrection
    }

    static func worklogMove(_ presentation: TrackerPresentation) {
        _ = presentation.worklogMove
        _ = presentation.canOpenWorklogMove
    }

    static func worklogDeletion(_ presentation: TrackerPresentation) {
        _ = presentation.worklogDeletion
        _ = presentation.canOpenWorklogDeletion
    }
}

/// Checks that every presentation value equals the session value it copies.
@MainActor
func assertMirrors(
    _ presentation: TrackerPresentation, _ session: TrackerSession,
    file: StaticString = #filePath, line: UInt = #line
) {
    XCTAssertEqual(presentation.active, session.active, file: file, line: line)
    XCTAssertEqual(presentation.worklogs, session.worklogs, file: file, line: line)
    XCTAssertEqual(presentation.hasMoreHistory, session.hasMoreHistory, file: file, line: line)
    XCTAssertEqual(presentation.error, session.error, file: file, line: line)
    XCTAssertEqual(presentation.trackingError, session.trackingError, file: file, line: line)
    XCTAssertEqual(presentation.historyUnavailable, session.historyUnavailable, file: file, line: line)
    XCTAssertEqual(presentation.isHistoryLoading, session.isHistoryLoading, file: file, line: line)
    XCTAssertEqual(presentation.visibleTasks, session.visibleTasks, file: file, line: line)
    XCTAssertEqual(presentation.selectedTaskID, session.selectedTaskID, file: file, line: line)
    XCTAssertEqual(presentation.selectedTask, session.selectedTask, file: file, line: line)
    XCTAssertEqual(presentation.tab, session.tab, file: file, line: line)
    XCTAssertEqual(presentation.connectionSettings, session.connectionSettings, file: file, line: line)
    XCTAssertEqual(presentation.connectionStatusText, session.connectionStatusText, file: file, line: line)
    XCTAssertEqual(presentation.connectionMessage, session.connectionMessage, file: file, line: line)
    XCTAssertEqual(presentation.isChangingConnection, session.isChangingConnection, file: file, line: line)
    XCTAssertEqual(presentation.isStale, session.isStale, file: file, line: line)
    XCTAssertEqual(presentation.pauseOnScreenLock, session.pauseOnScreenLock, file: file, line: line)
    XCTAssertEqual(presentation.autoPauseStatusText, session.autoPauseStatusText, file: file, line: line)
    XCTAssertEqual(presentation.dailyTotalsStatus, session.dailyTotalsStatus, file: file, line: line)
    XCTAssertEqual(presentation.canStopTracking, session.canStopTracking, file: file, line: line)
    XCTAssertEqual(presentation.timerDisplayText, session.timerDisplayText, file: file, line: line)
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
            presentation.canStartTracking(taskID: task.id), session.canStartTracking(taskID: task.id),
            file: file, line: line)
        XCTAssertEqual(
            presentation.canArchiveTask(taskID: task.id), session.canArchiveTask(taskID: task.id),
            file: file, line: line)
        XCTAssertEqual(
            presentation.canUnarchiveTask(taskID: task.id), session.canUnarchiveTask(taskID: task.id),
            file: file, line: line)
        XCTAssertEqual(
            presentation.taskDailyTotal(taskID: task.id).content?.text,
            session.dailyDuration(taskID: task.id).map(formatClockDuration) ?? "-", file: file, line: line)
        XCTAssertEqual(
            presentation.taskDailyTotal(taskID: task.id).content?.explanation, session.dailyTotalsExplanation,
            file: file, line: line)
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
