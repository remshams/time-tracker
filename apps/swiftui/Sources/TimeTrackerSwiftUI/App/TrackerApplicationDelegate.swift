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

final class TrackerAppRuntime {
    let store = TrackerStore()
    private lazy var statusItem = TrackerStatusItemController(store: store)
    lazy var presentation = TrackerTaskPresentationCoordinator(store: store) { [weak self] in
        self?.showTracker()
    }
    private var openWindow: OpenWindowAction?
    private var opensTrackerWhenOpenerArrives = false
    private var isRunning = false

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
