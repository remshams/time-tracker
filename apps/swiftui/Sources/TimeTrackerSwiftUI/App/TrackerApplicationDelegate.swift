import AppKit
import SwiftUI
import TrackerClient

final class TrackerApplicationDelegate: NSObject, NSApplicationDelegate {
    let runtime = TrackerAppRuntime()

    func applicationDidFinishLaunching(_ notification: Notification) {
        #if DEBUG
            if TrackerLaunchConfiguration.uiTestSuite != nil,
                let requestedAppearance = ProcessInfo.processInfo.environment["TT_UI_TEST_APPEARANCE"]
            {
                switch requestedAppearance {
                case "Light": NSApplication.shared.appearance = NSAppearance(named: .aqua)
                case "Dark": NSApplication.shared.appearance = NSAppearance(named: .darkAqua)
                default: break
                }
            }
        #endif
        runtime.start()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
        runtime.showTracker()
        return false
    }
}

/// Creates the feature objects, the menu observer, the menu bar settings and the lifecycle observer, and starts
/// the sync. The scenes, the app menus, the status item and the presentation coordinator get the objects here.
final class TrackerAppRuntime {
    let features: TrackerFeatures
    /// Settings tests a connection with this client.
    let client: any TrackerClient
    /// The Settings window reads this store through the SwiftUI environment; the status item gets it through `init`.
    let menuBarPreferences: MenuBarPreferencesStore
    private let menu: TrackerMenuPresentationObserver
    private let lifecycle: MacLifecycleObserver
    private lazy var statusItem = TrackerStatusItemController(
        sync: features.sync, tracking: features.tracking, menuBar: features.menuBar,
        connection: features.connection, menu: menu, preferences: menuBarPreferences)
    private lazy var presentation = TrackerTaskPresentationCoordinator(gate: features.presentationGate) { [weak self] in
        self?.showTracker()
    }
    private var openWindow: OpenWindowAction?
    private var opensTrackerWhenOpenerArrives = false
    private var isRunning = false

    init() {
        let launch = TrackerLaunchConfiguration.current
        let worker = TrackerWorker(localDatabasePath: launch.localDatabasePath)
        client = worker
        features = TrackerFeatures(
            client: worker, clock: SystemTrackerClock(),
            scheduler: RunLoopTrackerScheduler(),
            settings: UserDefaultsConnectionSettings(defaults: launch.defaults),
            trackingPreferences: UserDefaultsTrackingPreferences(defaults: launch.defaults),
            lastTrackedTasks: UserDefaultsLastTrackedTasks(defaults: launch.defaults),
            taskOrder: UserDefaultsTaskOrder(defaults: launch.defaults),
            reports: worker)
        let savedPreferences = UserDefaultsMenuBarPreferences(defaults: launch.defaults)
        menu = TrackerMenuPresentationObserver(sources: features.menuSources, showDailyTotal: savedPreferences.load())
        // The store turns the menu bar clock on or off for the saved setting.
        menuBarPreferences = MenuBarPreferencesStore(
            preferences: savedPreferences,
            keyboardPreferences: UserDefaultsMenuKeyboardPreferences(defaults: launch.defaults),
            menu: menu, menuBar: features.menuBar)
        lifecycle = MacLifecycleObserver(sync: features.sync, automation: features.trackingAutomation)
        lifecycle.start()
        features.sync.start()
    }

    func start() {
        guard !isRunning else { return }
        isRunning = true
        presentation.start()
        statusItem.start { [weak self] in self?.showTracker() }
    }

    /// SwiftUI hands `openWindow` only to scene content and commands. They pass it here, so the Dock and
    /// status menu, which are AppKit code, can open the tracker window too.
    func installSceneOpener(_ openWindow: OpenWindowAction) {
        self.openWindow = openWindow
        start()
        if opensTrackerWhenOpenerArrives {
            opensTrackerWhenOpenerArrives = false
            openWindow(id: TrackerSceneID.tracker)
        }
    }

    /// Brings the tracker window to the front, restoring it if minimized, or opens it if it is closed.
    /// Without an opener yet, the window opens once `installSceneOpener(_:)` receives one.
    func showTracker() {
        if let openWindow { openWindow(id: TrackerSceneID.tracker) } else { opensTrackerWhenOpenerArrives = true }
        NSApplication.shared.activate()
    }
}
