import AppKit
import SwiftUI
import TrackerClient

@MainActor
final class TrackerApplicationDelegate: NSObject, NSApplicationDelegate {
    let runtime = TrackerAppRuntime()

    func applicationDidFinishLaunching(_ notification: Notification) {
        #if DEBUG
            if TrackerLaunchConfiguration.current.localDatabasePath != nil,
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

    func applicationWillTerminate(_ notification: Notification) {
        runtime.stop()
    }
}

@MainActor
final class TrackerAppRuntime {
    let store = TrackerStore()
    private lazy var statusItem = TrackerStatusItemController(store: store)
    lazy var presentation = TrackerTaskPresentationCoordinator(
        store: store,
        presentingWindow: { [weak self] needsEditor in
            guard let self else { return nil }
            if needsEditor { return self.showTracker() }
            guard !NSApplication.shared.isHidden else { return nil }
            return self.windows.activePresentationWindowID
        }
    )
    private var windows = TrackerWindowRoutingState()
    private var openTracker: ((UUID) -> Void)?
    private var deferredOpenID: UUID?
    private var isRunning = false

    func start() {
        guard !isRunning else { return }
        isRunning = true
        presentation.start()
        statusItem.start { [weak self] in _ = self?.showTracker() }
    }

    func stop() {
        isRunning = false
        statusItem.stop()
        presentation.stop()
        openTracker = nil
        deferredOpenID = nil
    }

    func installSceneOpener(_ openTracker: @escaping (UUID) -> Void) {
        self.openTracker = openTracker
        start()
        if let id = deferredOpenID {
            deferredOpenID = nil
            openTracker(id)
        }
    }

    func windowAppeared(_ id: UUID, phase: ScenePhase) {
        windows.appeared(id, isActive: phase == .active)
        presentation.windowAvailable()
    }

    func windowPhaseChanged(_ id: UUID, phase: ScenePhase) {
        windows.setActive(id, isActive: phase == .active)
        presentation.windowAvailable()
    }

    func windowDisappeared(_ id: UUID) {
        windows.disappeared(id)
        presentation.windowClosed(id)
    }

    func preferWindow(_ id: UUID) {
        windows.prefer(id)
    }

    @discardableResult
    func showTracker() -> UUID {
        let request = windows.showTracker()
        if request.shouldOpen {
            if let openTracker { openTracker(request.id) } else { deferredOpenID = request.id }
        }
        NSApplication.shared.activate()
        presentation.windowAvailable()
        return request.id
    }
}
