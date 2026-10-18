import Combine
import Foundation
import Observation
import TrackerClient

@MainActor
@Observable
final class TrackerStore {
    private let session: TrackerSession
    private let presentation: TrackerPresentation
    private let menuBarPreferences: UserDefaultsMenuBarPreferences
    private(set) var showDailyTotalInMenuBar: Bool
    private(set) var menuTaskOrder: MenuTaskOrder
    private let keyboardPreferences: UserDefaultsMenuKeyboardPreferences
    private(set) var menuShortcuts: MenuKeyboardShortcuts
    private var menuShortcutValidationError: String?
    private(set) var menuGlobalShortcutError: String?
    @ObservationIgnored var registerMenuShortcut: ((MenuShortcut) -> String?)?
    var menuShortcutError: String? { menuShortcutValidationError ?? menuGlobalShortcutError }
    let dailyTotals: TrackerDailyTotalsStore
    let menu: TrackerMenuStore
    let creation: TaskCreationStore
    let rename: TaskRenameStore
    let bulkArchiving: BulkTaskArchivingStore
    let archiving: TaskArchivingStore
    let correction: WorklogCorrectionStore
    let move: WorklogMoveStore
    let deletion: WorklogDeletionStore
    @ObservationIgnored private var lifecycle: MacLifecycleObserver?

