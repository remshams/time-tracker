import Combine
import Foundation
import TrackerClient

@MainActor
final class TrackerStore: ObservableObject {
    let objectWillChange = ObservableObjectPublisher()
    private let session: TrackerSession
    private let presentation: TrackerPresentationObserver
    private let menuBarPreferences: UserDefaultsMenuBarPreferences
    private(set) var showDailyTotalInMenuBar: Bool
    let activity: TrackerActivityStore
    let timer: TrackerTimerStore
    let dailyTotals: TrackerDailyTotalsStore
    private var lifecycle: MacLifecycleObserver?

    init() {
        menuBarPreferences = UserDefaultsMenuBarPreferences()
        showDailyTotalInMenuBar = menuBarPreferences.load()
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
            updateMenuBarTimer()
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

    func setShowDailyTotalInMenuBar(_ enabled: Bool) {
        guard showDailyTotalInMenuBar != enabled else { return }
        objectWillChange.send()
        showDailyTotalInMenuBar = enabled
        menuBarPreferences.save(enabled)
        updateMenuBarTimer()
    }

    private func updateMenuBarTimer() {
        dailyTotals.updateMenuBarTimer(isRunning: showDailyTotalInMenuBar && session.active != nil)
    }

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
    private var menuBarTimer: Timer?

    init(session: TrackerSession, presentation: TrackerPresentationObserver) {
        self.session = session
        presentation.onDailyTotalsChange = { [weak self] in self?.objectWillChange.send() }
    }

    deinit { menuBarTimer?.invalidate() }

    func updateMenuBarTimer(isRunning: Bool) {
        guard isRunning else {
            menuBarTimer?.invalidate()
            menuBarTimer = nil
            return
        }
        guard menuBarTimer == nil else { return }
        let timer = Timer(timeInterval: 60, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.objectWillChange.send() }
        }
        timer.tolerance = 5
        menuBarTimer = timer
        RunLoop.main.add(timer, forMode: .common)
    }

    func text(taskID: String) -> String {
        session.dailyDuration(taskID: taskID).map(clockDuration) ?? "-"
    }

    var totalText: String { session.totalDailyDurationText }

    var menuBarText: String {
        guard let duration = session.totalDailyDuration else { return "-" }
        let minutes = Int(max(0, duration)) / 60
        let text = String(format: "%02d:%02d", minutes / 60, minutes % 60)
        return session.dailyTotalsStatus == .cached ? "~\(text)" : text
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
    func canStartTracking(taskID: String) -> Bool { session.canStartTracking(taskID: taskID) }
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
