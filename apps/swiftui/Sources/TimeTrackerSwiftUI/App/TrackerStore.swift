import Combine
import Foundation
import TrackerClient

@MainActor
final class TrackerStore: ObservableObject {
    let objectWillChange = ObservableObjectPublisher()
    private let session: TrackerSession
    private let presentation: TrackerPresentationObserver
    let activity: TrackerActivityStore
    let timer: TrackerTimerStore
    let dailyTotals: TrackerDailyTotalsStore
    private var lifecycle: MacLifecycleObserver?

    init() {
        let worker = TrackerWorker()
        session = TrackerSession(client: worker, clock: SystemTrackerClock(),
                                 scheduler: RunLoopTrackerScheduler(),
                                 settings: UserDefaultsConnectionSettings(),
                                 trackingPreferences: UserDefaultsTrackingPreferences(),
                                 reports: worker)
        presentation = TrackerPresentationObserver(session: session)
        activity = TrackerActivityStore(session: session, presentation: presentation)
        timer = TrackerTimerStore(session: session, presentation: presentation)
        dailyTotals = TrackerDailyTotalsStore(session: session, presentation: presentation)
        presentation.onContentChange = { [weak self] in self?.objectWillChange.send() }
        session.onChange = { [weak self] in
            guard let self else { return }
            presentation.update(from: session)
        }
        lifecycle = MacLifecycleObserver(session: session)
        lifecycle?.start()
        session.start()
    }

    var tasks: [TaskItem] { session.tasks }
    var active: WorklogItem? { session.active }
    var worklogs: [WorklogItem] { session.worklogs }
    var nextCursor: String? { session.nextCursor }
    var error: String? { session.error }
    var trackingError: String? { session.trackingError }
    var historyUnavailable: Bool { session.historyUnavailable }
    var selectedTaskID: String? { session.selectedTaskID }
    var tab: TaskTab { session.tab }
    var connectionSettings: ConnectionSettings { session.connectionSettings }
    var connectionStatusText: String { session.connectionStatusText }
    var connectionMessage: String? { session.connectionMessage }
    var isBusy: Bool { session.isBusy }
    var isChangingConnection: Bool { session.isChangingConnection }
    var isStale: Bool { session.isStale }
    var visibleTasks: [TaskItem] { session.visibleTasks }
    var selectedTask: TaskItem? { session.selectedTask }
    var hasMoreHistory: Bool { session.hasMoreHistory }
    var canStartSelectedTask: Bool { session.canStartSelectedTask }
    var canStopTracking: Bool { session.canStopTracking }
    var runningTaskName: String { session.runningTaskName }
    var timerDisplayText: String { session.timerDisplayText }
    var elapsed: TimeInterval? { session.elapsed }
    var pauseOnScreenLock: Bool { session.pauseOnScreenLock }
    var autoPauseStatusText: String? { session.autoPauseStatusText }
    var todayTasks: [TaskItem] { session.todayTasks }
    var dailyTotalsStatus: DailyTotalsStatus { session.dailyTotalsStatus }
    var dailyTotalsError: String? { session.dailyTotalsError }

    func setPauseOnScreenLock(_ enabled: Bool) { session.setPauseOnScreenLock(enabled) }
    func changeTab(_ tab: TaskTab) { session.changeTab(tab) }
    func select(_ taskID: String?) { session.select(taskID) }
    func refresh() { session.refresh() }
    func retryHistory() { session.retryHistory() }
    func loadOlder() { session.loadOlder() }
    func startTracking(taskID: String) { session.startTracking(taskID: taskID) }
    func stopTracking(worklogID: String) { session.stopTracking(worklogID: worklogID) }
    func dismissTrackingError() { session.dismissTrackingError() }
    func testConnection(_ settings: ConnectionSettings) async throws {
        try await session.testConnection(settings)
    }
    func connect(_ settings: ConnectionSettings) async -> Bool {
        await session.connect(settings)
    }
}

@MainActor
final class TrackerDailyTotalsStore: ObservableObject {
    let objectWillChange = ObservableObjectPublisher()
    private let session: TrackerSession

    init(session: TrackerSession, presentation: TrackerPresentationObserver) {
        self.session = session
        presentation.onDailyTotalsChange = { [weak self] in self?.objectWillChange.send() }
    }

    func text(taskID: String) -> String {
        session.dailyDuration(taskID: taskID).map(clockDuration) ?? "-"
    }

    var explanation: String {
        switch session.dailyTotalsStatus {
        case .current: return "Time logged today in your local time zone"
        case .cached: return "Today's total uses cached tracker state. Running time may be unconfirmed."
        case .loading: return "Loading today's totals"
        case .unavailable: return session.dailyTotalsError ?? "Today's total is unavailable"
        }
    }
}

@MainActor
final class TrackerActivityStore: ObservableObject {
    let objectWillChange = ObservableObjectPublisher()
    private let session: TrackerSession

    init(session: TrackerSession, presentation: TrackerPresentationObserver) {
        self.session = session
        presentation.onActivityChange = { [weak self] in self?.objectWillChange.send() }
    }

    var isBusy: Bool { session.isBusy }
    var canStartSelectedTask: Bool { session.canStartSelectedTask }
    var canStopTracking: Bool { session.canStopTracking }
}

@MainActor
final class TrackerTimerStore: ObservableObject {
    let objectWillChange = ObservableObjectPublisher()
    private let session: TrackerSession

    init(session: TrackerSession, presentation: TrackerPresentationObserver) {
        self.session = session
        presentation.onTimerChange = { [weak self] in self?.objectWillChange.send() }
    }

    var text: String { session.timerDisplayText }
}