    init() {
        let launch = TrackerLaunchConfiguration.current
        menuBarPreferences = UserDefaultsMenuBarPreferences(defaults: launch.defaults)
        // Observable properties are computed, so init reads these locals instead.
        let showDailyTotal = menuBarPreferences.load()
        let taskOrder = menuBarPreferences.loadTaskOrder()
        showDailyTotalInMenuBar = showDailyTotal
        menuTaskOrder = taskOrder
        keyboardPreferences = UserDefaultsMenuKeyboardPreferences(defaults: launch.defaults)
        menuShortcuts = keyboardPreferences.load()
        let worker = TrackerWorker(localDatabasePath: launch.localDatabasePath)
        session = TrackerSession(
            client: worker, clock: SystemTrackerClock(),
            scheduler: RunLoopTrackerScheduler(),
            settings: UserDefaultsConnectionSettings(defaults: launch.defaults),
            trackingPreferences: UserDefaultsTrackingPreferences(defaults: launch.defaults),
            lastTrackedTasks: UserDefaultsLastTrackedTasks(defaults: launch.defaults),
            reports: worker)
        presentation = TrackerPresentation(session: session)
        dailyTotals = TrackerDailyTotalsStore(presentation: presentation)
        menu = TrackerMenuStore(
            session: session, showDailyTotal: showDailyTotal, taskOrder: taskOrder)
        creation = TaskCreationStore(session: session, presentation: presentation)
        rename = TaskRenameStore(session: session, presentation: presentation)
        bulkArchiving = BulkTaskArchivingStore(session: session, presentation: presentation)
        archiving = TaskArchivingStore(session: session, presentation: presentation)
        correction = WorklogCorrectionStore(session: session, presentation: presentation)
        move = WorklogMoveStore(session: session, presentation: presentation)
        deletion = WorklogDeletionStore(session: session, presentation: presentation)
        dailyTotals.onMenuBarTick = { [weak self] in self?.menu.update() }
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

    // Views read these through the observable presentation, so each view redraws only for values it reads.
    var active: WorklogItem? { presentation.active }
    var worklogs: [WorklogItem] { presentation.worklogs }
    var error: String? { presentation.error }
    var trackingError: String? { presentation.trackingError }
    var historyUnavailable: Bool { presentation.historyUnavailable }
    var isHistoryLoading: Bool { presentation.isHistoryLoading }
    var selectedTaskID: String? { presentation.selectedTaskID }
    var tab: TaskTab { presentation.tab }
    var connectionSettings: ConnectionSettings { presentation.connectionSettings }
    var connectionStatusText: String { presentation.connectionStatusText }
    var connectionMessage: String? { presentation.connectionMessage }
    var isChangingConnection: Bool { presentation.isChangingConnection }
    var isStale: Bool { presentation.isStale }
    var visibleTasks: [TaskItem] { presentation.visibleTasks }
    var selectedTask: TaskItem? { presentation.selectedTask }
    var hasMoreHistory: Bool { presentation.hasMoreHistory }
    var pauseOnScreenLock: Bool { presentation.pauseOnScreenLock }
    var autoPauseStatusText: String? { presentation.autoPauseStatusText }
    var dailyTotalsStatus: DailyTotalsStatus { presentation.dailyTotalsStatus }
    var isBlockingControls: Bool { presentation.isBlockingControls }
    var canStopTracking: Bool { presentation.canStopTracking }
    var timerText: String { presentation.timerDisplayText }
    func canStartTracking(taskID: String) -> Bool { presentation.canStartTracking(taskID: taskID) }

    func setShowDailyTotalInMenuBar(_ enabled: Bool) {
        guard showDailyTotalInMenuBar != enabled else { return }
        showDailyTotalInMenuBar = enabled
        menuBarPreferences.save(enabled)
        menu.setShowDailyTotal(enabled)
        updateMenuBarTimer()
    }

    func setMenuTaskOrder(_ order: MenuTaskOrder) {
        guard menuTaskOrder != order else { return }
        menuTaskOrder = order
        menuBarPreferences.saveTaskOrder(order)
        menu.setTaskOrder(order)
    }

    private func updateMenuBarTimer() {
        dailyTotals.updateMenuBarTimer(isRunning: showDailyTotalInMenuBar && session.active != nil)
    }

    func setPauseOnScreenLock(_ enabled: Bool) { session.setPauseOnScreenLock(enabled) }
    func setMenuShortcut(_ action: MenuShortcutAction, shortcut: MenuShortcut) {
        var candidate = action == .openMenu ? MenuKeyboardShortcuts() : menuShortcuts
        candidate[action] = shortcut
        setMenuShortcuts(candidate)
    }

    func resetMenuShortcuts() { setMenuShortcuts(.defaults, retryGlobal: true) }

    private func setMenuShortcuts(_ candidate: MenuKeyboardShortcuts, retryGlobal: Bool = false) {
        if let error = candidate.validationError {
            reportMenuShortcutError(error)
            return
        }
        let changesGlobal = retryGlobal || candidate.openMenu != menuShortcuts.openMenu
        if changesGlobal {
            if let error = registerMenuShortcut?(candidate.openMenu) {
                reportMenuShortcutError(error)
                return
            }
        }
        menuShortcuts = candidate
        menuShortcutValidationError = nil
        if changesGlobal { menuGlobalShortcutError = nil }
        keyboardPreferences.save(candidate)
    }

    func reportMenuShortcutError(_ message: String?) {
        guard menuShortcutValidationError != message else { return }
        menuShortcutValidationError = message
    }

    func reportMenuGlobalShortcutError(_ message: String?) {
        guard menuGlobalShortcutError != message else { return }
        menuGlobalShortcutError = message
    }

    func menuCopyValue(
        _ action: MenuShortcutAction, taskID: String,
        connection: ConnectionSettings
    ) -> String? {
        session.menuCopyValue(action, taskID: taskID, connection: connection)
    }
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

/// Owns the menu bar's minute clock and looks up each task's observable daily total.
@MainActor
final class TrackerDailyTotalsStore {
    var onMenuBarTick: (() -> Void)?
    private let presentation: TrackerPresentation
    private var menuBarTimer: Timer?

    init(presentation: TrackerPresentation) { self.presentation = presentation }

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

    func task(_ taskID: String) -> TaskDailyTotal { presentation.taskDailyTotal(taskID: taskID) }
}

@MainActor
final class TrackerMenuStore {
    let label: TrackerMenuLabelStore
    let valuesDidChange = PassthroughSubject<TrackerMenuValues, Never>()
    private let session: TrackerSession
    private let presentation: TrackerMenuPresentationObserver
    private var showDailyTotal: Bool

    init(session: TrackerSession, showDailyTotal: Bool, taskOrder: MenuTaskOrder) {
        self.session = session
        self.showDailyTotal = showDailyTotal
        presentation = TrackerMenuPresentationObserver(
            session: session, showDailyTotal: showDailyTotal, taskOrder: taskOrder)
        label = TrackerMenuLabelStore(presentation: presentation)
        presentation.onValuesChange = { [weak self] in
            guard let self else { return }
            valuesDidChange.send(presentation.values)
        }
    }

    var content: TrackerMenuContent { presentation.content }
    var values: TrackerMenuValues { presentation.values }

    func update() { presentation.update(from: session, showDailyTotal: showDailyTotal) }

    func setShowDailyTotal(_ enabled: Bool) {
        showDailyTotal = enabled
        update()
    }

    func setTaskOrder(_ order: MenuTaskOrder) {
        presentation.setTaskOrder(order, session: session, showDailyTotal: showDailyTotal)
    }

    func menuOpened() { presentation.menuOpened(from: session, showDailyTotal: showDailyTotal) }
    func menuClosed() { presentation.menuClosed(from: session, showDailyTotal: showDailyTotal) }
}

@MainActor
final class TrackerMenuLabelStore {
    let objectWillChange = ObservableObjectPublisher()
    private let presentation: TrackerMenuPresentationObserver

    init(presentation: TrackerMenuPresentationObserver) {
        self.presentation = presentation
        presentation.onLabelChange = { [weak self] in self?.objectWillChange.send() }
    }

    var content: TrackerMenuLabelContent { presentation.label }
}

// The AppKit status item subscribes to this publisher; SwiftUI views do not observe it.
// Keep publisher access on the main actor with compilers that support isolated conformances.
#if compiler(>=6.2)
    extension TrackerMenuLabelStore: @MainActor ObservableObject {}
#else
    extension TrackerMenuLabelStore: ObservableObject {}
#endif
