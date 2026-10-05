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
    let menu: TrackerMenuStore
    let creation: TaskCreationStore
    let rename: TaskRenameStore
    private var lifecycle: MacLifecycleObserver?

    init() {
        menuBarPreferences = UserDefaultsMenuBarPreferences()
        showDailyTotalInMenuBar = menuBarPreferences.load()
        let worker = TrackerWorker()
        session = TrackerSession(client: worker, clock: SystemTrackerClock(),
                                 scheduler: RunLoopTrackerScheduler(),
                                 settings: UserDefaultsConnectionSettings(),
                                 trackingPreferences: UserDefaultsTrackingPreferences(),
                                 lastTrackedTasks: UserDefaultsLastTrackedTasks(),
                                 reports: worker)
        presentation = TrackerPresentationObserver(session: session)
        activity = TrackerActivityStore(session: session, presentation: presentation)
        timer = TrackerTimerStore(session: session, presentation: presentation)
        dailyTotals = TrackerDailyTotalsStore(presentation: presentation)
        menu = TrackerMenuStore(session: session, showDailyTotal: showDailyTotalInMenuBar)
        creation = TaskCreationStore(session: session, presentation: presentation)
        rename = TaskRenameStore(session: session, presentation: presentation)
        dailyTotals.onMenuBarTick = { [weak self] in self?.menu.update() }
        presentation.onContentChange = { [weak self] in self?.objectWillChange.send() }
        session.onChange = { [weak self] in
            guard let self else { return }
            updateMenuBarTimer()
            presentation.update(from: session)
            menu.update()
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
    var dailyTotalsStatus: DailyTotalsStatus { session.dailyTotalsStatus }

    func setShowDailyTotalInMenuBar(_ enabled: Bool) {
        guard showDailyTotalInMenuBar != enabled else { return }
        objectWillChange.send()
        showDailyTotalInMenuBar = enabled
        menuBarPreferences.save(enabled)
        menu.setShowDailyTotal(enabled)
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
    func performMenuPrimaryAction() -> MenuPrimaryAction { session.performMenuPrimaryAction() }

    func menuOpened() {
        menu.menuOpened()
        session.menuOpened()
    }

    func menuClosed() {
        session.menuClosed()
        menu.menuClosed()
    }
    func testConnection(_ settings: ConnectionSettings) async throws {
        try await session.testConnection(settings)
    }
    func connect(_ settings: ConnectionSettings) async -> Bool {
        await session.connect(settings)
    }
}

@MainActor
final class TrackerDailyTotalsStore {
    var onMenuBarTick: (() -> Void)?
    private let presentation: TrackerPresentationObserver
    private var taskStores: [String: TrackerTaskDailyTotalStore] = [:]
    private var menuBarTimer: Timer?

    init(presentation: TrackerPresentationObserver) {
        self.presentation = presentation
        presentation.onTaskDailyTotalsChange = { [weak self] taskIDs in
            guard let self else { return }
            for id in taskIDs {
                if let content = self.presentation.taskDailyTotals[id] {
                    taskStores[id]?.update(content)
                } else { taskStores.removeValue(forKey: id) }
            }
        }
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
            MainActor.assumeIsolated { self?.onMenuBarTick?() }
        }
        timer.tolerance = 5
        menuBarTimer = timer
        RunLoop.main.add(timer, forMode: .common)
    }

    func task(_ taskID: String) -> TrackerTaskDailyTotalStore {
        if let existing = taskStores[taskID] { return existing }
        let store = TrackerTaskDailyTotalStore(content: presentation.taskDailyTotals[taskID])
        taskStores[taskID] = store
        return store
    }
}

@MainActor
final class TrackerTaskDailyTotalStore: ObservableObject {
    let objectWillChange = ObservableObjectPublisher()
    private(set) var content: TaskDailyTotalPresentation?

    init(content: TaskDailyTotalPresentation?) { self.content = content }

    func update(_ next: TaskDailyTotalPresentation) {
        guard content != next else { return }
        content = next
        objectWillChange.send()
    }
}

@MainActor
final class TrackerMenuStore: ObservableObject {
    let objectWillChange = ObservableObjectPublisher()
    let label: TrackerMenuLabelStore
    private let session: TrackerSession
    private let presentation: TrackerMenuPresentationObserver
    private var showDailyTotal: Bool

    init(session: TrackerSession, showDailyTotal: Bool) {
        self.session = session
        self.showDailyTotal = showDailyTotal
        presentation = TrackerMenuPresentationObserver(session: session, showDailyTotal: showDailyTotal)
        label = TrackerMenuLabelStore(presentation: presentation)
        presentation.onContentChange = { [weak self] in self?.objectWillChange.send() }
    }

    var content: TrackerMenuContent { presentation.content }

    func update() { presentation.update(from: session, showDailyTotal: showDailyTotal) }

    func setShowDailyTotal(_ enabled: Bool) {
        showDailyTotal = enabled
        update()
    }

    func menuOpened() { presentation.menuOpened(from: session, showDailyTotal: showDailyTotal) }
    func menuClosed() { presentation.menuClosed(from: session, showDailyTotal: showDailyTotal) }
}

@MainActor
final class TrackerMenuLabelStore: ObservableObject {
    let objectWillChange = ObservableObjectPublisher()
    private let presentation: TrackerMenuPresentationObserver

    init(presentation: TrackerMenuPresentationObserver) {
        self.presentation = presentation
        presentation.onLabelChange = { [weak self] in self?.objectWillChange.send() }
    }

    var content: TrackerMenuLabelContent { presentation.label }
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
