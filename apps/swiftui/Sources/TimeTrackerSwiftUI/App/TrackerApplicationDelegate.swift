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
        presentingWindow: { [weak self] needsEditor, occupiedWindowIDs in
            guard let self else { return nil }
            if needsEditor { return self.showEditor(excluding: occupiedWindowIDs) }
            guard !NSApplication.shared.isHidden else { return nil }
            var available = self.windows
            for id in occupiedWindowIDs { available.disappeared(id) }
            return available.activePresentationWindowID
        }
    )
    private var windows = TrackerWindowRoutingState()
    private var openTracker: ((UUID) -> Void)?
    private var deferredOpenIDs: [UUID] = []
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
        deferredOpenIDs.removeAll()
    }

    func installSceneOpener(_ openTracker: @escaping (UUID) -> Void) {
        self.openTracker = openTracker
        start()
        let deferred = deferredOpenIDs
        deferredOpenIDs.removeAll()
        for id in deferred { openTracker(id) }
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
            if let openTracker { openTracker(request.id) } else { deferredOpenIDs.append(request.id) }
        }
        NSApplication.shared.activate()
        presentation.windowAvailable()
        return request.id
    }

    private func showEditor(excluding occupiedWindowIDs: Set<UUID>) -> UUID {
        if occupiedWindowIDs.isEmpty { return showTracker() }
        // Choose a free tracker window without changing the registry's existing window order.
        var available = windows
        for id in occupiedWindowIDs { available.disappeared(id) }
        let request = available.showTracker()
        // A pending window can already be reserved by another editor before it appears.
        let id = occupiedWindowIDs.contains(request.id) ? UUID() : request.id
        if let openTracker { openTracker(id) } else { deferredOpenIDs.append(id) }
        NSApplication.shared.activate()
        return id
    }
}
