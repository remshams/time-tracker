import Foundation
import Observation
import TrackerClient

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
    /// The AppKit status menu reads its content, label and live values here.
    let menu: TrackerMenuPresentationObserver
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
        menu = TrackerMenuPresentationObserver(
            sources: TrackerMenuSources(session: session, presentation: presentation),
            showDailyTotal: showDailyTotal, taskOrder: taskOrder)
        creation = TaskCreationStore(session: session, presentation: presentation)
        rename = TaskRenameStore(session: session, presentation: presentation)
        bulkArchiving = BulkTaskArchivingStore(session: session, presentation: presentation)
        archiving = TaskArchivingStore(session: session, presentation: presentation)
        correction = WorklogCorrectionStore(session: session, presentation: presentation)
        move = WorklogMoveStore(session: session, presentation: presentation)
        deletion = WorklogDeletionStore(session: session, presentation: presentation)
        session.onChange = { [weak self] in
            guard let self else { return }
            presentation.update(from: session)
        }
        session.setMenuBarClock(showDailyTotal)
        lifecycle = MacLifecycleObserver(session: session)
        lifecycle?.start()
        session.start()
    }

    /// Views read the task list, tab and selection from this store through the SwiftUI environment.
    var taskCatalog: TaskCatalogStore { session.taskCatalog }
    /// Views read the selected task's worklogs and the history panel's state from this store the same way.
    var worklogHistory: WorklogHistoryStore { session.worklogHistory }
    /// Views read the connection settings and status from this store the same way.
    var connection: ConnectionStore { session.connection }
    /// Views read the running worklog, the tracking error and the timer text from this store the same way.
    var tracking: TrackingStore { session.tracking }
    /// Views read today's status and task totals from this store the same way.
    var dailyTotals: DailyTotalsStore { session.dailyTotals }

    // Views read these through the observable presentation, so each view redraws only for values it reads.
    var pauseOnScreenLock: Bool { presentation.pauseOnScreenLock }
    var autoPauseStatusText: String? { presentation.autoPauseStatusText }
    var canStopTracking: Bool { presentation.canStopTracking }
    func canStartTracking(taskID: String) -> Bool { presentation.canStartTracking(taskID: taskID) }

    func setShowDailyTotalInMenuBar(_ enabled: Bool) {
        guard showDailyTotalInMenuBar != enabled else { return }
        showDailyTotalInMenuBar = enabled
        menuBarPreferences.save(enabled)
        menu.setShowDailyTotal(enabled)
        session.setMenuBarClock(enabled)
    }

    func setMenuTaskOrder(_ order: MenuTaskOrder) {
        guard menuTaskOrder != order else { return }
        menuTaskOrder = order
        menuBarPreferences.saveTaskOrder(order)
        menu.setTaskOrder(order)
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
        // The session publishes first, so the menu captures current values even when no display tick ran.
        session.menuOpened()
        menu.menuOpened()
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
