import Foundation
import Observation
import TrackerClient

@Observable
final class TrackerStore {
    private let features: TrackerFeatures
    // Connection tests go straight to the worker; they don't touch the stores.
    private let worker: TrackerWorker
    /// The AppKit status menu reads its content, label and live values here.
    let menu: TrackerMenuPresentationObserver
    /// The Settings window reads this store through the SwiftUI environment; the status item gets it through `init`.
    let menuBarPreferences: MenuBarPreferencesStore
    @ObservationIgnored private var lifecycle: MacLifecycleObserver?

    init() {
        let launch = TrackerLaunchConfiguration.current
        let worker = TrackerWorker(localDatabasePath: launch.localDatabasePath)
        self.worker = worker
        features = TrackerFeatures(
            client: worker, clock: SystemTrackerClock(),
            scheduler: RunLoopTrackerScheduler(),
            settings: UserDefaultsConnectionSettings(defaults: launch.defaults),
            trackingPreferences: UserDefaultsTrackingPreferences(defaults: launch.defaults),
            lastTrackedTasks: UserDefaultsLastTrackedTasks(defaults: launch.defaults),
            reports: worker)
        let savedPreferences = UserDefaultsMenuBarPreferences(defaults: launch.defaults)
        menu = TrackerMenuPresentationObserver(
            sources: features.menuSources, showDailyTotal: savedPreferences.load(),
            taskOrder: savedPreferences.loadTaskOrder())
        // The store turns the menu bar clock on or off for the saved setting.
        menuBarPreferences = MenuBarPreferencesStore(
            preferences: savedPreferences,
            keyboardPreferences: UserDefaultsMenuKeyboardPreferences(defaults: launch.defaults),
            menu: menu, sync: features.sync)
        lifecycle = MacLifecycleObserver(sync: features.sync)
        lifecycle?.start()
        features.sync.start()
    }

    /// Views read the task list, tab and selection from this store through the SwiftUI environment.
    var taskCatalog: TaskCatalogStore { features.taskCatalog }
    /// Views read the selected task's worklogs and the history panel's state from this store the same way.
    var worklogHistory: WorklogHistoryStore { features.worklogHistory }
    /// Views read the connection settings and status from this store the same way.
    var connection: ConnectionStore { features.connection }
    /// Views read the running worklog, the tracking error and the timer text from this store the same way.
    var tracking: TrackingStore { features.tracking }
    /// Views read today's status and task totals from this store the same way.
    var dailyTotals: DailyTotalsStore { features.dailyTotals }
    /// Views read the auto-pause preference and status from this store the same way.
    var trackingAutomation: TrackingAutomationStore { features.trackingAutomation }
    /// The environment and the coordinator read the rename dialog from this store.
    var taskRename: TaskRenameStore { features.taskRename }
    /// The environment and the coordinator read the archive dialog from this store.
    var taskArchiving: TaskArchivingStore { features.taskArchiving }
    /// The environment, the coordinator and the app menus read the creation dialog from this store.
    var taskCreation: TaskCreationStore { features.taskCreation }
    /// The environment and the coordinator read the worklog deletion dialog from this store.
    var worklogDeletion: WorklogDeletionStore { features.worklogDeletion }
    var worklogMove: WorklogMoveStore { features.worklogMove }
    /// The environment and the coordinator read the bulk archive dialog from this store.
    var bulkTaskArchiving: BulkTaskArchivingStore { features.bulkTaskArchiving }
    /// The environment and the coordinator read the worklog correction dialog from this store.
    var worklogCorrection: WorklogCorrectionStore { features.worklogCorrection }

    var canStopTracking: Bool { features.trackingAvailability.canStop }
    func canStartTracking(taskID: String) -> Bool { features.trackingAvailability.canStart(taskID: taskID) }

    func setPauseOnScreenLock(_ enabled: Bool) { features.sync.setPauseOnScreenLock(enabled) }
    func menuCopyValue(
        _ action: MenuShortcutAction, taskID: String,
        connection: ConnectionSettings
    ) -> String? {
        features.menuBar.menuCopyValue(action, taskID: taskID, connection: connection)
    }
    func changeTab(_ tab: TaskTab) { features.sync.changeTab(tab) }
    func selectTask(_ taskID: String?) { features.sync.selectTask(taskID) }
    func refresh() { features.sync.refresh() }
    func retryHistory() { features.sync.retryHistory() }
    func loadOlder() { features.sync.loadOlder() }
    func startTracking(taskID: String) { features.trackingActions.startTracking(taskID: taskID) }
    func stopTracking(worklogID: String) { features.trackingActions.stopTracking(worklogID: worklogID) }
    func dismissTrackingError() { features.trackingActions.dismissTrackingError() }
    func performMenuPrimaryAction() -> MenuPrimaryAction { features.trackingActions.performMenuPrimaryAction() }

    func menuOpened() {
        // The sync publishes first, so the menu captures current values even when no display tick ran.
        features.sync.setMenuVisible(true)
        menu.menuOpened()
    }

    func menuClosed() {
        features.sync.setMenuVisible(false)
        menu.menuClosed()
    }
    func testConnection(_ settings: ConnectionSettings) async throws {
        try await testConnectionSettings(settings, client: worker)
    }
    func connect(_ settings: ConnectionSettings) async -> Bool {
        await features.sync.connect(settings)
    }
}
